//! The key index: one stock-format SQLite file that maps an object key to its
//! content address (or, for a small object, to its bytes), with a version tag.
//!
//! This is the only mutable state in the store and the only place a
//! conditional write is decided. Every write is one `BEGIN IMMEDIATE`
//! transaction that checks the precondition and installs the new row, so
//! concurrent writers, in this process or another, serialize on SQLite's lock
//! and exactly one of any set of racers on a version wins.
//!
//! The version tag (`etag`) is a store-wide monotonic counter. It is never
//! reused, even after a delete and a re-create, so a conditional update cannot
//! be fooled by an object that went away and came back (the ABA case an
//! etag made from content would allow).

#![allow(clippy::disallowed_methods)]

use crate::error::{Result, StoreError};
use rusqlite::{params, Connection, OptionalExtension as _, TransactionBehavior};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const USER_VERSION: i64 = 1;
const SCHEMA: &str = "
CREATE TABLE objects (
  key         TEXT PRIMARY KEY NOT NULL,
  etag        INTEGER NOT NULL,
  size        INTEGER NOT NULL,
  modified_ms INTEGER NOT NULL,
  digest      TEXT,
  body        BLOB,
  attributes  TEXT NOT NULL,
  CHECK ((digest IS NULL) <> (body IS NULL))
) WITHOUT ROWID;
CREATE INDEX objects_by_digest ON objects(digest) WHERE digest IS NOT NULL;
CREATE TABLE sequence (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  next      INTEGER NOT NULL
);
INSERT INTO sequence(singleton, next) VALUES (1, 1);
";

/// Where an object's bytes live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    /// In the index row itself. Small objects: leases, pointers, tiny segments.
    Inline(Vec<u8>),
    /// In the content-addressed blob root, by SHA-256 hex.
    Blob(String),
}

impl Location {
    pub fn digest(&self) -> Option<&str> {
        match self {
            Location::Blob(digest) => Some(digest),
            Location::Inline(_) => None,
        }
    }
}

/// An index row without its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub etag: i64,
    pub size: u64,
    pub modified_ms: i64,
    pub digest: Option<String>,
    pub attributes: String,
}

/// The write precondition, already reduced from `object_store::PutMode`.
#[derive(Clone, Debug)]
pub enum Mode {
    Overwrite,
    /// The key must be absent.
    Create,
    /// The key must be present at exactly this version tag.
    Update(String),
}

#[derive(Debug)]
pub struct Committed {
    pub etag: i64,
    /// The digest the key held before this write, when it was a blob.
    pub replaced: Option<String>,
}

/// Bounds for a key scan: `key >= lo`, `key > after`, `key < hi`.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    pub lo: String,
    pub after: Option<String>,
    pub hi: Option<String>,
}

#[derive(Debug)]
pub struct Index {
    path: PathBuf,
    pool: Mutex<Vec<Connection>>,
    /// Serializes this process's writers before they reach SQLite's lock, so
    /// a contended key waits on a mutex instead of spinning in the busy
    /// handler. Other processes still serialize on SQLite.
    gate: Mutex<()>,
}

struct Pooled<'a> {
    conn: Option<Connection>,
    pool: &'a Mutex<Vec<Connection>>,
}

impl std::ops::Deref for Pooled<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn.as_ref().expect("held until drop")
    }
}

impl std::ops::DerefMut for Pooled<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        self.conn.as_mut().expect("held until drop")
    }
}

impl Drop for Pooled<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let mut pool = self.pool.lock().unwrap_or_else(|p| p.into_inner());
            if pool.len() < 8 {
                pool.push(conn);
            }
        }
    }
}

fn row_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        key: row.get(0)?,
        etag: row.get(1)?,
        size: row.get::<_, i64>(2)? as u64,
        modified_ms: row.get(3)?,
        digest: row.get(4)?,
        attributes: row.get(5)?,
    })
}

const ROW_COLUMNS: &str = "key, etag, size, modified_ms, digest, attributes";

