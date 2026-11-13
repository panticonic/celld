# P2P (Iroh)

A `p2p` binding lets a Worker or Durable Object open byte streams to other
celld nodes (or any [Iroh](https://www.iroh.computer/) endpoint) by public key,
and lets a Durable Object receive the streams that peers open to it. It is a
fork feature with no Cloudflare equivalent; the design is in
[the fork design](../fork/design.md) (§3a and §8, decision 4).

Each node has one Iroh endpoint. It dials by public key, punches through NATs
when it can and falls back to a relay when it cannot. Every connection is QUIC
with TLS 1.3, and the peer's key is verified before any stream is looked at.

## Example

The [p2p example](../../examples/p2p) runs one deployment on two nodes. Each
node dials the other, sends lines, and reads replies from the other node's
Durable Object. It dials from a request, and from an alarm whose stream later
requests keep using. The test in `crates/celld/tests/p2p_two_nodes.rs` runs it.

## Configuration

Declare the binding in the Wrangler config:

```jsonc
{
  "durable_objects": { "bindings": [{ "name": "NODE", "class_name": "Node" }] },
  "migrations": [{ "tag": "v1", "new_sqlite_classes": ["Node"] }],
  "p2p": [
    {
      "binding": "P2P",
      "alpn": "chronolog/1",
      "accept": { "class": "Node", "name": "node" }
    }
  ]
}
```

- `binding` is the name on `env`.
- `alpn` identifies the protocol, 1 to 255 of letters, digits and `- _ . / : +`.
  Two nodes talk only when both use the same ALPN. The ALPN is fixed by this
  config; a caller of `connect()` cannot choose another.
- `accept` is optional. It names the one Durable Object that receives the
  inbound streams of this ALPN: its class (which must be in
  `migrations[].new_sqlite_classes`) and the name its `getByName()` would take.
  Without `accept`, the binding dials out and accepts nothing.

A deployment with a `p2p` binding requires the `p2p-v1` feature, so a node
that predates it refuses the deployment instead of running it without the
binding.

## Node settings

The endpoint starts when a flag names a key file or a loaded deployment
declares a `p2p` binding. Each flag has an environment variable.

| Flag | Variable | Meaning |
| --- | --- | --- |
| `--p2p-key-file PATH` | `CELLD_P2P_KEY_FILE` | The node's identity: 32 raw bytes, created with mode 0600 (and its directory 0700) when missing. A file of another length is an error and is never replaced. Without it the id is generated again on every start, and the node says so. |
| `--p2p-relay MODE` | `CELLD_P2P_RELAY` | `default` (n0's public relays), `disabled` (direct UDP only), or comma-separated relay URLs. |
| `--p2p-discovery LIST` | `CELLD_P2P_DISCOVERY` | `dns` (n0's `iroh.link` DNS and pkarr), `mdns` (the local network), or `none`. Default: `dns` with the default relays, `none` otherwise. |
| `--p2p-bind IP:PORT` | `CELLD_P2P_BIND` | Bind this one UDP address instead of every interface on a random port. |
| | `CELLD_P2P_MAX_STREAMS` | Open streams per node, inbound and outbound together (default 4096). |

The key file is the 32-byte secret Vibestudio's server persists for its own
Iroh endpoint, and the node id is the same lowercase hex (64 characters) that
Vibestudio uses as its `endpointId`, so one identity can serve both.

`celld dev` keeps a key in `.celld/dev/p2p.key`, so a project's node id is
stable across restarts. The node id appears in the startup output:

```
celld p2p node 4f9c…e1 (relay=default, discovery=dns, key=file)
```

and on the internal listener at `GET /p2p`, which also lists the bound
addresses, the accepted ALPNs and the open stream count.

### Relays and discovery

With `--p2p-relay default` the node uses n0's production relays, which is what
Vibestudio's reviewed default set (`use1-1` and `euc1-1`) is drawn from. A
dialer that knows only a node id finds it by n0's DNS discovery. To keep every
packet and every lookup off third-party infrastructure, run your own
[`iroh-relay`](https://github.com/n0-computer/iroh/tree/main/iroh-relay), pass
its URL to `--p2p-relay`, and leave discovery at its default (`none`). Peers
then need the node's relay URL or direct addresses: `connect(nodeId, { relay })`.

With `--p2p-relay disabled --p2p-discovery none` the node is local-only: peers
reach it only by direct address, `connect(nodeId, { addrs: ["10.0.0.5:7777"] })`,
so fix its port with `--p2p-bind`. The tests run this way.

## The binding

```ts
interface P2P {
  readonly nodeId: string;      // this node's public key, 64 lowercase hex characters
  readonly alpn: string;        // from the config
  connect(nodeId: string, options?: {
    addrs?: string[];           // direct "ip:port" addresses to try
    relay?: string;             // the peer's relay URL
  }): Promise<P2PStream>;
}

interface P2PStream {
  readonly readable: ReadableStream<Uint8Array>;
  readonly writable: WritableStream<Uint8Array>;
  readonly remoteNodeId: string;     // authenticated by Iroh
  readonly closed: Promise<void>;
  close(code?: number): Promise<void>;
}
```

`nodeId` throws if the endpoint is not running on this node.

`connect()` opens one bidirectional QUIC stream. The node keeps one QUIC
connection per (peer, ALPN) and opens a stream on it for each call, so
connecting repeatedly to a peer costs one handshake, then one stream each. A
connection that died is dropped and dialed again. Concurrent calls share one
handshake. A dial that cannot reach the peer fails after 30 seconds.

A QUIC stream is announced to the peer when the first bytes arrive. The
accepting Durable Object's `onp2pstream` therefore runs when the dialer first
writes, so a protocol in which the accepting side speaks first needs the dialer
to send a hello first.

`close()` without a code is orderly: bytes already written are delivered, then
the peer's `readable` ends. `close(code)` resets the stream: the peer's reads
fail with `stream reset by peer (code N)` and its writes with
`peer closed the stream (code N)`. Closing the `writable` finishes only the send
half. celld uses codes 1 to 5 (below); applications should use codes of 256 or
more. A peer closing is how the other side learns the stream is over, so
close propagates in both directions. `close()` always returns a promise that
resolves once the stream is over.

### `closed`

`closed` settles when the stream is over, whichever way it ended:

| How it ended | `closed` |
| --- | --- |
| Both directions finished: the peer's end-of-stream was read and the `writable` was closed | resolves |
| `close()` or `close(code)` on this side | resolves (a close you asked for is not an error) |
| The peer reset the stream (`stream reset by peer (code N)`, `peer closed the stream (code N)`) | rejects with that error |
| The connection was lost, or the cell left the node | rejects |

Reading to end-of-stream alone does not settle it: a stream the peer has
finished can still be written to. When both sides have finished, celld releases
the stream, so a protocol that finishes both ways needs no `close()`. A caller
that never looks at `closed` is not charged with an unhandled rejection.

## Accepting streams

A Durable Object named by `accept` implements `onp2pstream`:

```js
export class Node extends DurableObject {
  async onp2pstream(stream) {
    if (!(await this.allowed(stream.remoteNodeId))) {
      return stream.close(403 + 256);   // refuse
    }
    try {
      for await (const chunk of stream.readable) {
        /* … */
      }
    } finally {
      await stream.close();             // done with it
    }
  }
}
```

The peer's key is authenticated by the Iroh handshake and arrives as
`stream.remoteNodeId`; deciding whether that peer may do anything is the
application's job. The handler runs as an ordinary event in the object, so the
object's storage input and output gates apply as they do to `fetch`: its writes
to the stream wait on the output gate. The stream is the object's, not the
handler's: it stays open when the handler returns (see below), so a handler can
return at once and leave the stream to later events, which is how a long session
is held without keeping an event open. A handler that is finished with the
stream closes it. A handler that throws has the stream reset with code 3. A
class without `onp2pstream` resets the stream with code 1.

An inbound stream is delivered as an internal `fetch` whose URL carries a
one-time, unguessable claim token, which the runtime redeems for the stream
before the application sees anything. A request that merely imitates that URL
reaches the application's own `fetch` unchanged, and cannot name or forge a
remote node id.

## Streams, gates and residency

**A stream belongs to the Durable Object that opened or accepted it, not to the
event that happened to do so.** This holds for `connect()` called from any
event of the object (a request, an alarm, a timer, an RPC call) and for the
stream handed to `onp2pstream`. It lives until

- it is closed, by either side (`close()`, a reset, or both directions
  finishing: see `closed` above);
- its connection is lost (the peer's node stopped or the network failed);
- or the object ends: its cell is evicted, handed to another node, stopped
  for a drain, a forced generation swap, a restore or a failed durability
  proof; or its instance is replaced (`ctx.abort()`, a failed
  `blockConcurrencyWhile`), or its facet stops. celld then resets the stream
  with code 5 (`CELL_ENDED`), so the peer's reads fail with
  `stream reset by peer (code 5)`.

What follows from that:

- **Use it from later events.** Keep the stream, or its reader and writer, in
  the object (`this.session = ...`) and read and write it from any later event,
  an alarm or a request. Reads and writes are not tied to the event that made
  the stream. Only one reader and one writer exist per stream, as for any Web
  stream, so a design in which several events take turns keeps one `Lines`-style
  wrapper around them (the example does).
- **An open stream keeps its cell resident, like an outbound WebSocket.** The
  stream is registered with the core as a `WebSocketKind::Outbound` transport of
  the cell. That is the accounting an outbound WebSocket uses and nothing new:
  the cell is not idle-evicted, shed under memory pressure, moved by balancing
  or evicted by an operator while a stream is open (`POST /evict/...` answers
  409), because a live transport cannot move with ownership. A drain or a swap
  ends the stream (code 5) instead of waiting for it.
- **It is budgeted.** One cell may hold `CELLD_MAX_OUTBOUND_WEBSOCKETS` outbound
  transports (default 32) and a node may pin at most a quarter of its resident
  ceiling in cells with outbound transports or residency holds. WebSockets and
  p2p streams draw on the same two budgets. At the budget `connect()` rejects
  with `outbound transport (WebSocket or p2p stream) refused: ...`; an inbound
  stream the cell cannot take is reset with code 4 (`BUSY`) and the handler does
  not run. A cell that is leaving the node refuses new streams too. The node's
  own limit, `CELLD_P2P_MAX_STREAMS`, is separate and counts every stream.
- **Close what you are done with.** A stream pins its cell until it ends, and
  `onp2pstream` returning does not end it: the handler may have handed the
  stream to code that keeps using it. A handler that reads to end-of-stream and
  answers should `await stream.close()` when it is finished (a `finally` block
  is the place), and a handler that throws has its stream reset with code 3.
  A peer that finishes and closes its own side does not free ours.
- **Output gate.** `connect()`, `write` and closing the writable wait until the
  storage writes the performing event has made are durable, on the same gate
  channel raw TCP sockets use. A peer never sees bytes that depend on a write
  that could still be lost. The gate is the performing event's: a later request
  writing to a stream waits for that request's writes. A write made from a
  callback whose event has already ended (a timer) has no event to answer for
  it, and waits for everything the cell has committed so far; that holds for
  a Durable Object's own stream and not for one a facet opened.
- **Egress.** `connect()` is outbound network access. It is refused for a worker
  that is denied the network, and for one whose outbound goes through a
  brokering Fetcher, which cannot carry a QUIC connection.

A stream opened by a plain Worker has no cell to own it, and stays with its
request exactly like a `connect()` TCP socket: it is closed when the request
retires. Dial from a Durable Object when the stream must outlive a request.
A Durable Object in a Worker Loader's isolate, which runs several objects, can
use cell-owned streams from the events that frame them, but a callback that
outlives its event cannot tell which object it belongs to, so its dials stay
with the request.

## Fleets

The Iroh identity belongs to a node, not to a cell. Decision: **a `p2p` binding
with `accept` is served only by the node that owns its accepting cell.** A
stream that arrives at another node of the fleet is reset with code 2
(`NOT_OWNER`), and that node logs which node owns the cell. It is not forwarded.

Why not forward over the peer channel: the stream is a live QUIC stream held in
the receiving process, and celld's peer channel forwards HTTP requests and
WebSocket upgrades, not an arbitrary duplex byte stream. Forwarding would also
make every hop a second authentication boundary for the remote node id.

What this means in practice:

- **One node per deployment** (Vibestudio's topology, design §8 decision 2)
  needs nothing: the one node owns the cell and has the one id.
- **A fleet** gives each node its own `--p2p-key-file`. Outbound `connect()`
  works from whichever node runs the cell, and shows the peer *that node's* id.
  Inbound works only against the owning node's id; when the cell fails over to
  another node, peers must redial that node's id. Do not share one key file
  across nodes: two endpoints with one id fight over a single relay slot.
- A **stable id across failover** needs the forwarding mode above and is not
  built. Chronolog on a fleet should address its peers per node for now.

## Errors

| Where | Message or code | Meaning |
| --- | --- | --- |
| `connect()` | `connect to …: …` or `timed out connecting to …` | No route to the peer, or the peer does not accept this ALPN. |
| `connect()` | `a node cannot open a p2p stream to itself` | |
| `connect()` | `the node's p2p stream limit is reached` | `CELLD_P2P_MAX_STREAMS`. |
| peer's stream | reset, code 1 | No deployment on that node accepts the ALPN, or the class lacks `onp2pstream`. |
| peer's stream | reset, code 2 | The accepting cell is owned by another node. |
| peer's stream | reset, code 3 | The handler threw, or the cell could not run it. |
| peer's stream | reset, code 4 | That node is at its stream limit, or the accepting cell is at its transport budget. |
| peer's stream | reset, code 5 | The object that held the stream ended: evicted, drained, swapped, restored, reset (`ctx.abort()`), or its facet stopped. |
| `connect()` | `outbound transport (WebSocket or p2p stream) refused: ...` | The cell is at `CELLD_MAX_OUTBOUND_WEBSOCKETS`, the node's pin budget is spent, or the cell is leaving. |

A stream whose connection dies is released too, and its pending read fails with
`connection lost`, so a cell that held a stream to a vanished peer does not
stay pinned.

A peer that restarts without a goodbye (killed, or the machine lost) leaves
the dialer a pooled connection to a process that no longer exists. The first
stream opened on it fails on its first read or write with `connection lost`, and
the next `connect()` dials afresh. The two-node test measures this: after a
SIGKILL the second attempt succeeds. A peer that stops cleanly (SIGTERM, a
`celld dev` restart) closes its endpoint, and the dialer's first attempt after
the restart succeeds. Applications that talk to peers should retry a stream that
fails with `connection lost`.

## Not covered

- Datagrams, unidirectional streams, and 0-RTT. Application bytes are never sent
  before the full handshake has authenticated the peer.
- Iroh's blobs, gossip, and docs protocols.
- Dynamic workers. The binding is declared in a deployment's config only.
- Windows (the endpoint builds, but this feature is not tested there).
