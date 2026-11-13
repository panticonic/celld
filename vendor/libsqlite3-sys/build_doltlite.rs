//! Fetches, verifies, copies and patches the DoltLite amalgamation, in Rust.
//!
//! This replaces `doltlite/fetch.sh` + `sha256sum` + `tar` + `patch(1)` in the
//! build, so a Windows build needs none of `sh`, `patch` or `sha256sum`. The
//! only external program is `curl`, which Windows 10 (1803) and later ship as
//! `curl.exe`. `doltlite/fetch.sh` stays the single place the pin (version and
//! tarball sha256) lives: this reads it as data, and `crates/dolt-exec`'s
//! build script reads it the same way to compute the engine digest.

use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Pin {
    version: String,
    sha256: String,
}

fn read_pin(manifest: &Path) -> Pin {
    let text = fs::read_to_string(manifest.join("doltlite/fetch.sh"))
        .expect("cannot read doltlite/fetch.sh");
    let value = |key: &str| -> String {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(&format!("{key}=")).map(str::to_string))
            .unwrap_or_else(|| panic!("{key}= not found in doltlite/fetch.sh"))
    };
    Pin {
        version: value("version"),
        sha256: value("sha256"),
    }
}

/// The directory holding the pinned, unpatched amalgamation.
fn source_dir(manifest: &Path) -> PathBuf {
    if let Some(dir) = env::var_os("DOLTLITE_SRC_DIR") {
        return PathBuf::from(dir);
    }
    let pin = read_pin(manifest);
    let dest = manifest.join("doltlite/src");
    let marker = dest.join(".doltlite_release_version");
    if dest.join("sqlite3.c").exists()
        && fs::read_to_string(&marker).is_ok_and(|v| v.trim() == pin.version)
    {
        return dest;
    }
    fetch(&pin, &dest);
    dest
}

fn fetch(pin: &Pin, dest: &Path) {
    use sha2::{Digest, Sha256};
    let tmp = dest
        .parent()
        .unwrap()
        .join(format!(".fetch-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let url = format!(
        "https://github.com/dolthub/doltlite/releases/download/v{0}/doltlite-autoconf-{0}.tar.gz",
        pin.version
    );
    let archive = tmp.join("src.tar.gz");
    let status = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&archive)
        .arg(&url)
        .status()
        .expect("could not run curl to fetch the DoltLite amalgamation");
    assert!(status.success(), "curl failed for {url}");
    let bytes = fs::read(&archive).unwrap();
    let digest = Sha256::digest(&bytes);
    let actual: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        actual, pin.sha256,
        "sha256 of {url} does not match the pin in doltlite/fetch.sh"
    );
    let unpacked = tmp.join("x");
    fs::create_dir_all(&unpacked).unwrap();
    tar::Archive::new(flate2::read::GzDecoder::new(&bytes[..]))
        .unpack(&unpacked)
        .expect("could not unpack the DoltLite tarball");
    let root = unpacked.join(format!("doltlite-autoconf-{}", pin.version));
    fs::write(root.join(".doltlite_release_version"), &pin.version).unwrap();
    // Another build script may have finished first; either result is the
    // same pinned tree.
    let _ = fs::remove_dir_all(dest);
    if fs::rename(&root, dest).is_err() && !dest.join("sqlite3.c").exists() {
        panic!("could not place the DoltLite source at {}", dest.display());
    }
    let _ = fs::remove_dir_all(&tmp);
}

/// Copy the amalgamation into `out_dir/doltlite-src`, apply the patch set
/// there (the pinned source is never modified), and return that directory.
pub fn prepare(manifest: &Path, out_dir: &Path) -> PathBuf {
    println!("cargo:rerun-if-env-changed=DOLTLITE_SRC_DIR");
    println!("cargo:rerun-if-changed=doltlite/patches");
    println!("cargo:rerun-if-changed=doltlite/fetch.sh");
    let src = source_dir(manifest);
    assert!(
        src.join("sqlite3.c").exists(),
        "DoltLite amalgamation not found at {}",
        src.display()
    );
    println!("cargo:rerun-if-changed={}", src.join("sqlite3.c").display());

    let work = out_dir.join("doltlite-src");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(work.join("ext/blake3")).unwrap();
    for f in ["sqlite3.c", "sqlite3.h", "sqlite3ext.h"] {
        fs::copy(src.join(f), work.join(f)).unwrap();
    }
    for entry in fs::read_dir(src.join("ext/blake3")).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_file() {
            fs::copy(entry.path(), work.join("ext/blake3").join(entry.file_name())).unwrap();
        }
    }
    let mut patches: Vec<_> = fs::read_dir(manifest.join("doltlite/patches"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "patch"))
        .collect();
    patches.sort();
    for patch in &patches {
        let text = fs::read(patch).unwrap();
        if let Err(error) = apply_patch(&work, &text) {
            panic!("failed to apply {}: {error}", patch.display());
        }
    }
    work
}

