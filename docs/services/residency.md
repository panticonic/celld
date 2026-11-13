# Residency holds

A Durable Object normally lives only as long as celld finds it convenient. An
idle object is evicted after `CELLD_IDLE_EVICT_S`, a node under memory pressure
sheds idle objects, and a drain or a deploy moves every object away. Nothing in
memory survives any of those, and Durable Objects are designed so that nothing
has to.

Some state cannot be rebuilt from storage: a live JavaScript heap in an eval
kernel, a warm model, an open parser. A **residency hold** keeps such an object
resident until its owner releases the hold. celld never ends a hold with a
timer, and never ends one to make room.

This is a celld extension. Cloudflare has no equivalent, so the API is opt-in
for each class, and a Worker that uses it does not run unchanged on workerd.

```js
export class Kernel {
  async run(code) {
    this.hold ??= await this.ctx.residency.hold();
    // ... the isolate and its heap now stay resident ...
  }
  finish() {
    this.hold?.release(); // the object is an ordinary idle object again
  }
}
```

## Declare it

A class can hold only if the deployment declares it, in a top-level `celld`
block of the Wrangler config:

```jsonc
{
  "durable_objects": { "bindings": [{ "name": "KERNEL", "class_name": "Kernel" }] },
  "migrations": [{ "tag": "v1", "new_sqlite_classes": ["Kernel"] }],
  "celld": {
    "durable_objects": {
      "Kernel": { "residency": true }
    }
  }
}
```

The block is the one place celld-only per-class capabilities live. Each
capability is an optional field of one object, so several (`residency`,
`codegen`) can sit on one class. An unknown key, a value that is not a boolean,
and a class that `durable_objects.bindings` does not declare are all errors at
`celld deploy` and `celld dev`, and the error names the keys that exist.
Granting residency changes the deployment version, and the manifest requires the
`residency-v1` feature so that a node without this support refuses the
deployment instead of evicting an object that depends on staying resident.

A class that is not declared gets `ctx.residency.enabled === false`, and
`hold()` rejects with `ERR_RESIDENCY_NOT_DECLARED`.

## The API

`ctx.residency.hold()` returns a promise for a handle:

| member | meaning |
| --- | --- |
| `release()` | Ends this hold. Idempotent. When the object's last hold ends it is an ordinary idle object again, eligible for eviction on the usual terms. |
| `released` | A promise that resolves to `{ reason }` when the hold ends: `"released"` (the owner), the node's reason (`"drain"`, `"swap"`, `"operator"`), or `"stopped"` when the object's runtime stopped for another reason (a crash or reset). |
| `onrevoked` | Assign an async function `({ reason }) => ...`. The node calls it before it takes the object away. See [Revocation](#revocation). |
| `isReleased`, `revokedReason` | The hold's state. |
| `[Symbol.dispose]()` | Same as `release()`, for `using`. |

An object can hold more than once (up to `CELLD_MAX_HOLDS_PER_CELL`, default
16), for example one hold per running evaluation. It stays resident until the
last one ends.

`hold()` is a promise rather than a synchronous call because the node's
single-writer decision runs on its core actor, and the isolate has to ask it.
The refusal below still happens at the call: a rejected `hold()` retained
nothing, and an accepted one is never silently taken back later for lack of
room.

### Refusal

A rejected `hold()` throws a `ResidencyError` with a stable `code`:

| code | meaning |
| --- | --- |
| `ERR_RESIDENCY_NOT_DECLARED` | The class has no `residency: true` in the deployment. |
| `ERR_RESIDENCY_NOT_GRANTED` | The object runs in a loaded Worker whose loader did not grant `celld: { residency: true }`. |
| `ERR_RESIDENCY_BUDGET` | The node already pins as many cells as it allows. Release a hold, or retry later. |
| `ERR_RESIDENCY_CELL_LIMIT` | The object already carries `CELLD_MAX_HOLDS_PER_CELL` holds. |
| `ERR_RESIDENCY_LEAVING` | The object is draining, being handed off, or swapping generations. A hold now would only be revoked. |
| `ERR_RESIDENCY_NOT_RESIDENT` | The core does not consider the object resident at the epoch the caller observed. |
| `ERR_RESIDENCY_UNAVAILABLE` | The node holds no authority right now (fenced, or not serving). |

## What a hold pins

A hold makes the cell **active**, the same predicate that a running request
makes true. Every voluntary way of removing a cell reads it, so a held cell is
protected from all of them at once:

- idle eviction (`CELLD_IDLE_EVICT_S`);
- the memory walk-down and the hard cap's shedding;
- an explicit `evict` (refused as `cell_active`);
- ownership balancing, which moves only cells already evicted;
- lending the isolate to a stateless Worker request.

It does not protect against the node's own terminal events, which a hold cannot
block for good. Those are listed next.

## The budget

Holds draw on the same node-wide budget as outbound WebSockets
(`may_pin_outbound`): at most 50% of the resident ceiling
(`CELLD_MAX_RESIDENT_CELLS`), counted in **cells** that are pinned by any hold
or outbound socket together, and at least one. A cell already pinned costs the
budget nothing more, so a second hold on a held cell is always allowed (up to
the per-cell limit).

The budget exists because a pinned cell leaves the pool the node sheds from.
Pin the whole ceiling and the node cannot give anything back. It is checked at
`hold()`: when a hold would exceed it, `hold()` rejects with
`ERR_RESIDENCY_BUDGET`. celld never admits a hold it might have to break for
budget reasons later, because breaking one destroys exactly the state it was
taken to protect.

Outbound WebSockets count holds too, so the two together cannot pin more than
the budget.

## Memory pressure

**A held cell is never shed.** When the node is above its memory ceiling it
walks down by evicting idle cells. A held cell is not idle, so the walk skips
it. If every candidate is held, the walk stops and evicts nothing.

The node stays latched in the shedding state, which refuses new cold
activations (backpressure: the request waits for capacity and then fails with
`CapacityExhausted`, the existing "not here, not now" answer). The node does not
kill a held cell to admit a new one. The same holds at the resident cap
(`CELLD_MAX_RESIDENT_CELLS`): a node full of held cells refuses further cells
rather than evicting one. Because holds are limited to half the ceiling, at
least half of a node's capacity always stays evictable.

Pressure ends the way it always does: the load falls, or an owner or operator
releases a hold. An operator can end the holds of one cell:

```sh
curl -X POST http://INTERNAL_ADDR/revoke-holds/Kernel:name
```

The object is told (see below), its holds drop when its handlers settle, and the
cell then evicts like any idle cell.

## Revocation

A hold is the owner's promise to keep something alive, and the node's promise
not to take it away for convenience. Three events end holds anyway, because the
isolate itself is going away. In each, the node **tells the object first**.

| event | what happens |
| --- | --- |
| **Drain** (shutdown, deploy, spot reclaim) | When the cell joins the handoff batch, the node revokes its holds with reason `"drain"`. |
| **Clean reload** (same-node restart) | The database is kept but the isolate is not. The node revokes every hold with reason `"drain"` and the reload waits for the handlers. |
| **Forced generation swap** | A held cell is not a safe point, so a swap waits for the maximum age (or at once for an eager class), then revokes with reason `"swap"`. |
| **Operator** | `POST /revoke-holds/<cell>`, reason `"operator"`. The cell stays resident. |

Revocation is a cell event: the node runs each handle's `onrevoked` handler
inside the object, and waits for it. When the handlers settle, the holds are
released and the drain, swap or reload continues. Writes the handler makes are
ordinary writes and are covered by the durability proof every eviction takes
before the runtime stops, so `onrevoked` is the place to persist what must
survive.

A hold can **never block these events for good**. The core arms a grace timer
when it revokes: `CELLD_HOLD_GRACE_MS` (default 5000). When it ends, whatever
holds remain are dropped, whether or not the handler finished. A held cell
therefore delays a drain by at most the grace period. A handler that throws is
logged and counts as finished. While a revocation is under way the object
refuses new holds (`ERR_RESIDENCY_LEAVING`), so the handler cannot extend its
own stay.

The core's drain-pin report (`State::drain_pins`) counts the holds still standing
on each quiescing cell.

## When holds end by themselves

A hold belongs to one runtime. Whatever ends that runtime ends its holds, with
no event needed:

- a crash or reset of the cell (a failed durability proof, a heap-limit kill, an
  `abort()`);
- a fence (the node lost its lease);
- a generation swap's stop, and every other path that leaves the resident
  phase.

Nothing is left behind. The core clears the cell's holds whenever its phase
leaves "resident", and every hold carries the epoch of the runtime that took it,
so a handle from a discarded isolate cannot attach to the cell's successor.
`handle.release()` on such a handle is a harmless no-op.

## Dynamic Workers

A Worker Loader can pass the same declaration to the Worker it loads:

```js
const worker = env.LOADER.get("kernel", () => ({
  compatibilityDate: "2026-01-01",
  mainModule: "kernel.js",
  modules: { "kernel.js": source },
  celld: { residency: true },
}));
```

The option is `celld: { codegen, residency, egressIdentity }`. An unknown key is
an error. A loader **cannot grant more than it holds**: `residency: true` needs
the Durable Object that calls the loader to have declared residency itself (or
to be a loaded Worker that was granted it), and loading throws otherwise. The
host refuses again when a hold is taken, against the deployment's declaration
for the root cell, so a loaded Worker can never hold a cell whose class did not
declare residency.

## Facets

A hold attaches to the root cell, which is the object celld keeps resident. A
facet that runs in its root's isolate is still held through the root: call
`hold()` on the root object. A facet class declared through a Worker Loader with
`celld: { residency: true }` can hold the root cell it runs under. Revocation
reaches handles registered in the root's isolate; for a facet that runs in a
separate loaded isolate the node still drops its hold when the grace period ends,
but its `onrevoked` handler does not run.

## Settings

| variable | default | meaning |
| --- | --- | --- |
| `CELLD_HOLD_GRACE_MS` | 5000 | How long a revoked hold may take to wind down. |
| `CELLD_MAX_HOLDS_PER_CELL` | 16 | Live holds one cell may carry. |
| `CELLD_MAX_RESIDENT_CELLS` | machine-derived | The ceiling that the hold budget is half of. |

## Example

[`examples/residency`](../../examples/residency) holds an in-memory counter
across `CELLD_IDLE_EVICT_S` and shows it evicted after `release()`.
`examples/residency/run.sh` runs that scenario against `celld dev`.

## Differences from Cloudflare

- Cloudflare evicts an idle Durable Object after about 70 to 140 seconds
  regardless of what its memory holds. There is no hold. Code that depends on
  `ctx.residency` does not run on workerd.
- A held object is still an ordinary Durable Object for everything else:
  requests, alarms, hibernatable WebSockets and storage behave as they do
  without a hold. A hold does not make anything durable. The heap is lost with
  the process, and the node's own drain tells the object so (above).
