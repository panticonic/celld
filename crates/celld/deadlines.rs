// Copyright 2026 Deno Land Inc. Apache-2.0 license.

//! The node-level switch for implicit deadlines.
//!
//! celld ships several time-based watchdogs that nobody asked for: they end an
//! operation, a stream or a wait because it took too long. Some mirror
//! Cloudflare (the 30 s `blockConcurrencyWhile` limit), some bound a real
//! external resource (an object-store acknowledgement). A deployment that owns
//! its own lifecycle, such as an embedding that already cancels work on explicit
//! events, wants none of them to fire on its behalf.
//!
//! `--no-implicit-deadlines` (or `CELLD_NO_IMPLICIT_DEADLINES=1`) turns every
//! watchdog in docs/fork/deadlines.md marked "node option" off. The default
//! keeps Cloudflare semantics. Deadlines the application asked for
//! (`AbortSignal.timeout`, workflow step timeouts, `setTimeout`, queue
//! visibility timeouts) and the process-wide shutdown bound are never affected:
//! they are explicit, or they are the explicit stop event itself.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// The environment spelling of `--no-implicit-deadlines`.
pub const ENV_NAME: &str = "CELLD_NO_IMPLICIT_DEADLINES";

static DISABLED_BY_FLAG: AtomicBool = AtomicBool::new(false);
static DISABLED_BY_ENV: OnceLock<bool> = OnceLock::new();

/// Record `--no-implicit-deadlines`. Called while the command line is parsed,
/// before any worker thread reads the policy.
pub fn disable_implicit_deadlines() {
    DISABLED_BY_FLAG.store(true, Ordering::Relaxed);
}

/// Whether implicit deadlines are off for this node. The environment is read
/// once; `env_vars::validate` has already refused a malformed value.
pub fn implicit_deadlines_disabled() -> bool {
    DISABLED_BY_FLAG.load(Ordering::Relaxed)
        || *DISABLED_BY_ENV.get_or_init(|| std::env::var(ENV_NAME).as_deref() == Ok("1"))
}

/// A deadline expressed as "never": far enough that no clock reaches it and
/// small enough that adding it to a timestamp cannot overflow.
pub const UNBOUNDED_MS: u64 = u64::MAX / 4;

/// `default_ms`, or `UNBOUNDED_MS` when implicit deadlines are off.
pub fn bound_ms(default_ms: u64) -> u64 {
    if implicit_deadlines_disabled() {
        UNBOUNDED_MS
    } else {
        default_ms
    }
}
