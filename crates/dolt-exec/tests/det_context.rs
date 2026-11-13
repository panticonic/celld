//! Patch 0002 (per-connection deterministic context), tested through the
//! public Rust surface: the clock, the PRNG, localtime, the counters, Dolt
//! timestamps, genesis, and concurrency.

mod common;

use celld_dolt_exec::{pin_clock, Genesis};
use common::*;
use rusqlite::Connection;

fn mem() -> Connection {
    Connection::open_in_memory().unwrap()
}

#[test]
fn library_is_the_patched_doltlite() {
    let c = mem();
    let v: String = q1(&c, "SELECT sqlite_version()");
    assert!(v.starts_with("3.55"), "{v}");
    // The context API is linked in.
    let _g = pin_clock(&c, 0, 0).unwrap();
}

#[test]
fn clock_is_pinned_on_this_connection_only() {
    let a = mem();
    let b = mem();
    {
        let _g = pin_clock(&a, T0, 1).unwrap();
        assert_eq!(q1::<String>(&a, "SELECT datetime('now')"), "2023-11-14 22:13:20");
        assert_eq!(q1::<String>(&a, "SELECT CURRENT_TIMESTAMP"), "2023-11-14 22:13:20");
        assert_eq!(q1::<String>(&a, "SELECT strftime('%Y-%m-%dT%H:%M:%f','now')"), "2023-11-14T22:13:20.000");
        assert_eq!(q1::<i64>(&a, "SELECT unixepoch('now')"), 1_700_000_000);
        assert!((q1::<f64>(&a, "SELECT julianday('now')") - 2460263.4259259258).abs() < 1e-9);
        // Another connection in the same process, at the same moment, is untouched.
        assert!(q1::<String>(&b, "SELECT datetime('now')").as_str() > "2025-01-01");
    }
    // Cleared: back to the real clock.
    assert!(q1::<String>(&a, "SELECT datetime('now')").as_str() > "2025-01-01");
}

#[test]
fn context_range_and_flags_are_checked() {
    use celld_dolt_exec::ExecError;
    let c = mem();
    assert!(matches!(pin_clock(&c, -1, 0), Err(ExecError::Operational { .. })));
    assert!(pin_clock(&c, 253_402_300_799_999, 0).is_ok());
    assert!(matches!(pin_clock(&c, 253_402_300_800_000, 0), Err(ExecError::Operational { .. })));
    // Setting twice replaces the first context; the guard's drop clears it.
    let g1 = pin_clock(&c, T0, 1).unwrap();
    let g2 = pin_clock(&c, T0 + 1000, 1).unwrap();
    assert_eq!(q1::<String>(&c, "SELECT datetime('now')"), "2023-11-14 22:13:21");
    drop(g2);
    drop(g1);
    assert!(q1::<String>(&c, "SELECT datetime('now')").as_str() > "2025-01-01");
}

#[test]
fn prng_is_seeded_per_connection_and_reproducible() {
    let sample = |seed: u64| -> Vec<String> {
        let c = mem();
        let _g = pin_clock(&c, T0, seed).unwrap();
        // 200 bytes of randomblob spans several ChaCha blocks.
        rows_as_strings(&c, "SELECT random(), hex(randomblob(200)), random(), hex(randomblob(1)), random()")
            .remove(0)
    };
    assert_eq!(sample(7), sample(7));
    assert_ne!(sample(7), sample(8));
    // A seed above 32 bits matters in its upper half.
    assert_ne!(sample(7), sample(7 | (1 << 40)));

    // The stream restarts when the context is set again.
    let c = mem();
    let first = {
        let _g = pin_clock(&c, T0, 5).unwrap();
        q1::<i64>(&c, "SELECT random()")
    };
    let _ = q1::<i64>(&c, "SELECT random()"); // real randomness in between
    let again = {
        let _g = pin_clock(&c, T0, 5).unwrap();
        q1::<i64>(&c, "SELECT random()")
    };
    assert_eq!(first, again);
}

