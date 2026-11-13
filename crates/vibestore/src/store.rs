//! The `object_store::ObjectStore` and `PaginatedListStore` implementation.
//!
//! Everything celld asks of its bucket is here, and anything it could ask that
//! this store cannot honor fails loudly with `Error::NotSupported` instead of
//! being ignored: S3 object tags, version-addressed reads, and GCS-style
//! generation preconditions.

#![allow(clippy::disallowed_methods)]

use crate::core::{self, Core, Source};
use crate::error::{Result, StoreError, STORE};
use crate::index::{Mode, Row, Scan};
use crate::spec::VibeSpec;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::stream::{self, BoxStream};
use futures_util::{StreamExt as _, TryStreamExt as _};
use object_store::list::{PaginatedListOptions, PaginatedListResult, PaginatedListStore};
use object_store::path::Path;
use object_store::{
    Attribute, AttributeValue, Attributes, GetOptions, GetResult, GetResultPayload, ListResult,
    MultipartUpload, ObjectMeta, ObjectStore, PutMode, PutMultipartOptions, PutOptions,
    PutPayload, PutResult, UploadPart,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// How a blocking job is handed to a thread. celld passes its execution
/// domain's `asyncrt::blocking`; the default is Tokio's blocking pool.
pub type Blocker = Arc<dyn Fn(Box<dyn FnOnce() + Send + 'static>) + Send + Sync>;

pub fn tokio_blocker() -> Blocker {
    Arc::new(|job| {
        drop(tokio::task::spawn_blocking(job));
    })
}

/// A page of the keyset scans that back `list`.
const LIST_PAGE: usize = 512;
/// The largest read served in one blocking job; longer ranges stream.
const READ_CHUNK: u64 = 1 << 20;

#[derive(Clone)]
pub struct VibeStore {
    core: Arc<Core>,
    blocker: Blocker,
}

impl fmt::Debug for VibeStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VibeStore")
            .field("root", &self.core.spec().root)
            .field("index", &self.core.spec().index)
            .finish()
    }
}

impl fmt::Display for VibeStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VibeStore({})", self.core.spec().root.display())
    }
}

async fn run_blocking<T: Send + 'static>(
    blocker: &Blocker,
    job: impl FnOnce() -> T + Send + 'static,
) -> object_store::Result<T> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    blocker(Box::new(move || {
        let _ = tx.send(job());
    }));
    rx.await
        .map_err(|_| generic("the blocking executor dropped a vibestore job"))
}

fn generic(message: impl Into<String>) -> object_store::Error {
    object_store::Error::Generic {
        store: STORE,
        source: Box::new(std::io::Error::other(message.into())),
    }
}

impl VibeStore {
    pub fn open(spec: &VibeSpec, blocker: Blocker) -> Result<Self> {
        Ok(Self {
            core: Arc::new(Core::open(spec)?),
            blocker,
        })
    }

    pub fn core(&self) -> &Arc<Core> {
        &self.core
    }

    pub fn spec(&self) -> &VibeSpec {
        self.core.spec()
    }

    async fn run<T, E>(
        &self,
        job: impl FnOnce(&Core) -> std::result::Result<T, E> + Send + 'static,
    ) -> object_store::Result<T>
    where
        T: Send + 'static,
        E: Into<object_store::Error> + Send + 'static,
    {
        let core = self.core.clone();
        run_blocking(&self.blocker, move || job(&core).map_err(Into::into)).await?
    }
}

// ---- conversions --------------------------------------------------------

fn put_mode(mode: PutMode) -> Result<Mode> {
    Ok(match mode {
        PutMode::Overwrite => Mode::Overwrite,
        PutMode::Create => Mode::Create,
        PutMode::Update(version) => match (version.e_tag, version.version) {
            (Some(tag), _) => Mode::Update(tag),
            (None, _) => {
                return Err(StoreError::NotSupported(
                    "a conditional update by object version: vibestore's tokens are ETags".into(),
                ))
            }
        },
    })
}

fn object_meta(row: &Row) -> Result<ObjectMeta> {
    let modified = SystemTime::UNIX_EPOCH + Duration::from_millis(row.modified_ms.max(0) as u64);
    Ok(ObjectMeta {
        location: stored_location(&row.key)?,
        last_modified: modified.into(),
        size: row.size,
        e_tag: Some(row.etag.to_string()),
        version: None,
    })
}

/// The `Path` a stored key came from. Keys are stored as `Path::as_ref()`,
/// already percent-encoded, so this parses that form back rather than
/// encoding it a second time (denoland/celld#232).
fn stored_location(key: &str) -> Result<Path> {
    Path::parse(key).map_err(|error| StoreError::Corrupt(format!("stored key {key:?}: {error}")))
}

