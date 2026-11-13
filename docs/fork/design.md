# Unified celld fork: design

Status: proposal, 2026-11-13. Branch `unified-fork` of celld v0.6.3 (`8702f6d`).
Consumers: Vibestudio (replaces its bundled workerd) and Chronolog DB (hosts a
Chronolog node, so Chronolog can be embedded in Vibestudio). Supersedes
`vibestudio/docs/celld-migration-plan.md` (draft, 2026-11-12) and the celld and
workerd options in `chronolog-db/docs/workerd-reconstruction-plan.md`.

Facts are from source reads of celld `8702f6d`, DoltLite 0.11.29 (pinned in
Chronolog) and Chronolog `simplify-core` `a8e5954`. Anything marked *spike*
has not been run yet.

## Status (2026-11-13)

| Piece | State | Where |
| --- | --- | --- |
| DoltLite 0.51.11 as the only SQLite | merged | `vendor/libsqlite3-sys` |
| Test harness: E2E, fleet fault injection, simulator | merged | `tests/e2e`, `tests/fleet`, `crates/logic-sim` |
| Dolt cells: storage, replication, `ctx.storage.dolt`, fork | merged | `docs/services/dolt-cells.md` |
| Deterministic execution: per-connection context patch, executor, `dolt.execute`, engine digest | merged | `crates/dolt-exec`, patch 0002 |
| Dolt facets, `ctx.facets.fork`, chunk files past 1 GiB | merged | §3b.7-8, `docs/services/durable-object-facets.md` |
| Whole-object fork, restore and import (G1, G2, F3) | done (branch `feature/object-fork-restore`) | §3b.9, `docs/services/object-fork.md` |
| Codegen confinement, `--no-implicit-deadlines` | merged | `docs/fork/codegen.md`, `docs/fork/deadlines.md` |
| Residency hold | merged | `docs/services/residency.md` |
| Egress hook | merged | `docs/services/egress.md` |
| Iroh P2P binding | merged | `docs/services/p2p.md` |
| Vibestore local bucket | merged | `docs/fork/vibestore.md` |
| Fixes for the upstream defects the harness found (S1–S4, E1, E3) | merged | `docs/fork/upstream-findings.md` |
| V8 inspector (CDP: discovery, evaluate, CPU profiles, heap snapshots, breakpoints) | merged | `docs/services/inspector.md` |
| Chronolog `transport-iroh` | done (Chronolog branch `feature/transport-iroh`) | |
| Chronolog node on the fork | two-node end-to-end test passes (Chronolog branch `feature/celld-host`, `HANDOFF-celld-host.md`); its fork APIs are merged | |
| Windows port (compile and cross-link; CI workflow for a real Windows run) | merged | `docs/fork/windows.md` |
| Vibestudio compatibility audit | done (Vibestudio branch `audit/celld-compat`, `docs/celld-compat-audit.md`) | fork bugs A1 and A2 fixed |
| Vibestudio runtime cutover | partial, paused (Vibestudio branch `feat/celld-runtime`, `HANDOFF-celld-runtime.md`) | |
| Vibestudio Dolt redesign | design done, P0 not started (Vibestudio branch `design/dolt-redesign`, `docs/dolt-redesign.md`); questions Q1–Q5 open | |

### Single-track backlog (from 2026-11-13, in order)

1. ~~Dolt facets~~ merged (`ctx.facets.get(name, () => ({ class, celld: { storage: "dolt" } }))`). Forking a root together with its facets is done with item 3 (§3b.9).
2. ~~Review the fork APIs Chronolog needed~~ merged with tests: `execute(…, { log })`, `dolt.query` and `beginTransactionSync()`.
3. Fork gaps blocking the Vibestudio cutover:
   - ~~G1: `env.NS.fork` and `restore` for SQLite objects, including their facets~~
     done (§3b.9, `docs/services/object-fork.md`): `env.NS.fork(sourceId,
     { to?, name?, at?, facets? })` and `env.NS.restore(targetId, fromId,
     { stop?, at?, facets? })` for any SQLite-backed class, root and facets,
     each with its engine;
   - ~~G2: `celld cell import` from a plain SQLite file~~ done:
     `celld cell import SCRIPT CLASS NAME --file ROOT [--facet NAME=FILE[:ENGINE]]...
     (--bucket B | --dev PROJECT)`, the library call `celld::object_image::import`
     for a control plane;
   - G3: per-class secrets.
