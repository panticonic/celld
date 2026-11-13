//! The content-addressed blob root, byte-compatible with Vibestudio's
//! `src/server/storage/blobCas.ts`.
//!
//! * Address: lowercase hex SHA-256 of the object's bytes.
//! * Layout: `<root>/sha256/<d[0..2]>/<d[2..4]>/<d[4..]>`, mode 0600.
//! * Publication: write a private temp file, fsync it, `link(2)` it to its
//!   address (an existing address means identical bytes, verified), fsync the
//!   directories it created, then unlink the temp. A blob is therefore never
//!   visible half-written, and never named before its bytes are durable.
//!
//! One deliberate difference: temp files live in `<root>/tmp-celld/`, not
//! `<root>/tmp/`. Vibestudio's blobstore deletes everything in `tmp/` when it
//! starts, which would kill an in-flight celld write.

#![allow(clippy::disallowed_methods)]

use crate::error::{Result, StoreError};
use sha2::{Digest as _, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

const TMP: &str = "tmp-celld";
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

pub fn is_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[derive(Debug)]
pub struct Cas {
    root: PathBuf,
    tmp: PathBuf,
}

/// A temp file being filled. Dropping it without [`Cas::install`] removes it.
pub struct Staging {
    pub path: PathBuf,
    pub file: File,
    /// Whether dropping removes the temp file.
    armed: bool,
}

impl Staging {
    /// Remove the temp file now; its link at the address, if any, remains.
    fn release(mut self) {
        self.armed = false;
        let _ = fs::remove_file(&self.path);
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Cas {
    /// Create the layout Vibestudio's `ensureBlobCasLayout` creates (`tmp/`,
    /// `sha256/`), durably, plus celld's own temp directory.
    pub fn open(root: &Path) -> Result<Cas> {
        let root = root.to_path_buf();
        for dir in [root.join("tmp"), root.join("sha256"), root.join(TMP)] {
            ensure_durable_dir(&dir)
                .map_err(|error| StoreError::other(format!("create {}", dir.display()), error))?;
        }
        let tmp = root.join(TMP);
        Ok(Cas { root, tmp })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, digest: &str) -> Result<PathBuf> {
        if !is_digest(digest) {
            return Err(StoreError::Other(format!("invalid sha256 digest {digest:?}")));
        }
        Ok(self
            .root
            .join("sha256")
            .join(&digest[..2])
            .join(&digest[2..4])
            .join(&digest[4..]))
    }

    /// A fresh exclusive temp file: `<pid>-<nanos>-<n>.tmp`, mode 0600.
    pub fn stage(&self) -> Result<Staging> {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!(
            "{}-{nanos:x}-{}.tmp",
            std::process::id(),
            TMP_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let path = self.tmp.join(name);
        let mut options = OpenOptions::new();
        options.write(true).read(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options
            .open(&path)
            .map_err(|error| StoreError::other(format!("create {}", path.display()), error))?;
        Ok(Staging {
            path,
            file,
            armed: true,
        })
    }

    /// Whether the address is present with the right size and bytes. A hit
    /// refreshes the blob's age so a host GC's grace period restarts.
    pub fn verify_existing(&self, digest: &str, size: u64) -> Result<bool> {
        let path = self.path(digest)?;
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(StoreError::other(path.display(), error)),
        };
        if !meta.is_file() {
            return Err(StoreError::Corrupt(format!(
                "CAS object is not a regular file: {}",
                path.display()
            )));
        }
        if meta.len() != size {
            return Err(StoreError::Corrupt(format!(
                "CAS object size mismatch at {}: expected {size}, observed {}",
                path.display(),
                meta.len()
            )));
        }
        let observed = sha256_file(&path)?;
        if observed != digest {
            return Err(StoreError::Corrupt(format!(
                "CAS object digest mismatch at {}: observed {observed}",
                path.display()
            )));
        }
        self.touch(&path, &meta);
        Ok(true)
    }

    /// Restart the host GC's age clock for a blob we just relied on. Vibestudio
    /// sweeps only blobs quiescent for `minAgeMs`, measured from the newer of
    /// mtime and ctime. Best effort, and skipped for a blob touched recently.
    fn touch(&self, path: &Path, meta: &fs::Metadata) {
        let recent = meta
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age < Duration::from_secs(300));
        if recent {
            return;
        }
        if let Ok(file) = File::open(path) {
            let _ = file.set_modified(SystemTime::now());
        }
    }

    /// Publish a staged file at its address. The caller has hashed it.
    pub fn install(&self, staged: Staging, digest: &str, size: u64) -> Result<()> {
        let final_path = self.path(digest)?;
        staged
            .file
            .sync_all()
            .map_err(|error| StoreError::other("fsync staged blob", error))?;
        let final_dir = final_path.parent().expect("an address has a parent");
        let missing = missing_chain(final_dir)
            .map_err(|error| StoreError::other(final_dir.display(), error))?;
        fs::create_dir_all(final_dir)
            .map_err(|error| StoreError::other(format!("create {}", final_dir.display()), error))?;
        let mut raced = false;
        match fs::hard_link(&staged.path, &final_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => raced = true,
            Err(error) => {
                return Err(StoreError::other(
                    format!(
                        "publish {} (the blob root must support hard links)",
                        final_path.display()
                    ),
                    error,
                ))
            }
        }
        let staged_meta = staged
            .file
            .metadata()
            .map_err(|error| StoreError::other("stat staged blob", error))?;
        let published = fs::symlink_metadata(&final_path)
            .map_err(|error| StoreError::other(format!("verify {}", final_path.display()), error))?;
        if !published.is_file() || published.len() != size {
            return Err(StoreError::Corrupt(format!(
                "CAS object at {} is not a regular file of {size} bytes",
                final_path.display()
            )));
        }
        if !same_inode(&published, &staged_meta) {
            let observed = sha256_file(&final_path)?;
            if observed != digest {
                return Err(StoreError::Corrupt(format!(
                    "CAS object digest mismatch at {}: observed {observed}",
                    final_path.display()
                )));
            }
        }
        if raced {
            sync_chain(&self.root, final_dir)
                .map_err(|error| StoreError::other("fsync blob directories", error))?;
        } else {
            for dir in sync_order(final_dir, &missing) {
                sync_dir(&dir).map_err(|error| StoreError::other("fsync blob directory", error))?;
            }
        }
        staged.release();
        Ok(())
    }

    /// Open a blob for reading, checking only what is cheap: it is a regular
    /// file of the recorded size.
    pub fn open_blob(&self, digest: &str, size: u64) -> Result<File> {
        let path = self.path(digest)?;
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(StoreError::Corrupt(format!(
                    "the index names blob {digest} but {} is missing; it was deleted, or the \
                     blob root was swept without this store's retentions",
                    path.display()
                )))
            }
            Err(error) => return Err(StoreError::other(path.display(), error)),
        };
        let meta = file
            .metadata()
            .map_err(|error| StoreError::other(path.display(), error))?;
        if !meta.is_file() || meta.len() != size {
            return Err(StoreError::Corrupt(format!(
                "blob {digest} is {} bytes on disk, the index says {size}",
                meta.len()
            )));
        }
        Ok(file)
    }

    /// Every address on disk, with its path.
    pub fn addresses(&self) -> Result<Vec<(String, PathBuf)>> {
        let mut found = Vec::new();
        let shas = self.root.join("sha256");
        for first in read_dir_names(&shas)? {
            if first.len() != 2 {
                continue;
            }
            for second in read_dir_names(&shas.join(&first))? {
                if second.len() != 2 {
                    continue;
                }
                let dir = shas.join(&first).join(&second);
                for leaf in read_dir_names(&dir)? {
                    let digest = format!("{first}{second}{leaf}");
                    if is_digest(&digest) {
                        found.push((digest, dir.join(leaf)));
                    }
                }
            }
        }
        Ok(found)
    }

    /// Unlink one blob. Only exclusive mode calls this.
    pub fn unlink(&self, path: &Path) -> Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(StoreError::other(path.display(), error)),
        }
    }

    /// Remove temp files older than `age`. A temp file has no authority, so a
    /// stale one is only wasted space.
    pub fn sweep_staging(&self, age: Duration) -> usize {
        let mut removed = 0;
        let Ok(entries) = fs::read_dir(&self.tmp) else {
            return 0;
        };
        for entry in entries.flatten() {
            let old = entry
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok())
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .is_some_and(|elapsed| elapsed >= age);
            if old && fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        removed
    }

    /// Age of a blob by the host GC's measure: the newer of mtime and ctime.
    pub fn age(&self, path: &Path) -> Option<Duration> {
        let meta = fs::symlink_metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        #[cfg(unix)]
        let changed = {
            use std::os::unix::fs::MetadataExt as _;
            SystemTime::UNIX_EPOCH
                + Duration::new(meta.ctime().max(0) as u64, meta.ctime_nsec().max(0) as u32)
        };
        #[cfg(not(unix))]
        let changed = modified;
        SystemTime::now().duration_since(modified.max(changed)).ok()
    }
}

