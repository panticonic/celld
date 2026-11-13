# Windows x64 port

Status of the `x86_64-pc-windows-msvc` target in the fork (design.md section 5:
Windows is required, because Vibestudio ships Windows and there is no dual
runtime). Branch `feature/windows`.

## 1. What was verified, and where

| Check | Result |
| --- | --- |
| `cargo xwin check --workspace --all-targets --target x86_64-pc-windows-msvc` (Linux host, clang-cl 19 + xwin MSVC CRT/SDK) | clean |
| `cargo xwin build -p celld --target x86_64-pc-windows-msvc` (cross-link with lld-link, debug profile) | links: `celld.exe`, a 120 MB PE32+ console image (V8's 235 MB `rusty_v8.lib` included). Release / fat-LTO not tried |
| Linux native: `cargo test -p doltlite-spike`, `cargo build -p celld-dolt-exec` with the new Rust build script | pass; the patched amalgamation is byte-identical to the `patch(1)` result |
| Anything that executes on Windows | **not run**; `.github/workflows/windows.yml` does it |

Everything under "Untested" below is a claim about Windows behavior that
cross-compilation cannot confirm. The CI workflow is the verification plan.

## 2. Build

### v8

`v8 = 152.1` publishes a prebuilt static library for `x86_64-pc-windows-msvc`
(`rusty_v8.lib`, 235 MB, fetched by the crate's build script; no
`RUSTY_V8_MIRROR` needed). No V8 source build is involved.

### DoltLite / SQLite C build (`vendor/libsqlite3-sys`)

The old build ran `sh doltlite/fetch.sh` (needs `sh`, `curl`, `sha256sum`,
`tar`) and then `patch(1)`. None of that exists on a stock Windows machine.

`build_doltlite.rs` (new, included by `build.rs`) now does the whole step in
Rust:

* **Pin.** Read from `doltlite/fetch.sh` as data (`version=`, `sha256=`), so
  that file stays the single source of truth that `crates/dolt-exec/build.rs`
  also hashes into the engine digest.
* **Fetch.** `curl` (`curl.exe` ships with Windows 10 1803 and later) into a
  temp directory; the sha256 is checked with the `sha2` crate; the tarball is
  unpacked with `flate2` + `tar`. The result is cached in `doltlite/src`
  (gitignored) exactly as before. `DOLTLITE_SRC_DIR` still overrides.
* **Patch.** A small unified-diff applier (verifies every context and removed
  line, tolerates a line offset like `patch --forward`, strips `\r`). It is
  applied to the copy in `OUT_DIR`, never to the pinned source. Output was
  compared with GNU `patch` and is byte-identical.
* `.gitattributes` forces LF for `vendor/libsqlite3-sys/doltlite/**` and
  `*.patch`. The patch set is hashed into the engine digest, so a CRLF
  checkout on Windows would otherwise change the digest.
* **MSVC flags.** The amalgamation embeds ed25519's `fixedint.h`. Its own
  `<stdint.h>` probe tests `__STDC__`, `__GNUC__` and Watcom, none of which
  hold under MSVC or clang-cl, so it typedefs `uint32_t` as `unsigned long`
  against the CRT's `unsigned int` and the compile fails. On MSVC the build
  adds `-DFIXEDINT_H_INCLUDED -FIstdint.h`. It is a compile flag, not a
  source patch, so the patch set and the engine digest are identical on every
  platform.
* **BLAKE3.** `DOLTLITE_CFLAGS` already disables SSE4.1, AVX2 and AVX-512;
  `blake3_sse2.c` is compiled for x86_64 (SSE2 is the x64 baseline).
  `blake3_neon.c` is selected for aarch64, which includes Windows on ARM,
  but that target is untried.
* The SQLite Windows VFS is used as is (`SQLITE_OS_WIN` is implied by the
  target); `-DHAVE_LOCALTIME_R` is already skipped for Windows targets.

### C runtime

The linked image imports `VCRUNTIME140.dll`, `VCRUNTIME140_1.dll` and the
Universal CRT (`api-ms-win-crt-*`): the dynamic CRT, which is what the prebuilt
V8 library and `cc` default to. Vibestudio must ship, or require, the Visual C++
redistributable until the whole link moves to the static CRT
(`-C target-feature=+crt-static`, which the DoltLite build script already
honors). That switch is not tried here, and whether the prebuilt V8 library links
cleanly against the static CRT was not checked.

### Allocator

`tikv-jemallocator` does not support MSVC. `tikv-jemalloc-ctl` and
`tikv-jemallocator` are now `[target.'cfg(not(windows))'.dependencies]`,
`main.rs` selects `#[global_allocator]` only off Windows, and Windows uses the
system (Win32 heap) allocator. Consequences: see Memory below.

### windows-sys

One workspace dependency, `windows-sys 0.61`, with the feature set listed in
the root `Cargo.toml`. Used by `crates/celld` (`winos.rs`) and `crates/ltx`
(`winfs.rs`), Windows targets only.

## 3. Site table

Unix-specific sites and what the Windows build does. "Same" means the Unix
code was already portable and is untouched.

| # | Site | Unix | Windows |
| - | --- | --- | --- |
| 1 | `main.rs` global allocator | jemalloc | system allocator |
| 2 | `memory.rs` `resident_bytes` | `/proc/self/statm`, `proc_pidinfo` | `K32GetProcessMemoryInfo`, `WorkingSetSize` (`winos::working_set_bytes`) |
| 3 | `memory.rs` `allocator_slack_bytes`, `allocator_stats`, `tune_allocator` | jemalloc ctl | slack 0 (sample equals the working set), stats `None` (`/state` reports null), tuning no-op |
| 4 | `memory.rs` `libc_malloc_stats`, `trim_c_heap` | `mallinfo2`, `malloc_trim` (Linux) | already `None` / no-op for non-Linux; the Win32 heap returns pages itself |
| 5 | `memory.rs` `cgroup_*` | cgroup files | already `None` off Linux |
| 6 | `machine.rs` `total_memory_bytes` | `/proc/meminfo`, `sysctl hw.memsize` | `GlobalMemoryStatusEx().ullTotalPhys` |
| 7 | `host_services.rs` open fds | `/proc/self/fd` | `GetProcessHandleCount` |
| 8 | `host_services.rs` fd limit | `getrlimit(NOFILE)` | constant 2^24, the per-process handle limit (telemetry only) |
| 9 | `host_services.rs` CPU ticks | `/proc/self/stat`, `sysconf(CLK_TCK)` | `GetProcessTimes` in 100 ns units, tick rate 10,000,000 |
| 10 | `startup.rs` `raise_file_limit` | raise `RLIMIT_NOFILE` | not needed; already a no-op `cfg(not(unix))` |
| 11 | `startup.rs` listeners | `SO_REUSEADDR` | not set (on Windows it permits port hijacking); already `cfg(unix)` |
| 12 | `main.rs` SIGTERM / SIGINT | `tokio::signal::unix` | `startup::TerminationSignals`: CTRL_C, CTRL_BREAK, CTRL_CLOSE, CTRL_LOGOFF, CTRL_SHUTDOWN all begin the same graceful handoff |
| 13 | SIGHUP reload | none exists in this tree | n/a. Reload is `POST /reload` and the credential-rotation re-exec (site 17) |
| 14 | `dev.rs` shutdown signals | SIGINT/SIGTERM | same `TerminationSignals` |
| 15 | `dev.rs` node in its own process group | `process_group(0)` | `CREATE_NEW_PROCESS_GROUP`: the node ignores the console's Ctrl-C, so only the supervisor picks the shutdown mode |
| 16 | `dev.rs` `PR_SET_PDEATHSIG` | node dies with the supervisor | Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, the child assigned right after spawn (`winos::kill_on_parent_exit`) |
| 17 | `dev.rs` `kill(SIGTERM)` fallback | graceful stop | `GenerateConsoleCtrlEvent(CTRL_BREAK)` to the node's group, which the node treats as the graceful stop. `Child::kill` (`TerminateProcess`) remains the 35 s timeout fallback, as on Unix |
| 18 | `control_plane.rs` re-exec after credential rotation | `exec()` | no `exec` on Windows. Exits 75 (EX_TEMPFAIL), the status the Unix path uses when `exec` fails, for the supervisor to restart. Spawning a replacement here would race the old process for its ports |
| 19 | `control_plane.rs` `restrict_permissions` (cloud.json, `0600`) | `chmod 0600` | protected DACL, current user only, inheritance off (`winos::restrict_to_current_user`), applied to the temp file before the rename |
| 20 | `control_plane.rs` config dir | `$XDG_CONFIG_HOME`, `~/.config/celld` | `%APPDATA%\celld` (`CELLD_CONFIG_DIR` still wins) |
| 21 | `deploy/python.rs` runtime cache | `$XDG_CACHE_HOME`, `~/.cache` | `%LOCALAPPDATA%` (`CELLD_PYTHON_RUNTIME_DIR` still wins) |
| 22 | `js.rs` thread CPU clock (watchdog, CPU limits) | `clock_gettime(THREAD_CPUTIME)`, `pthread_getcpuclockid`, Mach `thread_info` | `GetThreadTimes` on a handle opened by thread id per sample (`THREAD_QUERY_LIMITED_INFORMATION`); user + kernel, 100 ns |
| 23 | `asyncrt.rs` `filesystem_name_max` | `fpathconf(NAME_MAX)` | `GetVolumeInformationW` max component length (255 on NTFS/ReFS) |
| 24 | `docker.rs` engine transport | unix socket | named pipe, `\\.\pipe\docker_engine` then `\\.\pipe\podman-machine-default`; `DOCKER_HOST=npipe:////./pipe/NAME`; `ERROR_PIPE_BUSY` retried. The container feature is enabled, not disabled |
| 25 | `ltx/host.rs` `sync_all(path)`, `rename` | `open(path).sync_all()`, `rename(2)` | `ltx/winfs.rs`; see Durability |
| 26 | cell scope to directory name | the scope itself | `fsname.rs`; see Paths |
| 27 | data root | as given | `fsname::data_root`; see Paths |
| 28 | `libsqlite3-sys` build | `sh`, `patch`, `sha256sum`, `tar` | Rust build script; see Build |

## 4. Durability

The recipe celld relies on is `write tmp; fsync(tmp); rename(tmp, final);
fsync(parent directory)`, used by the node log (`node_log.rs`, an
acknowledgement means "fsynced on every member"), LTX replica files and the
clean-reload marker. All of it goes through `celld_ltx::FileSystem::sync_all`
and `rename`, so the Windows replacement lives in one place
(`crates/ltx/src/winfs.rs`).

What differs on Windows:

1. **A file flush needs a handle with write access.** `FlushFileBuffers`
   (what `File::sync_all` calls) fails with `ERROR_ACCESS_DENIED` on a
   read-only handle, and the Unix idiom is `File::open(path)?.sync_all()`.
   `winfs::sync_path` opens the file for write (no truncate) and flushes.
2. **There is no `fsync(dirfd)`.** The Unix directory fsync makes a rename or
   create durable. On Windows a directory can be opened with
   `FILE_FLAG_BACKUP_SEMANTICS`, and `sync_path` calls `FlushFileBuffers` on
   that handle, opened with write access. NTFS accepts this and flushes the
   directory's metadata through its log. Where the filesystem refuses (ReFS,
   some network shares: `ERROR_ACCESS_DENIED`, `ERROR_INVALID_FUNCTION`,
   `ERROR_NOT_SUPPORTED`) the directory flush is skipped and a warning is
   logged once per process. Flushing the volume is the only stronger
   primitive and it needs administrator rights, so it is not used.