#[derive(Serialize, Deserialize)]
struct StoredAttribute {
    kind: String,
    name: Option<String>,
    value: String,
}

fn encode_attributes(attributes: &Attributes) -> Result<String> {
    let mut stored = Vec::with_capacity(attributes.len());
    for (attribute, value) in attributes {
        let (kind, name) = match attribute {
            Attribute::ContentDisposition => ("content-disposition", None),
            Attribute::ContentEncoding => ("content-encoding", None),
            Attribute::ContentLanguage => ("content-language", None),
            Attribute::ContentType => ("content-type", None),
            Attribute::CacheControl => ("cache-control", None),
            Attribute::StorageClass => ("storage-class", None),
            Attribute::Metadata(name) => ("metadata", Some(name.as_ref().to_string())),
            other => {
                return Err(StoreError::NotSupported(format!(
                    "the object attribute {other:?}"
                )))
            }
        };
        stored.push(StoredAttribute {
            kind: kind.to_string(),
            name,
            value: value.as_ref().to_string(),
        });
    }
    serde_json::to_string(&stored).map_err(|error| StoreError::other("encode attributes", error))
}

fn decode_attributes(encoded: &str) -> Result<Attributes> {
    let stored: Vec<StoredAttribute> = serde_json::from_str(encoded)
        .map_err(|error| StoreError::Corrupt(format!("stored attributes: {error}")))?;
    let mut attributes = Attributes::with_capacity(stored.len());
    for stored in stored {
        let attribute = match stored.kind.as_str() {
            "content-disposition" => Attribute::ContentDisposition,
            "content-encoding" => Attribute::ContentEncoding,
            "content-language" => Attribute::ContentLanguage,
            "content-type" => Attribute::ContentType,
            "cache-control" => Attribute::CacheControl,
            "storage-class" => Attribute::StorageClass,
            "metadata" => Attribute::Metadata(stored.name.unwrap_or_default().into()),
            kind => {
                return Err(StoreError::Corrupt(format!(
                    "unknown stored attribute {kind:?}"
                )))
            }
        };
        attributes.insert(attribute, AttributeValue::from(stored.value));
    }
    Ok(attributes)
}

fn reject_tags(tags: &object_store::TagSet) -> Result<()> {
    if tags.encoded().is_empty() {
        Ok(())
    } else {
        Err(StoreError::NotSupported("object tags".into()))
    }
}

/// The smallest string greater than every string that starts with `prefix`,
/// or `None` when there is none. Keys compare as UTF-8 bytes in SQLite's
/// default collation, which orders the same as Unicode scalar values.
pub fn prefix_upper(prefix: &str) -> Option<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    while let Some(last) = chars.pop() {
        let mut next = last as u32 + 1;
        if (0xD800..=0xDFFF).contains(&next) {
            next = 0xE000;
        }
        if let Some(next) = char::from_u32(next) {
            chars.push(next);
            return Some(chars.into_iter().collect());
        }
    }
    None
}

/// The scan bounds for "keys strictly below the directory `prefix`".
fn dir_bounds(prefix: Option<&Path>) -> (String, Option<String>) {
    match prefix {
        Some(path) if !path.as_ref().is_empty() => {
            let lo = format!("{}/", path.as_ref());
            let hi = prefix_upper(&lo);
            (lo, hi)
        }
        _ => (String::new(), None),
    }
}

// ---- ObjectStore --------------------------------------------------------

struct Prepared {
    meta: ObjectMeta,
    attributes: Attributes,
    range: Range<u64>,
    body: Body,
}

enum Body {
    Empty,
    Bytes(Bytes),
    File(Arc<std::fs::File>),
}

