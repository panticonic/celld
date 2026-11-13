//! The GC root set, compatible with Vibestudio's
//! `src/server/storage/blobRetentions.ts`.
//!
//! Vibestudio's blob GC sweeps `<root>/sha256` and spares every digest in
//! `<root>/retentions.db`, table `blob_retentions(namespace, owner, digest,
//! PRIMARY KEY (namespace, owner))`. This module opens that same file and
//! writes rows in celld's own namespace, `["celld", <id>]`, so Vibestudio's GC
//! never collects an object a live key points at, and so that releasing a row
//! is all celld has to do to let a deleted object be collected.
//!
//! The file belongs to Vibestudio, so this code neither changes its journal
//! mode nor adds an object to it. It creates the exact schema Vibestudio
//! creates when the file is absent (`user_version = 1`) and refuses to touch a
//! file whose schema is anything else, as `openCanonicalSqliteDatabase` does.
//!
//! One owner is one row naming one digest, and Vibestudio refuses to change a
//! row's digest in place. Celld's owner for a key is `<key>@<digest>`: a key
//! that is overwritten holds the old and the new row for an instant, never an
//! unprotected one. That ordering is what makes a crash safe, since the old
//! row is released only after the index names the new digest.

#![allow(clippy::disallowed_methods)]

use crate::cas::is_digest;
use crate::error::{Result, StoreError};
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

pub const FILE: &str = "retentions.db";
const USER_VERSION: i64 = 1;
/// Verbatim from blobRetentions.ts, so the two schemas compare equal.
const TABLE_SQL: &str = "CREATE TABLE blob_retentions (
      namespace TEXT NOT NULL,
      owner TEXT NOT NULL,
      digest TEXT NOT NULL,
      PRIMARY KEY (namespace, owner)
    )";

/// The owner string celld uses for `key` holding `digest`.
pub fn owner(key: &str, digest: &str) -> String {
    format!("{key}@{digest}")
}

/// The inverse of [`owner`]. A key may itself contain `@`, so split on the
/// last one and require a digest after it.
pub fn split_owner(owner: &str) -> Option<(&str, &str)> {
    let at = owner.len().checked_sub(65)?;
    if owner.as_bytes().get(at) != Some(&b'@') || !owner.is_char_boundary(at) {
        return None;
    }
    let (key, digest) = (&owner[..at], &owner[at + 1..]);
    is_digest(digest).then_some((key, digest))
}

#[derive(Debug)]
pub struct Retentions {
    path: PathBuf,
    namespace: String,
    conn: Mutex<Connection>,
}

impl Retentions {
    pub fn open(root: &Path, namespace: &str) -> Result<Self> {
        let path = root.join(FILE);
        let created = !path.exists();
        let conn = Connection::open(&path)
            .map_err(|error| StoreError::other(format!("open {}", path.display()), error))?;
        // The same wait Vibestudio uses, doubled: its readers are short.
        conn.busy_timeout(Duration::from_secs(10))?;
        ensure_schema(&conn, &path)?;
        if created {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
        assert_stock_format(&path)?;
        Ok(Self {
            path,
            namespace: namespace.to_string(),
            conn: Mutex::new(conn),
        })
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Retain `digest` for `owner`. Idempotent. The blob must already be
    /// durable in the CAS: Vibestudio's own `retain` checks the same.
    pub fn retain(&self, owner: &str, digest: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT OR IGNORE INTO blob_retentions(namespace, owner, digest) VALUES (?1, ?2, ?3)",
            params![self.namespace, owner, digest],
        )?;
        Ok(())
    }

    /// Release one owner. Never removes bytes: it only permits a later GC.
    pub fn release(&self, owner: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM blob_retentions WHERE namespace = ?1 AND owner = ?2",
            params![self.namespace, owner],
        )?;
        Ok(())
    }

