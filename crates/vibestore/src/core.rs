//! The synchronous heart of the store: blob root + key index + retentions,
//! and the order in which they are touched.
//!
//! # Crash safety
//!
//! A write of a blob-backed object takes these steps, in this order:
//!
//! 1. the bytes are written to a temp file, fsynced, hard-linked to their
//!    address and the directories fsynced (`Cas::install`);
//! 2. a retention row `<key>@<digest>` is committed to `retentions.db`;
//! 3. the index transaction commits: the precondition is checked and the key
//!    now names the digest (this is the commit point of the write);
//! 4. the retention row of the digest this key held before is released.
//!
//! A process that dies after step 1 or 2 leaves an unreferenced blob and, at
//! most, a retention row no key points at. A death after 3 leaves a stale
//! retention row for the previous digest. Nothing a live key points at is ever
//! unprotected or missing, so recovery needs no replay: `Core::recover` only
//! reconciles the retention rows with the index, removes stale temp files and,
//! when celld owns the root, unlinks unreferenced blobs older than the grace
//! period. A small object skips steps 1, 2 and 4: its bytes are in the index
//! row, so the one transaction is the whole write.

#![allow(clippy::disallowed_methods)]

use crate::cas::{self, Cas, Staging};
use crate::error::{Result, StoreError};
use crate::index::{Committed, Index, Location, Mode, Row, Scan};
use crate::retention::{self, Retentions};
use crate::spec::VibeSpec;
use bytes::Bytes;
use object_store::PutPayload;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::fs::File;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// Where a simulated crash stops a write. Test-only in effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashPoint {
    /// The blob is durable at its address; nothing else has happened.
    AfterBlobInstalled,
    /// The retention row is committed; the index has not been.
    AfterRetained,
    /// The index has committed; the old retention row has not been released.
    AfterIndexCommit,
}

impl CrashPoint {
    fn name(self) -> &'static str {
        match self {
            CrashPoint::AfterBlobInstalled => "after the blob was installed",
            CrashPoint::AfterRetained => "after the retention row was committed",
            CrashPoint::AfterIndexCommit => "after the index committed",
        }
    }
}

/// What [`Core::recover`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Recovery {
    pub stale_staging_removed: usize,
    pub retentions_added: usize,
    pub retentions_released: usize,
    pub orphan_blobs_removed: usize,
}

pub enum Source {
    Inline(Bytes),
    Blob(File),
}

pub struct Opened {
    pub row: Row,
    pub source: Source,
}

#[derive(Debug)]
pub struct Core {
    spec: VibeSpec,
    cas: Cas,
    index: Index,
    retentions: Option<Retentions>,
    crash: Mutex<Option<CrashPoint>>,
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

impl Core {
    pub fn open(spec: &VibeSpec) -> Result<Core> {
        let cas = Cas::open(&spec.root)?;
        let index = Index::open(&spec.index)?;
        let retentions = spec
            .shared
            .then(|| Retentions::open(&spec.root, &spec.retention_namespace()))
            .transpose()?;
        let core = Core {
            spec: spec.clone(),
            cas,
            index,
            retentions,
            crash: Mutex::new(None),
        };
        let recovery = core.recover()?;
        if recovery != Recovery::default() {
            tracing::info!(?recovery, root = %spec.root.display(), "vibestore recovered");
        }
        Ok(core)
    }

    pub fn spec(&self) -> &VibeSpec {
        &self.spec
    }

    pub fn index(&self) -> &Index {
        &self.index
    }

    pub fn cas(&self) -> &Cas {
        &self.cas
    }

    pub fn retentions(&self) -> Option<&Retentions> {
        self.retentions.as_ref()
    }

    #[doc(hidden)]
    pub fn set_crash_point(&self, point: Option<CrashPoint>) {
        *self.crash.lock().unwrap() = point;
    }

    fn crash(&self, point: CrashPoint) -> Result<()> {
        if *self.crash.lock().unwrap() == Some(point) {
            return Err(StoreError::Crashed(point.name()));
        }
        Ok(())
    }