#[async_trait]
impl ObjectStore for VibeStore {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        options: PutOptions,
    ) -> object_store::Result<PutResult> {
        reject_tags(&options.tags)?;
        let key = location.to_string();
        let attributes = encode_attributes(&options.attributes)?;
        let mode = put_mode(options.mode)?;
        let committed = self
            .run(move |core| core.put(&key, &payload, &mode, &attributes))
            .await?;
        Ok(PutResult {
            e_tag: Some(committed.etag.to_string()),
            version: None,
        })
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        options: PutMultipartOptions,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        reject_tags(&options.tags)?;
        let attributes = encode_attributes(&options.attributes)?;
        let staged = self.run(|core| core.stage()).await?;
        let writer = Arc::new(
            staged
                .file
                .try_clone()
                .map_err(|error| generic(format!("share the staged upload: {error}")))?,
        );
        Ok(Box::new(VibeUpload {
            store: self.clone(),
            key: location.to_string(),
            attributes,
            staged: Some(staged),
            writer,
            size: 0,
            in_flight: Arc::new(AtomicUsize::new(0)),
            failure: Arc::new(Mutex::new(None)),
        }))
    }

    async fn get_opts(
        &self,
        location: &Path,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        if options.version.is_some() {
            return Err(StoreError::NotSupported("reading a specific object version".into()).into());
        }
        let key = location.to_string();
        let prepared = self
            .run(move |core| -> object_store::Result<Prepared> {
                let opened = core
                    .open_object(&key)?
                    .ok_or_else(|| StoreError::NotFound(key.clone()))?;
                let meta = object_meta(&opened.row)?;
                options.check_preconditions(&meta)?;
                let attributes = decode_attributes(&opened.row.attributes)?;
                let size = opened.row.size;
                if options.head {
                    return Ok(Prepared {
                        meta,
                        attributes,
                        range: 0..size,
                        body: Body::Empty,
                    });
                }
                let range = match &options.range {
                    Some(range) => range
                        .as_range(size)
                        .map_err(|error| StoreError::Other(error.to_string()))?,
                    None => 0..size,
                };
                let body = match opened.source {
                    Source::Inline(bytes) => {
                        Body::Bytes(bytes.slice(range.start as usize..range.end as usize))
                    }
                    Source::Blob(file) if range.end - range.start <= READ_CHUNK => {
                        let mut buffer = vec![0u8; (range.end - range.start) as usize];
                        core::read_exact_at(&file, &mut buffer, range.start).map_err(|error| {
                            StoreError::other(format!("read blob of {key}"), error)
                        })?;
                        Body::Bytes(Bytes::from(buffer))
                    }
                    Source::Blob(file) => Body::File(Arc::new(file)),
                };
                Ok(Prepared {
                    meta,
                    attributes,
                    range,
                    body,
                })
            })
            .await?;
        let payload = match prepared.body {
            Body::Empty => stream::empty().boxed(),
            Body::Bytes(bytes) => stream::once(async move { Ok(bytes) }).boxed(),
            Body::File(file) => file_stream(self.blocker.clone(), file, prepared.range.clone()),
        };
        Ok(GetResult {
            payload: GetResultPayload::Stream(payload),
            meta: prepared.meta,
            range: prepared.range,
            attributes: prepared.attributes,
        })
    }

    async fn delete(&self, location: &Path) -> object_store::Result<()> {
        let key = location.to_string();
        self.run(move |core| core.delete(&key)).await
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.list_stream(prefix, None)
    }

    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        self.list_stream(prefix, Some(offset.to_string()))
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> object_store::Result<ListResult> {
        let (lo, hi) = dir_bounds(prefix);
        self.run(move |core| -> Result<ListResult> {
            let mut common = std::collections::BTreeSet::new();
            let mut objects = Vec::new();
            let mut scan = Scan {
                lo: lo.clone(),
                after: None,
                hi,
            };
            'restart: loop {
                let rows = core.scan(&scan, LIST_PAGE)?;
                let full = rows.len() == LIST_PAGE;
                let mut last = None;
                for row in rows {
                    let remainder = &row.key[lo.len()..];
                    match remainder.find('/') {
                        Some(at) => {
                            let child = format!("{lo}{}", &remainder[..at]);
                            common.insert(stored_location(&child)?);
                            // Everything below `child/` belongs to it: jump past.
                            match prefix_upper(&format!("{child}/")) {
                                Some(past) if scan.hi.as_deref().is_none_or(|hi| past.as_str() < hi) => {
                                    scan.lo = past;
                                    scan.after = None;
                                    continue 'restart;
                                }
                                _ => break 'restart,
                            }
                        }
                        None => {
                            last = Some(row.key.clone());
                            objects.push(object_meta(&row)?);
                        }
                    }
                }
                match (full, last) {
                    (true, Some(last)) => scan.after = Some(last),
                    _ => break,
                }
            }
            Ok(ListResult {
                common_prefixes: common.into_iter().collect(),
                objects,
            })
        })
        .await
    }

    async fn copy(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        let (from, to) = (from.to_string(), to.to_string());
        self.run(move |core| core.copy(&from, &to, &Mode::Overwrite).map(|_| ()))
            .await
    }

    async fn copy_if_not_exists(&self, from: &Path, to: &Path) -> object_store::Result<()> {
        let (from, to) = (from.to_string(), to.to_string());
        self.run(move |core| core.copy(&from, &to, &Mode::Create).map(|_| ()))
            .await
    }
}