    pub fn owners(&self) -> Result<BTreeMap<String, String>> {
        let conn = self.conn();
        let mut statement = conn
            .prepare("SELECT owner, digest FROM blob_retentions WHERE namespace = ?1")?;
        let rows = statement.query_map([&self.namespace], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Make the namespace hold exactly `live`, in one transaction: add what is
    /// missing, drop what no key points at. Returns `(added, released)`.
    pub fn reconcile(&self, live: &BTreeMap<String, String>) -> Result<(usize, usize)> {
        let mut conn = self.conn();
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut held: BTreeMap<String, String> = BTreeMap::new();
        {
            let mut statement = tx
                .prepare("SELECT owner, digest FROM blob_retentions WHERE namespace = ?1")?;
            let rows =
                statement.query_map([&self.namespace], |row| Ok((row.get(0)?, row.get(1)?)))?;
            for row in rows {
                let (owner, digest): (String, String) = row?;
                held.insert(owner, digest);
            }
        }
        let (mut added, mut released) = (0, 0);
        for (owner, digest) in live {
            if held.get(owner) != Some(digest) {
                tx.execute(
                    "INSERT OR REPLACE INTO blob_retentions(namespace, owner, digest) \
                     VALUES (?1, ?2, ?3)",
                    params![self.namespace, owner, digest],
                )?;
                added += 1;
            }
        }
        for owner in held.keys() {
            if !live.contains_key(owner) {
                tx.execute(
                    "DELETE FROM blob_retentions WHERE namespace = ?1 AND owner = ?2",
                    params![self.namespace, owner],
                )?;
                released += 1;
            }
        }
        tx.commit()?;
        Ok((added, released))
    }
}

/// Initialize an empty file, or validate an existing one against the exact
/// schema, mirroring `openCanonicalSqliteDatabase`.
fn ensure_schema(conn: &Connection, path: &Path) -> Result<()> {
    let describe = format!("blob retention registry in {}", path.display());
    let user_version = |conn: &Connection| -> Result<i64> {
        Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
    };
    let pragma = |conn: &Connection, name: &str| -> Result<i64> {
        Ok(conn.query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))?)
    };
    let schema_objects = |conn: &Connection| -> Result<i64> {
        Ok(conn.query_row("SELECT COUNT(*) FROM sqlite_schema", [], |row| row.get(0))?)
    };
    // As in openCanonicalSqliteDatabase: judge emptiness and a current schema
    // in a read snapshot, because inside a write transaction a brand-new file
    // already reports one page. Only initialization takes the write lock.
    conn.execute_batch("BEGIN")?;
    let snapshot = (|| -> Result<Option<bool>> {
        let version = user_version(conn)?;
        if version == USER_VERSION {
            assert_canonical(conn, &describe)?;
            return Ok(None);
        }
        let truly_empty = schema_objects(conn)? == 0
            && version == 0
            && pragma(conn, "page_count")? == 0
            && pragma(conn, "freelist_count")? == 0;
        if !truly_empty {
            return Err(StoreError::Other(format!(
                "Unsupported {describe}: schema version is {version}, expected {USER_VERSION}; \
                 the database was left unchanged"
            )));
        }
        Ok(Some(truly_empty))
    })();
    let _ = conn.execute_batch("ROLLBACK");
    if snapshot?.is_none() {
        return Ok(());
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let outcome = (|| -> Result<()> {
        // Another process may have initialized it while we waited.
        let version = user_version(conn)?;
        if version == USER_VERSION {
            return assert_canonical(conn, &describe);
        }
        if version != 0 || schema_objects(conn)? != 0 {
            return Err(StoreError::Other(format!(
                "Unsupported {describe}: schema version is {version}, expected {USER_VERSION}; \
                 the database was left unchanged"
            )));
        }
        conn.execute_batch(TABLE_SQL)?;
        conn.execute_batch(&format!("PRAGMA user_version = {USER_VERSION}"))?;
        assert_canonical(conn, &describe)
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

fn assert_canonical(conn: &Connection, description: &str) -> Result<()> {
    let mut statement = conn.prepare(
        "SELECT type, name, sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
    )?;
    let rows: Vec<(String, String, Option<String>)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let canonical = rows.len() == 1
        && rows[0].0 == "table"
        && rows[0].1 == "blob_retentions"
        && rows[0].2.as_deref().map(normalized_sql) == Some(normalized_sql(TABLE_SQL));
    if canonical {
        Ok(())
    } else {
        Err(StoreError::Other(format!(
            "Unsupported {description}: schema object set is not canonical"
        )))
    }
}

/// A port of `normalizedSql` in packages/sqlite/src/index.ts: whitespace runs
/// collapse, and none survives next to `(`, `)` or `,`.
fn normalized_sql(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut result = String::new();
    let mut pending_space = false;
    let mut quote: Option<char> = None;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if let Some(open) = quote {
            result.push(c);
            let closer = if open == '[' { ']' } else { open };
            if c == closer {
                if open != '[' && chars.get(index + 1) == Some(&c) {
                    result.push(c);
                    index += 1;
                } else {
                    quote = None;
                }
            }
            index += 1;
            continue;
        }
        if matches!(c, '\'' | '"' | '`' | '[') {
            if pending_space && !result.is_empty() && !result.ends_with('(') && !result.ends_with(',') {
                result.push(' ');
            }
            pending_space = false;
            quote = Some(c);
            result.push(c);
            index += 1;
            continue;
        }
        if c.is_whitespace() {
            pending_space = true;
            index += 1;
            continue;
        }
        if matches!(c, ',' | ')' | ';') {
            result = result.trim_end().to_string();
            let trailing = chars[index + 1..].iter().collect::<String>();
            if c != ';' || !trailing.trim().is_empty() {
                result.push(c);
            }
            pending_space = false;
            index += 1;
            continue;
        }
        if c == '(' {
            result = result.trim_end().to_string();
            result.push(c);
            pending_space = false;
            index += 1;
            continue;
        }
        if pending_space && !result.is_empty() && !result.ends_with('(') && !result.ends_with(',') {
            result.push(' ');
        }
        pending_space = false;
        result.push(c);
        index += 1;
    }
    result.trim().to_string()
}

/// The file must be a stock-format SQLite database that Vibestudio's
/// `node:sqlite` can open. DoltLite creates one by default; a prolly-format
/// file would make Vibestudio's GC fail to read the retention set.
fn assert_stock_format(path: &Path) -> Result<()> {
    use std::io::Read as _;
    let mut header = [0u8; 16];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| StoreError::other(format!("read {}", path.display()), error))?;
    if &header != b"SQLite format 3\0" {
        return Err(StoreError::Other(format!(
            "{} is not a stock-format SQLite file; Vibestudio could not read it",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const D1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const D2: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    #[test]
    fn owner_round_trips_even_when_the_key_has_an_at_sign() {
        let key = "cells/a@b/ltx/0001.ltx";
        let text = owner(key, D1);
        assert_eq!(split_owner(&text), Some((key, D1)));
        assert_eq!(split_owner("short"), None);
        assert_eq!(split_owner(&format!("key#{D1}")), None);
    }

    #[test]
    fn creates_the_exact_vibestudio_schema() {
        let dir = tempfile::tempdir().unwrap();
        let retentions = Retentions::open(dir.path(), r#"["celld","t"]"#).unwrap();
        let conn = Connection::open(dir.path().join(FILE)).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 1);
        let sql: String = conn
            .query_row("SELECT sql FROM sqlite_schema WHERE name = 'blob_retentions'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(normalized_sql(&sql), normalized_sql(TABLE_SQL));
        let objects: i64 = conn
            .query_row("SELECT COUNT(*) FROM sqlite_schema", [], |r| r.get(0))
            .unwrap();
        assert_eq!(objects, 2, "the table and its primary-key autoindex, nothing else");
        drop(retentions);
        // Reopening an existing file validates rather than recreates.
        Retentions::open(dir.path(), r#"["celld","t"]"#).unwrap();
    }

    #[test]
    fn refuses_a_file_with_another_schema() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join(FILE)).unwrap();
        conn.execute_batch("CREATE TABLE other(x); PRAGMA user_version = 7;").unwrap();
        drop(conn);
        let error = Retentions::open(dir.path(), "ns").unwrap_err();
        assert!(error.to_string().contains("Unsupported"), "{error}");
    }

    #[test]
    fn refuses_a_file_with_the_right_version_and_a_drifted_table() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join(FILE)).unwrap();
        conn.execute_batch(
            "CREATE TABLE blob_retentions(namespace TEXT, owner TEXT, digest TEXT, extra TEXT); \
             PRAGMA user_version = 1;",
        )
        .unwrap();
        drop(conn);
        assert!(Retentions::open(dir.path(), "ns").is_err());
    }

    #[test]
    fn retain_release_and_namespaces_are_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let a = Retentions::open(dir.path(), r#"["celld","a"]"#).unwrap();
        let b = Retentions::open(dir.path(), r#"["celld","b"]"#).unwrap();
        a.retain("k@1", D1).unwrap();
        a.retain("k@1", D1).unwrap();
        b.retain("k@1", D2).unwrap();
        assert_eq!(a.owners().unwrap().len(), 1);
        a.release("k@1").unwrap();
        assert!(a.owners().unwrap().is_empty());
        assert_eq!(b.owners().unwrap().get("k@1").map(String::as_str), Some(D2));
    }

    #[test]
    fn reconcile_adds_missing_and_releases_stale() {
        let dir = tempfile::tempdir().unwrap();
        let r = Retentions::open(dir.path(), "ns").unwrap();
        r.retain(&owner("old", D1), D1).unwrap();
        let live: BTreeMap<String, String> =
            [(owner("new", D2), D2.to_string())].into_iter().collect();
        assert_eq!(r.reconcile(&live).unwrap(), (1, 1));
        assert_eq!(r.owners().unwrap(), live);
        assert_eq!(r.reconcile(&live).unwrap(), (0, 0));
    }

    #[test]
    fn normalization_matches_the_typescript_function() {
        assert_eq!(
            normalized_sql("CREATE TABLE t (\n  a TEXT NOT NULL,\n  b TEXT\n)"),
            "CREATE TABLE t(a TEXT NOT NULL,b TEXT)"
        );
        assert_eq!(normalized_sql("x  'a  b'  ;"), "x 'a  b'");
    }
}