4. Fork gaps from Chronolog:
   - P2P streams owned by the cell, not by the event that opened them;
   - `closed` does not resolve on a clean end of stream;
   - Ed25519 PKCS#8 v1 versus v2;
   - configurable dev lease.
5. Fork requests from the Dolt redesign, F1–F6:
   - F1: merge with an in-transaction resolver;
   - F2: commit at the end of a transaction;
   - ~~F3: forks that carry facets~~ done with G1 (`facets: { [name]: { at } }`);
   - F4: local tables;
   - F5: cheap checkout;
   - F6: read access to history and diff tables.
   - F7: Dolt remotes between cells. Locally this is chunk transfer through
     DoltLite's `DoltliteRemote` vtable, behind a capability; across nodes,
     the same transfer over the P2P binding. It is required for
     **decentralized GAD** (user decision, 2026-11-13). GAD can stay one
     Dolt database if its size allows. Participants (agents, the user,
     contexts, later devices and other workspaces) work in parallel on their
     own forks or branches and merge back by fetch, push and merge, and no
     single object serializes the work.
6. Finish the Vibestudio runtime cutover (`feat/celld-runtime`, per its handoff).
7. Vibestudio Dolt redesign, P0 onward. Q1–Q5 are answered; the GAD
   section (§5) is to be rewritten as decentralized before P5.
8. Chronolog cleanup: port the conformance/replay fixture, then delete `chronologd`, `materializer-doltlite`, the DoltLite addon and the SSB network layer.

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

## 3a. Declaring fork features (shared contract for all fork work)

Every fork feature is declared in one place, so features don't each invent
their own switches.

- **Dolt classes**: `migrations[].new_dolt_classes: ["Class"]`, alongside
  `new_sqlite_classes`. The engine is fixed when the class is created.
- **Per-class capabilities**: a top-level `"celld"` block in the wrangler
  config:

  ```jsonc
  "celld": {
    "durable_objects": {
      "Class": { "codegen": true, "residency": true }
    }
  }
  ```

  Dynamic workers get the same keys from the Worker Loader config, under
  `celld: { codegen, residency, egressIdentity }`. A loaded worker can
  never be granted more than its loader holds.
- **Node-level settings** (CLI flag plus env var, documented in `--help`):
  `--egress-proxy URL` with `CELLD_EGRESS_PROXY`, plus the egress HMAC
  secret; `--p2p-key-file` for the Iroh endpoint.
  `--no-implicit-deadlines` with `CELLD_NO_IMPLICIT_DEADLINES=1` arms none of
  celld's own watchdogs (docs/fork/deadlines.md). Codegen confinement is in
  docs/fork/codegen.md.
- **JS surface**, all on objects that already exist:
  - `ctx.storage.dolt` (Dolt cells only);
  - `ctx.residency.hold()`, which returns a handle with `release()`;
  - `env.<NS>.fork(id, opts)`;
  - `env.<P2P binding>` for Iroh, declared like any other binding as
    `"p2p": [{ "binding": "P2P", "alpn": "..." }]`.
- **Deterministic execution core**: `crates/celld/dolt_exec.rs`.
  - `execute(conn: &rusqlite::Connection, program: &Program, ctx: &ExecContext) -> Result<Outcome>`.
  - `Program` is an ordered list of `{ sql, bindings }`, using SQLite
    storage-class values.
  - `ExecContext` is `{ now_ms, seed, actor, profile, limits }`.
  - `Outcome` is `Accepted` or `Rejected { code, statement_index }`.
  - `ctx.storage.dolt.execute` is a thin wrapper over it.