#[test]
fn random_rowids_are_deterministic() {
    let run = || {
        let c = mem();
        let _g = pin_clock(&c, T0, 3).unwrap();
        c.execute_batch(
            "CREATE TABLE t(id INTEGER PRIMARY KEY, v);
             INSERT INTO t VALUES (9223372036854775807, 'max');
             INSERT INTO t(v) VALUES ('a'), ('b'), ('c');",
        )
        .unwrap();
        rows_as_strings(&c, "SELECT id FROM t ORDER BY id")
    };
    let a = run();
    assert_eq!(a.len(), 4);
    assert_eq!(a, run());
}

#[test]
fn counters_start_at_zero() {
    let c = mem();
    c.execute_batch("CREATE TABLE t(a); INSERT INTO t VALUES (1),(2),(3);").unwrap();
    assert_eq!(q1::<i64>(&c, "SELECT last_insert_rowid()"), 3);
    assert_eq!(q1::<i64>(&c, "SELECT changes()"), 3);
    let _g = pin_clock(&c, T0, 1).unwrap();
    assert_eq!(q1::<i64>(&c, "SELECT last_insert_rowid()"), 0);
    assert_eq!(q1::<i64>(&c, "SELECT changes()"), 0);
}

/// The marker line the parent test reads from the child.
const M: &str = "TZLINE:";

#[test]
fn localtime_is_utc_under_a_context_whatever_tz_says() {
    if !in_tz_child() {
        let utc = run_child_under_tz("localtime_is_utc_under_a_context_whatever_tz_says", "UTC", M);
        let la = run_child_under_tz("localtime_is_utc_under_a_context_whatever_tz_says", "America/Los_Angeles", M);
        let kolkata = run_child_under_tz("localtime_is_utc_under_a_context_whatever_tz_says", "Asia/Kolkata", M);
        // Pinned results do not depend on TZ ...
        assert_eq!(utc[0], la[0]);
        assert_eq!(utc[0], kolkata[0]);
        // ... while the unpinned ones do, which proves TZ took effect.
        assert_eq!(utc[1], "2024-07-01 12:00:00");
        assert_eq!(la[1], "2024-07-01 05:00:00");
        assert_eq!(kolkata[1], "2024-07-01 17:30:00");
        return;
    }
    let c = mem();
    let probes = "SELECT datetime('2024-07-01 12:00:00','localtime'), datetime('2024-07-01 12:00:00','utc'),\
                  strftime('%s','2024-07-01 12:00:00','localtime'), datetime('now','localtime'), datetime(0,'unixepoch','localtime'),\
                  datetime('2100-03-01 00:00:00','localtime'), datetime('1960-01-01 00:00:00','localtime')";
    let unpinned = q1::<String>(&c, "SELECT datetime('2024-07-01 12:00:00','localtime')");
    let pinned = {
        let _g = pin_clock(&c, T0, 1).unwrap();
        rows_as_strings(&c, probes).remove(0).join("|")
    };
    println!("{M}{pinned}");
    println!("{M}{unpinned}");
}

