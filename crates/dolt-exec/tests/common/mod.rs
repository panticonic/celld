//! Shared helpers for the determinism and conformance tests.
#![allow(dead_code)]

use celld_dolt_exec::{prolly_uri, Genesis};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

pub const T0: i64 = 1_700_000_000_000; // 2023-11-14 22:13:20 UTC
pub const GENESIS: Genesis = Genesis { now_ms: 1_600_000_000_000, seed: 42 };

/// Open (creating) a prolly/Dolt store at `path`. With `genesis`, its seed
/// commit is pinned.
pub fn open_dolt(path: &Path, genesis: Option<Genesis>) -> Connection {
    let uri = prolly_uri(path, genesis);
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_URI,
    )
    .expect("open prolly store")
}

/// A fresh Dolt store in its own temp dir, with a pinned genesis.
pub fn fresh_dolt() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let conn = open_dolt(&dir.path().join("cell.db"), Some(GENESIS));
    (dir, conn)
}

fn render(v: ValueRef<'_>) -> String {
    match v {
        ValueRef::Null => "N".into(),
        ValueRef::Integer(i) => format!("I{i}"),
        ValueRef::Real(f) => format!("R{:016x}", f.to_bits()),
        ValueRef::Text(t) => format!("T{}", hex::encode(t)),
        ValueRef::Blob(b) => format!("B{}", hex::encode(b)),
    }
}

/// An ordered dump of every table (and the schema) hashed with SHA-256: the
/// "state digest". Rows are rendered with exact storage classes (reals by bit
/// pattern) and sorted, so scan order cannot matter. Includes rowids, shadow
/// tables of virtual tables and sqlite_sequence.
pub fn state_digest(conn: &Connection) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut schema = conn
        .prepare("SELECT type, name, tbl_name, coalesce(sql,'') FROM sqlite_schema ORDER BY type, name")
        .unwrap();
    let rows = schema
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(3)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect::<Vec<_>>();
    for (kind, name, sql) in &rows {
        lines.push(format!("schema|{kind}|{name}|{sql}"));
    }
    for (kind, name, _) in &rows {
        if kind != "table" || name.starts_with("dolt_") {
            continue;
        }
        // Tables without rowid have no rowid column to select.
        let without_rowid: bool = conn
            .query_row("SELECT wr FROM pragma_table_list(?1) WHERE name = ?1", [name], |r| r.get(0))
            .unwrap_or(false);
        let projection = if without_rowid { "*" } else { "rowid, *" };
        let sql = format!("SELECT {projection} FROM \"{}\"", name.replace('"', "\"\""));
        // Virtual tables themselves cannot always be scanned; their shadow tables carry the state.
        let Ok(mut stmt) = conn.prepare(&sql) else { continue };
        let n = stmt.column_count();
        let Ok(mut rows) = stmt.query([]) else { continue };
        let mut table_lines = Vec::new();
        while let Ok(Some(row)) = rows.next() {
            let cells: Vec<String> = (0..n).map(|i| render(row.get_ref(i).unwrap())).collect();
            table_lines.push(format!("row|{name}|{}", cells.join(",")));
        }
        table_lines.sort();
        lines.extend(table_lines);
    }
    hex::encode(Sha256::digest(lines.join("\n").as_bytes()))
}

/// Run a statement for its effect, ignoring the value it returns.
pub fn run(conn: &Connection, sql: &str) {
    conn.query_row(sql, [], |_| Ok(())).unwrap_or_else(|e| panic!("{sql}: {e}"));
}

pub fn q1<T: rusqlite::types::FromSql>(conn: &Connection, sql: &str) -> T {
    conn.query_row(sql, [], |r| r.get(0)).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

pub fn rows_as_strings(conn: &Connection, sql: &str) -> Vec<Vec<String>> {
    let mut stmt = conn.prepare(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
    let n = stmt.column_count();
    stmt.query_map([], |r| {
        Ok((0..n)
            .map(|i| match r.get_ref(i).unwrap() {
                ValueRef::Null => "NULL".to_string(),
                ValueRef::Integer(i) => i.to_string(),
                ValueRef::Real(f) => f.to_string(),
                ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                ValueRef::Blob(b) => format!("x'{}'", hex::encode(b)),
            })
            .collect())
    })
    .unwrap()
    .map(|r| r.unwrap())
    .collect()
}

/// Re-run this test binary for one test with `TZ` set, and return the lines
/// the child printed that start with `marker`. The environment of the parent
/// cannot be changed reliably after startup (libc caches the zone), so the
/// zone is set before the child process starts.
pub fn run_child_under_tz(test_name: &str, tz: &str, marker: &str) -> Vec<String> {
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(exe)
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env("TZ", tz)
        .env("CELLD_TZ_CHILD", "1")
        .output()
        .expect("spawn child test process");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "child test {test_name} under TZ={tz} failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // libtest prints "test name ... " on the same line as the first output.
    stdout.lines().filter_map(|l| l.find(marker).map(|at| l[at + marker.len()..].to_string())).collect()
}

pub fn in_tz_child() -> bool {
    std::env::var_os("CELLD_TZ_CHILD").is_some()
}