3. **Rename.** `winfs::rename` calls `MoveFileExW` with
   `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`, retrying five times
   on `ERROR_ACCESS_DENIED` / `ERROR_SHARING_VIOLATION` (virus scanners and
   indexers briefly hold new files open). The write-through flag is documented
   to guarantee durability only for a move done as copy and delete (cross
   volume). A same-volume NTFS rename is one journaled metadata operation, so
   after it `rename` flushes the destination file (write-access handle,
   `FlushFileBuffers`). An NTFS flush of a file with a pending metadata change
   forces the log up to that record, and the log is sequential, so earlier
   directory operations become durable with it. Directory renames use
   `std::fs::rename` (`MOVEFILE_REPLACE_EXISTING` does not replace
   directories) followed by the directory flush above.
4. **SQLite.** The Windows VFS flushes database and WAL files with
   `FlushFileBuffers` and, like every SQLite on Windows, does not flush the
   containing directory. A cell's WAL and database file creation therefore
   relies on the NTFS journal. This is the stock SQLite behavior on Windows
   and is not changed here.

**Untested, and not testable by ordinary CI.** The claims in 2 and 3 about
what NTFS makes durable cannot be confirmed without cutting power (or a
filesystem fault-injection layer) during a run. The mechanism is the standard
one (Postgres, LMDB and SQLite all rely on NTFS metadata journaling and
`FlushFileBuffers`), but an acknowledged-before-fsync window equal to one NTFS
log flush interval cannot be excluded on the evidence available. The
replication layer's own guarantee (RPO 0 through LTX shipping, node-log
acknowledgement from all members) does not depend on local directory
durability alone.

