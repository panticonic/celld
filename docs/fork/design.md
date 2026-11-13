# Unified celld fork: design

Status: proposal, 2026-11-13. Branch `unified-fork` of celld v0.6.3 (`8702f6d`).
Consumers: Vibestudio (replaces its bundled workerd) and Chronolog DB (hosts a
Chronolog node, so Chronolog can be embedded in Vibestudio). Supersedes
`vibestudio/docs/celld-migration-plan.md` (draft, 2026-11-12) and the celld and
workerd options in `chronolog-db/docs/workerd-reconstruction-plan.md`.

Facts are from source reads of celld `8702f6d`, DoltLite 0.11.29 (pinned in
Chronolog) and Chronolog `simplify-core` `a8e5954`. Anything marked *spike*
has not been run yet.

## 1. Goal

There is one runtime, and it has two kinds of cell:

- **SQLite cells**: celld's cells as they are today. WAL pages are captured
  as LTX, held by followers, and uploaded to the bucket.
- **Dolt cells**: a DoltLite prolly-tree store with commits, branches,
  diff, merge and cheap forks. Their durable unit is an appended byte
  range of the chunk file. A Dolt cell can also execute a **deterministic
  program**: a pinned clock, a seeded PRNG, UTC, an authorizer profile and
  a step budget. This is Chronolog Spec 10, provided by the runtime rather
  than by a Node addon.

On top of these, the fork adds what Vibestudio needs from a runtime:
codegen confinement, one egress hook, an explicit residency hold, no hidden
deadlines, a local blobstore-backed store, native fork, clone and restore,
a V8 inspector, and Windows support.

## 2. Key decision: DoltLite is the only SQLite in the binary

DoltLite is a SQLite 3.5x amalgamation that keeps the original B-tree and
pager under an `orig_` prefix. `sqlite3BtreeOpen` (`prolly_btree.c:4497`)
uses the stock B-tree for every file that already has a SQLite header
(`origBtreeIsSqliteFile`, `btree_orig_api.c:167`), and for temp and
in-memory databases. It uses the prolly store only for new or empty files.

A second, symbol-prefixed SQLite therefore does not need to be linked
alongside the first. Instead, `libsqlite3-sys` is patched (`[patch.crates-io]`)
to compile the DoltLite amalgamation. rusqlite, the LTX crate, sqlite-vec,
`local_store.rs` and every existing open keep working on stock-format files.
A Dolt cell is just a cell whose file is created in prolly mode.

The alternative, two engines with prefixed symbols, means a second FFI layer,
a second sqlite-vec build and a process-global VFS registry shared by two
libraries. That is the parallel path we want to avoid. It stays the
fallback only if the spike in phase 0 fails.

The cell's engine is chosen at creation, with an explicit open-time flag
(a small DoltLite patch: `?engine=sqlite|prolly`). Today the choice is
implied by whether a header already exists, and we don't rely on that.

## 3. Dolt cells

### 3.1 Storage and replication

The chunk file is laid out as a 168-byte manifest, then a compacted region
and index, then an append-only WAL tail. The tail holds chunk records
(`0x01`) and root records (`0x02`) (`chunk_store.h:16-94`). An ordinary
commit only appends to the file, then fsyncs once (`chunk_store.c:1830-1932`).
The header is rewritten only at creation and by GC. GC rewrites the whole
file and renames it into place (`doltlite_gc.c:536-751`).

This maps onto celld's existing transport, which carries opaque
`(cell, epoch, seq, bytes)` entries (`ShipEntry`, `ltx_repl.rs:180`;
`node_log.rs:767`):

| celld concept | Dolt cell |
| --- | --- |
| durable unit (L0 LTX) | `DoltSegment{generation, start_offset, bytes}`, cut at the end of the last root record after COMMIT |
| txid / position | byte offset within a generation, monotonic |
| snapshot (L9 handoff) | the whole compacted file after GC, as a new **generation** |
| restore | download the latest generation snapshot, append its segments in order, open. DoltLite's WAL replay validates every chunk hash and root on open |
| compaction | `dolt_gc()` at eviction or by tail size, which produces a new generation |
| torn tail | already handled by DoltLite: it truncates to the last root |

A new `CellStore` trait is extracted from `Replication` (`ltx_replication.rs`),
with two implementations: `LtxStore` (existing) and `DoltStore` (new).
Ownership, fencing, epochs, followers, bundles and bucket upload are shared.
The logic crate already treats position as an opaque `u64`
(`types.rs:1180`). The LTX-specific parts (`log_tier.rs`, `epoch_gc.rs`,
`format.rs`) get an engine tag.

