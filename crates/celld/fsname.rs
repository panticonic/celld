//! How a cell scope is spelled as a file system path.
//!
//! A cell scope (`celld_logic::cell::valid_cell_scope`) is ASCII alphanumerics
//! plus `_ - . : $`, up to 255 bytes, and it names a directory under the data
//! root: `<data>/<scope>/ltx/e<epoch>/db.sqlite`. On Unix the scope is the
//! directory name, unchanged, and that layout is the stored format.
//!
//! Three properties of the Windows file system break that mapping, and the
//! scope charset (the fence against path traversal) cannot be narrowed, since
//! every node of a fleet must accept the same scopes:
//!
//! * `:` is not a file name character. In an NTFS path `Class:abc` names the
//!   alternate data stream `abc` of a file `Class`, so the cell would not get
//!   a directory at all.
//! * NTFS is case-insensitive by default. Scopes that differ only in case
//!   (`Counter:AB` and `Counter:ab`) are different cells and must not share
//!   a directory.
//! * Trailing dots are stripped, and the device names (`CON`, `NUL`, `COM1`
//!   and the like) cannot be file names, with or without an extension.
//!
//! The Windows spelling escapes each of those with `~`, a character outside
//! the scope charset, so the mapping is injective and reversible:
//!
//! | scope character          | file name   |
//! | ------------------------ | ----------- |
//! | `:`                      | `~_`        |
//! | uppercase letter `X`     | `~x`        |
//! | a trailing `.`           | `~-`        |
//! | a reserved device stem   | `~!` prefix |
//!
//! A `/` in the argument separates components (facet streams are
//! `<root>/facets/<hash>`) and each component is escaped on its own; the
//! separator becomes a backslash, because a verbatim path (see [`data_root`])
//! does not accept `/`. Escaping widens a name, so a scope near the 255-byte
//! limit that is full of uppercase letters can exceed the component limit on
//! Windows; the file system call then fails with `ERROR_FILENAME_EXCED_RANGE`
//! rather than aliasing another cell.
//!
//! The data root itself goes through [`data_root`], which on Windows makes it
//! absolute and verbatim, so that paths longer than the legacy 260-character
//! limit open, and rejects a root too long for SQLite's Windows VFS (a
//! 1040-byte path buffer).

use std::borrow::Cow;

const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// The path (relative to the data root) for `cell`, a scope or a
/// `/`-separated facet stream.
pub fn cell_path(cell: &str) -> Cow<'_, str> {
    if cfg!(windows) {
        Cow::Owned(encode(cell))
    } else {
        Cow::Borrowed(cell)
    }
}

/// The cell scope for a data-root directory name produced by [`cell_path`].
pub fn cell_from_dir_name(name: &str) -> Cow<'_, str> {
    if cfg!(windows) {
        Cow::Owned(decode(name))
    } else {
        Cow::Borrowed(name)
    }
}

fn encode(cell: &str) -> String {
    let mut out = String::with_capacity(cell.len() + 8);
    for (index, component) in cell.split('/').enumerate() {
        if index > 0 {
            out.push('\\');
        }
        let stem = component.split('.').next().unwrap_or_default();
        if RESERVED.contains(&stem) {
            out.push_str("~!");
        }
        let last = component.chars().count().saturating_sub(1);
        for (position, character) in component.chars().enumerate() {
            match character {
                ':' => out.push_str("~_"),
                '.' if position == last => out.push_str("~-"),
                'A'..='Z' => {
                    out.push('~');
                    out.push(character.to_ascii_lowercase());
                }
                other => out.push(other),
            }
        }
    }
    out
}

fn decode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut characters = name.chars();
    while let Some(character) = characters.next() {
        if character != '~' {
            out.push(if character == '\\' { '/' } else { character });
            continue;
        }
        match characters.next() {
            Some('_') => out.push(':'),
            Some('-') => out.push('.'),
            Some('!') | None => {}
            Some(letter) => out.push(letter.to_ascii_uppercase()),
        }
    }
    out
}

/// The longest data root that still leaves room, inside SQLite's 1040-byte
/// Windows path buffer, for a 255-byte escaped scope and the
/// `ltx/e<epoch>/db.sqlite-wal` tail.
#[cfg(windows)]
const MAX_DATA_ROOT_BYTES: usize = 700;

/// The data root as the process should use it. Unix: unchanged. Windows:
/// created if missing, absolute, in verbatim form (`canonicalize` returns
/// one), and checked against the length SQLite's Windows VFS allows.
// Resolving the node's own root happens before the injected filesystem exists.
#[allow(clippy::disallowed_methods)]
pub fn data_root(path: std::path::PathBuf) -> anyhow::Result<std::path::PathBuf> {
    #[cfg(windows)]
    {
        use anyhow::Context as _;
        std::fs::create_dir_all(&path)
            .with_context(|| format!("create data directory {}", path.display()))?;
        let root = std::fs::canonicalize(&path)
            .with_context(|| format!("resolve data directory {}", path.display()))?;
        anyhow::ensure!(
            root.as_os_str().len() <= MAX_DATA_ROOT_BYTES,
            "the data directory {} is {} bytes long; SQLite's Windows VFS limits a database \
             path to 1040 bytes, so the data directory must be at most {MAX_DATA_ROOT_BYTES}",
            root.display(),
            root.as_os_str().len()
        );
        Ok(root)
    }
    #[cfg(not(windows))]
    {
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_is_case_and_colon_safe() {
        for scope in [
            "Counter:0123abcd",
            "Counter:AB",
            "Counter:ab",
            "plain",
            "CON",
            "con",
            "nul.txt",
            "a.",
            "Class:x/facets/00ff",
            "$a-b_c.d",
        ] {
            let encoded = encode(scope);
            assert_eq!(decode(&encoded), scope, "{scope} -> {encoded}");
            assert!(!encoded.contains(':'), "{encoded}");
            assert!(!encoded.contains('/'), "{encoded}");
            assert!(!encoded.ends_with('.'), "{encoded}");
            assert_eq!(encoded, encoded.to_ascii_lowercase(), "{encoded}");
        }
        assert_ne!(encode("Counter:AB"), encode("Counter:ab"));
        assert_eq!(encode("Counter:ab"), "~counter~_ab");
        assert_eq!(encode("con"), "~!con");
        assert_eq!(encode("a/b"), "a\\b");
    }
}
