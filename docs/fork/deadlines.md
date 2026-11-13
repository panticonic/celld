# Implicit deadlines: audit and node option

Status: implemented on `feature/codegen-deadlines`, 2026-11-13. Source reads
are of the fork at `6976878`.

Vibestudio's policy is that nothing ends work only because it took too long:
a deadline exists when an application or an operator asked for it. celld ships
several that nobody asked for. This audit lists each one, says what it
protects, and records the decision.

## The node option

`celld --no-implicit-deadlines`, or `CELLD_NO_IMPLICIT_DEADLINES=1` (the
environment accepts `0` or `1`, like every boolean). Both are in `celld
--help`. The default keeps Cloudflare and upstream semantics, so a deployment
that does not set it behaves exactly as before.

With the option on, each row marked **node option** below is not armed. Work
then ends in one of these ways, which are events and not clocks:

- its own result, success or failure, from the I/O it is waiting on;
- the caller's cancellation (client disconnect, `AbortSignal`, an abandoned
  RPC), which already reaches these paths;
- an eviction, an ownership change or a restart of the cell, all of which tear
  down what the cell holds;
- `ctx.abort()`;
- node shutdown, which keeps its own explicit bound
  (`CELLD_SHUTDOWN_TOTAL_MS`) and is never affected by the option.

Deadlines an application chooses are never affected: `setTimeout`,
`AbortSignal.timeout`, workflow step timeouts and sleeps, queue delivery and
retry delays, alarm times.

The policy lives in `crates/celld/deadlines.rs`. It is read once per process;
the JS harness gets it as `__cell.implicitDeadlines`.

## Audit