fn read_dir_names(dir: &Path) -> Result<Vec<String>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(StoreError::other(dir.display(), error)),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| StoreError::other(dir.display(), error))?;
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names)
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).map_err(|error| StoreError::other(path.display(), error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| StoreError::other(path.display(), error))?;
        if read == 0 {
            return Ok(hex::encode(hasher.finalize()));
        }
        hasher.update(&buffer[..read]);
    }
}

/// Write `bytes` to a staged file and flush it to the OS (not yet to disk).
pub fn fill(staged: &mut Staging, chunks: impl IntoIterator<Item = impl AsRef<[u8]>>) -> Result<()> {
    for chunk in chunks {
        staged
            .file
            .write_all(chunk.as_ref())
            .map_err(|error| StoreError::other("write staged blob", error))?;
    }
    Ok(())
}

fn same_inode(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        left.ino() != 0 && left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (left, right);
        false
    }
}

/// Directories that do not exist yet on the way to `dir`, deepest first.
fn missing_chain(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut missing = Vec::new();
    let mut cursor = dir.to_path_buf();
    loop {
        match fs::metadata(&cursor) {
            Ok(meta) if meta.is_dir() => return Ok(missing),
            Ok(_) => {
                return Err(std::io::Error::other(format!(
                    "CAS directory path is not a directory: {}",
                    cursor.display()
                )))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(cursor.clone());
                match cursor.parent() {
                    Some(parent) => cursor = parent.to_path_buf(),
                    None => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
    }
}

/// The directories to fsync after creating `missing` under `mutated`: the
/// leaf itself and the parent of every directory that was created.
fn sync_order(mutated: &Path, missing: &[PathBuf]) -> Vec<PathBuf> {
    let mut ordered = vec![mutated.to_path_buf()];
    for dir in missing {
        if let Some(parent) = dir.parent() {
            if !ordered.iter().any(|seen| seen == parent) {
                ordered.push(parent.to_path_buf());
            }
        }
    }
    ordered
}

/// fsync `leaf` and every directory up to `root`, then `root`'s parent.
fn sync_chain(root: &Path, leaf: &Path) -> std::io::Result<()> {
    let mut cursor = leaf.to_path_buf();
    loop {
        sync_dir(&cursor)?;
        if cursor == root {
            if let Some(parent) = root.parent() {
                sync_dir(parent)?;
            }
            return Ok(());
        }
        match cursor.parent() {
            Some(parent) => cursor = parent.to_path_buf(),
            None => return Ok(()),
        }
    }
}

pub fn ensure_durable_dir(dir: &Path) -> std::io::Result<()> {
    let missing = missing_chain(dir)?;
    fs::create_dir_all(dir)?;
    if let Some(leaf) = missing.first() {
        for dir in sync_order(leaf, &missing) {
            sync_dir(&dir)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
pub fn sync_dir(dir: &Path) -> std::io::Result<()> {
    File::open(dir)?.sync_all()
}

/// Windows cannot open a directory for flushing; NTFS journals the metadata.
#[cfg(not(unix))]
pub fn sync_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(cas: &Cas, bytes: &[u8]) -> String {
        let digest = sha256_hex(bytes);
        let mut staged = cas.stage().unwrap();
        fill(&mut staged, [bytes]).unwrap();
        cas.install(staged, &digest, bytes.len() as u64).unwrap();
        digest
    }

    #[test]
    fn layout_matches_vibestudio() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(dir.path()).unwrap();
        let digest = put(&cas, b"hello");
        // sha256("hello")
        assert_eq!(
            digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        let expected = dir
            .path()
            .join("sha256/2c/f2/4dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
        assert_eq!(fs::read(expected).unwrap(), b"hello");
        assert!(dir.path().join("tmp").is_dir(), "Vibestudio's tmp/ exists");
        assert_eq!(
            fs::read_dir(dir.path().join("tmp-celld")).unwrap().count(),
            0,
            "the temp file is gone after install"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(cas.path(&digest).unwrap()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn installing_an_existing_address_dedups_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(dir.path()).unwrap();
        let digest = put(&cas, b"same bytes");
        assert_eq!(put(&cas, b"same bytes"), digest);
        assert!(cas.verify_existing(&digest, 10).unwrap());
        assert!(!cas.verify_existing(&sha256_hex(b"absent"), 6).unwrap());
    }

    #[test]
    fn a_corrupt_blob_is_loud() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(dir.path()).unwrap();
        let digest = put(&cas, b"original");
        let path = cas.path(&digest).unwrap();
        fs::write(&path, b"tampered").unwrap();
        assert!(matches!(
            cas.verify_existing(&digest, 8),
            Err(StoreError::Corrupt(_))
        ));
        fs::write(&path, b"short").unwrap();
        assert!(matches!(
            cas.open_blob(&digest, 8),
            Err(StoreError::Corrupt(_))
        ));
    }

    #[test]
    fn a_missing_blob_is_corruption_not_absence() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(dir.path()).unwrap();
        let digest = sha256_hex(b"never stored");
        assert!(matches!(
            cas.open_blob(&digest, 12),
            Err(StoreError::Corrupt(_))
        ));
    }

    #[test]
    fn dropping_a_staging_file_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(dir.path()).unwrap();
        let staged = cas.stage().unwrap();
        let path = staged.path.clone();
        assert!(path.exists());
        drop(staged);
        assert!(!path.exists());
    }

    #[test]
    fn digests_are_validated() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(dir.path()).unwrap();
        assert!(cas.path("../../etc/passwd").is_err());
        assert!(cas.path(&"A".repeat(64)).is_err());
        assert!(cas.path(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn addresses_lists_what_is_stored() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(dir.path()).unwrap();
        let a = put(&cas, b"one");
        let b = put(&cas, b"two");
        let mut found: Vec<String> = cas.addresses().unwrap().into_iter().map(|(d, _)| d).collect();
        found.sort();
        let mut expected = vec![a, b];
        expected.sort();
        assert_eq!(found, expected);
    }
}