### 3.2 Forks and clones

A fork is a new cell whose base is the source's latest generation plus a
segment prefix. Nothing is copied until the fork writes. Inside one cell,
branches are Dolt branches (`dolt_branch`, `dolt_checkout`, `dolt_reset`).

Later, the `DoltliteRemote` vtable (`doltlite_remote.h:9-22`: get, put and
has chunks; compare-and-swap refs) gives chunk-level content addressing
across cells. That lets forks share storage, and lets Chronolog export
snapshots incrementally. Phase 1 doesn't need it.

### 3.3 JS API

The wrangler config marks a class as a Dolt class
(`migrations[].new_dolt_classes`). `ctx.storage.sql` is unchanged. A new
API is added:

```ts
ctx.storage.dolt.commit({ message, author?, date? }) -> hash
ctx.storage.dolt.branch / checkout / reset / merge / diff / log / hashOf
ctx.storage.dolt.execute(program, { nowMs, seed, actor, profile, limits })
  -> { outcome: 'accepted' } | { outcome: 'rejected', code, statementIndex }
env.NS.fork(sourceId, { at?: hash }) -> id
```

Every `dolt.execute` call uses the same setup:

- `BEGIN IMMEDIATE` plus a savepoint, the authorizer profile, the limits,
  and a progress-handler budget;
- `chronolog_assert` (and `chronolog_actor` and
  `chronolog_policy_violation` when owner-rows lands) registered as native
  Rust functions;
- the engine manifest digest exposed as `ctx.storage.dolt.engineDigest`.

This is Chronolog's `DatabaseLike` contract implemented once, natively.

## 4. Determinism: fixes over the earlier plans

Things the workerd reconstruction plan and the current Chronolog patch get
wrong or leave open:

1. **Process-global state.** The Chronolog addon pins the clock on a
   process-default VFS, and pins the PRNG with
   `sqlite3_test_control(PRNG_SEED)` and `LOCALTIME_FAULT`, all
   process-wide. celld runs many cells on many threads. The fork makes all
   three per connection:
   - the clock through a per-connection VFS, which already decides
     `'now'` (`db->pVfs`);
   - the PRNG through a DoltLite core patch that gives each connection its
     own seeded state;
   - localtime through a per-connection UTC flag.
2. **Commit timestamps** use libc `time(0)` (`doltlite.c:433`; tags, merge,
   cherry-pick, revert and rebase do the same). The clock VFS does not
   reach them, so commit hashes cannot be reproduced. The patch takes every
   commit date from the connection's VFS clock. It also stops treating
   `--date 0` as "unset" (the zero-timestamp sentinel).
3. **Seed commit at creation** uses `time(0)` (`doltlite.c:5019-5035`).
   Under a pinned clock, two independently created stores now get the same
   genesis hash. Replicas no longer have to be cloned to agree.
4. **The reducer API is obsolete.** `packages/runtime-workerd`
   (`reduce(databases, input)`, `getInput`/`getOutput`, `ExactDatabaseRef`,
   the WASM/Emscripten shims) predates Spec 10. The Spec 10 equivalent is
   §3.3's `dolt.execute` plus `ChronologMaterializationRuntime`. The WASM
   route is unnecessary: the engine is native in the runtime.
5. **Patch drift.** Chronolog's installed addon (`...-v5`: `chronolog_actor`,
   trigger lint) is newer than `patches/@dolthub__doltlite@0.11.29.patch`
   (`...-v4`). A clean install would not reproduce it. The fork carries the
   C-side patches as one versioned patch set with a recorded digest.
6. **Version.** Chronolog pins 0.11.29. Upstream is 0.51.11 (2026-11-12), with
   the same format version 12 but a much larger WAL implementation
   (`chunk_wal.c` 505 → 1530 lines). The fork targets 0.51.x, and Chronolog
   moves with it. The new engine digest means a new execution manifest.
7. **Unbounded WAL.** Nothing triggers GC, and every open replays the whole
   tail (O(WAL bytes), re-hashing every chunk). The fork runs GC by tail size
   and at eviction. GC's `int` buffer limits live data to about 2 GiB
   (`doltlite_gc.c:377`); this is fixed upstream or in the patch set before
   large cells.

## 5. Vibestudio requirements