impl Index {
    pub fn open(path: &Path) -> Result<Index> {
        if let Some(parent) = path.parent() {
            crate::cas::ensure_durable_dir(parent)
                .map_err(|error| StoreError::other(format!("create {}", parent.display()), error))?;
        }
        let index = Index {
            path: path.to_path_buf(),
            pool: Mutex::new(Vec::new()),
            gate: Mutex::new(()),
        };
        let conn = index.connect()?;
        init_schema(&conn, path)?;
        index.pool.lock().unwrap().push(conn);
        Ok(index)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connect(&self) -> Result<Connection> {
        let conn = Connection::open(&self.path)
            .map_err(|error| StoreError::other(format!("open {}", self.path.display()), error))?;
        // `synchronous` is per connection: a commit is durable only if every
        // connection that writes asks for FULL.
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.busy_timeout(Duration::from_secs(30))?;
        // WAL lets readers proceed during a write and lets two processes (the
        // `celld dev` supervisor and its node) share the file.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Ok(conn)
    }

    fn conn(&self) -> Result<Pooled<'_>> {
        let pooled = self.pool.lock().unwrap_or_else(|p| p.into_inner()).pop();
        let conn = match pooled {
            Some(conn) => conn,
            None => self.connect()?,
        };
        Ok(Pooled {
            conn: Some(conn),
            pool: &self.pool,
        })
    }