#[test]
fn dolt_timestamps_come_from_the_pinned_clock() {
    let (_d, c) = fresh_dolt();
    c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT); INSERT INTO t VALUES (1,'x');").unwrap();
    let _g = pin_clock(&c, T0, 9).unwrap();
    let h1: String = q1(&c, "SELECT dolt_commit('-A','-m','first')");
    // dolt_log carries the pinned time (seconds resolution).
    let ts: i64 = q1(&c, &format!("SELECT CAST(strftime('%s', date) AS INTEGER) FROM dolt_log WHERE commit_hash = '{h1}'"));
    assert_eq!(ts, T0 / 1000);

    // branch, diverge, merge, tag, revert, cherry-pick: every commit takes the clock.
    run(&c, "SELECT dolt_branch('feature')");
    run(&c, "SELECT dolt_checkout('feature')");
    c.execute_batch("INSERT INTO t VALUES (2,'f');").unwrap();
    let feature: String = q1(&c, "SELECT dolt_commit('-A','-m','feature work')");
    run(&c, "SELECT dolt_checkout('main')");
    c.execute_batch("INSERT INTO t VALUES (3,'m');").unwrap();
    let main2: String = q1(&c, "SELECT dolt_commit('-A','-m','main work')");
    run(&c, "SELECT dolt_merge('feature')");
    run(&c, "SELECT dolt_tag('v1')");
    run(&c, &format!("SELECT dolt_revert('{main2}')"));
    let _ = &feature;

    let dates: Vec<String> = rows_as_strings(&c, "SELECT DISTINCT CAST(strftime('%s', date) AS INTEGER) FROM dolt_log WHERE message NOT LIKE 'Initialize%'")
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(dates, vec![(T0 / 1000).to_string()], "every commit must carry the pinned time");
    let tag_ts: i64 = q1(&c, "SELECT CAST(strftime('%s', date) AS INTEGER) FROM dolt_tags WHERE tag_name = 'v1'");
    assert_eq!(tag_ts, T0 / 1000);
}

#[test]
fn rebased_commits_take_the_connection_clock() {
    let (_d, c) = fresh_dolt();
    c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT); INSERT INTO t VALUES (1,'x');").unwrap();
    {
        let _g = pin_clock(&c, T0, 9).unwrap();
        run(&c, "SELECT dolt_commit('-A','-m','first')");
        run(&c, "SELECT dolt_branch('rb')");
        run(&c, "SELECT dolt_checkout('rb')");
        c.execute_batch("INSERT INTO t VALUES (2,'r');").unwrap();
        run(&c, "SELECT dolt_commit('-A','-m','rb work')");
        run(&c, "SELECT dolt_checkout('main')");
        c.execute_batch("INSERT INTO t VALUES (3,'m');").unwrap();
        run(&c, "SELECT dolt_commit('-A','-m','main work')");
        run(&c, "SELECT dolt_checkout('rb')");
    }
    // The rebase happens later, under a different pinned time: the rewritten commit carries that one.
    let _g = pin_clock(&c, T0 + 5000, 9).unwrap();
    run(&c, "SELECT dolt_rebase('main')");
    let ts = |msg: &str| -> i64 {
        q1(&c, &format!("SELECT CAST(strftime('%s', date) AS INTEGER) FROM dolt_log WHERE message = '{msg}'"))
    };
    assert_eq!(ts("rb work"), T0 / 1000 + 5);
    assert_eq!(ts("main work"), T0 / 1000);
}

