//! Durable file operations on Windows.
//!
//! The Unix durability recipe is `write tmp; fsync(tmp); rename(tmp, final);
//! fsync(parent directory)`. Windows differs in two ways, and this module
//! carries the replacement for each. `docs/fork/windows.md` has the full
//! reasoning; the short form follows.
//!
//! **File flush.** `FlushFileBuffers` (what `File::sync_all` calls) needs a
//! handle opened with write access. The Unix idiom of opening a file
//! read-only and fsyncing it fails with `ERROR_ACCESS_DENIED`, so
//! [`sync_path`] opens the file for write (without truncating) first.
//!
//! **Directory flush.** Windows has no `fsync(dirfd)`. A directory can be
//! opened with `FILE_FLAG_BACKUP_SEMANTICS`, and NTFS accepts
//! `FlushFileBuffers` on that handle when it was opened with write access,
//! which flushes the directory's metadata through the NTFS log. [`sync_path`]
//! does that. When the filesystem refuses (`ERROR_ACCESS_DENIED`,
//! `ERROR_INVALID_FUNCTION`, `ERROR_NOT_SUPPORTED`; ReFS and network shares
//! behave differently) the directory flush is skipped, because there is no
//! stronger primitive short of flushing the whole volume, which needs
//! administrator rights. That fallback is logged once per process.
//!
//! **Rename.** [`rename`] calls `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`
//! (atomic replace of an existing target on the same volume) and
//! `MOVEFILE_WRITE_THROUGH`. The write-through flag is documented to cover
//! the copy-and-delete fallback of a cross-volume move; for a same-volume
//! rename NTFS performs one journaled metadata operation. To make that
//! journal record durable the destination is flushed after the move: an NTFS
//! flush of a file whose metadata changed forces the log up to that
//! record, and the log is sequential, so every earlier directory operation
//! becomes durable with it.

#![allow(clippy::disallowed_methods)]

use std::fs::OpenOptions;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_FUNCTION, ERROR_NOT_SUPPORTED, ERROR_SHARING_VIOLATION,
};
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, FILE_FLAG_BACKUP_SEMANTICS, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

static DIRECTORY_FLUSH_WARNED: AtomicBool = AtomicBool::new(false);

fn is_flush_unsupported(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error().map(|code| code as u32),
        Some(ERROR_ACCESS_DENIED | ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED)
    )
}

/// Make `path` durable: `FlushFileBuffers` on a file, the best available
/// metadata flush on a directory.
pub fn sync_path(path: &Path) -> io::Result<()> {
    if std::fs::metadata(path)?.is_dir() {
        let directory = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path);
        let result = directory.and_then(|directory| directory.sync_all());
        return match result {
            Err(error) if is_flush_unsupported(&error) => {
                if !DIRECTORY_FLUSH_WARNED.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        %error,
                        path = %path.display(),
                        "this filesystem cannot flush a directory; directory \
                         entries rely on the NTFS journal for durability"
                    );
                }
                Ok(())
            }
            other => other,
        };
    }
    OpenOptions::new().write(true).open(path)?.sync_all()
}

/// Attempts at a move that a scanner or indexer briefly holds open.
const RENAME_ATTEMPTS: u32 = 5;

/// Rename `from` onto `to`, replacing a file at `to`, and flush the result.
pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    if std::fs::symlink_metadata(from)?.is_dir() {
        // MOVEFILE_REPLACE_EXISTING does not replace a directory. The std
        // call keeps the same behavior as the Unix path for directories.
        std::fs::rename(from, to)?;
        return sync_path(to).or_else(|error| {
            if is_flush_unsupported(&error) {
                Ok(())
            } else {
                Err(error)
            }
        });
    }
    let wide = |path: &Path| -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let (source, target) = (wide(from), wide(to));
    let mut attempt = 0;
    loop {
        // SAFETY: both buffers are NUL-terminated and outlive the call.
        let moved = unsafe {
            MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if moved != 0 {
            break;
        }
        let error = io::Error::last_os_error();
        attempt += 1;
        let transient = matches!(
            error.raw_os_error().map(|code| code as u32),
            Some(ERROR_ACCESS_DENIED | ERROR_SHARING_VIOLATION)
        );
        if !transient || attempt >= RENAME_ATTEMPTS {
            return Err(error);
        }
        std::thread::sleep(std::time::Duration::from_millis(10 * u64::from(attempt)));
    }
    match sync_path(to) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Ok(()),
        other => other,
    }
}