    pub fn get(&self, key: &str) -> Result<Option<Row>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                &format!("SELECT {ROW_COLUMNS} FROM objects WHERE key = ?1"),
                [key],
                row_from,
            )
            .optional()?)
    }

    /// The row and, for an inline object, its bytes, read in one statement so
    /// they cannot come from two different versions.
    pub fn get_with_body(&self, key: &str) -> Result<Option<(Row, Option<Vec<u8>>)>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                &format!("SELECT {ROW_COLUMNS}, body FROM objects WHERE key = ?1"),
                [key],
                |row| Ok((row_from(row)?, row.get::<_, Option<Vec<u8>>>(6)?)),
            )
            .optional()?)
    }

    /// Install `location` at `key` if `mode` allows it.
    ///
    /// `before_commit` runs inside the transaction, after the precondition
    /// has passed and while the write lock is held. A blob-backed write takes
    /// its retention row there: because it holds the index lock, no
    /// reconciliation in another process can run between the retention row
    /// and the commit and release the row as stale. If it fails the
    /// transaction rolls back.
    #[allow(clippy::too_many_arguments)]
    pub fn commit_put(
        &self,
        key: &str,
        location: &Location,
        size: u64,
        attributes: &str,
        mode: &Mode,
        now_ms: i64,
        before_commit: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Committed> {
        let _gate = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        check_mode(&tx, key, mode)?;
        before_commit()?;
        let committed = put_in(&tx, key, location, size, attributes, now_ms)?;
        tx.commit()?;
        Ok(committed)
    }

    /// Copy `from` to `to` inside one transaction. `before_commit` receives
    /// the source's digest (if it is a blob) so the destination's retention
    /// row can be taken under the same lock.
    pub fn commit_copy(
        &self,
        from: &str,
        to: &str,
        mode: &Mode,
        now_ms: i64,
        before_commit: &mut dyn FnMut(Option<&str>) -> Result<()>,
    ) -> Result<Committed> {
        let _gate = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source: Option<(Row, Option<Vec<u8>>)> = tx
            .query_row(
                &format!("SELECT {ROW_COLUMNS}, body FROM objects WHERE key = ?1"),
                [from],
                |row| Ok((row_from(row)?, row.get(6)?)),
            )
            .optional()?;
        let Some((source, body)) = source else {
            return Err(StoreError::NotFound(from.to_string()));
        };
        check_mode(&tx, to, mode)?;
        let location = match (&source.digest, body) {
            (Some(digest), _) => Location::Blob(digest.clone()),
            (None, Some(body)) => Location::Inline(body),
            (None, None) => {
                return Err(StoreError::Corrupt(format!(
                    "row {from} has neither body nor digest"
                )))
            }
        };
        before_commit(location.digest())?;
        let committed = put_in(&tx, to, &location, source.size, &source.attributes, now_ms)?;
        tx.commit()?;
        Ok(committed)
    }

    /// Run `inspect` over every blob-backed `(key, digest)` while holding the
    /// index write lock, so nothing can commit (or be about to) meanwhile.
    pub fn with_blob_keys_locked<T>(
        &self,
        inspect: impl FnOnce(Vec<(String, String)>) -> Result<T>,
    ) -> Result<T> {
        let _gate = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let rows: Vec<(String, String)> = {
            let mut statement =
                tx.prepare("SELECT key, digest FROM objects WHERE digest IS NOT NULL")?;
            let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        let outcome = inspect(rows)?;
        tx.commit()?;
        Ok(outcome)
    }

    /// Delete `key`. Returns the digest it held, if it existed and was a blob.
    pub fn commit_delete(&self, key: &str) -> Result<Option<String>> {
        let _gate = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        let mut conn = self.conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let digest: Option<Option<String>> = tx
            .query_row("SELECT digest FROM objects WHERE key = ?1", [key], |row| row.get(0))
            .optional()?;
        tx.execute("DELETE FROM objects WHERE key = ?1", [key])?;
        tx.commit()?;
        Ok(digest.flatten())
    }

    /// Rows in key order within `scan`, at most `limit`.
    pub fn scan(&self, scan: &Scan, limit: usize) -> Result<Vec<Row>> {
        let conn = self.conn()?;
        let mut statement = conn.prepare_cached(&format!(
            "SELECT {ROW_COLUMNS} FROM objects \
             WHERE key >= ?1 AND (?2 IS NULL OR key > ?2) AND (?3 IS NULL OR key < ?3) \
             ORDER BY key LIMIT ?4"
        ))?;
        let rows = statement.query_map(
            params![scan.lo, scan.after, scan.hi, limit as i64],
            row_from,
        )?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// The greatest key within `[lo, hi)`.
    pub fn max_key(&self, lo: &str, hi: Option<&str>) -> Result<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT key FROM objects WHERE key >= ?1 AND (?2 IS NULL OR key < ?2) \
                 ORDER BY key DESC LIMIT 1",
                params![lo, hi],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Whether any key points at `digest`.
    pub fn digest_referenced(&self, digest: &str) -> Result<bool> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT 1 FROM objects WHERE digest = ?1 LIMIT 1",
                [digest],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn live_digests(&self) -> Result<BTreeMap<String, u64>> {
        let conn = self.conn()?;
        let mut statement = conn.prepare(
            "SELECT digest, size FROM objects WHERE digest IS NOT NULL",
        )?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u64)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn count(&self) -> Result<u64> {
        let conn = self.conn()?;
        Ok(conn.query_row("SELECT COUNT(*) FROM objects", [], |row| row.get::<_, i64>(0))? as u64)
    }

    /// Fold the WAL into the main file, so a copy of the index file alone is
    /// a complete backup. Called when the store is closed.
    pub fn checkpoint(&self) -> Result<()> {
        let conn = self.conn()?;
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        Ok(())
    }
}

/// The write precondition, evaluated inside the transaction.
fn check_mode(tx: &rusqlite::Transaction<'_>, key: &str, mode: &Mode) -> Result<()> {
    let current: Option<i64> = tx
        .query_row("SELECT etag FROM objects WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?;
    match mode {
        Mode::Overwrite => Ok(()),
        Mode::Create if current.is_some() => Err(StoreError::AlreadyExists(key.to_string())),
        Mode::Create => Ok(()),
        Mode::Update(tag) => match current {
            Some(etag) if etag.to_string() == *tag => Ok(()),
            _ => Err(StoreError::Precondition(key.to_string())),
        },
    }
}

fn put_in(
    tx: &rusqlite::Transaction<'_>,
    key: &str,
    location: &Location,
    size: u64,
    attributes: &str,
    now_ms: i64,
) -> Result<Committed> {
    let replaced: Option<Option<String>> = tx
        .query_row("SELECT digest FROM objects WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?;
    let etag: i64 = tx.query_row(
        "UPDATE sequence SET next = next + 1 WHERE singleton = 1 RETURNING next - 1",
        [],
        |row| row.get(0),
    )?;
    let (digest, body): (Option<&str>, Option<&[u8]>) = match location {
        Location::Blob(digest) => (Some(digest.as_str()), None),
        Location::Inline(body) => (None, Some(body.as_slice())),
    };
    tx.execute(
        "INSERT INTO objects(key, etag, size, modified_ms, digest, body, attributes) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(key) DO UPDATE SET \
           etag = excluded.etag, size = excluded.size, modified_ms = excluded.modified_ms, \
           digest = excluded.digest, body = excluded.body, attributes = excluded.attributes",
        params![key, etag, size as i64, now_ms, digest, body, attributes],
    )?;
    Ok(Committed {
        etag,
        replaced: replaced.flatten(),
    })
}

/// Create the schema in an empty file, or check that an existing file is one
/// this build wrote. An unknown version is an error, never an upgrade in place.
fn init_schema(conn: &Connection, path: &Path) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let outcome = (|| -> Result<()> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let objects: i64 =
            conn.query_row("SELECT COUNT(*) FROM sqlite_schema", [], |row| row.get(0))?;
        if version == USER_VERSION {
            for table in ["objects", "sequence"] {
                let found: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )?;
                if found != 1 {
                    return Err(StoreError::Other(format!(
                        "{} is damaged: table {table} is missing",
                        path.display()
                    )));
                }
            }
            return Ok(());
        }
        if version != 0 || objects != 0 {
            return Err(StoreError::Other(format!(
                "{} is not a vibestore index (user_version {version}, {objects} schema objects); \
                 refusing to use it",
                path.display()
            )));
        }
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(&format!("PRAGMA user_version = {USER_VERSION}"))?;
        Ok(())
    })();
    match outcome {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> (tempfile::TempDir, Index) {
        let dir = tempfile::tempdir().unwrap();
        let index = Index::open(&dir.path().join("i.sqlite")).unwrap();
        (dir, index)
    }

    fn put(index: &Index, key: &str, body: &[u8], mode: Mode) -> Result<Committed> {
        index.commit_put(
            key,
            &Location::Inline(body.to_vec()),
            body.len() as u64,
            "[]",
            &mode,
            1,
            &mut || Ok(()),
        )
    }

    #[test]
    fn the_index_is_a_stock_format_file() {
        let (dir, index) = index();
        put(&index, "a", b"x", Mode::Overwrite).unwrap();
        index.checkpoint().unwrap();
        let header = std::fs::read(dir.path().join("i.sqlite")).unwrap();
        assert_eq!(&header[..16], b"SQLite format 3\0");
    }

    #[test]
    fn create_is_exclusive_and_update_checks_the_tag() {
        let (_dir, index) = index();
        let first = put(&index, "k", b"1", Mode::Create).unwrap();
        assert!(matches!(
            put(&index, "k", b"2", Mode::Create),
            Err(StoreError::AlreadyExists(_))
        ));
        assert!(matches!(
            put(&index, "k", b"2", Mode::Update("999".into())),
            Err(StoreError::Precondition(_))
        ));
        assert!(matches!(
            put(&index, "absent", b"2", Mode::Update("1".into())),
            Err(StoreError::Precondition(_))
        ));
        let second = put(&index, "k", b"2", Mode::Update(first.etag.to_string())).unwrap();
        assert!(second.etag > first.etag);
        assert!(matches!(
            put(&index, "k", b"3", Mode::Update(first.etag.to_string())),
            Err(StoreError::Precondition(_))
        ));
        assert_eq!(index.get("k").unwrap().unwrap().etag, second.etag);
    }

    #[test]
    fn a_tag_is_never_reused_after_delete_and_recreate() {
        let (_dir, index) = index();
        let first = put(&index, "k", b"1", Mode::Create).unwrap();
        index.commit_delete("k").unwrap();
        let again = put(&index, "k", b"1", Mode::Create).unwrap();
        assert_ne!(first.etag, again.etag);
        assert!(matches!(
            put(&index, "k", b"2", Mode::Update(first.etag.to_string())),
            Err(StoreError::Precondition(_))
        ));
    }

    #[test]
    fn commit_reports_the_replaced_blob() {
        let (_dir, index) = index();
        let d1 = "a".repeat(64);
        let d2 = "b".repeat(64);
        index
            .commit_put("k", &Location::Blob(d1.clone()), 3, "[]", &Mode::Overwrite, 1, &mut || Ok(()))
            .unwrap();
        let committed = index
            .commit_put("k", &Location::Blob(d2.clone()), 3, "[]", &Mode::Overwrite, 2, &mut || Ok(()))
            .unwrap();
        assert_eq!(committed.replaced.as_deref(), Some(d1.as_str()));
        assert!(index.digest_referenced(&d2).unwrap());
        assert!(!index.digest_referenced(&d1).unwrap());
        assert_eq!(index.commit_delete("k").unwrap().as_deref(), Some(d2.as_str()));
        assert_eq!(index.commit_delete("k").unwrap(), None);
    }

    #[test]
    fn scan_honors_bounds_and_order() {
        let (_dir, index) = index();
        for key in ["a/1", "a/2", "a/3", "b/1", "c"] {
            put(&index, key, b"x", Mode::Overwrite).unwrap();
        }
        let keys = |scan: &Scan, limit| -> Vec<String> {
            index.scan(scan, limit).unwrap().into_iter().map(|r| r.key).collect()
        };
        let a = Scan { lo: "a/".into(), after: None, hi: Some("a0".into()) };
        assert_eq!(keys(&a, 10), ["a/1", "a/2", "a/3"]);
        assert_eq!(keys(&a, 2), ["a/1", "a/2"]);
        let resumed = Scan { after: Some("a/2".into()), ..a.clone() };
        assert_eq!(keys(&resumed, 10), ["a/3"]);
        assert_eq!(index.max_key("a/", Some("a0")).unwrap().as_deref(), Some("a/3"));
        assert_eq!(keys(&Scan::default(), 100), ["a/1", "a/2", "a/3", "b/1", "c"]);
    }

    #[test]
    fn reconciliation_cannot_run_between_a_retention_row_and_its_commit() {
        // Two index instances over one file stand for two processes. The
        // writer is parked inside `before_commit`, where a blob-backed write
        // has just taken its retention row. A reconciliation then must wait
        // for the commit, or it would see the row as stale and release it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("i.sqlite");
        let writer = std::sync::Arc::new(Index::open(&path).unwrap());
        let reader = std::sync::Arc::new(Index::open(&path).unwrap());
        let (parked_tx, parked_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let w = writer.clone();
        let write = std::thread::spawn(move || {
            let digest = "c".repeat(64);
            w.commit_put(
                "k",
                &Location::Blob(digest),
                1,
                "[]",
                &Mode::Create,
                1,
                &mut || {
                    parked_tx.send(()).unwrap();
                    go_rx.recv().unwrap();
                    Ok(())
                },
            )
            .unwrap();
        });
        parked_rx.recv().unwrap();
        let (seen_tx, seen_rx) = std::sync::mpsc::channel();
        let r = reader.clone();
        let reconcile = std::thread::spawn(move || {
            r.with_blob_keys_locked(|keys| {
                seen_tx.send(keys.len()).unwrap();
                Ok(())
            })
            .unwrap();
        });
        assert!(
            seen_rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "the reconciliation ran while a write held the lock"
        );
        go_tx.send(()).unwrap();
        write.join().unwrap();
        assert_eq!(seen_rx.recv_timeout(Duration::from_secs(10)).unwrap(), 1);
        reconcile.join().unwrap();
    }

    #[test]
    fn refuses_a_foreign_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.sqlite");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE unrelated(x)").unwrap();
        drop(conn);
        assert!(Index::open(&path).is_err());
    }

    #[test]
    fn reopening_keeps_the_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("i.sqlite");
        {
            let index = Index::open(&path).unwrap();
            put(&index, "k", b"v", Mode::Overwrite).unwrap();
        }
        let index = Index::open(&path).unwrap();
        let (row, body) = index.get_with_body("k").unwrap().unwrap();
        assert_eq!(body.as_deref(), Some(&b"v"[..]));
        assert_eq!(row.size, 1);
    }
}
