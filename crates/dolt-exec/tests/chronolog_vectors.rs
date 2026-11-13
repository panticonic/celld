//! The per-connection PRNG reproduces the stream of Chronolog's process-global
//! `SQLITE_TESTCTRL_PRNG_SEED` approach, so a node on this engine draws the
//! same `random()` / `randomblob()` bytes for the same transaction as a node
//! on the 0.11.29 addon.
//!
//! The vectors below were produced by `@dolthub/doltlite@0.11.29` (patch
//! profile ...-v5) in chronolog-db, with
//! `db.setDeterministicContext(1700000000000n, <seed>, null)` and
//! `SELECT CAST(random() AS TEXT), hex(randomblob(70)), CAST(random() AS TEXT), hex(randomblob(1))`.

// Test code: the workspace clippy.toml restrictions are for the celld runtime.
#![allow(clippy::disallowed_macros, clippy::disallowed_methods, clippy::excessive_precision)]

mod common;

use celld_dolt_exec::pin_clock;
use common::*;
use rusqlite::Connection;

const VECTORS: &[(u64, [&str; 4])] = &[
    (
        1,
        [
            "-3228534820262478579",
            "10F656E6D1FD55053E50C4875C9930A33F6D0263BD14DFD6AB8C70521C19338B2308B95CF8D0BB7D202D2102780EA3528F1CB48560F76B2025A8F8968AA7242402170DE92C28",
            "7079258528792837104",
            "FB",
        ],
    ),
    (
        7,
        [
            "5363697700374643622",
            "7F05F073A1A31D46905AA8D5A71AEEEC560B9B18F039BE2DF1FCB92AB5911110CC2B897837CF0D6B066E246A6B11923F840FB483554153569C9BDE5F76678A5D6BB80B71BA63",
            "-2200804430816381844",
            "32",
        ],
    ),
    (
        4294967295,
        [
            "3110117772199606649",
            "946D1344B2FF7D4E1E8F2EE86961D3720B5D4930CB9F8E4028E1242FB3609874C1253DEF48F45D5CD11D0FC8F89ACE01B8D63D953E99CA6426CAA0645674D31FF950AAD9325D",
            "-7846432170669751344",
            "E3",
        ],
    ),
    (
        305419896,
        [
            "893052574789701531",
            "37DE81CC4F5E70EFEBA86B77B9FAC85A9D16A84EFEFB061603E12FDB9D7D93D70CCA1A8F2FF0685925893718D92C53D26E9D7863B22D0A4BDABBE089E0E6B417711DDFA5BD54",
            "8064444999653453444",
            "78",
        ],
    ),
];

#[test]
fn per_connection_prng_matches_the_chronolog_addon_stream() {
    for (seed, want) in VECTORS {
        let c = Connection::open_in_memory().unwrap();
        let _g = pin_clock(&c, 1_700_000_000_000, *seed).unwrap();
        let got = rows_as_strings(
            &c,
            "SELECT CAST(random() AS TEXT), hex(randomblob(70)), CAST(random() AS TEXT), hex(randomblob(1))",
        )
        .remove(0);
        assert_eq!(got, want.to_vec(), "seed {seed}");
    }
}
