//! Micro-benchmark of the SQLite operations celld leans on, on stock-format
//! files. Run the same source against upstream SQLite 3.45 (libsqlite3-sys
//! bundled) and against the DoltLite amalgamation, then compare.
//!
//! `cargo run --release -p doltlite-spike --example bench`
use rusqlite::Connection;
use std::time::Instant;

fn schema(c: &Connection) {
    // Mirrors storage.rs `schema`: synchronous OFF while creating, WAL, NORMAL.
    c.pragma_update(None, "synchronous", "OFF").unwrap();
    c.pragma_update(None, "journal_mode", "WAL").unwrap();
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS _cf_KV (scope TEXT, k TEXT, v TEXT, PRIMARY KEY(scope,k));
         CREATE TABLE IF NOT EXISTS _cf_ALARM (scope TEXT PRIMARY KEY, at_ms INTEGER, retry INTEGER DEFAULT 0);
         CREATE TABLE IF NOT EXISTS _cf_METADATA (scope TEXT PRIMARY KEY, actor_name TEXT);",
    )
    .unwrap();
    c.pragma_update(None, "synchronous", "NORMAL").unwrap();
}

fn report(name: &str, n: usize, t: Instant) {
    let s = t.elapsed().as_secs_f64();
    println!("{name:<34} n={n:<7} {:>8.3} s  {:>10.0} ops/s  {:>8.2} us/op", s, n as f64 / s, s * 1e6 / n as f64);
}

fn main() {
    let dir = std::env::var("BENCH_DIR").unwrap_or_else(|_| "/tmp".into());
    let dir = tempfile::tempdir_in(dir).unwrap();

    // 1. Cold cell creation: new file, schema.
    let n = 3000;
    let t = Instant::now();
    for i in 0..n {
        let c = Connection::open(dir.path().join(format!("cold{i}.db"))).unwrap();
        schema(&c);
    }
    report("cold cell create (open+schema)", n, t);

    let c = Connection::open(dir.path().join("warm.db")).unwrap();
    schema(&c);

    // 2. Warm single-row autocommit writes (one WAL commit each).
    let n = 30000;
    let t = Instant::now();
    let mut st = c.prepare("INSERT OR REPLACE INTO _cf_KV(scope,k,v) VALUES ('s', ?1, ?2)").unwrap();
    for i in 0..n {
        st.execute(rusqlite::params![format!("key{i}"), "value-value-value-value"]).unwrap();
    }
    drop(st);
    report("autocommit INSERT OR REPLACE", n, t);

    // 3. Batched writes in one transaction.
    let n = 300000;
    let t = Instant::now();
    c.execute_batch("BEGIN").unwrap();
    let mut st = c.prepare("INSERT OR REPLACE INTO _cf_KV(scope,k,v) VALUES ('b', ?1, ?2)").unwrap();
    for i in 0..n {
        st.execute(rusqlite::params![format!("key{i}"), "value-value-value-value"]).unwrap();
    }
    drop(st);
    c.execute_batch("COMMIT").unwrap();
    report("batched INSERT (1 txn)", n, t);

    // 4. Point reads.
    let n = 300000;
    let t = Instant::now();
    let mut st = c.prepare("SELECT v FROM _cf_KV WHERE scope='s' AND k=?1").unwrap();
    for i in 0..n {
        let _: String = st.query_row([format!("key{}", i % 30000)], |r| r.get(0)).unwrap();
    }
    drop(st);
    report("point SELECT", n, t);

    // 5. Range scan.
    let t = Instant::now();
    let mut total = 0usize;
    for _ in 0..20 {
        let mut st = c.prepare("SELECT k, v FROM _cf_KV WHERE scope='b' ORDER BY k").unwrap();
        let mut rows = st.query([]).unwrap();
        while let Some(r) = rows.next().unwrap() {
            let _: String = r.get(0).unwrap();
            total += 1;
        }
    }
    report("range scan rows", total, t);

    // 6. Checkpoint of the WAL built above.
    let t = Instant::now();
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
    report("wal_checkpoint(TRUNCATE)", 1, t);

    // 7. Cross-connection data_version poll.
    let r = Connection::open(dir.path().join("warm.db")).unwrap();
    let n = 200000;
    let t = Instant::now();
    for _ in 0..n {
        let _: i64 = r.query_row("PRAGMA data_version", [], |x| x.get(0)).unwrap();
    }
    report("PRAGMA data_version", n, t);
}