The draft migration plan's claims were checked against Vibestudio `32e992c1d`.
Corrections are marked ✱.

| Need | Fork change |
| --- | --- |
| Codegen confinement ✱ mandatory: celld has no restriction, so any userland DO could `eval` | Isolate option, off by default. A class gets it by declaring it in the deployment and being granted it, like any other capability |
| Egress with caller identity, plus a "no network" mode, covering fetch, `connect()` and outbound WebSocket (used by model transport and the CDP client) | One `EgressHook` in front of `op_fetch`, `op_tcp_connect` and `op_ws_connect` (`js.rs:8990`, `js/tcp.rs:136`, `js/websocket.rs:475`), with the policy carried per isolate (`EgressPolicy`, `js.rs:2430`) |
| Residency hold, available to any cell that declares it (eval kernels are ordinary per-owner Dolt cells, §8) | `Cell.held` folded into `is_active` (`logic/lib.rs:1748`), with a node-wide budget like `may_pin_outbound` |
| No hidden deadlines | Make explicit, or remove: the JS-side 30 s `blockConcurrencyWhile` timer (`harness.js:2736`), `CELLD_OPERATION_DEADLINE_MS` (15 s, `actor.rs:41`), the 60 s stream idle expiry (`http_streams.rs:34`) and the 10 s durability timeout |
| Local store on the Vibestudio blobstore | `ObjectStore` implementation over the CAS, with conditional writes for ownership records |
| Fork, backup, restore without private files | Dolt cells (§3.2) for DOs that want versioning. Snapshot-and-segment fork for SQLite cells. Deletes `cloneDO` and `computeWorkerdObjectIdHash` |
| ✱ SES `lockdown()` for internal DOs | Verify in phase 0 |
| V8 inspector | New module, bridged like `workerdInspectorBridge.ts` |
| Windows x64 | Port about 85 Unix-specific sites. Required, because Vibestudio ships Windows and there is no dual runtime |
| ✱ Not needed | `connect()` through a Fetcher (no `cloudflare:sockets` use found). Intel Mac (already dropped) |

The host-to-DO hairpin (`/_docode`, `/_doadmission` and the gateway relay) is
Vibestudio's own design. It stays until the refactor (phase 5).

## 6. Chronolog requirements

What a node needs, and where it lives in the fork:

| Piece | Host |
| --- | --- |
| Materializer (`materializer-doltlite`) | A Dolt cell, `dolt.execute`, checkpoints and revisions as Dolt branches |
| Feed store `feeds.sqlite`, node state `node-state.sqlite` | SQLite cells (replicated by celld), behind a small `DatabaseSync` shim |
| `canonical`, `protocol`, `capabilities`, `crypto`, `client` | Pure JS over WebCrypto. *Spike*: Ed25519 and X25519 in celld's WebCrypto |
| Timers, clock, random | Already injected into node-core (`node-core/src/types.ts:104-123`) |
| Peer transport | Iroh binding in the fork, replacing the SSB network layer (§8) |
| Keys | A host-provided secret binding, not a file |
| RPC (`node:http`) | A Worker `fetch` handler |

Two trust layers stack without conflict. celld makes one operator's node
durable across that operator's machines. Chronolog's feeds and validators
provide agreement between mutually distrusting operators. The materialized
Dolt state is derived. Replicating it only makes restores faster, and
correctness never depends on it.

## 7. Phases and gates

0. **Engine spike.** Build celld with `libsqlite3-sys` patched to DoltLite
   0.51.x.
   - *Gate*: celld's full test suite passes on stock-format cells. LTX
     capture, the backup API (DoltLite reimplements it, `pager_shim.c:1000`),
     `data_version`, and deserialize for facets all behave the same. Write
     latency matches upstream within noise.
   - Also in this phase: a prolly-mode cell opens and commits from JS, SES
     `lockdown()` runs, and WebCrypto Ed25519 works.
   - *Result (2026-11-13, `b3501e1`)*:
     - celld builds and runs with DoltLite 0.51.11 as its only SQLite. A
       vendored `libsqlite3-sys` compiles the sha256-pinned amalgamation and
       applies `vendor/libsqlite3-sys/doltlite/patches/`.
     - Patch 0001 makes stock format the default for new files; prolly
       requires `?doltlite_engine=prolly`. It also keeps `dolt_*` functions
       off stock connections.
     - `SQLITE_ENABLE_API_ARMOR` is dropped, because it silently stops
       `dolt_*` from registering.
     - 11 examples behave the same on upstream and on the spike, including
       a restart and an LTX restore. Write latency is unchanged; CPU-bound
       reads may be 10–40% slower (not yet profiled).
     - **The gate could not be run: upstream publishes no tests.** There are
       1,166 `cfg(celld_internal_tests)` sites and no test bodies. The
       workerd conformance corpus, simulator, TLA+ models and fault
       injection described in `docs/testing.md` are all private.
