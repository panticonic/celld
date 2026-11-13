// Copyright 2026 Deno Land Inc. Apache-2.0 license.

//! The `vibestore://` bucket: celld's adapter over the `celld-vibestore`
//! crate, which stores immutable objects in Vibestudio's content-addressed
//! blobstore and keeps mutable keys (ownership records, leases, pointers) in a
//! small SQLite index. See `docs/fork/vibestore.md`.
//!
//! The store itself lives in its own crate so it builds and tests without the
//! rest of celld. This file supplies the one thing the crate cannot know: how
//! to run a blocking job on celld's execution domain.

use celld_vibestore::{Blocker, VibeSpec, VibeStore};
use std::sync::Arc;

/// Run store jobs on the execution domain's blocking pool, like `LocalStore`.
fn blocker() -> Blocker {
    Arc::new(|job| {
        crate::asyncrt::blocking(job).detach();
    })
}

/// The one store for a `vibestore://` spec in this process, plus its parsed
/// form. Opening twice (the lease client, the ownership client, the replica
/// store) returns the same store.
pub(crate) fn open(spec: &str) -> anyhow::Result<(Arc<VibeStore>, VibeSpec)> {
    let spec = VibeSpec::parse(spec)?;
    let store = celld_vibestore::open_shared(&spec, blocker())?;
    Ok((store, spec))
}