## 3b. Dolt cells as built (phase 1 decisions, 2026-11-13)

User-facing contract: `docs/services/dolt-cells.md`. Decisions that refine §3:

1. **The seam is below `Replication`, at the capture.** `celld_ltx::CellStore`
   (`crates/ltx/src/cell_store.rs`) is what a `Replica` holds; `Db` is the
   LTX store, `DoltStore` (`crates/ltx/src/dolt.rs`) the Dolt one.
   `ltx_repl.rs` picks the store by `ActivationOptions.engine` and is
   otherwise engine-blind.
2. **A `DoltSegment` travels as an L0 LTX of page images.** The appended
   byte range (cut at the end of the newest root whose BLAKE3 seal verifies)
   is carried as the 4 KiB pages that cover it, last page zero-filled past
   the cut. Position is the TXID; the byte cut is in the L0 header
   (`wal_size`, `wal_offset` = next record, `wal_salt1` = `DOLT`).
   Consequence: followers, bundles, upload, compaction, epoch chains, epoch
   GC, handoff snapshots and the output gate's proofs are literally the same
   code, so no engine tag was needed in `log_tier`, `epoch_gc` or
   `BUCKET_FORMAT` (the object format is unchanged; the manifest feature
   `dolt-cells-v1` keeps old nodes away). A **generation** (`dolt_gc`,
   restore) is detected by a changed offset-0 manifest or a short file and
   captured whole, which truncates on restore like a VACUUM. Cost: each
   capture re-sends at most one partial page. Limit: the LTX lock page
   capped a chunk file at 1 GiB (lifted, item 8); no paged restore for Dolt cells.
3. **`_cf_*` tables live in the prolly store, unversioned.** One root then
   commits a handler's application rows and celld's rows together, and one
   stream replicates both. `dolt_ignore` (`_cf_%`) keeps them out of every
   commit, `reset --hard` and `merge`. DoltLite's checkout is per
   connection and working sets are per branch, so checkout copies the
   runtime tables into the target branch's working set and then flips
   `_cf_DOLT.branch` on `main`, which every open reads first (crash-safe
   in either order of failure; cost O(runtime tables)). Rejected:
   `dolt_nonlocal_tables` (needs ATTACH of `file/branch`, which the
   authorizer denies, and failed to open in a probe), and a sidecar SQLite
   file (two streams, no atomicity with application writes).
4. **Gate:** Dolt operations change no SQLite row counter, so each mutating
   operation bumps a per-activation counter folded into the committed-write
   position; the output gate then waits for the same proofs as a SQL write.
5. **Fork (simplest correct):** the source publishes a full image from its
   own turn behind its output gate, claims the new id with a conditional
   create of a released `own.json` at epoch 1, and uploads a TXID-1
   snapshot to `e1`; the fork's first activation restores from it like an
   operator-released cell. No storage is shared yet (§3.2's prefix
   reference remains future work).
6. **Wake triggers are TEMP on Dolt cells.** Dolt versions persistent
   triggers (`dolt_schemas`), so reset/merge/checkout could drop the
   `_cf_WAKE` triggers and user commits would carry them; Dolt cells create
   them per connection and re-ensure them after every mutating operation.