/// How far from its stated line a hunk may sit and still apply, the way
/// `patch --forward` tolerates an offset.
const MAX_OFFSET: usize = 5_000;

fn trimmed(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

struct Hunk {
    old_start: usize,
    old: Vec<Vec<u8>>,
    new: Vec<Vec<u8>>,
}

/// Apply a unified diff (one or more files, `-p1`) under `root`. Text before
/// the first `---` line is a free-form description and is ignored.
fn apply_patch(root: &Path, patch: &[u8]) -> Result<(), String> {
    let lines: Vec<&[u8]> = patch.split_inclusive(|b| *b == b'\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let line = trimmed(lines[i]);
        if !(line.starts_with(b"--- ") && i + 1 < lines.len() && lines[i + 1].starts_with(b"+++ "))
        {
            i += 1;
            continue;
        }
        let name = file_name(line)?;
        i += 2;
        let mut hunks = Vec::new();
        while i < lines.len() && lines[i].starts_with(b"@@") {
            let (old_start, old_count, new_count) = parse_hunk_header(trimmed(lines[i]))?;
            i += 1;
            let (mut old, mut new) = (Vec::new(), Vec::new());
            while (old.len() < old_count || new.len() < new_count) && i < lines.len() {
                let raw = trimmed(lines[i]);
                match raw.first() {
                    Some(b' ') | None => {
                        old.push(raw.get(1..).unwrap_or(b"").to_vec());
                        new.push(raw.get(1..).unwrap_or(b"").to_vec());
                    }
                    Some(b'-') => old.push(raw[1..].to_vec()),
                    Some(b'+') => new.push(raw[1..].to_vec()),
                    Some(b'\\') => {}
                    Some(_) => return Err(format!("unexpected patch line in {name}")),
                }
                i += 1;
            }
            // A trailing "\ No newline at end of file" marker.
            while i < lines.len() && lines[i].starts_with(b"\\") {
                i += 1;
            }
            hunks.push(Hunk {
                old_start,
                old,
                new,
            });
        }
        let path = root.join(&name);
        let mut file = Vec::new();
        fs::File::open(&path)
            .and_then(|mut f| f.read_to_end(&mut file))
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let patched = apply_hunks(&file, &hunks).map_err(|error| format!("{name}: {error}"))?;
        fs::write(&path, patched).map_err(|error| format!("cannot write {name}: {error}"))?;
    }
    Ok(())
}

fn file_name(header: &[u8]) -> Result<String, String> {
    let text = String::from_utf8_lossy(&header[4..]).into_owned();
    let path = text.split(['\t', ' ']).next().unwrap_or_default();
    let path = path
        .strip_prefix("a/")
        .ok_or_else(|| format!("expected an a/ path in `{text}`"))?;
    Ok(path.to_string())
}

fn parse_hunk_header(line: &[u8]) -> Result<(usize, usize, usize), String> {
    let text = String::from_utf8_lossy(line).into_owned();
    let mut parts = text.split_whitespace();
    let (_, old, new) = (parts.next(), parts.next(), parts.next());
    let range = |part: Option<&str>, sign: char| -> Result<(usize, usize), String> {
        let part = part
            .and_then(|p| p.strip_prefix(sign))
            .ok_or_else(|| format!("bad hunk header `{text}`"))?;
        let (start, count) = part.split_once(',').unwrap_or((part, "1"));
        Ok((
            start.parse().map_err(|_| format!("bad hunk header `{text}`"))?,
            count.parse().map_err(|_| format!("bad hunk header `{text}`"))?,
        ))
    };
    let (old_start, old_count) = range(old, '-')?;
    let (_, new_count) = range(new, '+')?;
    Ok((old_start, old_count, new_count))
}

fn apply_hunks(file: &[u8], hunks: &[Hunk]) -> Result<Vec<u8>, String> {
    let source: Vec<&[u8]> = file.split_inclusive(|b| *b == b'\n').collect();
    let mut out = Vec::with_capacity(file.len() + 4096);
    let mut cursor = 0_usize;
    for (n, hunk) in hunks.iter().enumerate() {
        let wanted = hunk.old_start.saturating_sub(1);
        let matches_at = |at: usize| -> bool {
            at >= cursor
                && at + hunk.old.len() <= source.len()
                && hunk
                    .old
                    .iter()
                    .zip(&source[at..])
                    .all(|(old, actual)| trimmed(actual) == old.as_slice())
        };
        let start = (0..=MAX_OFFSET)
            .flat_map(|d| [wanted.checked_add(d), wanted.checked_sub(d)])
            .flatten()
            .find(|at| matches_at(*at))
            .ok_or_else(|| format!("hunk {} (line {}) does not apply", n + 1, hunk.old_start))?;
        for line in &source[cursor..start] {
            out.extend_from_slice(line);
        }
        for line in &hunk.new {
            out.extend_from_slice(line);
            out.push(b'\n');
        }
        cursor = start + hunk.old.len();
    }
    for line in &source[cursor..] {
        out.extend_from_slice(line);
    }
    Ok(out)
}
