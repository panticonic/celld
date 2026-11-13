// This crate is infrastructure outside celld's execution-determinism boundary:
// it owns host files and a blocking pool, like `local_store.rs`. The boundary
// lints exist for node storage, which goes through the injected filesystem.
#![allow(clippy::disallowed_methods)]

//! A local `object_store` backend over Vibestudio's content-addressed blobstore.
//!
//! See `docs/fork/vibestore.md` for the design, the on-disk format and the
//! crash-safety argument.

pub mod cas;
pub mod core;
pub mod error;
pub mod index;
pub mod retention;
pub mod spec;
pub mod store;

pub use crate::core::{Core, CrashPoint, Recovery};
pub use crate::error::StoreError;
pub use crate::spec::{is_spec, VibeSpec, SCHEME};
pub use crate::store::{tokio_blocker, Blocker, VibeStore};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, Weak};

type Registry = Mutex<HashMap<(PathBuf, PathBuf), Weak<VibeStore>>>;

/// The one store for a (blob root, index) pair in this process.
///
/// celld opens a bucket several times (a dedicated lease client, the ownership
/// client, the replica store). Handing every opener the same instance keeps one
/// SQLite connection pool and one write gate per index instead of one per
/// caller. The first opener's `blocker` is the one that sticks.
pub fn open_shared(spec: &VibeSpec, blocker: Blocker) -> Result<Arc<VibeStore>, StoreError> {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    let registry = REGISTRY.get_or_init(Default::default);
    let mut registry = registry.lock().unwrap_or_else(|p| p.into_inner());
    let key = (spec.root.clone(), spec.index.clone());
    if let Some(store) = registry.get(&key).and_then(Weak::upgrade) {
        if store.spec() != spec {
            return Err(StoreError::Other(format!(
                "{} is already open with different parameters ({} vs {})",
                spec.root.display(),
                store.spec().canonical(),
                spec.canonical()
            )));
        }
        return Ok(store);
    }
    let store = Arc::new(VibeStore::open(spec, blocker)?);
    registry.insert(key, Arc::downgrade(&store));
    Ok(store)
}
