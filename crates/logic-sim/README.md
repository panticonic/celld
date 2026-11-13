# logic-sim: a deterministic simulator for the coordination core

`crates/logic` is a pure state machine (`on_event(&mut State, Event) ->
Vec<Effect>`). This crate runs several copies of it, one per simulated node,
and plays the shell: it executes every `Effect` against a modelled bucket,
peer network, clock and disk, and feeds the results back as `Event`s. One
seeded scheduler orders everything, so a seed is a complete reproduction.

```sh
cargo test -p logic-sim                                 # fixed seeds + mutation checks (~5 s)
cargo run --release -p logic-sim -- --seed 52           # replay one seed: config, stats, verdict,
                                                        # minimised config, focused trace
cargo run --release -p logic-sim -- --sweep 0..10000    # long sweep
cargo run --release -p logic-sim -- --sweep 0..200 --mutation ack-before-durable
cargo run --release -p logic-sim -- --seed 52 --strict  # also fail on already-reported findings
LOGIC_SIM_FULL_TRACE=1 cargo run ... --seed N           # unfocused, untruncated trace
```

## How the core is compiled

The core is compiled a second time by `instrumented/`, a package whose `[lib]
path` is `../../logic/lib.rs` and whose `build.rs` sets
`cfg(celld_internal_tests)`. The simulator therefore tests the shipped
source, gets the observer hooks (`step_safety_observer_snapshot`,
`authority_observer_snapshot`, `phase_snapshot_for_test`), and `celld` never
links the instrumented copy (a separate crate, so feature unification cannot
leak the cfg into a release binary). `crates/logic` is not modified. The
workspace `Cargo.toml` gains a `profile.dev.package` override that compiles
the instrumented copy without debug assertions: in debug builds `on_event`
panics on a failed `validate()`, while the simulator calls `validate()` after
every event itself and reports the failure with a trace.

## The model

Per run (all derived from the seed by `SimConfig::for_seed`): 2-4 nodes, 1-4
cells, bucket or fleet durability, a 30 s faulty phase, then a final phase.

* **Bucket** — linearizable. Ownership records (`own.json`: node, epoch,
  etag) and node leases (`nodes/<n>.json`, with the folded log state, session
  and ensemble) take conditional create/CAS exactly as `ownership_store.rs`.
  Each `cells/<c>/ltx/e<E>/` prefix holds the restorable image of that epoch.
  Every request has its own latency; a request can fail definitely, time
  out before applying, or apply and lose its response (ambiguous); a "slow"
  request linearizes up to 3 TTLs later, after its caller has moved on
  (in-flight PUTs of a dead process still land). A node can be cut off from
  the bucket.
* **Network** — per-message delay (reordering), drops, and node partitions.
* **Clocks** — per-node wall offset (skew) and a monotonic clock per
  incarnation. Pauses defer all of a node's events; a "suspend" pause also
  stops its monotonic clock while wall time advances.
* **Processes** — crash (memory lost, disk kept; restart after more than one
  TTL, as `guarantees.md` requires of the supervisor), self-fence `Halt`
  (exit + restart), graceful drain (`ReleaseAll`, then exit).
* **Clients** — writes and reads to random cells through random ingress nodes;
  `Route::Remote` is forwarded (≤3 hops). A local handler commits the write
  to the published runtime and raises `OutputAt` (write: `position`; read:
  `observed` beyond the restored baseline). Sometimes the activity finishes
  before the gate settles (the gate-pinned path).
* **Shell** (`world.rs`, each arm cites the `actor.rs` arm it models) —
  lease read/CAS stamping the shell's own folded log slot; owner read/CAS/
  release (release re-reads and requires this node at this epoch);
  `VerifyOwnership` reads the record; `Restore` follows `activate_with`:
  fresh → empty, previous-epoch `.evicted` snapshot when
  `previous_epoch_reusable`, otherwise fold this node's own open-session tail
  (requires one reachable ensemble member) and take the newest non-empty
  prefix below the epoch (refusing a used epoch). `AwaitDurable` /
  `EnsureDurable` race a bucket upload (retried until a 10 s durability
  timeout) against a fleet proof (every ensemble member acked, and the session
  record published by a lease write that carried `Open`). Stops: evict and
  cleanup upload a final snapshot before completing (keeping `.evicted`
  unless rebalancing), honour `AbandonEvictionStop` before the PUT, and report
  `RuntimeStopFailed` at the bounded deadline; reset discards; fence closes in
  place. `RecoverNodeLog` fences the dead owner's lease to `Recovering`, seals
  the ensemble members, uploads what one sealed member returns, and marks the
  lease `Sealed`; a booting node first recovers its own predecessor the same
  way (plus its local log). `AdoptReleased` offers the cell to peers, which
  answer once `accepted_epoch` passes the released epoch.
* **Final phase** — heal everything, crash every node at once, restart, read
  every cell. Every read restores from the bucket and the recovered logs.

## Invariants (checked after every core step, and at the end)

| Kind | Rule |
| --- | --- |
| `CoreValidate` | `State::validate()` holds on every node after every event. |
| `StepSafety` | `StepSafetyObserverSnapshot`: occupied counter = capacity holders, ≤ `max_resident`; activation permits ⊆ activation holders. |
| `Authority` | `AuthorityObserverSnapshot`: a fenced node never reports authority; a `Fenced` phase only on a fenced node. |
| `OneWriterPerEpoch` | The bucket grants each (cell, epoch) once; every core phase that asserts an owned epoch, and every committed write, is on the node the bucket granted that epoch to. |
| `EpochRegression` | Grants strictly increase per cell; the record's epoch never decreases. |
| `StaleAck` | No write committed after a successor epoch was granted is ever acknowledged (a deposed writer keeps accepting writes). |
| `LostAck` | Every restore at epoch E contains every write acknowledged at an epoch below E (checked at each restore and at each ack). |
| `LostRevealed` | Same for writes a released read revealed beyond its baseline. |
| `StaleServe` | No output decided after a successor epoch was granted, beyond 2× the skew bound (linearizable reads). |
| `Liveness` | Every cell is readable after the final heal (reported, not a safety failure). |
| `ModelError` | Two different images uploaded into one epoch: a simulator bug. |

