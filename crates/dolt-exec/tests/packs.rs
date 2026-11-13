//! Patch 0004: `dolt_pack_build`, `dolt_pack_fetch`, `dolt_pack_haves`.
//!
//! A pack is the transport of a Dolt remote between two cells that are not
//! two files a peer can open. These tests are the engine half of the contract
//! (docs/fork/design.md section 3b.13): the receiver learns exactly the
//! branches the sender revealed, the chunks it already had are not sent, a
//! corrupt pack installs nothing, and the fetch lands as an ordinary
//! remote-tracking branch that `dolt_merge` can use.

mod common;

use common::{open_dolt, GENESIS};
use rusqlite::Connection;

fn text(conn: &Connection, sql: &str) -> String {
    conn.query_row(sql, [], |row| row.get::<_, String>(0)).unwrap()
}

fn call(conn: &Connection, sql: &str) {
    conn.query_row(sql, [], |row| row.get::<_, rusqlite::types::Value>(0)).unwrap();
}

fn commit(conn: &Connection, message: &str) -> String {
    conn.query_row(
        "SELECT dolt_commit('-A', '-m', ?1, '--date', '2024-01-01T00:00:00', '--author', 'T <t@example.com>')",
        [message],
        |row| row.get::<_, String>(0),
    )
    .unwrap()
}

/// Two stores that start from one history: `b` is a copy of `a`'s file.
fn pair() -> (tempfile::TempDir, Connection, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let a_path = dir.path().join("a.db");
    let a = open_dolt(&a_path, Some(GENESIS));
    a.execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);").unwrap();
    a.execute("INSERT INTO notes VALUES (1, 'one')", []).unwrap();
    commit(&a, "base");
    let b_path = dir.path().join("b.db");
    std::fs::copy(&a_path, &b_path).unwrap();
    let b = open_dolt(&b_path, None);
    (dir, a, b)
}

fn pack(source: &Connection, branches: &str, haves: &str) -> Vec<u8> {
    source
        .query_row("SELECT dolt_pack_build(?1, ?2)", [branches, haves], |row| row.get(0))
        .unwrap()
}

#[test]
fn a_pack_carries_only_what_the_receiver_lacks() {
    let (_dir, a, b) = pair();
    for n in 2..40 {
        a.execute("INSERT INTO notes VALUES (?1, 'row')", [n]).unwrap();
    }
    let tip = commit(&a, "more rows");

    let haves = text(&b, "SELECT dolt_pack_haves()");
    let full = pack(&a, "main", "");
    let delta = pack(&a, "main", &haves);
    assert!(delta.len() < full.len(), "{} !< {}", delta.len(), full.len());

    let fetched = text(&b, &format!("SELECT dolt_pack_fetch('a', ?1)").replace("?1", &blob_literal(&delta)));
    assert_eq!(fetched.trim(), format!("main {tip}"));
    assert_eq!(text(&b, "SELECT dolt_hashof('a/main')"), tip);
    // The fetch changed no branch and no working data.
    assert_ne!(text(&b, "SELECT dolt_hashof('main')"), tip);
    assert_eq!(
        b.query_row("SELECT count(*) FROM notes", [], |row| row.get::<_, i64>(0)).unwrap(),
        1
    );
}

fn blob_literal(bytes: &[u8]) -> String {
    format!("x'{}'", hex::encode(bytes))
}

#[test]
fn a_fetched_branch_merges_and_converges() {
    let (_dir, a, b) = pair();
    a.execute("INSERT INTO notes VALUES (2, 'from a')", []).unwrap();
    commit(&a, "a adds 2");
    b.execute("INSERT INTO notes VALUES (3, 'from b')", []).unwrap();
    commit(&b, "b adds 3");

    let haves = text(&b, "SELECT dolt_pack_haves()");
    let delta = pack(&a, "main", &haves);
    b.query_row(
        &format!("SELECT dolt_pack_fetch('a', {})", blob_literal(&delta)),
        [],
        |row| row.get::<_, String>(0),
    )
    .unwrap();
    // A fast-forward is not possible, so this is a real merge, and the row
    // data of both sides is in the result.
    b.execute_batch("BEGIN").unwrap();
    b.query_row("SELECT dolt_merge('--no-ff', '-m', 'merge a', 'a/main')", [], |row| row.get::<_, String>(0))
        .unwrap();
    b.execute_batch("COMMIT").unwrap();
    let ids: Vec<i64> = b
        .prepare("SELECT id FROM notes ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(ids, vec![1, 2, 3]);
}

#[test]
fn only_the_named_branches_are_revealed() {
    let (_dir, a, b) = pair();
    call(&a, "SELECT dolt_branch('private')");
    call(&a, "SELECT dolt_checkout('private')");
    a.execute("INSERT INTO notes VALUES (9, 'secret')", []).unwrap();
    commit(&a, "private work");
    call(&a, "SELECT dolt_checkout('main')");

    let delta = pack(&a, "main=shared", &text(&b, "SELECT dolt_pack_haves()"));
    b.query_row(
        &format!("SELECT dolt_pack_fetch('a', {})", blob_literal(&delta)),
        [],
        |row| row.get::<_, String>(0),
    )
    .unwrap();
    let tracked: Vec<String> = b
        .prepare("SELECT name FROM dolt_remote_branches WHERE name LIKE 'remotes/a/%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(tracked, vec!["remotes/a/shared"]);
    // The pack holds no byte of the private commit.
    let needle = b"private work";
    assert!(!delta.windows(needle.len()).any(|w| w == needle));
    let error = a
        .query_row("SELECT dolt_pack_build('nope', '')", [], |row| row.get::<_, Vec<u8>>(0))
        .unwrap_err();
    assert!(error.to_string().contains("branch not found"), "{error}");
}

#[test]
fn a_corrupt_or_short_pack_installs_nothing() {
    let (_dir, a, b) = pair();
    a.execute("INSERT INTO notes VALUES (2, 'two')", []).unwrap();
    commit(&a, "two");
    let good = pack(&a, "main", &text(&b, "SELECT dolt_pack_haves()"));
    let before = text(&b, "SELECT dolt_pack_haves()");

    let mut flipped = good.clone();
    let at = flipped.len() - 5;
    flipped[at] ^= 0xff;
    let mut cut = good.clone();
    cut.truncate(good.len() - 9);
    for bad in [flipped, cut, b"nonsense".to_vec()] {
        let result = b.query_row(
            &format!("SELECT dolt_pack_fetch('a', {})", blob_literal(&bad)),
            [],
            |row| row.get::<_, String>(0),
        );
        assert!(result.is_err());
    }
    // A pack missing a chunk of the graph is refused as well.
    let thin = pack(&a, "main", &text(&a, "SELECT dolt_pack_haves()"));
    assert!(b
        .query_row(
            &format!("SELECT dolt_pack_fetch('a', {})", blob_literal(&thin)),
            [],
            |row| row.get::<_, String>(0),
        )
        .is_err());
    assert_eq!(text(&b, "SELECT dolt_pack_haves()"), before);
    // And the good one still works afterwards.
    b.query_row(
        &format!("SELECT dolt_pack_fetch('a', {})", blob_literal(&good)),
        [],
        |row| row.get::<_, String>(0),
    )
    .unwrap();
}
