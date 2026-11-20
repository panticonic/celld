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
| E5 | An alarm set far in the future (year 2100) panics the node: "invalid deadline", `actor/effects.rs:252`. Found while testing object fork. | **Yes.** See below. | |
| E4 | static-assets: `/index.html` redirects with 307 rather than a permanent redirect. A 404 is served as `Not found` with `content-type: application/json`. | No. | |
| E10 | An op started by a continuation of event A, run in another event's turn while A's driver is on its way out (A answered and has no op of its own left), is handed to A and dropped with A: its promise never settles, and whatever awaits it hangs. The driver decides an event is finished between turns, without the isolate, so another turn can hand it an op in that gap; `abandon` then closes the mailbox and drops the op's resolver. Found by Vibestudio: an agent `__alarm` that never completed (no model frame sent, a commit or reply that never resumed), about one console-streaming run in three. | **Yes.** See below. | |
| E11 | A continuation of an event that has answered but still runs (it keeps I/O it left pending) is refused when it starts a block in another event's turn ("the cell event ended before it could acquire an input gate"): the origin's cross-entry claim lifecycle closed at its retirement check, and the refused claim was taken as a refusal of the block. A gap of the E6 fix. Found by Vibestudio: a fresh agent failing its first alarm pass, about one session in five. | **Yes.** See below. | |
| E12 | A Durable Object event that its CPU limit ends drops every operation it left pending: a timer, a subrequest or a storage read its continuations wait on. Their promises never settle (`celld::ops`: "an op was handed to an event that no longer takes ops; its promise never settles"), although the object keeps its heap and keeps serving. A completed event keeps the same operations (`completed_cell_event`); a terminated one did not. A kernel that runs code under `waitUntil` behind a deadline timer never saw the timer fire, so its run never settled. Found by Vibestudio (an `eval run` of `while (true) {}` with `--timeout`). | **Yes.** See below. |
| E13 | A termination (a CPU limit, `ctx.abort()`, `process.exit()`) inside `transactionSync()` or `transaction()` leaves the transaction open: the `catch` and `finally` that roll it back never run. The connection stays inside `BEGIN IMMEDIATE`, and the instance's storage still names the dead transaction, so the next `transactionSync()` nests inside it and its commit makes the unwound writes durable. On a Dolt cell, every later `transactionSync({ commit })` fails ("{ commit } is only for the outermost transaction"), and every `transaction()` fails with "cannot start a transaction within a transaction". | **Yes.** See below. |
| E14 | A continuation whose own event has ended is refused a block (`transaction()`, `blockConcurrencyWhile`: "the cell event ended before it could acquire an input gate") when it runs in a turn of an event that is still driving the object's I/O but no longer counts as running: an event polling ops handed to it on its way out (after E10 closed its mailbox), or a call its CPU limit ended (E12: its gate claims were retired for good). The refusal repeats: a retry's timer is polled by the same event, so the retry runs there again. A turn's closing checkpoints also ran with no event installed, so a continuation run there had no driver at all. Gaps of the E6, E10 and E12 fixes. Found by Vibestudio: Pi's commit chain in an agent on a Dolt facet, refused on all 41 attempts over 6 s, in about three agent instances in ten. | **Yes.** See below. |
| E9 | `fetch(url, { headers: { Upgrade: "websocket" } })` from a Durable Object fails with "WebSocket upgrade failed: isolate stopped reading WebSocket" when it runs in a continuation that outlived its event (an alarm's detached work, a promise another request released): the object's socket is taken for a Worker's, polled by the isolate and closed when the running event ends. Separately, the same call inside `blockConcurrencyWhile` (an alarm that dials in its initialisation block) waited for the 30 s limit and reset the object. | **Yes.** See below. | |
| E8 | `Response.clone()` of a response whose body is a stream throws "Cannot clone a streaming response before consumption". workerd tees the body. `Request.clone()` already teed. | **Yes.** `Response.clone()` tees a streaming body as `Request.clone()` does: the response keeps one branch and the clone gets the other, so either can be read first, and a `fetch()` response (a host stream) tees through the host. Scenario `clone-stream`. | |
| E7 | A request in flight to a facet is not ended when the facet is aborted (`ctx.facets.abort`, or the parent's release): the facet's handler keeps running, the caller's call never settles, and the caller's own response is never sent even when it caught or raced the call. A handler that another event's turn settles is not noticed either while one of its own operations is pending: the driver waits on that operation alone, and for an operation that never ends the event's response is never sent. workerd rejects the pending call with the abort reason. Found by Vibestudio (retiring an object with a request in flight). Both are in upstream code (`op_facet_fetch`, the drive loop's `wake`). | **Yes.** See below. | |
| E6 | A promise continuation that runs after the event that created it ended cannot use its Durable Object. `blockConcurrencyWhile` (and so `transaction()`) rejects with "the cell event ended before it could acquire an input gate"; `fetch`, a stub call or an RPC call starts, and its promise never settles. workerd allows all of it while the object is alive. Found by Vibestudio's agent runtime, which starts blocks from background continuations. Upstream refuses it on purpose (`RETIRED_INPUT_GATE`, `op_gate_acquire`) and drops the op of an owner that is gone. | **Yes.** See below. | |

**E15. Loaded Dolt facets lost the storage root's remote declaration.** The
API looked up the facet's public id class in the current isolate's deployment
config. A loaded isolate does not have that config, and a custom public id
does not identify the storage owner. Adoption now carries the root's declaration
in the runtime configuration for the actual facet scope. The Dolt remotes
example checks both a declared and an undeclared owner with the same loaded
code and a custom id. The Vibestudio probe relays a push from another cell and
checks that the loaded target's callback observes the new head and rows.

**E16. Memory pressure ignored nested Linux cgroup v2 limits.** The node read
only the controller mount root, so an ordinary systemd scope reported no
cgroup charge or limit. A model-free Vibestudio benchmark was killed inside
its 6 GiB scope after 6,784 observed turns. The node now resolves its group
from `/proc/self/cgroup` and reads the charge from the visible group with
the lowest finite limit. When there is no finite limit it samples the
nearest group with a memory controller. The legacy cgroup v1 paths remain.

**E17. Dolt capture allocated extra copies of whole generations.** Full
captures copied every page and built a full encoded LTX before writing it.
An ordinary reopen also read the entire chunk file and newest L0 just to
inspect their headers. Capture and snapshot export now encode pages directly
into a writer, and reopen reads just the headers when it can resume an existing
capture. A 96 MiB Dolt collection under the 6 GiB cap brought peak anonymous
memory in the scope from 827 MiB to 644 MiB, with all 96 rows and their byte
counts preserved. The regression checks that snapshot output arrives in
small writes and matches the previous wire format byte for byte.

**E18. Eviction accumulated each handoff snapshot in memory.** A diagnostic
run found an eviction snapshot growing the writer's buffer from 176 MiB
to 352 MiB while the encoder also held a 275 MiB input file. The preceding
run died in the allocator's large-buffer copy routine. The handoff now
streams encoded bytes to an anonymous scratch file and publishes it through
the existing multipart uploader. Dolt supplies the encoder with 1 MiB
reads. The byte budget still sends oversized snapshots through the L0 proof.
A 96 MiB Dolt fixture published a 97 MiB snapshot through VibeStore, then
restored all 96 rows from the bucket after the local state was removed.
Peak anonymous memory during publication fell from 399 MiB to 178 MiB
in the same test scope.

**E19. Dolt packs inherited the ordinary SQL value limit.** An installed
Vibestudio workspace could not export its published history even though
each stored value was within the limit. The native pack statements passed
the whole transfer through SQLite as one BLOB, which exceeded 2,200,000
bytes. The runtime now applies the existing wire length budget during
those statements and restores the connection limit before returning.
The native regression covers a multi-megabyte fetch and push in both
directions.

**E14 detail.** The E6 rule gives a block from a continuation whose event has
ended to the event whose turn runs it: that event is the block's driver
(`op_with_input_gate_context`), takes the gate (`op_gate_acquire`) and polls the
block's ops. The driver had to be *live* (`live_io_context`: its mailbox open)
and able to take claims (`acquire_cell_gate`: not retired), and three turns ran
JavaScript for an event that was neither:

- **An event polling what it was handed on its way out.** E10 made the driver
  close the mailbox inside a turn once the event is finished, and poll what was
  handed in the gap. It kept the mailbox closed while it did, and the continuation
  those ops resume runs in its turns: a `transaction()` there named that event
  as driver, which was "not live", and was refused. Every retry ran in the same
  event, because the retry's timer, attributed to no live event, was polled by
  the turn that started it. The mailbox now closes only when it is empty
  (`IoContext::close_handed_ops_if_empty`): an event that stays to poll ops is
  running, and its driver asks again when it next finds the event finished.
- **A call its CPU limit ended (E12).** It keeps driving the object's pending
  I/O, and its gate claims were retired when its blocks were released
  (`finish_retired_input_gate_context`), so every block a later continuation
  asked for in its turns was refused (a kernel's deadline continuation that
  records the run in a transaction). The claims are taken again after the
  release (`IoContext::reopen_input_gates`); a block that had queued is still
  refused through the retirement it already saw.
- **A turn's closing checkpoints** (`finish_turn`: after `settle`, and after the
  foreground task queue is pumped) ran with no event installed as current, so a
  continuation run there had no driver and its block named its own dead event.
  The turn's event is now current there, as it is in `deliver`.

Refused still: a block of an instance that was reset ("the Durable Object was
reset before the block could start"); a queued block whose driving event is
ended while it waits (the node stopping the object, a handler timeout, a CPU
limit ending that event); and a block asked for outside every event of its
object (a stateless request's turn in the same isolate, where workerd refuses
the I/O as belonging to another request). Upstream v0.6.3 refuses every block
of a continuation whose event has ended (E6), so it has this refusal in a
wider form; the three turns above are the fork's own E6, E10 and E12 code,
except the unguarded closing checkpoints, which upstream has too. Scenarios
`continuations` ("a continuation resumed by a request on its way out runs a
chain of transactions") and `facet-limit-recovery` (a run recorded by a
transaction after its call's CPU limit, on SQLite and Dolt facets); both fail
on fork `2d1749d` with the refusal.

**E12 and E13 detail.** A CPU limit terminates the isolate's execution; the turn
that notices it (`finish_turn`, or the first turn in `start_limited_cell_event`)
failed the event and called `abandon`, which drops the resolver of every
operation the event still owned. That is right for code that ends with its
event, and wrong for a Durable Object, whose pending I/O belongs to the object
and outlives the event that started it (workerd keeps it on the actor's
`IoContext`; celld keeps it after a completed event). The terminated event now
keeps its referenced operations as a completed one does, releases the critical
sections it held (as `finish_terminated_actor_event` does for an abort: a held
block resets the object), and restarts its CPU count, so the work it left runs
under the same limit counted from zero instead of being ended on its first turn.
Other terminations, and a stateless request, still end with their operations.
V8 itself discards the microtask queue on termination and never settles the
promise of a function it unwound; nothing can settle those, so code that awaits
the terminated call must race it against its own deadline.

A termination taken in a cell's turn (either path, any kind) now checks the
cell's connection (`storage::transaction_open`). If a transaction is open, the
instance is reset (`__resetTerminatedActor`: its state is aborted, its queued
puts dropped, the instance removed) and the connection rolled back
(`storage::rollback_abandoned_transaction`); the next event constructs a new
instance. The caller sees the termination's error. Scenario
`facet-limit-recovery` (example `facet-limits`): before the fix (fork `ec0d6f5`), its run step
stays `running` and four of its five transaction steps fail.

**E10 detail.** `drive_cell_inner` (`crates/celld/runtime.rs`) leaves its loop
when `entry.finished()`, which it decides between turns. An op a continuation of
the event starts in another event's turn is attributed to the event while its
mailbox is open (`op!` wrapper, `adopt` → `hand_op`), and reached the driver
only through `take_handed_ops` on its next wake; when there was none, `abandon`
closed the mailbox and dropped the resolver. The driver now closes the mailbox
inside a turn of its isolate once it finds the event finished
(`InFlight::close_handed_ops_taking`), so no turn can hand an op between the
check and the close, and polls whatever was handed in the gap before it leaves.
After the close the event no longer counts as running, so a later continuation's
op goes to the turn that runs it (E6). `hand_op` logs a dropped op at `warn`
(`celld::ops`). Scenario `continuations` ("an op handed to a request on its way
out still settles"; the arming request holds the isolate so the releasing
request's turn runs in the gap, deterministic).

**E11 detail.** `op_gate_acquire` claims the origin event (`claim_cross_entry_gate`)
when a block's origin and driver differ, so the origin stays until the block
ends. `InFlight::retired()` closes that lifecycle as soon as the event has
answered, holds no gate and has no claim, while the event can still be running
(its mailbox open, its pending I/O polled). A continuation of it is then
neither an orphan (E6: its event looks live) nor able to claim it, and the block
was refused. A failed claim now means what an ended origin means: the driver
stands for it and the block is the object's own. Scenario `continuations`
("lingering": the arming request leaves a timer pending).

**E6 detail.** A continuation keeps the token of the event that created it. When
that event is gone, the token names nothing: the gate op refused it, and an op
the continuation started was handed to the dead event, whose resolver was then
dropped (a hang, not an error; storage reads, SQL and `transactionSync` worked
only because those ops are synchronous). The object is alive, so the work now
belongs to it, as in workerd, where it runs in the request that is current:
- a block from such a continuation (`blockConcurrencyWhile`, `transaction()`)
  starts a block of the object's own, queued behind one that is running, with the
  event whose turn runs the continuation as its origin and driver. That event
  stays until the block ends, as for a block it started itself;
- an op it starts is polled by the event whose turn runs it, which stays until
  the op settles;
- its outbound effects are attributed to its object, not to the event that runs
  it (in a shared isolate that is often another cell): the egress identity is
  the object's, and the output gate is the one a read of the whole cell takes,
  behind every write the cell has committed and not yet proven. The core keeps
  the cell of each of the last 8192 ended events for this; a continuation older
  than that is attributed to the event that runs it;
- an instance that was reset (`ctx.abort()`, a failed block) refuses a new block
  with an error instead of starting work for an object that is gone.

A block that never ends holds its object (`--no-implicit-deadlines`; by default
it is reset after 30 s) and nothing else: a request queued behind it is released
when the node stops, and the node stops and starts again.
Scenario `continuations`, example `examples/continuations`.

**E7 detail.** Three parts. (1) The facet stub rejects every call still pending
on an aborted run with the abort reason (`ctx.facets.abort(name, reason)`, and
the parent's release): `record.abortion` is raced against each call, whatever
the host does with the request behind it. (2) A fetch into a facet is a request
of the facet's own cell, now listed with an id (`FACET_FETCHES`), and an abort
cancels each one as a client's disconnect cancels a request, so the facet's
handler is told and the caller's host operation ends with an error instead of
waiting forever (a facet RPC is not cancelled this way: the call rejects in
JavaScript, and the facet's event is left to its isolate). (3) The reply of a
root handler that settles in another event's turn: each cell event watches its
handler promise (`watch_settlement`), and a settlement wakes the driver, which
looks in (`Wake::Poll`) instead of waiting out an operation that may never end.
Scenario `facet-abort`, example `examples/facet-abort`.

**E9 detail.** Two causes. (1) Whether a socket is the object's or a Worker's was
decided from the actor-event stack, which holds only the events whose handler
is still running; a continuation after that has no entry, so the socket was
request-scoped. The owner is now `__ownerScope()`: the actor event's object, or
the cell of the event the continuation began in (`__io_context_cell`, from the
event or from the table E6 keeps), and the p2p ownership rule uses the same
lookup before falling back to "the isolate's only object". (2) The host opened a
socket from `fetch()` by running a `webSocketOpen` event in its cell, which
waits for the cell's input gate. A caller inside `blockConcurrencyWhile` holds
that gate and awaits this upgrade, so each waited for the other until the
block's limit. That event only marks the socket open, which the caller does
itself with `accept()`, so a socket from `fetch()` is registered with its cell
and no event is run. A `new WebSocket(url)` still gets its `open` event.
Scenario `ws-alarm`, example `examples/ws-alarm`: from a request, from a
continuation another request releases, and from an alarm that awaits the dial,
dials in a critical section, or returns before the dial ends.

**E5 detail.** The actor arms every timer in a `tokio_util` `DelayQueue`, whose
timer wheel spans about two years, and `insert` and `reset` panic on a delay
beyond it. A `setAlarm` for any date past that took the node down, and the alarm
was durable, so the node panicked again at each start while it was pending. The
fix is at the arming: a deadline is armed for at most a chunk (7 days,
`MAX_DELAY_CHUNK_MS`). When a chunk expires with the deadline still more than a
second ahead, `actor::effects::expire` arms the next chunk, for the same arm,
and the timer reaches the core at its deadline as a near one does. `reaim_delays`
(the resume path) clamps the same way. The e2e `alarm` scenario sets an alarm for
2100 and keeps it across a restart, SIGKILL and restore, then replaces it with a
near one; `actor::effects` has unit tests for the chunking. Before the fix that
scenario fails with the panic. Not changed: the deterministic test drivers, which
never arm a far timer.

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