| Deadline | Location | Purpose | Decision | Rationale |
| --- | --- | --- | --- | --- |
| `blockConcurrencyWhile` 30 s | `js/harness.js`, `_runConcurrencyBlock` | Cloudflare resets a Durable Object whose `blockConcurrencyWhile` callback runs past 30 s. It protects the object's input gate: a callback that never settles holds every later event out forever. | Keep by default. **Node option** skips the timer. | Applications written for Cloudflare may rely on the reset, so upstream-compat nodes keep it. Without it the block ends when the callback settles, or on `ctx.abort()`, eviction or shutdown, each of which resets the object through `abort()`. Nothing is stranded: the gate is released by the same `finally` that releases it today. |
| Operation deadline, 15 s (`CELLD_OPERATION_DEADLINE_MS`) | `actor.rs` `operation_deadline()`; core: `crates/logic` `arm_operation_deadlines` and `watch_queued` | A swallowed effect is otherwise invisible: no event arrives, no timer watches, and a request waits forever while all state stays consistent. celld once parked requests past ninety seconds this way. It also bounds a stalled bucket or peer call during activation, durability proof, release and adoption, and a cell parked behind the admission gate. | Keep by default. **Node option** passes `operation_deadline_ms: None` to the core, which already treats `None` as "no deadlines" (`logic/types.rs`). The crates/logic code is unchanged. | The core needed no change. The shell sites that read the same value follow it: wake-entry maintenance runs without a timeout, a balancing move, which is optional, ends after one pass over the candidate peers that adopts nothing (the cell stays unowned for the next request; a drain keeps retrying as before), a bounded eviction's stop retries until the host releases the cell or a request reaches the cell and abandons it (`EvictionAbandoned`), and the remote route retry ends on the caller's cancellation or a resolved route. `Restore` and `StopRuntime` were never watched. What is lost: a genuinely swallowed effect is no longer converted into an error. That is why the default stays. |
| Queue lease settlement budget | `queue_policy.rs`, `actor::operation_deadline_ms()` | Sizes a queue consumer lease: admission wait + handler budget + settlement. Consumers observe the lease as a visibility timeout. | Keep. Not affected by the option. | It is a protocol value that decides when a message is redelivered. Removing it would change delivery semantics, not remove a watchdog. |
| Idle HTTP stream expiry, 60 s (`HTTP_STREAM_IDLE_TIMEOUT_MS`) | `http_streams.rs` | A stream source that nothing can read any more (no request context, no active read, write or close) holds a socket and a buffer. The sweeper expires it. | Keep by default. **Node option** gives idle sources no deadline. | The sweep is a leak guard, not a protocol rule. Without it a source ends on read-to-end, on cancel, or when the cell that owns it stops, which closes every stream of that owner (`close_locked`). Residual: an orphaned stream whose cell stays resident lives as long as the cell. That is bounded by the cell and visible in `/state`, so it was accepted over a hidden clock. |
| Workflow stall grace, 60 s (`__WF_STALL_GRACE_MS`) | `js/harness.js`, `driver.check` | A workflow `run()` awaiting something that is not a step, with no step running or blocked, can never be resumed by replay. The grace turns that silent wedge into a failed instance. | Keep by default. **Node option** arms no grace timer. | The instance stays running until the wait settles, or a pause, termination, eviction or shutdown ends the invocation, and replay re-enters the same wait. The timer was a plain in-isolate timeout and not durable, so removing it loses no persisted state. Authors who wrap long work in `step.do` are unaffected. |
| LTX durability proof budget, 10 s (`CELLD_LTX_DURABILITY_TIMEOUT_SECS`) | `ltx_repl.rs`; budget rules in `crates/logic/durability.rs` | The wait of one write (and of an eviction's final proof) for its capture and upload. It fails the write with an error instead of holding the client while a store is stalled. | Keep by default, already configurable. **Node option** raises it to "never" unless the variable is set. An explicit value always wins. | It bounds a real external resource, the object store, and a timed-out write is answered with an error and is never acknowledged, so safety does not depend on the clock. On a local store it can only fail a write that is merely slow. The proof wait is anchored to the capture and recomputed on every wake, so with no budget a stalled store holds waiters until it recovers, the cell is lost or the caller cancels. The snapshot size bound derived from the budget saturates, so every handoff uses a snapshot. Applies to SQLite cells. Dolt cells have their own durability path, owned by the Dolt work. |
| Outbound `fetch` timeout, 120 s (`CELLD_FETCH_TIMEOUT_S`) | `js.rs`, `op_fetch` | Stops a black-hole host from parking a request. reqwest applies it to the whole exchange, so it also ends a streaming response that runs longer than 120 s. | Keep by default, already configurable. **Node option** sends no timeout. | A model or SSE stream that legitimately runs for minutes is the case this breaks. The exchange then ends with the response, the caller's `AbortSignal`, the cell's end, or the connection. The egress hook work owns `op_fetch`; this is a one-line condition there. |
| Handler budget, 300 s (`CELLD_HANDLER_BUDGET_S`) | `engine_api::handler_budget()`; `runtime.rs` drive loop | Bounds the work a handler leaves running after it answered (`waitUntil`), measured from the request start. | Keep by default, already configurable. **Node option** makes it unbounded. | Without it that work ends when it settles, when nothing can move it any more (`Wake::Idle` marks the request stuck), or on cancellation, eviction or shutdown. The queue lease still reads the configured value (`configured_handler_budget()`). |

### Reviewed and left alone

These look like timers but are explicit, protocol or housekeeping, so the
option does not touch them.

| Item | Why it stays |
| --- | --- |
| Node lease TTL, 10 s (`CELLD_TTL_MS`), fence timers | Distributed correctness: one writer per epoch depends on it. It is configurable, not removable: a loaded or suspended dev machine raises `--lease-ttl-ms` (docs/README.md, "Node lease timing"). |
| Shutdown total bound (`CELLD_SHUTDOWN_TOTAL_MS`) | It is the explicit stop event itself. |
| Admission wait, 1 s (`CELLD_ADMISSION_WAIT_MS`) | Backpressure: it decides between queueing and `CapacityExhausted`, and `0` is valid. |
| Workflow step timeouts (`__wfTimeout`), `AbortSignal.timeout`, `setTimeout`, queue producer grouping (`producerGroupMs`) | Requested by the application or part of the API contract. |
| `REMOTE_ABORT_TTL`, `REMOTE_COMPLETION_TTL`, `REAP_INTERVAL`, `HTTP_STREAM` sweeper wake-ups | Bookkeeping garbage collection. None of them ends application work. |
| Idle eviction (`CELLD_IDLE_EVICT_S`), alarm residency, ready-fleet gate, rebalance interval | Already opt-in or disable-able (`0`), and they move cells without failing work. |

## What was verified

All runs are through `celld dev` on a debug build of this branch, against a
debug build of the unmodified base (`6976878`).

- `examples/deadlines/check.sh` (about 80 s). A `blockConcurrencyWhile`
  callback that waits 35 s is reset with "waited for too long" by default, and
  completes after 35 s under `CELLD_NO_IMPLICIT_DEADLINES=1`.
- `hello`, `counter`, `facets`, `rpc` and `dynamic-worker-tails` give the same
  responses on the base build, on this build, and on this build with
  `CELLD_NO_IMPLICIT_DEADLINES=1`.
- `CELLD_NO_IMPLICIT_DEADLINES=2` stops startup with "must be 0 or 1".

Not exercised by a test, so they rest on reading the code: the operation
deadline sites in `actor.rs`, the idle stream sweep, the workflow stall grace,
the durability budget, the fetch timeout and the handler budget. Each is a
single condition or a saturating value. The simulator that could drive the
operation-deadline paths is built in a later phase (design.md phase 0b).

## Not done

- No simulator or fault-injection run (see above).
- Dolt cells' durability and generation timeouts are not covered. They do not
  exist yet. When they do, they join this table.