7. **Dolt facets (2026-11-13).** Vibestudio loads its userland objects
   (agent, GAD workspace, channels) as facets of one root through the
   Worker Loader, and redeploys nothing, so the engine is a per-facet
   choice and not a class declaration:
   `ctx.facets.get(name, () => ({ class, celld: { storage: "dolt" } }))`.
   The key sits under `celld` (§3a) because workerd's `FacetStartupOptions`
   is `{ class, id }` and may grow. Decisions:
   - **Layout.** No new layout was needed. Facets already had a file and a
     replication stream of their own (`facet_streams.rs`,
     `<root>/facets/<h>/ltx/e<N>/db.sqlite`; the deserialize+backup path is
     only the one-time import of legacy `_cf_FACETS` images). A Dolt facet
     is the same stream opened with `Engine::Dolt`, so its capture is a
     `DoltStore` and everything downstream (followers, bucket, handoff,
     restore with the root's `RestoreSpec`, delete of the stream prefix,
     the output gate's facet proof) is unchanged.
   - **Engine record and guard.** The file's magic is the record, as for a
     root cell. `FacetReq::Open` carries the engine the startup options
     named. A resident or restored file of the other engine is refused at
     the connection open, and `LtxRepl::activate_with` refuses to put the
     LTX store over a chunk file, so a wrong-engine start after the local
     files were deleted fails before anything captures. Rejected: an
     engine table in the root (a second writer to the root's file from the
     facet's isolate, and it would be ungated).
   - `attach_active_branch` became idempotent (a facet opens in two steps,
     and the second read of the pointer off `main` moved the connection
     back to `main`). A facet connection disables lookaside before its
     first statement, because a prolly store keeps slots in use.
   - **Fork.** `ctx.facets.fork(source, target, { at })` forks a Dolt
     facet into a new facet of the same parent, through the host loop
     (`FacetReq::Fork`): activate the target's stream and require it to be
     empty (nothing restored, no file), read the source file (append-only,
     so any read is a committed prefix cut at the newest sealed root),
     `fork_image`, prove the source stream, write the image, prove the
     target stream (which captures it whole as a new generation). A fork
     of the root with its facets: §3b.9.
8. **The 1 GiB cap is lifted.** A chunk file's page `n` is LTX page `n`
   below SQLite's lock page and `n + 1` from it on (`dolt::ltx_pgno`), so
   no LTX file ever carries the lock page. Every reader zero-fills it, so an
   image rebuilt from the bucket has a 4 KiB hole at 1 GiB, which
   `dolt::unmap_restored_image` removes before the store opens the file.
   It runs only after a remote restore: a local eviction snapshot or a
   resumed file is the live file and has no hole. Remaining limits: whole
   captures and restores hold the file in memory, and DoltLite's GC buffer
   (§4.7, about 2 GiB).

9. **Whole-object fork, restore and import (G1, G2, F3; 2026-11-13).**
   User-facing contract: `docs/services/object-fork.md`. One mechanism:
   an object is a tree of components (root, facets), each a whole-file
   image with its engine (`crates/celld/object_image.rs`). Three
   producers (the source's export, plain files, a staging object read back
   from the bucket) and two consumers (publish as a new cell, install into
   a quiesced target's live streams). Decisions:
   - **Facet index in each parent, not a bucket listing.** Each parent
     (root or facet) records its direct facets in its own database,
     `_cf_FACET_INDEX(name, engine)`, written in the parent's turn when it
     starts a facet (`op_facet_rpc`/`op_facet_fetch`, before the parent's
     identity sample, so the parent's gate covers the row before any facet
     output), removed by `ctx.facets.delete`, kept by `deleteAll()`. It is
     unversioned in a Dolt parent (`_cf_%`). Per parent rather than all in
     the root, so the writer is always the parent's own connection in its
     own isolate (a nested facet's parent is a facet). The engine recorded
     is the latest start's; the file decides, so a fork opens a facet the
     index names wrongly with the other engine. Facets started before the
     index existed are carried once their parent starts them again; a
     restore deletes the whole `<root>/facets` prefix regardless.
   - **Export from the source's turn, parents before children.** The root
     is read in the source's turn (SQLite: online backup through the cell's
     own connection, so a paged root works; Dolt: committed cut), facets
     after it on the host, each read then proved durable; the publication
     waits on the root's output gate. Parent-before-child gives: every
     child write a parent's image depends on is in the child's image,
     because a child's replies and effects leave only after its stream
     proves them. Not an instant cut across components (no root input gate
     is held; a relay already in flight can put a sibling's write in a
     later-read facet only), which is no stronger than what a crash of the
     source can leave. Rejected: holding the root's gate and waiting for
     in-flight facet calls (a facet that calls back into its root's
     namespace would deadlock).
   - **Publication: claim first, with a `publishing` record.** The Dolt
     fork's claim (conditional create of a released `own.json` at epoch 1,
     then upload) let a node activate a half-uploaded cell. Now the claim
     carries `"publishing": nonce`, which `ownership_store::read_owner`
     refuses, then every component's TXID-1 snapshot is uploaded at `e1`,
     then a compare-and-swap clears `publishing`. An interrupted
     publication leaves a cell no node activates; only an import with
     `--replace-unfinished` takes it over. The old `__dolt_fork_export`
     path is gone; Dolt roots use this one (and keep their key-value data,
     which the old fork left under the source's scope).
   - **`at` is per Dolt component**: `at` for a Dolt root,
     `facets[name].at` for a Dolt facet; refused for a SQLite component,
     which has no history. A root's `at` does not rewind which facets exist.
   - **Restore = export to a staging object + adopt.** The source exports
     itself as for a fork, to a hidden unique id whose record names the
     target (`staging_for`), so a restore can never consume or delete an
     ordinary fork such as a backup. The target, on its owner, in an event
     answered before its instance exists, installs it: SQLite root by
     backup into its own live connection (one captured write
     transaction), Dolt root by file replacement and reopen (a new
     generation, as after `gc()`), facets by deleting `<root>/facets` and
     recreating each stream. Rejected: writing the target's bucket prefix
     from outside its owner (it needs the released-record repair procedure
     of `docs/guarantees.md` and races a live owner).
   - **Quiesced, precisely**: no instance in the current activation, no
     facet open or being started, no WebSocket, no residency hold, no other
     event, no export in progress. Refused with `ERR_RESTORE_TARGET_ACTIVE`
     otherwise; `{ stop: true }` makes the restore event itself reset the
     target first (instance dropped with its storage closed, holds ended,
     sockets closed 1012, loaded-isolate facets closed as at a generation
     swap). No timer anywhere.
   - **Roll-forward, not a multi-stream transaction.** The restore writes
     its intent (`_cf_RESTORE`) in the target root and proves it, replaces
     the facets, then the root, whose image carries no intent. Every event
     that would construct the object first waits for a running restore or
     completes a pending one (`_readyInstance`), so no event observes a
     half-restored object, and a crash in between is completed by the next
     activation from the same staging object (`CELLD_TEST_RESTORE_CRASH`
     drives this in the example). The target keeps its wake record
     (sequence moves on), its id name, and loses its alarm.
   - **Import** uses the same publication; the class's engine comes from
     the script's current deployment manifest, a facet's from
     `NAME=FILE[:ENGINE]`. workerd's `_cf_KV(key, value)` becomes celld's
     `_cf_KV(scope, k, v)` and workerd's alarm metadata is dropped. The
     Vibestudio migration maps `<hash>.sqlite` to `--file` and
     `<hash>.1.sqlite` to `--facet do=...` under the object name
     `encodeUniversalKey(ref)`.
   - Limits: images are whole files in memory; no storage is shared with
     the source; `stop` cannot close a facet of the root's own script; an
     import creates direct facets only.

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
| Local store on the Vibestudio blobstore | `ObjectStore` implementation over the CAS, with conditional writes for ownership records. Built: [vibestore.md](vibestore.md) |
| Fork, backup, restore without private files | Dolt cells (§3.2) for DOs that want versioning. Whole-object fork, restore and import for every class (§3b.9). Deletes `cloneDO` and `computeWorkerdObjectIdHash` |
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
