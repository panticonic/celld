//! `engine_digest()`: the hash of everything that decides what a replicated
//! program computes. Chronolog's `digestNativeEngineManifest`, for this engine.

// Test code: the workspace clippy.toml restrictions are for the celld runtime.
#![allow(clippy::disallowed_macros, clippy::disallowed_methods, clippy::excessive_precision)]

use celld_dolt_exec::{engine_digest, engine_digest_hex, engine_manifest, EngineManifest};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn digest_is_stable_and_is_sha256_of_the_descriptor() {
    let m = engine_manifest();
    assert_eq!(engine_digest(), engine_digest());
    assert_eq!(engine_digest(), <[u8; 32]>::from(Sha256::digest(m.descriptor().as_bytes())));
    assert_eq!(engine_digest_hex().len(), 64);
    assert!(m.descriptor().starts_with("celld-engine-manifest-v1\n{"));
    println!("engine digest {}", engine_digest_hex());
}

#[test]
fn descriptor_is_compact_and_key_sorted_at_every_level() {
    let m = engine_manifest();
    let d = m.descriptor();
    let json = d.strip_prefix("celld-engine-manifest-v1\n").unwrap();
    assert!(!json.contains(": ") && !json.contains(", ") && !json.contains('\n'));
    let v: serde_json::Value = serde_json::from_str(json).unwrap();
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    // The text keeps the keys in sorted order too.
    let positions: Vec<usize> = sorted.iter().map(|k| json.find(&format!("\"{k}\":")).unwrap()).collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn every_input_changes_the_digest() {
    let base = engine_manifest().clone();
    let digest = |m: &EngineManifest| m.digest();
    let variants: Vec<(&str, EngineManifest)> = vec![
        ("doltlite version", EngineManifest { doltlite_version: "0.0.0".into(), ..base.clone() }),
        ("release sha", EngineManifest { doltlite_release_sha256: "x".into(), ..base.clone() }),
        ("amalgamation sha", EngineManifest { doltlite_amalgamation_sha256: "x".into(), ..base.clone() }),
        ("patch set", EngineManifest { patch_set_digest: "x".into(), ..base.clone() }),
        ("patch names", EngineManifest { patch_names: vec![], ..base.clone() }),
        ("cflags", EngineManifest { doltlite_cflags: vec![], ..base.clone() }),
        ("sqlite version", EngineManifest { sqlite_version: "3.0".into(), ..base.clone() }),
        ("compile options", EngineManifest { compile_options: vec![], ..base.clone() }),
        ("sqlite-vec version", EngineManifest { sqlite_vec_version: "0.0.0".into(), ..base.clone() }),
        ("sqlite-vec source", EngineManifest { sqlite_vec_source_sha256: "x".into(), ..base.clone() }),
        ("exec profile", EngineManifest { exec_profile: "other".into(), ..base.clone() }),
        ("fts5", EngineManifest { fts5: !base.fts5, ..base.clone() }),
    ];
    let mut seen = std::collections::HashSet::new();
    seen.insert(digest(&base));
    for (name, m) in variants {
        assert!(seen.insert(digest(&m)), "changing {name} must change the digest");
    }
}

#[test]
fn manifest_describes_the_linked_engine() {
    let m = engine_manifest();
    assert_eq!(m.doltlite_version, "0.51.11");
    assert!(m.sqlite_version.starts_with("3.55"), "{}", m.sqlite_version);
    assert_eq!(m.doltlite_release_sha256, "b88e2192a7fb0e0e04502556ec599799a510aa93385ef4f8bb002adcb22dde01");
    assert!(m.patch_names.iter().any(|n| n.starts_with("0001-")));
    assert!(m.patch_names.iter().any(|n| n.starts_with("0002-")));
    assert!(m.doltlite_cflags.iter().any(|f| f == "-DDOLTLITE_DEFAULT_STOCK=1"));
    assert!(m.compile_options.iter().any(|o| o == "ENABLE_FTS5"));
    assert!(m.compile_options.windows(2).all(|w| w[0] <= w[1]), "sorted");
    assert!(m.fts5 && m.json1 && m.rtree && !m.dynamic_extensions);
    assert_eq!(m.sqlite_vec_version, "0.1.9");
}

/// The patch-set digest and the patch list are what is on disk, not a stale constant.
#[test]
fn patch_set_digest_matches_the_patch_files() {
    let dir = workspace().join("vendor/libsqlite3-sys/doltlite/patches");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".patch"))
        .collect();
    names.sort();
    assert_eq!(names, engine_manifest().patch_names);
    let mut set = Sha256::new();
    for n in &names {
        set.update(n.as_bytes());
        set.update([0u8]);
        set.update(hex::encode(Sha256::digest(std::fs::read(dir.join(n)).unwrap())).as_bytes());
        set.update(b"\n");
    }
    assert_eq!(hex::encode(set.finalize()), engine_manifest().patch_set_digest);
}

/// The recorded sqlite-vec source digest is the one of the pinned crate.
#[test]
fn sqlite_vec_source_digest_matches_the_pinned_crate() {
    let Some(home) = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo"))) else {
        return;
    };
    let Ok(registries) = std::fs::read_dir(home.join("registry/src")) else { return };
    for registry in registries.flatten() {
        let source = registry.path().join("sqlite-vec-0.1.9/sqlite-vec.c");
        if let Ok(bytes) = std::fs::read(&source) {
            assert_eq!(hex::encode(Sha256::digest(bytes)), engine_manifest().sqlite_vec_source_sha256);
            return;
        }
    }
}
