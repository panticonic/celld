//! The engine manifest and its digest, in the spirit of Chronolog's
//! `digestNativeEngineManifest`: everything that decides what a replicated
//! program computes, hashed into one value that two replicas compare before
//! they agree to execute the same transactions.
//!
//! The hashed descriptor is `celld-engine-manifest-v1\n` followed by the
//! manifest as recursively key-sorted JSON without whitespace.

use rusqlite::Connection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

/// Bump when the Rust-side execution semantics change (authorizer table,
/// rejection mapping, profile pragmas, limits handling).
pub const EXEC_PROFILE: &str = "celld-spec10-v1";

/// The sqlite-vec crate is pinned exactly (`=0.1.9`) in the workspace.
pub const SQLITE_VEC_VERSION: &str = "0.1.9";
/// SHA-256 of the `sqlite-vec.c` shipped in the sqlite-vec 0.1.9 crate. A test
/// recomputes it from the cargo registry when the source is available.
pub const SQLITE_VEC_SOURCE_SHA256: &str = "ba081a47fa02eadc3cf6b16c314b695b84081269349aac722b4efa338fe8fd85";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineManifest {
    pub doltlite_version: String,
    /// SHA-256 of the pinned release tarball (`doltlite/fetch.sh`).
    pub doltlite_release_sha256: String,
    /// SHA-256 of the patched `sqlite3.c` that was compiled.
    pub doltlite_amalgamation_sha256: String,
    /// SHA-256 over the sorted patch files (name and content hash).
    pub patch_set_digest: String,
    pub patch_names: Vec<String>,
    /// The DoltLite-specific compile defines.
    pub doltlite_cflags: Vec<String>,
    /// `sqlite_version()` of the linked engine.
    pub sqlite_version: String,
    /// Sorted `PRAGMA compile_options`.
    pub compile_options: Vec<String>,
    pub sqlite_vec_version: String,
    pub sqlite_vec_source_sha256: String,
    pub exec_profile: String,
    pub fts5: bool,
    pub json1: bool,
    pub rtree: bool,
    pub dynamic_extensions: bool,
}

impl EngineManifest {
    fn read() -> EngineManifest {
        let conn = Connection::open_in_memory().expect("in-memory connection for the engine manifest");
        let sqlite_version: String = conn.query_row("SELECT sqlite_version()", [], |r| r.get(0)).unwrap();
        let mut compile_options: Vec<String> = conn
            .prepare("PRAGMA compile_options")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        compile_options.sort();
        // Probe the features instead of reading option names: JSON is built in
        // and no longer appears in compile_options.
        let has_module = |m: &str| -> bool {
            conn.query_row("SELECT count(*) FROM pragma_module_list WHERE name = ?1", [m], |r| r.get::<_, i64>(0)).unwrap() > 0
        };
        let has_json = conn.query_row("SELECT json_valid('{}')", [], |r| r.get::<_, i64>(0)).unwrap_or(0) == 1;
        EngineManifest {
            doltlite_version: env!("CELLD_DOLTLITE_VERSION").into(),
            doltlite_release_sha256: env!("CELLD_DOLTLITE_RELEASE_SHA256").into(),
            doltlite_amalgamation_sha256: env!("CELLD_DOLTLITE_AMALGAMATION_SHA256").into(),
            patch_set_digest: env!("CELLD_DOLTLITE_PATCH_SET_DIGEST").into(),
            patch_names: env!("CELLD_DOLTLITE_PATCH_NAMES").split(',').map(str::to_string).collect(),
            doltlite_cflags: env!("CELLD_DOLTLITE_CFLAGS").split_whitespace().map(str::to_string).collect(),
            sqlite_version,
            fts5: has_module("fts5"),
            json1: has_json,
            rtree: has_module("rtree"),
            // Extension loading is compiled in but switched off per connection
            // by the execution profile (DBCONFIG_ENABLE_LOAD_EXTENSION=0) and
            // denied by the authorizer (`load_extension`).
            dynamic_extensions: false,
            compile_options,
            sqlite_vec_version: SQLITE_VEC_VERSION.into(),
            sqlite_vec_source_sha256: SQLITE_VEC_SOURCE_SHA256.into(),
            exec_profile: EXEC_PROFILE.into(),
        }
    }

    /// The manifest as a JSON value with Chronolog's camelCase field names.
    pub fn to_json(&self) -> Value {
        json!({
            "doltliteVersion": self.doltlite_version,
            "doltliteReleaseSha256": self.doltlite_release_sha256,
            "doltliteAmalgamationSha256": self.doltlite_amalgamation_sha256,
            "patchSetDigest": self.patch_set_digest,
            "patchNames": self.patch_names,
            "doltliteCflags": self.doltlite_cflags,
            "sqliteVersion": self.sqlite_version,
            "compileOptions": self.compile_options,
            "sqliteVecVersion": self.sqlite_vec_version,
            "sqliteVecSourceSha256": self.sqlite_vec_source_sha256,
            "execProfile": self.exec_profile,
            "fts5": self.fts5,
            "json1": self.json1,
            "rtree": self.rtree,
            "dynamicExtensions": self.dynamic_extensions,
        })
    }

    /// `celld-engine-manifest-v1\n` + key-sorted compact JSON.
    pub fn descriptor(&self) -> String {
        // serde_json's map is a BTreeMap here (no `preserve_order`), so keys
        // come out sorted at every level; the output carries no whitespace.
        format!("celld-engine-manifest-v1\n{}", serde_json::to_string(&self.to_json()).unwrap())
    }

    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.descriptor().as_bytes()).into()
    }
}

/// The linked engine's manifest, read once.
pub fn engine_manifest() -> &'static EngineManifest {
    static MANIFEST: OnceLock<EngineManifest> = OnceLock::new();
    MANIFEST.get_or_init(EngineManifest::read)
}

/// SHA-256 of the engine manifest: what `ctx.storage.dolt.engineDigest` exposes.
pub fn engine_digest() -> [u8; 32] {
    engine_manifest().digest()
}

/// Lower-case hex of [`engine_digest`].
pub fn engine_digest_hex() -> String {
    hex::encode(engine_digest())
}
