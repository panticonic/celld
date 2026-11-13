# Findings in upstream celld v0.6.3

These defects were found by the fork's own harnesses: `crates/logic-sim`,
`tests/e2e` and `tests/fleet`. They are present in upstream code, not
introduced by the fork.

None of them loses an acknowledged write in what we tested:
- 5,000 simulator seeds (about 1.3 million acknowledged writes checked);
- 18 fleet fault scenarios.

## Coordination core (`crates/logic`), found by the simulator

Each finding reproduces with `cargo run -p logic-sim -- --seed N --strict`.

| # | Seeds | Finding | Impact |
| --- | --- | --- | --- |
| S1 | 52 | Shedding or idle eviction can start a second owner release while an earlier release of the same cell is still in flight. `validate()` then fails ("indexed cell op … has no owning slot"). Explicit evictions refuse in this state; the shed and idle path does not. | A debug build panics. The late release is rejected by its epoch check, so safety holds. |
| S2 | 97, 4 | Fencing during a startup ownership confirmation or a rebalance move leaves markers set that `validate()` rejects. | A debug build panics instead of halting cleanly. |
| S3 | 3 | **Stale reads after a suspension that stops the monotonic clock.** The core decides `node_authoritative` from monotonic time only, and `CLOCK_MONOTONIC` stops during system suspend. After a suspension longer than the lease, peers take the cell over at a higher epoch. The resumed node keeps serving reads until its next lease timer fires. Request and output handling do not compare against wall time. | Stale reads. Writes stay safe (an ownership check or follower seal stops them). Contradicts the comment on `node_authoritative`. |
| S4 | 1442, 2395, 3393 | A drain exceeds its own release ceiling ("handoffs 3 exceeds release ceiling 2"). An activation already in flight publishes and is nominated while the batch is full. | Root cause not yet isolated. |
| S5 | (model) | The core will release a cell whose last restore failed, with no proof that its data is durable. The window closes only because the leader drains its log copy to the bucket asynchronously. | Not confirmed as a bug; it depends on drain timing. |

## Runtime, found by the E2E suite

| # | Finding |
| --- | --- |
| E1 | `new Response("text")` is sent with no `content-type`. workerd sends `text/plain;charset=UTF-8`. |
| E2 | The dev-mode R2 etag is a counter, not the content MD5, so identical writes get different etags. This contradicts `docs/services/r2.md`. |
| E3 | A graceful shutdown with an open WebSocket waits out the full 25 s drain, then the client sees close code 1006 instead of 1001. |
| E4 | static-assets: `/index.html` redirects with 307 rather than a permanent redirect. A 404 is served as `Not found` with `content-type: application/json`. |

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
