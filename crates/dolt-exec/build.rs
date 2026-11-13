//! Build identity of the engine, folded into `engine_digest()`.
//!
//! The vendored libsqlite3-sys (the DoltLite build) publishes where its
//! patched amalgamation and its patch set live. This script hashes both, so a
//! changed patch or a changed amalgamation changes the digest even if nobody
//! remembers to bump a version string.

use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let include = env::var("DEP_SQLITE3_INCLUDE").expect(
        "DEP_SQLITE3_INCLUDE is not set: celld-dolt-exec must build against the vendored \
         DoltLite libsqlite3-sys (see [patch.crates-io] in the workspace Cargo.toml)",
    );
    let patches_dir = PathBuf::from(
        env::var("DEP_SQLITE3_DOLTLITE_PATCHES")
            .expect("DEP_SQLITE3_DOLTLITE_PATCHES is not set; is vendor/libsqlite3-sys in use?"),
    );
    let cflags = env::var("DEP_SQLITE3_DOLTLITE_CFLAGS").unwrap_or_default();

    // The amalgamation actually compiled: pinned release source plus patches.
    let amalgamation = PathBuf::from(&include).join("sqlite3.c");
    println!("cargo:rerun-if-changed={}", amalgamation.display());
    let amalgamation_sha = sha256_file(&amalgamation);

    // The patch set: sorted by file name, each entry `name NUL sha256(content)`.
    println!("cargo:rerun-if-changed={}", patches_dir.display());
    let mut names: Vec<String> = fs::read_dir(&patches_dir)
        .expect("cannot read the DoltLite patch directory")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".patch"))
        .collect();
    names.sort();
    let mut set = Sha256::new();
    for name in &names {
        let body = fs::read(patches_dir.join(name)).unwrap();
        set.update(name.as_bytes());
        set.update([0u8]);
        set.update(hex::encode(Sha256::digest(&body)).as_bytes());
        set.update(b"\n");
    }

    // Release version and tarball digest, pinned in doltlite/fetch.sh.
    let fetch = patches_dir.parent().unwrap().join("fetch.sh");
    println!("cargo:rerun-if-changed={}", fetch.display());
    let fetch_text = fs::read_to_string(&fetch).expect("cannot read doltlite/fetch.sh");
    let pinned = |key: &str| -> String {
        fetch_text
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("{key}= not found in fetch.sh"))
            .trim()
            .to_string()
    };

    println!("cargo:rustc-env=CELLD_DOLTLITE_VERSION={}", pinned("version"));
    println!("cargo:rustc-env=CELLD_DOLTLITE_RELEASE_SHA256={}", pinned("sha256"));
    println!("cargo:rustc-env=CELLD_DOLTLITE_AMALGAMATION_SHA256={amalgamation_sha}");
    println!("cargo:rustc-env=CELLD_DOLTLITE_PATCH_SET_DIGEST={}", hex::encode(set.finalize()));
    println!("cargo:rustc-env=CELLD_DOLTLITE_PATCH_NAMES={}", names.join(","));
    println!("cargo:rustc-env=CELLD_DOLTLITE_CFLAGS={cflags}");
}

fn sha256_file(path: &PathBuf) -> String {
    hex::encode(Sha256::digest(
        fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display())),
    ))
}