Other sites that write files without going through `winfs`:

* `ltx/client/file.rs` `write_then_rename` and the `NamedTempFile::persist`
  path (restore downloads) use `tokio::fs::rename` / `persist`. On Windows
  `std` replaces the target atomically, but nothing flushes the rename. A
  restore is re-runnable, so this is a known, accepted gap.
* `ltx::HostFile::sync_all` on a handle from `FileSystem::open` is a
  read-only handle and would fail on Windows; no current caller syncs such a
  handle (`create` handles have write access).

## 5. Paths

### Cell directory names (`fsname.rs`)

A cell scope is `[A-Za-z0-9_.:$-]{1,255}` and is the directory name under the
data root (`Class:hexid/ltx/e1/db.sqlite`). Windows breaks that in three ways:

* `:` is the NTFS alternate-data-stream separator, so `Counter:abc` is not a
  directory name at all. Every Durable Object cell has one.
* NTFS is case-insensitive, so `Counter:AB` and `Counter:ab`, different
  cells, would share files. This is the dangerous one: silent aliasing.
* Trailing dots are dropped and device names (`CON`, `NUL`, `COM1`) are not
  creatable.

On Windows only, the directory name is an escaped form: `:` becomes `~_`,
uppercase `X` becomes `~x`, a trailing `.` becomes `~-`, a reserved device
stem gets a `~!` prefix. `~` is outside the scope charset, so the mapping is
injective and reversible; `local_cells` decodes directory names. Unix layouts
are untouched. The table, the reasoning and a round-trip test are in
`fsname.rs`.

