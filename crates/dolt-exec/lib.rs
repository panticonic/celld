//! Deterministic SQL execution for Dolt cells.
//!
//! `celld-dolt-exec` is Chronolog's Spec 10 (replicated SQL transactions)
//! implemented once, natively, on top of the DoltLite build in
//! `vendor/libsqlite3-sys` and its patch 0002 (per-connection clock, PRNG and
//! timezone). See `docs/fork/design.md` sections 3.3, 3a and 4.
//!
//! - [`execute`] / [`execute_in_transaction`]: run a [`Program`] under an
//!   [`ExecContext`] and get an [`Outcome`].
//! - [`engine_digest`]: the hash of everything that decides what a program
//!   computes.
//! - [`pin_clock`], [`prolly_uri`]: the same pinned clock for Dolt commits and
//!   genesis, outside programs.
//! - [`authorizer`]: the consensus block-list as a pure function.
//!
//! `celld` re-exports this crate as `celld::dolt_exec`.

pub mod authorizer;
mod exec;
mod ffi;
mod manifest;

pub use authorizer::{AuthMode, AuthOptions, Authorizer};
pub use exec::{
    check_statement, det_context_active, execute, execute_in_transaction, local_query, LocalRows, pin_clock, prng_seed_from_tx_id, register_functions, Binding, DetScope,
    ExecContext, ExecError, ExecState, Limits, Outcome, Parameter, Profile, Program, SqlValue, Statement,
};
pub use manifest::{
    engine_digest, engine_digest_hex, engine_manifest, EngineManifest, EXEC_PROFILE, SQLITE_VEC_SOURCE_SHA256,
    SQLITE_VEC_VERSION,
};

use std::path::Path;

/// Pins the genesis ("Initialize data repository") commit of a NEW prolly
/// store: the clock and the PRNG seed that makes its nonce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Genesis {
    pub now_ms: i64,
    pub seed: u64,
}

/// The URI that opens `path` as a prolly (Dolt) store, creating it when the
/// file is new. With `genesis`, two independently created stores get the same
/// seed commit, so the same genesis hash.
///
/// Open it with `OpenFlags::SQLITE_OPEN_URI`. The genesis parameters are only
/// read while the store is being created.
pub fn prolly_uri(path: &Path, genesis: Option<Genesis>) -> String {
    let escaped: String = path
        .to_string_lossy()
        .chars()
        .flat_map(|c| match c {
            '%' => vec!['%', '2', '5'],
            '?' => vec!['%', '3', 'f'],
            '#' => vec!['%', '2', '3'],
            c => vec![c],
        })
        .collect();
    let mut uri = format!("file:{escaped}?doltlite_engine=prolly");
    if let Some(g) = genesis {
        uri.push_str(&format!("&doltlite_det_ms={}&doltlite_det_seed={}", g.now_ms, g.seed as i64));
    }
    uri
}
