# Findings in upstream celld v0.6.3

These defects were found by the fork's own harnesses: `crates/logic-sim`,
`tests/e2e` and `tests/fleet`. They are present in upstream code, not
introduced by the fork.

None of them loses an acknowledged write in what we tested:
- 5,000 simulator seeds (about 1.3 million acknowledged writes checked);
- 18 fleet fault scenarios.

## Coordination core (`crates/logic`), found by the simulator

Each finding reproduced on upstream v0.6.3 with `cargo run -p logic-sim -- --seed N --strict`.

| # | Seeds | Finding | Impact | Fixed in fork |
| --- | --- | --- | --- | --- |
| S1 | 52 | Shedding or idle eviction can start a second owner release while an earlier release of the same cell is still in flight. `validate()` then fails ("indexed cell op … has no owning slot"). Explicit evictions refuse in this state; the shed and idle path does not. | A debug build panics. The late release is rejected by its epoch check, so safety holds. | **Yes.** `begin_eviction` (shed, idle, drain and explicit paths) refuses a cell whose `releasing` slot is set, as `admit_eviction` did; `owner_released` pumps capacity so a refused shed resumes. The release was never lost, only its index entry. | |
| S2 | 97, 4 | Fencing during a startup ownership confirmation or a rebalance move leaves markers set that `validate()` rejects. | A debug build panics instead of halting cleanly. | **Yes.** `fence()` clears `confirming`/`confirming_cells` and each cell's `handoff` marker. | |
| S3 | 3 | **Stale reads after a suspension that stops the monotonic clock.** The core decides `node_authoritative` from monotonic time only, and `CLOCK_MONOTONIC` stops during system suspend. After a suspension longer than the lease, peers take the cell over at a higher epoch. The resumed node keeps serving reads until its next lease timer fires. Request and output handling do not compare against wall time. | Stale reads. Writes stay safe (an ownership check or follower seal stops them). Contradicts the comment on `node_authoritative`. | **Yes.** The shell's `now_mono_ms` now counts suspended time (`CLOCK_BOOTTIME` on Linux, `CLOCK_MONOTONIC` = `mach_continuous_time` on macOS); a watcher re-aims sleeps and the actor timer queue after a resume. See below. | |
| S4 | 1442, 2395, 3393 | A drain exceeds its own release ceiling ("handoffs 3 exceeds release ceiling 2"). An activation already in flight publishes and is nominated while the batch is full. | Root cause not yet isolated. | **Yes.** Root cause found: see below. | |
| S5 | (model) | The core will release a cell whose last restore failed, with no proof that its data is durable. The window closes only because the leader drains its log copy to the bucket asynchronously. | Not confirmed as a bug; it depends on drain timing. | No (not confirmed as a bug). | |

### Root causes and fixes (S1 to S4)

**S1.** A cell woken while its owner release was still in flight is `Resident`
again but still has `releasing = Some(A)`. `runtime_stopped` of a second
rebalancing eviction then wrote `releasing = Some(B)` over it, so operation A
stayed in `cell_ops` with no owning slot. Only `admit_eviction` (explicit
eviction) refused this state; `begin_eviction`, which shedding, idle eviction
and the drain use, did not. The refusal now lives in `begin_eviction`, which
every path reaches. A shed that was refused for this reason would otherwise
wait for the next unrelated trigger, so `owner_released` pumps capacity.

**S2.** `fence()` cleared the index sets it knew (`adopting_cells`,
`quiescing_cells`) but not `confirming_cells`, `State::confirming` or the
cell-level `confirming` and `handoff` markers. `validate()` requires both to be
inside a pipeline phase, and a fenced cell is in none. They are cleared with the
rest of the fence, and a late `LocalCellsRead` no longer starts a confirmation.

**S3.** The shell fed the core `Instant`-based milliseconds. `Instant` is
`CLOCK_MONOTONIC` on Linux and `CLOCK_UPTIME_RAW` on macOS, and neither counts
suspended time, while peers measure the lease in wall time. After a suspension
longer than the lease, a peer took the cell over and the resumed node still saw
a fresh lease. Fix (`crates/celld/asyncrt.rs`):
* `mono_ms()` reads `CLOCK_BOOTTIME` (Linux, Android) or `CLOCK_MONOTONIC`
  (macOS, iOS; on Apple it is `mach_continuous_time`, the reverse of Linux). Other
  targets, Windows included, keep `Instant`; QPC counts sleep on current
  Windows, which is not tested here.