Limitation: escaping widens a name. A 255-byte scope with many uppercase
letters can exceed the 255-unit component limit on Windows; the filesystem
call then fails (`ERROR_FILENAME_EXCED_RANGE`) rather than aliasing. Real
scopes (`Class:` plus 64 hex characters) are far below this.

Other directory names that come from node identifiers (`peerlog/<node>/<generation>`)
use the node-name charset (`A-Za-z0-9._-`, no colon). They are not escaped.
Two node names that differ only in case would collide on Windows. The names
are generated (`node_<hex>`, `dev-<hex>`) or operator-chosen; keep them
case-distinct on a Windows host.

### Long paths and SQLite's 1040-byte limit

The data root goes through `fsname::data_root` at startup (`main.rs`): created,
resolved with `canonicalize`, which on Windows returns the verbatim form
`\\?\C:\...`. Verbatim paths avoid the 260-character `MAX_PATH` limit without
an application manifest, and SQLite's Windows VFS accepts them. Because a
verbatim path does not accept `/`, `cell_path` returns `\` separators for
facet streams.

SQLite's Windows VFS has a hard 1040-byte (`SQLITE_WIN32_MAX_PATH_BYTES`)
buffer for a database path. The data root is therefore capped at 700 bytes at
startup with an error that names the limit, leaving room for a 255-byte
escaped scope and the `\ltx\e<epoch>\db.sqlite-wal` tail. A facet nested
several levels (`\facets\<32 hex>` per level, 41 bytes) under a long root can
still reach the limit and fails at open with `SQLITE_CANTOPEN`. Keep the data
directory short (the default `.celld\dev` under the project is fine).

An application manifest with `longPathAware` is not added; the verbatim root
makes it unnecessary for celld's own files.

### Other

* Hostname falls back to `COMPUTERNAME`.
* esbuild: `Command::new("esbuild")` does not resolve `esbuild.cmd` shims on
  Windows. Set `CELLD_ESBUILD` to the native `esbuild.exe`
  (`node_modules\@esbuild\win32-x64\esbuild.exe`).
* `docker` resolves as `docker.exe` on PATH; `CELLD_DOCKER` overrides.
* Bind mounts to a container pass a Windows host path (`C:\...`); Docker
  Desktop in Linux-container mode accepts that form. Untested.

## 6. Memory (behavioral differences)

* RSS is the working set (`WorkingSetSize`). It includes shared pages and is
  not directly comparable to Linux RSS, but is the closest equivalent and
  what Task Manager shows.
* No allocator slack split: `in_use_bytes` equals `rss_bytes`. The pressure
  classifier therefore measures the working set including any pages the Win32
  heap keeps. `CELLD_MAX_RSS_MB` remains the operator's lever.
* No cgroup equivalent. A Windows Job Object memory limit is not read; the
  total is physical RAM.
* The C heap (SQLite, V8 C++) has no trim step; the Win32 heap decommits
  freed segments itself.

## 7. Features

* **Containers**: enabled, via the engine's named pipe (site 24).
* **Python** (Pyodide runtime): the runtime cache moves to `%LOCALAPPDATA%`;
  nothing else is platform-specific. The esbuild requirement above applies.
* **`celld dev` filesystem watching**: `notify` uses `ReadDirectoryChangesW`.
* **Unix domain sockets**: the only use was the container engine; everything
  else is TCP on loopback or the configured listeners.
* **Windows service mode** is not implemented. Run celld under a supervisor
  (Vibestudio, NSSM, Task Scheduler). Console control events cover Ctrl-C and
  the supervisor's CTRL_BREAK; `CTRL_CLOSE`, `CTRL_LOGOFF` and
  `CTRL_SHUTDOWN` leave the process only a few seconds before Windows kills
  it, so a long preserve handoff may be cut short in those cases.

## 8. Untested on real Windows (the CI plan)

`.github/workflows/windows.yml` (committed, not pushed) builds on
`windows-latest`, runs `celld-dolt-exec` and `doltlite-spike` tests and the
`fsname` unit test, and smoke-tests `celld dev` on `examples/hello` and
`examples/counter` (`.github/windows-smoke.ps1`). The counter smoke
exercises the colon-in-cell-name path. A final `continue-on-error` step
hard-kills the supervisor, checks the Job Object took the node with it, and
restarts to see the counter continue.

Not covered by that workflow and needing a Windows run or Windows hardware:

* ACL result of `restrict_to_current_user` (inspect with `icacls`).
* Behavior of `CTRL_BREAK` delivery in `celld dev` when the supervisor has no
  console (a GUI parent such as Electron without a console: the event is not
  delivered; the HTTP `/shutdown?handoff=preserve` path, which is tried first,
  is the working route, and `Child::kill` the last resort).
* Directory-flush behavior on ReFS, network shares and Dev Drive.
* Power-loss durability (section 4).
* `dolt-exec` tests that assume Unix paths or `HOME` (`engine_digest.rs`
  reads `CARGO_HOME` / `HOME`).
* Windows on ARM (`aarch64-pc-windows-msvc`): the BLAKE3 NEON source and v8
  prebuilt availability are unchecked.
* Containers through Docker Desktop and Podman pipes.

## 9. Cross-build from Linux

```sh
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin
# clang-cl, lld-link, llvm-lib on PATH (an LLVM 19 release tarball works)
export XWIN_ACCEPT_LICENSE=1
cargo xwin check --workspace --all-targets --target x86_64-pc-windows-msvc
cargo xwin build -p celld --target x86_64-pc-windows-msvc
```

`check` runs the build scripts, so it already compiles the DoltLite C
amalgamation with clang-cl and downloads the prebuilt V8 library. The build
needs `curl` on PATH for the first fetch of the DoltLite tarball.