impl VibeStore {
    fn list_stream(
        &self,
        prefix: Option<&Path>,
        offset: Option<String>,
    ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
        let (lo, hi) = dir_bounds(prefix);
        let store = self.clone();
        stream::try_unfold(
            Some(Scan {
                lo,
                after: offset,
                hi,
            }),
            move |scan| {
                let store = store.clone();
                async move {
                    let Some(scan) = scan else {
                        return Ok::<_, object_store::Error>(None);
                    };
                    let page_scan = scan.clone();
                    let rows = store.run(move |core| core.scan(&page_scan, LIST_PAGE)).await?;
                    let next = match rows.last() {
                        Some(last) if rows.len() == LIST_PAGE => Some(Scan {
                            after: Some(last.key.clone()),
                            ..scan
                        }),
                        _ => None,
                    };
                    let metas = rows
                        .iter()
                        .map(object_meta)
                        .collect::<Result<Vec<_>>>()?;
                    Ok(Some((metas, next)))
                }
            },
        )
        .map_ok(|metas| stream::iter(metas.into_iter().map(Ok)))
        .try_flatten()
        .boxed()
    }
}

/// A body too large for one read, streamed in `READ_CHUNK` pieces.
fn file_stream(
    blocker: Blocker,
    file: Arc<std::fs::File>,
    range: Range<u64>,
) -> BoxStream<'static, object_store::Result<Bytes>> {
    stream::unfold(Some(range.start), move |offset| {
        let blocker = blocker.clone();
        let file = file.clone();
        let end = range.end;
        async move {
            let offset = offset?;
            if offset >= end {
                return None;
            }
            let length = (end - offset).min(READ_CHUNK);
            let read = run_blocking(&blocker, move || {
                let mut buffer = vec![0u8; length as usize];
                core::read_exact_at(&file, &mut buffer, offset).map(|()| Bytes::from(buffer))
            })
            .await;
            match read {
                Ok(Ok(bytes)) => Some((Ok(bytes), Some(offset + length))),
                Ok(Err(error)) => Some((Err(generic(format!("read blob: {error}"))), None)),
                Err(error) => Some((Err(error), None)),
            }
        }
    })
    .boxed()
}

// ---- PaginatedListStore -------------------------------------------------

/// One entry of a delimiter listing: an object, or a rolled-up prefix with the
/// bounds of the keys beneath it.
enum Entry {
    Object(ObjectMeta),
    Prefix {
        path: Path,
        common: String,
        upper: Option<String>,
    },
}

#[async_trait]
impl PaginatedListStore for VibeStore {
    /// S3 `ListObjectsV2` semantics, as `LocalStore` implements them: `prefix`
    /// is a raw string prefix; `offset` and `page_token` both mean "keys
    /// strictly after"; with a delimiter, the run of a key between the end of
    /// `prefix` and the delimiter stands for every key sharing it and counts
    /// once against `max_keys`; the token names the last key consumed, so the
    /// next page resumes after every key under a rolled-up prefix.
    async fn list_paginated(
        &self,
        prefix: Option<&str>,
        options: PaginatedListOptions,
    ) -> object_store::Result<PaginatedListResult> {
        let prefix = prefix.unwrap_or_default().to_string();
        let after = options.page_token.or(options.offset);
        let delimiter = options
            .delimiter
            .map(|delimiter| delimiter.into_owned())
            .filter(|delimiter| !delimiter.is_empty());
        let limit = options.max_keys.unwrap_or(usize::MAX);
        // One entry past the limit says whether the listing is truncated.
        let want = limit.saturating_add(1);
        self.run(move |core| -> Result<PaginatedListResult> {
            let mut scan = Scan {
                hi: prefix_upper(&prefix),
                lo: prefix.clone(),
                after,
            };
            let mut entries: Vec<Entry> = Vec::new();
            'scan: loop {
                let rows = core.scan(&scan, LIST_PAGE)?;
                let full = rows.len() == LIST_PAGE;
                let mut last = None;
                for row in rows {
                    if entries.len() == want {
                        break 'scan;
                    }
                    let remainder = &row.key[prefix.len()..];
                    let rolled = delimiter
                        .as_deref()
                        .and_then(|d| remainder.find(d).map(|at| at + d.len()));
                    match rolled {
                        Some(end) => {
                            let common = format!("{prefix}{}", &remainder[..end]);
                            let upper = prefix_upper(&common);
                            entries.push(Entry::Prefix {
                                path: stored_location(&common)?,
                                common,
                                upper: upper.clone(),
                            });
                            // Resume past every key under it.
                            match upper {
                                Some(past)
                                    if scan.hi.as_deref().is_none_or(|hi| past.as_str() < hi) =>
                                {
                                    scan.lo = past;
                                    scan.after = None;
                                    continue 'scan;
                                }
                                _ => break 'scan,
                            }
                        }
                        None => {
                            last = Some(row.key.clone());
                            entries.push(Entry::Object(object_meta(&row)?));
                        }
                    }
                }
                match (full, last) {
                    (true, Some(last)) => scan.after = Some(last),
                    _ => break,
                }
            }
            let truncated = entries.len() > limit;
            entries.truncate(limit);
            let page_token = match (truncated, entries.last()) {
                (true, Some(Entry::Object(meta))) => Some(meta.location.to_string()),
                (true, Some(Entry::Prefix { common, upper, .. })) => {
                    // The highest key under the prefix, so the next page starts after it.
                    Some(
                        core.index()
                            .max_key(common, upper.as_deref())?
                            .unwrap_or_else(|| common.clone()),
                    )
                }
                _ => None,
            };
            let mut result = ListResult {
                common_prefixes: Vec::new(),
                objects: Vec::new(),
            };
            for entry in entries {
                match entry {
                    Entry::Object(meta) => result.objects.push(meta),
                    Entry::Prefix { path, .. } => result.common_prefixes.push(path),
                }
            }
            Ok(PaginatedListResult { result, page_token })
        })
        .await
    }
}

