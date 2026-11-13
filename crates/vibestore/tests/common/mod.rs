#![allow(dead_code)]

use bytes::Bytes;
use celld_vibestore::cas::Cas;
use celld_vibestore::{tokio_blocker, VibeSpec, VibeStore};
use object_store::path::Path;
use object_store::{ObjectStore, PutPayload};
use std::path::{Path as FsPath, PathBuf};
use std::time::Duration;

/// A store whose objects of 9 bytes or more live in the CAS and smaller ones
/// in the index, so a test controls where its bytes go by their length.
pub const INLINE_MAX: u64 = 8;

pub struct Env {
    pub dir: tempfile::TempDir,
}

impl Env {
    pub fn new() -> Env {
        Env {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    pub fn root(&self) -> PathBuf {
        self.dir.path().join("blobs")
    }

    pub fn spec(&self) -> VibeSpec {
        let mut spec = VibeSpec::new(self.root());
        spec.index = self.dir.path().join("index.sqlite");
        spec.inline_max = INLINE_MAX;
        spec.namespace_id = "test".into();
        spec
    }

    /// An independent store instance: another "process" over the same files.
    pub fn open(&self) -> VibeStore {
        VibeStore::open(&self.spec(), tokio_blocker()).unwrap()
    }

    pub fn open_with(&self, edit: impl FnOnce(&mut VibeSpec)) -> VibeStore {
        let mut spec = self.spec();
        edit(&mut spec);
        VibeStore::open(&spec, tokio_blocker()).unwrap()
    }
}

pub fn path(key: &str) -> Path {
    Path::from(key)
}

/// A payload of `len` bytes that is distinct per `tag`.
pub fn payload(tag: u8, len: usize) -> Bytes {
    Bytes::from((0..len).map(|i| tag.wrapping_add(i as u8)).collect::<Vec<u8>>())
}

pub async fn put(store: &VibeStore, key: &str, body: impl Into<PutPayload>) -> String {
    store
        .put(&path(key), body.into())
        .await
        .unwrap()
        .e_tag
        .unwrap()
}

pub async fn get(store: &VibeStore, key: &str) -> Bytes {
    store.get(&path(key)).await.unwrap().bytes().await.unwrap()
}

/// What Vibestudio's blob GC does (`sweepUnreachableBlobs` with the reachable
/// set `retainedBlobDigests`): unlink every blob whose digest no retention row
/// names and that has been quiet for `min_age`. Returns the digests removed.
pub fn vibestudio_gc(root: &FsPath, min_age: Duration) -> Vec<String> {
    let conn = rusqlite::Connection::open(root.join("retentions.db")).unwrap();
    let mut statement = conn
        .prepare("SELECT DISTINCT digest FROM blob_retentions")
        .unwrap();
    let reachable: std::collections::BTreeSet<String> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let cas = Cas::open(root).unwrap();
    let mut removed = Vec::new();
    for (digest, file) in cas.addresses().unwrap() {
        if reachable.contains(&digest) {
            continue;
        }
        if min_age > Duration::ZERO && cas.age(&file).is_some_and(|age| age < min_age) {
            continue;
        }
        std::fs::remove_file(file).unwrap();
        removed.push(digest);
    }
    removed.sort();
    removed
}

pub fn retention_owners(root: &FsPath) -> Vec<(String, String)> {
    let conn = rusqlite::Connection::open(root.join("retentions.db")).unwrap();
    let mut statement = conn
        .prepare("SELECT owner, digest FROM blob_retentions ORDER BY owner")
        .unwrap();
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    rows
}

pub fn blob_digests(root: &FsPath) -> Vec<String> {
    let mut digests: Vec<String> = Cas::open(root)
        .unwrap()
        .addresses()
        .unwrap()
        .into_iter()
        .map(|(digest, _)| digest)
        .collect();
    digests.sort();
    digests
}