0b. **Test harness (before any change to durability code).**
   - *E2E*: a differential suite over every example, run through
     `celld dev`, with golden outputs, restarts and restores.
   - *Simulation*: a deterministic simulator that drives
     `logic::on_event` with modelled object store, peers, clocks and
     crashes. It checks one writer per epoch and no lost acknowledged
     write, using the `celld_internal_tests` observer hooks.
   - *Fleet*: Docker fault injection with MinIO plus three nodes: kill the
     owner, partition a node, lag a follower, tear a write.
   - The DoltLite spike tests become the engine conformance suite.
1. **Dolt cells.** `CellStore` trait, `DoltStore`, generations and segments,
   restore, GC, fork, and the `ctx.storage.dolt` API.
   - *Gate*: the simulator and integration tests cover takeover mid-tail,
     torn tail, GC during follower lag, and fork-then-write.
2. **Deterministic execution.** The DoltLite patch set (§4.1-4.3, 4.7),
   `dolt.execute`, native `chronolog_*` functions, and the engine digest.
   - *Gate*: Chronolog's SQLite differential corpus and replay fixture give
     identical state, outcome and replay digests on two nodes, under two
     `TZ` values, on Linux and macOS.
3. **Vibestudio runtime features.** Codegen confinement, egress hook,
   residency hold, deadline removal, blobstore store, inspector, Windows.
   - *Gate*: each has simulator or integration tests, and the fork's suite
     passes on Linux, macOS arm64 and Windows x64.
4. **Chronolog on the fork.** The materialization runtime on a Dolt cell,
   the feed and node-state stores on SQLite cells, the transport (§8), and
   `chronologd` reduced to a celld deployment.
   - *Gate*: Chronolog's `pnpm test`, conformance, and chaos smoke pass
     against the fork.
5. **Vibestudio cutover and refactor.** One change replaces
   `workerdManager.ts`, the programs, EvalDO and the kernel lease, the
   egress props stamping and `cloneDO`. GAD and the agent system move to
   Dolt cells, and eval kernels become per-owner Dolt cells (§8). Workspace
   DO state is migrated once. Chronolog is exposed as a workspace
   capability.
   - *Gate*: the full conventional suite plus the system-test suite pass on
     every shipped platform.

Generic patches (egress hook, residency hint, inspector, Windows, the
codegen option) are kept at clean seams, so rebasing onto upstream releases
stays cheap.

## 8. Decisions

Decided 2026-11-13:

1. **Chronolog has one host.** `chronologd` becomes a celld deployment.
   The N-API addon and its pnpm patch are retired. One engine build gives
   one digest.
2. **Topology.** One fork node (fleet) per Vibestudio workspace.
3. **Eval and the agent system move onto Dolt cells.** GAD (the semantic
   workspace graph, refs and forks) and the agent system are refactored to
   use Dolt cells. Each eval kernel runs in its own Dolt-backed cell, owned
   by its caller, with no central EvalDO. Nothing gets special treatment:
   - codegen, the residency hold and self-held cells become ordinary,
     declarable cell capabilities;
   - the `UNSAFE_EVAL` binding, the host kernel lease and the host-only
     eval authority methods are deleted.

   Phase 3 therefore builds these features as general capabilities. Phase 5
   includes the GAD and agent refactor.

4. **Chronolog peer transport is Iroh, built into the fork.** The fork
   gets a Rust Iroh binding (dial by public key, inbound stream handler,
   hole punching with relay fallback). Chronolog's remaining SSB network
   layer is replaced by a small feed-sync protocol over Iroh streams:
   secret-stack, the SHS handshake, muxrpc `history`, multiserver,
   `ssb-keys` and `ssb-caps`. The signed, hash-chained feeds and the
   SQLite feed store stay as they are, since they are Chronolog's own
   format. Vibestudio's remote access can later use the same binding.
   (Note: today's SHS uses the public `ssb-caps` network key, so it does
   not isolate groups from one another.)