/// Replays a fixed sequence of Dolt operations on a fresh store with a pinned
/// genesis and clock, and returns every commit hash in log order plus the tag.
fn dolt_history(genesis: Option<Genesis>, clock: i64) -> Vec<String> {
    let dir = tempfile::tempdir().unwrap();
    let c = open_dolt(&dir.path().join("cell.db"), genesis);
    c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT); INSERT INTO t VALUES (1,'x');").unwrap();
    let _g = pin_clock(&c, clock, 9).unwrap();
    run(&c, "SELECT dolt_commit('-A','-m','first')");
    run(&c, "SELECT dolt_branch('feature')");
    run(&c, "SELECT dolt_branch('pick')");
    run(&c, "SELECT dolt_checkout('pick')");
    c.execute_batch("INSERT INTO t VALUES (4,'p');").unwrap();
    let picked: String = q1(&c, "SELECT dolt_commit('-A','-m','pick work')");
    run(&c, "SELECT dolt_checkout('feature')");
    c.execute_batch("INSERT INTO t VALUES (2,'f');").unwrap();
    run(&c, "SELECT dolt_commit('-A','-m','feature work')");
    run(&c, "SELECT dolt_checkout('main')");
    c.execute_batch("INSERT INTO t VALUES (3,'m');").unwrap();
    let main2: String = q1(&c, "SELECT dolt_commit('-A','-m','main work')");
    run(&c, "SELECT dolt_merge('feature')");
    run(&c, "SELECT dolt_tag('v1')");
    run(&c, &format!("SELECT dolt_revert('{main2}')"));
    run(&c, &format!("SELECT dolt_cherry_pick('{picked}')"));
    run(&c, "SELECT dolt_branch('late')");
    run(&c, "SELECT dolt_checkout('late')");
    c.execute_batch("INSERT INTO t VALUES (5,'l');").unwrap();
    run(&c, "SELECT dolt_commit('-A','-m','late work')");
    run(&c, "SELECT dolt_checkout('main')");
    c.execute_batch("INSERT INTO t VALUES (6,'n');").unwrap();
    run(&c, "SELECT dolt_commit('-A','-m','newer main work')");
    run(&c, "SELECT dolt_checkout('late')");
    run(&c, "SELECT dolt_rebase('main')");
    let mut out: Vec<String> = rows_as_strings(&c, "SELECT commit_hash FROM dolt_log ORDER BY date, commit_hash")
        .into_iter()
        .flatten()
        .collect();
    out.extend(rows_as_strings(&c, "SELECT tag_name || ':' || tagger || ':' || date FROM dolt_tags").into_iter().flatten());
    out
}

#[test]
fn commit_hashes_are_reproducible_with_a_pinned_clock_and_genesis() {
    if !in_tz_child() {
        let utc = run_child_under_tz("commit_hashes_are_reproducible_with_a_pinned_clock_and_genesis", "UTC", "HASHES:");
        let la = run_child_under_tz("commit_hashes_are_reproducible_with_a_pinned_clock_and_genesis", "America/Los_Angeles", "HASHES:");
        assert_eq!(utc, la, "TZ must not change a commit hash");
        assert!(!utc.is_empty());
        return;
    }
    let a = dolt_history(Some(GENESIS), T0);
    let b = dolt_history(Some(GENESIS), T0);
    assert_eq!(a, b, "two independent stores, same inputs");
    assert!(a.len() >= 10, "{a:?}");
    // The genesis is pinned too: with another clock everything after it changes.
    let c = dolt_history(Some(GENESIS), T0 + 1000);
    assert_ne!(a, c);
    // Without a pinned genesis the seed commit (time and nonce) differs per store.
    let d = dolt_history(None, T0);
    let e = dolt_history(None, T0);
    assert_ne!(d, e);
    println!("HASHES:{}", a.join(","));
}

#[test]
fn epoch_zero_date_is_an_explicit_timestamp_not_unset() {
    let (_d, c) = fresh_dolt();
    c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);").unwrap();
    let _g = pin_clock(&c, T0, 1).unwrap();
    let h: String = q1(&c, "SELECT dolt_commit('-A','-m','epoch','--date','1970-01-01T00:00:00Z')");
    let ts: i64 = q1(&c, &format!("SELECT CAST(strftime('%s', date) AS INTEGER) FROM dolt_log WHERE commit_hash = '{h}'"));
    assert_eq!(ts, 0, "--date at the epoch must not fall back to the clock");
    c.execute_batch("INSERT INTO t VALUES (2);").unwrap();
    let h2: String = q1(&c, "SELECT dolt_commit('-A','-m','clock')");
    let ts2: i64 = q1(&c, &format!("SELECT CAST(strftime('%s', date) AS INTEGER) FROM dolt_log WHERE commit_hash = '{h2}'"));
    assert_eq!(ts2, T0 / 1000);
}

