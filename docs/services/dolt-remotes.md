# Dolt remotes between cells

A [Dolt cell](dolt-cells.md) can name another cell as a **remote**, fetch its
branches, merge them, and push its own branches back. This is how work is
decentralized: participants (agents, a user, devices, other workspaces) each
work on their own fork of one Dolt database, in parallel, and merge back with
`fetch`, `pull` and `push`. No object serializes the work. The shared cell only
moves a branch when a push is a fast-forward.

This is a feature of the celld fork; Cloudflare has no equivalent API.

## Example

[`examples/dolt-remotes`](../../examples/dolt-remotes) runs the whole story:
two agents fork a shared graph, commit in parallel (one row conflicts), and
merge back; the graph and both agents end on the same commit hash. `demo.sh`
runs it under `celld dev` (and ends with a restart and a restore from
replication); `cluster.sh` runs it on two nodes of one fleet, the graph on one
and the agents on the other.

## Declare it

A class declares the capability in the deployment config's `celld` block:

```jsonc
"celld": { "durable_objects": { "Graph": { "doltRemotes": true } } }
```

Without it `ctx.storage.dolt.remote`, `fetch`, `pull` and `push` do not exist
for the class's cells, and a request to *serve* a fetch or accept a push is
answered `403` (`ERR_DOLT_REMOTE_FORBIDDEN`), whatever grants the object holds:
a class that never asked for remotes cannot be fetched from. A **Dolt facet**
has the declaration of the class of its id (its root's class, normally), which
may be a SQLite class. A deployment that declares it requires the
`dolt-remotes-v1` feature.

## Authority: grants

A cell lets others in only through a **grant** it issues itself:

```js
// on the source
const grant = ctx.storage.dolt.remote.grant({
  label: "agent-a",          // for your own bookkeeping
  fetch: true,               // may fetch the branches below
  branches: ["main", "ctx/*"], // what a fetch shows (default ["*"]); `*` is the only wildcard
  push: ["main"],            // branches a push may update (default none)
  expiresAt: Date.now() + 3600_000,  // optional
  peer: "<endpoint id>",     // optional: only a carrier that presents this peer (see below)
});
// { id, token, label, fetch, push, branches, peer, expiresAt }
```

The token is 256 random bits and is shown **once**. The source keeps only its
SHA-256 (`_cf_DOLT_GRANTS`), so a stolen database page does not give the token
back. It is a bearer capability: hand it to the holder over any channel the
application has (the demo passes it in a function call). `remote.grants()`
lists grants (never tokens), and `remote.revoke(id)` ends one at once.

Why a grant and not the class, a header or a list in the config: every fork is
of the same class, so "same class" would let any fork read its source and every
sibling; a header naming the caller cannot be believed, because any code that
holds a stub can write any header; and a list in the config cannot name cells
made at run time. A grant is the source's own decision about one holder.

What a holder can never see: a branch no pattern of its grant names (a fetch
for it by name is `ERR_DOLT_REMOTE_NOT_FOUND`), uncommitted data (the pack is
built from commits only), `_cf_*` tables, tags and other remotes.

## API

All on `ctx.storage.dolt`, on a cell whose class declared `doltRemotes`.

| call | what it does |
| --- | --- |
| `remote.add(name, { id, token })` | names the cell `id` (a `DurableObjectId`, or its `"Class:<id>"` string) as the remote `name` (a letter, then up to 63 letters, digits, `_`, `-`). The holder stores the token in its own `_cf_*` table, never in a commit |
| `remote.remove(name)` | forgets the remote and its remote-tracking branches |
| `remote.list()` | `[{ name, ref }]` (never a token) |
| `remote.branches()` | `[{ name: "origin/main", hash }]`, the remote-tracking branches |
| `remote.grant(options)`, `remote.grants()`, `remote.revoke(id)` | the source's side, above |
| `remote.serve(bytes, { peer? })` | answers one request (below) with one response, for a carrier of your own |
| `await fetch(name, { branches?, transport? })` | brings the branches (those the grant shows, or those matching `branches`) into `name/<branch>`. Changes no branch of yours and no working data. `{ remote, branches: { branch: hash }, bytes }` |
| `await pull(name, branch, { message?, author?, date?, resolve?, fastForward?, transport? })` | `fetch(name, { branches: [branch] })` and then `merge("name/branch", ...)`: with `resolve`, [merge with a resolver](dolt-cells.md#merge-with-a-resolver) in the merge's own transaction. The merge's result plus `fetched` |
| `await push(name, branch, { to?, transport? })` | asks the remote to move `to` (default the same name) to your **committed** `branch`. `{ branch, hash, created, unchanged }` |

The remote-tracking branches are ordinary refs. `merge("origin/main")`,
`diff({ from: "origin/main" })`, `hashOf("origin/main")` and
`dolt_at_<table>('origin/main')` read them, and nothing else does anything
special with them.

```js
// the agent: work, then bring the shared graph's changes, resolve, send back
dolt.commit({ message: "agent work" });
try {
  await dolt.push("origin", "main");
} catch (error) {
  if (error.code !== "ERR_DOLT_NON_FAST_FORWARD") throw error;
  await dolt.pull("origin", "main", { message: "merge main", resolve: (merge) => { /* F1 */ } });
  await dolt.push("origin", "main");     // now a fast-forward
}
```

### Push

A push is **"ask the target to fetch"**. The pusher asks for the target's
branch tips, builds a pack of its branch against them, and sends it with the
token. The *target* installs the pack as remote-tracking branches
`pushed-<grant id>/<branch>` and then moves the branch in **its own turn**:

- only a **fast-forward** (the new tip contains the branch's current tip) is
  accepted, never a force: `ERR_DOLT_NON_FAST_FORWARD`, whose message names the
  current tip; fetch, merge and push again;
- the branch must match a `push` pattern of the grant
  (`ERR_DOLT_REMOTE_FORBIDDEN`), and a branch that does not exist is created;
- if the branch is **checked out** on the target, it moves only when its working
  set has no uncommitted application changes (`ERR_DOLT_TARGET_DIRTY`);
- the pack's claimed tip must be what it carries (`ERR_DOLT_REMOTE_BAD_REQUEST`).

The target's single writer is never bypassed: the pack lands through the
target's own connection, in the target's own event. Many pushers contend only
for the target's turn, which is a hash comparison and a ref move.

### Learning that a push was accepted: `ondoltpush`

A cell that is pushed to (the shared graph of a decentralized workspace) often
has to react to published work. It declares an optional method on its class:

```js
export class Graph {
  // Called in this cell's own turn when a push into it was accepted: the
  // branch has moved (`hashOf(event.branch) === event.to`), and the pusher has
  // not been answered yet.
  async ondoltpush({ remote, grant, branch, from, to, created }) {
    // remote:  "pushed-<grant id>", the tracking remote that holds what was pushed
    // grant:   the id of the grant the pusher used (`remote.grants()` has its label)
    // branch:  the branch that moved; from: its previous tip, or null when created
    // to:      its new tip
    this.ctx.storage.put(`published:${to}`, { branch, from, grant });
  }
}
```

- It runs once for each push that **moved a branch**, after the move and after
  the head and working set follow it when the branch is checked out. A push
  that changed nothing (`unchanged`), a refused one (a non-fast-forward, a
  forbidden branch, a dirty target, a bad pack) and a fetch call nothing.
- It runs in the target's turn and before the pusher is answered, so what it
  writes is covered by the same output gate: the pusher's answer waits until the
  handler's writes are durable too. Keep it short; hand long work to an alarm
  or a queue. An `async` handler is awaited.
- The push is already made. A handler that throws or rejects is logged
  (`ondoltpush failed`) and changes nothing; the pusher still gets its answer.
- It runs the same way whichever carrier delivered the push: the built-in one,
  or a carrier of your own that calls `remote.serve(bytes)` (the handler runs
  inside that call, before `serve` returns; an `async` handler keeps the event
  alive). A **Dolt facet** that its parent relays pushes to with `remote.serve`
  is notified as well, on the facet's own class.
- The pusher is not shown the notification: its `push` result is
  `{ branch, hash, created, unchanged }` as before.
- The class needs the method, not an instance property; the runtime wakes the
  object (constructing it) to call it. There is no event for a cell that is
  only a holder.

## What it guarantees

- **A fetch reads committed state its source cannot lose.** The source answers
  from its own turn, on its own connection, outside any transaction, and the
  response leaves through the source's output gate like any response, which
  waits until every write the source has made is proved durable. The pack holds
  only commits.
- **What lands is replicated like any commit.** A fetch or an accepted push
  installs chunks and refs through the receiver's own connection; celld
  captures them as it captures a commit and ships them to followers and the
  bucket. The operation counts as a write for the output gate. A restore from
  replication brings back the remote-tracking branches, and the moved branch.
- **The transfer is DoltLite's own fetch** over a pack (patch 0004,
  `dolt_pack_build`, `dolt_pack_fetch`): every chunk is hashed against its
  address, the whole commit graph is checked before a tracking ref is installed,
  and a damaged or incomplete pack installs nothing. A pack carries only the
  chunks the receiver's commits do not cover.
- **A fork is a new identity.** `env.NS.fork`, `restore` and `celld cell import`
  clear the remotes, the grants and every remote-tracking branch of the image: a
  fork does not inherit its source's authority. Set the remote up again after
  forking.
- **Every operation is durable once answered.** `grant`, `revoke`, `add`,
  `remove`, a fetch's install, an accepted push, a pull's merge and
  `ondoltpush`'s own writes are writes for the output gate; a node killed right
  after the answer comes back with them (e2e scenario `dolt-remotes-durability`
  kills the node after each and restores the cells from the replication log).
- **The source pays one turn**, bounded by the size of the delta (the first
  fetch of a long history is the whole history). Nothing reads a source's file
  from outside its owner, so nothing can block or corrupt its writer.

## Across nodes, and over P2P

The built-in carrier is an internal request to the source's Durable Object
(`POST https://celld.internal/__celld/dolt-remote`), answered by the runtime
before the object's instance, as `fork` and `restore` are. It goes through the
same dispatch as any stub call, so a source on another node of the fleet is
reached over the inter-node channel and a source that is not active is
activated first; there is no second cross-node path. The remote must be a cell
of a class of the same deployment (`makeNamespace` resolves the class).

The wire is three requests (`refs`, `fetch`, `push`) and a framing that knows
no transport, so any request/response carrier will do:

```js
// holder: any async (bytes) => bytes
await dolt.fetch("origin", { transport: async (request) => {
  const response = await send(request);   // your carrier
  return response;
}});

// source: answer one request
const response = ctx.storage.dolt.remote.serve(request, { peer: authenticatedPeerKey });
```

Over the [P2P binding](p2p.md), the serving object's `onp2pstream` reads one
request from the stream, calls `remote.serve`, and writes the response; the
holder's `transport` opens a stream, writes the request and reads the response.
A grant with `peer` refuses a request whose carrier did not pass that key to
`serve`, so a grant can be tied to a peer's endpoint id. `examples/dolt-remotes`
runs the same shape with a loopback carrier (`transport=loopback`). **Not
built:** the cross-workspace run of this over two real nodes; the protocol and
the hooks are, the glue is the application's.

## Errors

A failure throws with `error.code`:

| code | meaning |
| --- | --- |
| `ERR_DOLT_REMOTE_FORBIDDEN` | no valid grant (unknown, revoked, expired, not for this peer), the grant does not allow the action or branch, or the class did not declare `doltRemotes` |
| `ERR_DOLT_REMOTE_NOT_FOUND` | a branch named in a fetch is not shown by the grant, or the target is not a Dolt cell |
| `ERR_DOLT_NON_FAST_FORWARD` | the push is not a fast-forward; the message names the target's tip |
| `ERR_DOLT_TARGET_DIRTY` | the target's checked-out branch has uncommitted changes |
| `ERR_DOLT_REMOTE_BAD_REQUEST` | a malformed request, or a pack that does not carry what it says |
| `ERR_DOLT_REMOTE_FAILED` | the transfer failed (a damaged pack, an incomplete graph, an I/O error) |

## Limits

- A Dolt **facet** is a holder (`add`, `fetch`, `pull`, `push` to a root cell);
  it is a source or a push target only through its parent, which relays the
  request bytes with `remote.serve` over a facet call: a facet is reached
  through its root's isolate and has no id to route to. The relay is tested end
  to end (`examples/dolt-remotes`: a root cell fetches from a facet and pushes a
  branch to it through the parent, with the grant, the refusals and
  `ondoltpush` on the facet). In a facet loaded
  through a Worker Loader the built-in carrier needs a namespace binding for the
  source's class; pass `transport` otherwise.
- No force push, no tag transfer, no deleting a remote branch.
- A pack is one buffer in memory (2 GiB at most), and the source's turn is
  occupied while it builds one.
- `fork` does not set up remotes. The source must grant and the holder must
  `remote.add` after the fork.
- The remote-tracking refs `pushed-<grant id>/<branch>` that accepted pushes
  leave are bookkeeping (the last push of each grant); `revoke` removes them.