A `CorePanic` from inside `on_event` is caught and reported too.

## Mutation self-checks

Each must produce a violation of the expected kind (`tests/seeds.rs`); over
seeds 0..100: non-conditional owner CAS 38/100 (`OneWriterPerEpoch`),
ack-before-durable 93/100 (`LostAck`), skip-verify-ownership 4/100
(`StaleAck`), skip-log-recovery 6/100 (`LostAck`/`StaleAck`),
follower-ignores-seal 3/100 (`StaleAck`), restore-oldest-epoch 100/100
(`LostAck`), lease-always-expired 74/100 (`StaleServe`).

## Fidelity gaps (what the model does not do)

* Data is a list of write ids, one image per epoch prefix (the non-paged
  layout where each epoch opens with a snapshot). No pages, LTX levels,
  compaction, paged restore, epoch GC or `retired.json`.
* Node bundles under `log/` are not modelled. The leader keeps its own log
  copy and a background "drain" folds it into the per-cell prefix after
  0.5-3 s; a takeover recovery reads only follower copies.
* Fleet ensembles are fixed at session open (up to two leased peers); there
  is no reconfiguration and no force-tiering on member loss. Write-all
  ack-all and seal-before-certify are modelled; follower truncation is not.
* Recovery and the restore-time tail fold read follower disks atomically at
  one instant rather than through a sequence of RPCs.
* Not modelled: alarms and wake entries, WebSockets, generation swaps, clean
  reload (`resume_local`, `BeginPreserve`), capacity placement (the capacity
  read returns no peers), memory pressure (only idle eviction, random
  `Evict` and `Rebalance`), facets, queues.
* The lease TTL is 3 s (production 10 s) with a 15 s operation deadline, so
  lease events are relatively more frequent than in production.
* The minimiser zeroes fault classes and shrinks nodes/cells while the same
  kind of violation recurs; any change re-rolls the schedule, so it removes
  irrelevant fault classes rather than producing a shortest trace.

## Findings

Seeds 0..5000 (30 s simulated each, both postures) run with **no**
lost or stale acknowledged write, no second writer per epoch, and no epoch
regression. The core's own invariants are another matter; these are
recorded as `KnownCoreIssue` so sweeps can look past them (`--strict` turns
them back into failures):

1. **A second eviction overwrites an in-flight owner release** (`--seed 52
   --strict`). A shed/idle eviction releases a cell (`ReleaseOwner` op A
   in flight, slow store request). A request wakes the cell meanwhile
   (dormant → claim E+1 → resident); `releasing` still holds A. A second
   eviction completes and `runtime_stopped` sets `releasing = Some(B)`
   without retiring A, so `cell_ops` keeps A with no owning slot and
   `validate()` fails ("indexed cell op A has no owning slot"); in a debug
   build `on_event` panics. `admit_eviction` refuses a cell with
   `releasing.is_some()` (CellTransitioning); `begin_eviction`, used by
   shedding and idle eviction, does not. Safety impact seen: none (the late
   release is rejected by `release_owner`'s epoch check; A's completion is
   ignored and its index entry leaks).
2. **`fence()` leaves per-cell flags that `validate()` rejects** (`--seed 97
   --strict`, `--seed 4 --strict`). A node that fences while a startup
   ownership confirmation is in flight keeps the cell in `confirming_cells`
   ("confirming cell .. left the owner read in Fenced"); one that fences
   with a balancing move in progress keeps `cell.handoff` ("handoff cell ..
   is outside the pipeline in Fenced"). The node halts right after, so the
   effect is a debug-build panic instead of a clean `Halt`.
3. **Stale reads after a suspension that stops the monotonic clock**
   (`--seed 3 --strict`). `node_authoritative()` compares only monotonic
   time with the last renewal. After a VM suspension longer than the TTL
   (wall time advanced, monotonic did not), peers rightly treat the lease as
   expired and take the cell over at a higher epoch, but the resumed node
   still answers `Local` routes and releases read-only outputs until its
   next lease timer fires (the timer path does check `now_ms >=
   expires_ms`; `request`/`output` do not, and `RequestAt`/`OutputAt` carry
   no wall clock). Writes stay safe: their bucket proofs fail
   `VerifyOwnership` and their fleet proofs hit sealed followers. This
   contradicts the comment on `node_authoritative` ("a request evaluated
   after resume still refuses") for clocks that stop during suspend; Rust's
   `Instant` uses `CLOCK_MONOTONIC`, which does not count system suspend.
4. **A drain can exceed its own release ceiling** (`--seed 284 --strict`,
   also 1358, 1442). An activation that was already claiming when
   `ReleaseAll` arrived publishes, and the drain nominates it while the
   batch is full: `validate()` reports "handoffs 3 exceeds release ceiling
   2". Root cause not isolated beyond that.

An earlier model (without the leader's own log drain) reported a lost
acknowledged write: a fleet-acked tail stranded on followers after a reset,
restores failing because the only follower was down, a `Rebalance` releasing
the never-restored dormant cell, and a peer claiming the released record
(prior `None`, so no node-log recovery). With the drain modelled the window
closes in the sweeps run so far; it is noted here because the core does
release a dormant cell whose last restore failed without any durability
proof, and the real drain is asynchronous.