* So `request`, `output` and every other core entry point see the lapse
  immediately (`RequestAt` and `OutputAt` carry `now_mono_ms`). No extra
  wall-clock check is needed in the core: the clock contract is now what the
  comment on `node_authoritative` assumed.
* Tokio timers still run on `Instant`, so a timer armed before a suspension fires
  late. A watcher task compares the two clocks every 250 ms and, on a divergence
  of 100 ms or more, wakes every `asyncrt` sleep and the actor's `DelayQueue`; each
  re-aims at what the node clock says is left. A lease that lapsed during the
  suspension fires its fence timer within about 250 ms of the resume.
* The simulator's suspend model follows: a suspension is now a long pause (the
  clock keeps running), and the old stopped clock is the `suspend-stops-monotonic`
  mutation, which the checker catches (seed 3).

**S4.** `pump_release` bounds everything leaving a draining node by
`max_releases`: a nomination marks the cell `quiescing` (one handoff) before the
eviction starts. But a cell can become resident during the drain (a queued
activation is admitted as capacity frees), and demand shedding then called
`begin_eviction` on it with no reservation, on top of a full batch of releases
and adoptions: eviction 1 + adopting 2 = 3 > 2. `begin_eviction` now refuses a
non-quiescing cell while draining when `handoffs_in_flight() >= max_releases`;
`pump_release` nominates the cell when a slot frees.

Strict sweep after the fixes: seeds 0..3000, zero failures and zero liveness
stalls (`KnownCoreIssue` and `--strict` are gone; every finding is fatal).
`FIXED_FINDINGS` in `crates/logic-sim/tests/seeds.rs` replays the reproducing
seeds.

## Runtime, found by the E2E suite

| # | Finding | Fixed in fork |
| --- | --- | --- |
| E1 | `new Response("text")` is sent with no `content-type`. workerd sends `text/plain;charset=UTF-8`. | **Yes.** `Response` defaults `content-type` to `text/plain;charset=UTF-8` for a string (or other non-object) body, per Fetch's "extract a body". Blob type, `URLSearchParams` and `FormData` already carried theirs; streams, `ArrayBuffer` and views carry none; `Response.json` already set JSON. Responses rebuilt from host results pass `__rawBody` so they never gain a header the origin did not send. `Request` has the same gap and is not changed. | |
| E2 | The dev-mode R2 etag is a counter, not the content MD5, so identical writes get different etags. This contradicts `docs/services/r2.md`. | No. | |
| E3 | A graceful shutdown with an open WebSocket waits out the full 25 s drain, then the client sees close code 1006 instead of 1001. | **Yes**, for the preserved restart that `celld dev` and `POST /shutdown?handoff=preserve` use. See below. | |
| E4 | static-assets: `/index.html` redirects with 307 rather than a permanent redirect. A 404 is served as `Not found` with `content-type: application/json`. | No. | |

**E3 detail.** `celld dev` turns SIGTERM into `POST /shutdown?handoff=preserve`,
a same-node restart that keeps cells resident. That drain waited for the shell to
be quiet (no connections, no sockets), and nothing ever told an open client
socket to close, so it sat out the 25 s deadline and the process exit dropped
the socket (1006). It was not waiting for sockets to migrate: a socket's
transport cannot outlive the process, and `docs/cloudflare-compat.md` says a
client must reconnect. Once every request has answered and the core is
drained, the drain now closes every open socket with 1001 and exits when the
close handshakes finish. A SIGTERM handoff shutdown was unaffected: it closes
each cell's sockets with 1012 ("service restart") when it nominates the cell.
Outbound sockets are not part of this registry sweep.

## Fleet behaviour, found by the fleet harness

None of these violates an invariant.

- Sending SIGTERM to every node at once takes the full 40 s
  `CELLD_SHUTDOWN_TOTAL_MS`. The log shows `shutdown_handoff_stalled`, because
  cells are handed to peers that are also shutting down.
- Requests in flight to a dead owner fail with
  `DurableObjectRoutingError` or hang. They are not replayed.
- Under load, nodes logged `gray follower evicted` and ran with one follower for
  several seconds. During that window an acknowledged write lives on two nodes
  only, which `docs/guarantees.md` permits.