#[test]
fn genesis_commit_is_reproducible_across_independent_stores() {
    let head = |genesis: Option<Genesis>| -> (String, String) {
        let dir = tempfile::tempdir().unwrap();
        let c = open_dolt(&dir.path().join("g.db"), genesis);
        let rows = rows_as_strings(&c, "SELECT commit_hash, message FROM dolt_log");
        assert_eq!(rows.len(), 1, "{rows:?}");
        (rows[0][0].clone(), rows[0][1].clone())
    };
    let a = head(Some(GENESIS));
    let b = head(Some(GENESIS));
    assert_eq!(a, b);
    assert!(a.1.starts_with("Initialize data repository ("), "{}", a.1);
    let other = head(Some(Genesis { now_ms: GENESIS.now_ms, seed: GENESIS.seed + 1 }));
    assert_ne!(a.0, other.0, "the seed feeds the nonce");
    let later = head(Some(Genesis { now_ms: GENESIS.now_ms + 1000, seed: GENESIS.seed }));
    assert_ne!(a.0, later.0);
    assert_ne!(head(None).0, head(None).0, "unpinned stores differ");
    // The URI parameters are gone once the store exists: reopening does not re-pin anything.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("g.db");
    drop(open_dolt(&path, Some(GENESIS)));
    let c = open_dolt(&path, Some(Genesis { now_ms: 5, seed: 5 }));
    assert_eq!(rows_as_strings(&c, "SELECT commit_hash FROM dolt_log")[0][0], a.0);
    assert!(q1::<String>(&c, "SELECT datetime('now')").as_str() > "2025-01-01", "open-time context must be cleared");
}

#[test]
fn many_threads_with_different_contexts_each_see_their_own() {
    use std::sync::{Arc, Barrier};
    const THREADS: usize = 8;
    const ROUNDS: usize = 60;
    let probe = |c: &Connection| -> Vec<String> {
        rows_as_strings(
            c,
            "SELECT datetime('now'), random(), hex(randomblob(70)), datetime('2024-07-01 12:00:00','localtime'), last_insert_rowid(), changes()",
        )
        .remove(0)
    };
    // The expected value for each thread's context, computed alone.
    let expected: Vec<Vec<String>> = (0..THREADS)
        .map(|i| {
            let c = mem();
            let _g = pin_clock(&c, T0 + 1000 * i as i64, 100 + i as u64).unwrap();
            probe(&c)
        })
        .collect();
    assert_eq!(expected.iter().collect::<std::collections::HashSet<_>>().len(), THREADS);

    let barrier = Arc::new(Barrier::new(THREADS + 1));
    let handles: Vec<_> = (0..THREADS)
        .map(|i| {
            let barrier = barrier.clone();
            let want = expected[i].clone();
            std::thread::spawn(move || {
                let c = Connection::open_in_memory().unwrap();
                barrier.wait();
                for round in 0..ROUNDS {
                    let got = {
                        let _g = pin_clock(&c, T0 + 1000 * i as i64, 100 + i as u64).unwrap();
                        let got = rows_as_strings(
                            &c,
                            "SELECT datetime('now'), random(), hex(randomblob(70)), datetime('2024-07-01 12:00:00','localtime'), last_insert_rowid(), changes()",
                        )
                        .remove(0);
                        // Churn the connection so counters are dirty for the next round.
                        c.execute_batch("CREATE TEMP TABLE IF NOT EXISTS x(a); INSERT INTO x VALUES (1);").unwrap();
                        got
                    };
                    assert_eq!(got, want, "thread {i} round {round}");
                }
            })
        })
        .collect();
    // An unpinned connection running at the same time never sees a pinned clock or stream.
    let free = std::thread::spawn({
        let barrier = barrier.clone();
        move || {
            let c = Connection::open_in_memory().unwrap();
            barrier.wait();
            for _ in 0..ROUNDS {
                let now = q1::<String>(&c, "SELECT datetime('now')");
                assert!(now.as_str() > "2025-01-01", "{now}");
            }
        }
    });
    for h in handles {
        h.join().unwrap();
    }
    free.join().unwrap();
}