// ---- multipart ----------------------------------------------------------

/// A multipart upload. Parts are written at their final offsets into one
/// private temp file as they arrive, in any completion order, so a large
/// object is never held in memory and completion is one rename-free publish.
struct VibeUpload {
    store: VibeStore,
    key: String,
    attributes: String,
    staged: Option<crate::cas::Staging>,
    writer: Arc<std::fs::File>,
    /// Total bytes promised so far: the next part's offset.
    size: u64,
    in_flight: Arc<AtomicUsize>,
    failure: Arc<Mutex<Option<String>>>,
}

impl fmt::Debug for VibeUpload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VibeUpload")
            .field("key", &self.key)
            .field("size", &self.size)
            .finish()
    }
}

/// Counts a part as in flight until its future finishes or is dropped. A part
/// future dropped before it finished is a failed part: its range is a hole.
struct PartGuard {
    in_flight: Arc<AtomicUsize>,
    failure: Arc<Mutex<Option<String>>>,
    finished: bool,
}

impl Drop for PartGuard {
    fn drop(&mut self) {
        if !self.finished {
            let mut failure = self.failure.lock().unwrap_or_else(|p| p.into_inner());
            failure.get_or_insert_with(|| "a part upload was abandoned".to_string());
        }
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

#[async_trait]
impl MultipartUpload for VibeUpload {
    fn put_part(&mut self, data: PutPayload) -> UploadPart {
        let offset = self.size;
        self.size += data.content_length() as u64;
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        let mut guard = PartGuard {
            in_flight: self.in_flight.clone(),
            failure: self.failure.clone(),
            finished: false,
        };
        let file = self.writer.clone();
        let blocker = self.store.blocker.clone();
        Box::pin(async move {
            let written = run_blocking(&blocker, move || {
                let mut at = offset;
                for chunk in &data {
                    core::write_all_at(&file, chunk, at)?;
                    at += chunk.len() as u64;
                }
                Ok::<_, std::io::Error>(())
            })
            .await;
            guard.finished = true;
            match written {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => {
                    let message = format!("write multipart part: {error}");
                    guard
                        .failure
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .get_or_insert(message.clone());
                    Err(generic(message))
                }
                Err(error) => Err(error),
            }
        })
    }

    async fn complete(&mut self) -> object_store::Result<PutResult> {
        if self.in_flight.load(Ordering::SeqCst) != 0 {
            return Err(generic(
                "multipart upload completed while parts were still being written",
            ));
        }
        if let Some(message) = self
            .failure
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            return Err(generic(message));
        }
        let staged = self
            .staged
            .take()
            .ok_or_else(|| generic("multipart upload already completed or aborted"))?;
        let (key, attributes, size) = (self.key.clone(), self.attributes.clone(), self.size);
        let committed = self
            .store
            .run(move |core| core.put_staged(&key, staged, size, &Mode::Overwrite, &attributes))
            .await?;
        Ok(PutResult {
            e_tag: Some(committed.etag.to_string()),
            version: None,
        })
    }

    async fn abort(&mut self) -> object_store::Result<()> {
        // Dropping the staging file removes it.
        self.staged = None;
        Ok(())
    }
}