    /// The orphan cleanup that replaces a repair pass. Safe to run at any time,
    /// from any process: it only reconciles derived state with the index.
    pub fn recover(&self) -> Result<Recovery> {
        let mut recovery = Recovery {
            // A temp file has no authority; a day is longer than any upload.
            stale_staging_removed: self.cas.sweep_staging(Duration::from_secs(24 * 3600)),
            ..Recovery::default()
        };
        if let Some(retentions) = &self.retentions {
            // Under the index write lock: a writer in another process takes
            // its retention row inside that lock too, so a row this pass sees
            // as stale cannot belong to a write that is about to commit.
            let (added, released) = self.index.with_blob_keys_locked(|keys| {
                let live: BTreeMap<String, String> = keys
                    .into_iter()
                    .map(|(key, digest)| (retention::owner(&key, &digest), digest))
                    .collect();
                retentions.reconcile(&live)
            })?;
            recovery.retentions_added = added;
            recovery.retentions_released = released;
        } else {
            recovery.orphan_blobs_removed = self.sweep_orphans(self.spec.grace)?;
        }
        Ok(recovery)
    }

    /// Unlink every blob no key points at that has been quiet for `grace`.
    /// Only for a root celld owns alone; a shared root is collected by the host.
    pub fn sweep_orphans(&self, grace: Duration) -> Result<usize> {
        let mut removed = 0;
        for (digest, path) in self.cas.addresses()? {
            if self.index.digest_referenced(&digest)? {
                continue;
            }
            if self.cas.age(&path).is_some_and(|age| age >= grace) {
                self.cas.unlink(&path)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Re-hash every blob a key points at. Returns the problems found.
    pub fn verify(&self) -> Result<Vec<String>> {
        let mut problems = Vec::new();
        for (digest, size) in self.index.live_digests()? {
            match self.cas.verify_existing(&digest, size) {
                Ok(true) => {}
                Ok(false) => problems.push(format!("{digest}: missing")),
                Err(error) => problems.push(format!("{digest}: {error}")),
            }
        }
        Ok(problems)
    }

    // ---- writes ----------------------------------------------------------

    pub fn put(
        &self,
        key: &str,
        payload: &PutPayload,
        mode: &Mode,
        attributes: &str,
    ) -> Result<Committed> {
        let size = payload.content_length() as u64;
        let location = if size <= self.spec.inline_max {
            let mut body = Vec::with_capacity(size as usize);
            for chunk in payload {
                body.extend_from_slice(chunk);
            }
            Location::Inline(body)
        } else {
            let mut hasher = Sha256::new();
            for chunk in payload {
                hasher.update(chunk);
            }
            let digest = hex::encode(hasher.finalize());
            if !self.cas.verify_existing(&digest, size)? {
                let mut staged = self.cas.stage()?;
                cas::fill(&mut staged, payload.iter())?;
                self.cas.install(staged, &digest, size)?;
            }
            Location::Blob(digest)
        };
        self.publish(key, location, size, attributes, mode)
    }

    /// Finish a multipart upload whose parts were written to `staged`.
    pub fn put_staged(
        &self,
        key: &str,
        staged: Staging,
        size: u64,
        mode: &Mode,
        attributes: &str,
    ) -> Result<Committed> {
        staged
            .file
            .sync_all()
            .map_err(|error| StoreError::other("fsync staged upload", error))?;
        let observed = staged
            .file
            .metadata()
            .map_err(|error| StoreError::other("stat staged upload", error))?
            .len();
        if observed != size {
            return Err(StoreError::Other(format!(
                "multipart upload of {key} holds {observed} bytes, its parts add up to {size}"
            )));
        }
        let location = if size <= self.spec.inline_max {
            let body = std::fs::read(&staged.path)
                .map_err(|error| StoreError::other("read staged upload", error))?;
            Location::Inline(body)
        } else {
            let digest = cas::sha256_file(&staged.path)?;
            self.cas.install(staged, &digest, size)?;
            Location::Blob(digest)
        };
        self.publish(key, location, size, attributes, mode)
    }

    pub fn stage(&self) -> Result<Staging> {
        self.cas.stage()
    }

    fn publish(
        &self,
        key: &str,
        location: Location,
        size: u64,
        attributes: &str,
        mode: &Mode,
    ) -> Result<Committed> {
        let digest = location.digest().map(str::to_owned);
        if digest.is_some() {
            self.crash(CrashPoint::AfterBlobInstalled)?;
        }
        // Inside the index transaction, once the precondition has passed: a
        // rejected write takes no retention row, so there is nothing to undo.
        let mut retain = || -> Result<()> {
            if let Some(digest) = &digest {
                if let Some(retentions) = &self.retentions {
                    retentions.retain(&retention::owner(key, digest), digest)?;
                }
                self.crash(CrashPoint::AfterRetained)?;
            }
            Ok(())
        };
        let committed = self
            .index
            .commit_put(key, &location, size, attributes, mode, now_ms(), &mut retain)?;
        self.crash(CrashPoint::AfterIndexCommit)?;
        self.release_replaced(key, digest.as_deref(), committed.replaced.as_deref());
        Ok(committed)
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        let old = self.index.commit_delete(key)?;
        self.crash(CrashPoint::AfterIndexCommit)?;
        self.release_replaced(key, None, old.as_deref());
        Ok(())
    }

    pub fn copy(&self, from: &str, to: &str, mode: &Mode) -> Result<Committed> {
        let mut retained = None;
        let mut retain = |digest: Option<&str>| -> Result<()> {
            retained = digest.map(str::to_owned);
            if let (Some(retentions), Some(digest)) = (&self.retentions, digest) {
                retentions.retain(&retention::owner(to, digest), digest)?;
            }
            Ok(())
        };
        let committed = self
            .index
            .commit_copy(from, to, mode, now_ms(), &mut retain)?;
        self.release_replaced(to, retained.as_deref(), committed.replaced.as_deref());
        Ok(committed)
    }

    fn release_replaced(&self, key: &str, new: Option<&str>, old: Option<&str>) {
        let (Some(retentions), Some(old)) = (&self.retentions, old) else {
            return;
        };
        if new == Some(old) {
            return;
        }
        if let Err(error) = retentions.release(&retention::owner(key, old)) {
            // Harmless: the row only delays a collection, and the next open
            // reconciles it away.
            tracing::warn!(%error, key, "vibestore could not release a retention");
        }
    }

    // ---- reads -----------------------------------------------------------

    pub fn open_object(&self, key: &str) -> Result<Option<Opened>> {
        let Some((row, body)) = self.index.get_with_body(key)? else {
            return Ok(None);
        };
        let source = match (&row.digest, body) {
            (Some(digest), _) => Source::Blob(self.cas.open_blob(digest, row.size)?),
            (None, Some(body)) => Source::Inline(Bytes::from(body)),
            (None, None) => {
                return Err(StoreError::Corrupt(format!(
                    "row {key} has neither a body nor a digest"
                )))
            }
        };
        Ok(Some(Opened { row, source }))
    }

    pub fn scan(&self, scan: &Scan, limit: usize) -> Result<Vec<Row>> {
        self.index.scan(scan, limit)
    }

    /// Flush the index so a copy of its file is a complete backup.
    pub fn close(&self) {
        if let Err(error) = self.index.checkpoint() {
            tracing::warn!(%error, "vibestore could not checkpoint its index");
        }
    }
}

/// Read exactly `buf.len()` bytes at `offset`.
pub fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt as _;
        file.read_exact_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt as _;
        let mut done = 0;
        while done < buf.len() {
            let read = file.seek_read(&mut buf[done..], offset + done as u64)?;
            if read == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            done += read;
        }
        Ok(())
    }
}

/// Write all of `data` at `offset`.
pub fn write_all_at(file: &File, data: &[u8], offset: u64) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt as _;
        file.write_all_at(data, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt as _;
        let mut done = 0;
        while done < data.len() {
            let wrote = file.seek_write(&data[done..], offset + done as u64)?;
            done += wrote;
        }
        Ok(())
    }
}
