# Egress proxy

A node can send every outbound request of every isolate through one proxy that
decides what is allowed. celld attaches the identity of the caller to each
request and authenticates it, so the proxy can admit, log, rewrite or refuse
traffic per deployment, class, cell or tenant. It is an outbound policy hook:
celld does not know what the proxy allows, and the proxy does not need to trust
the network between them.

Without the setting, nothing changes: isolates dial their destinations
themselves, as before.

## Example

The [egress example](../../examples/egress) runs a Durable Object, a plain
Worker and a dynamic worker against a small proxy that checks and records
identities. `node test.mjs --celld PATH` starts `celld dev` three times, with
the proxy, with a wrong secret, and without a proxy, and asserts what arrives.

## Configuration

| Flag | Environment | Meaning |
| --- | --- | --- |
| `--egress-proxy URL` | `CELLD_EGRESS_PROXY` | `http://` or `https://` proxy. Setting it routes all outbound traffic through it |
| `--egress-secret-file PATH` | `CELLD_EGRESS_SECRET_FILE` | File holding the HMAC secret |
| `--egress-secret VALUE` | `CELLD_EGRESS_SECRET` | The secret itself. Prefer the file: a command-line argument is visible to other users of the machine |

The secret must be at least 16 bytes; a trailing newline is ignored. A proxy
without a secret, a secret without a proxy, both a secret and a secret file,
or a short secret stops startup. A flag replaces the environment variable of
the same kind. `celld dev` passes its environment to its node, so the
environment variables are how you point a dev node at a proxy.

The proxy URL carries no credentials; the secret is the credential. celld
verifies an `https://` proxy against the Mozilla root set.

## What goes through the proxy

All of it, from every isolate: deployed Workers, Durable Objects and their
facets, and dynamic workers.

| Traffic | Sent to the proxy as |
| --- | --- |
| `fetch()` | `METHOD <absolute-url> HTTP/1.1`, in plaintext, for `http://` and `https://` destinations alike. The proxy performs the upstream request and its TLS. This is the forward-proxy absolute form, so the proxy can see and rewrite the request |
| outbound WebSocket (`new WebSocket()`, or `fetch()` with `Upgrade: websocket`) | `GET ws://host/path HTTP/1.1` (or `wss://`) with the upgrade headers, in absolute form. `http(s)` URLs are sent as their `ws(s)` twins |
| `connect()` | `CONNECT host:port HTTP/1.1`. With `secure: true` or `startTls()`, TLS runs through the tunnel, end to end, and the proxy sees ciphertext |

Redirects are followed by celld, one hop at a time, and every hop is a new
request to the proxy with its own authentication. A redirect therefore never
reaches a destination the proxy did not see. celld does not resolve a
destination's name or connect to its address: the only name it resolves is the
proxy's, so DNS cannot be used to bypass the proxy. If the proxy is
unreachable the request fails; it is never sent directly.

A `fetch()` to a `globalOutbound` Fetcher, to a service binding, to a Durable
Object, to R2, KV, D1 or a queue, and a container port are not egress and do
not go through the proxy. A Worker that a Fetcher reaches is itself an isolate,
and its own outbound requests go through the proxy under its own identity.

## One decision for all outbound traffic

`fetch`, outbound WebSocket and `connect()` ask one function,
`egress_route` (`crates/celld/js/egress.rs`), whether and how a request may
leave. The isolate's policy comes first:

| Isolate policy | Node proxy | Result |
| --- | --- | --- |
| `Deny` (a dynamic worker with `globalOutbound: null`) | any | all three refused: "no network". Nothing reaches the proxy |
| `Broker` (a dynamic worker with a Fetcher `globalOutbound`) | any | `fetch()` goes to the Fetcher; a WebSocket and `connect()` throw, because the service protocol has no tunnel |
| `Allow` (every other isolate) | not set | the op dials the destination itself |
| `Allow` | set | the op sends the request to the proxy with the caller's identity |

A dynamic worker that omits `globalOutbound` inherits its loader's policy, so
under a configured proxy it uses the proxy.

## Caller identity

Three headers on every request to the proxy:

- `x-celld-egress-identity`: base64url, without padding, of this JSON:

  ```json
  {
    "v": 1,
    "deployment": "<deployment version string>",
    "script": "<deployed script name>",
    "loaded": false,
    "class": "Counter",
    "cell": "Counter:1a2b...",
    "root": null,
    "egressIdentity": []
  }
  ```

  - `deployment` is the fleet-wide version string of the deployment that holds
    the isolate.
  - `script` is the deployed script. For a dynamic worker it is the deployed
    script at the root of its loader chain, never the loader-minted
    `__loader:N` name.
  - `class` and `cell` come from the running event: the Durable Object class
    and the cell scope whose event made the request. They are `null` in a
    Worker's `fetch` handler and in a dynamic worker. For a facet, `cell` is the
    facet's scope and `root` is the cell that owns it.
  - `loaded` is true for a dynamic worker. `egressIdentity` holds the
    `celld: { egressIdentity }` strings of its loaders, outermost first.
- `x-celld-egress-nonce`: base64url of 16 fresh random bytes.
- `x-celld-egress-mac`: base64url of the HMAC-SHA256, keyed with the secret, of

  ```
  "celld-egress/v1\n" kind "\n" method "\n" target "\n" identity "\n" nonce "\n"
  ```

  where `kind` is `fetch`, `websocket` or `connect`; `method` is the method on
  the request line (`CONNECT` for a tunnel, `GET` for a WebSocket); `target` is
  the request-line target exactly as sent (the absolute URL, or `host:port`);
  and `identity` and `nonce` are the two header values verbatim.

The MAC is bound to the request and has no timestamp. There is no expiry and no
clock to keep in sync. A copy of the headers cannot be attached to another
destination, method or kind of traffic. It can be replayed for the same
request, as any request-bound token can; a proxy that wants to prevent that can
remember nonces. The MAC does not cover the request body. Hash the identity
header's bytes as received. Do not parse and re-serialize them.

### Who can set the identity

Only the node and the loader of a dynamic worker. celld reads the identity from
the isolate and from the event that is running, never from an argument of the
call. A Worker that sends `x-celld-egress-*` headers of its own has them
removed. The code in a dynamic worker cannot name itself: its
`egressIdentity` is the `celld: { egressIdentity: "..." }` field of the
`WorkerCode` that its loader returned:

```js
env.LOADER.get(id, () => ({
  compatibilityDate: "2025-01-01",
  mainModule: "main.js",
  modules: { "main.js": code },
  celld: { egressIdentity: "tenant-42" },
}));
```

The value is an opaque non-empty string of at most 512 bytes; the load fails
for anything else. A loaded worker that sets none keeps the chain of its
loader. A dynamic worker has no Worker Loader binding today, but the rule is
in place for when one can load another: the child's chain is its loader's plus
its own value, so a loaded worker can add to the identity it was given and
never remove or replace it.

## For Vibestudio: what `egressProxy.ts` must check

Today the proxy trusts `X-Vibestudio-Egress-Caller` when
`X-Vibestudio-Egress-Secret` equals a shared secret, a header that
`EgressGateway` stamps in workerd. With celld the node stamps identity itself,
and `startShared` has to verify the MAC scheme instead. In
`resolveAttributedCaller` (`src/server/services/egressProxy.ts`):

1. Read `x-celld-egress-identity`, `x-celld-egress-nonce` and
   `x-celld-egress-mac`. A request with any of them missing is unattributed
   (403, as now).
2. Pick `kind` from the entry point: `request` is `fetch`, `upgrade` is
   `websocket`, `connect` is `connect`. Take `target` from `req.url` exactly as
   Node received it. Both `request` and `upgrade` receive the absolute URL;
   `connect` receives `host:port`. Do not use the `Host` header, the parsed
   `URL` or the `__vibestudio_ws_headers` rewrite for the MAC input.
3. Compute `HMAC-SHA256(secret, "celld-egress/v1\n" + kind + "\n" + req.method +
   "\n" + target + "\n" + identity + "\n" + nonce + "\n")` over the header
   strings as received, and compare with `crypto.timingSafeEqual` against
   `base64url`-decoded `mac`. Compare lengths first.
4. Only after the MAC verifies, decode `identity` and map it to a
   `VerifiedCaller`: `callerResolver` takes the identity object in place of the
   `callerId` string. Key on `deployment` and `script` plus `class` and `cell`
   for deployed code, and on `egressIdentity` for dynamic workers
   (`loaded: true`). For a dynamic worker, `egressIdentity[0]` is what the
   deployed code that loaded it chose, for example the runtime id that
   `EgressGateway` carried as `props.id`. Treat unknown identities as 403.
   Do not trust any field before the MAC has verified.
5. Add `x-celld-egress-identity`, `x-celld-egress-nonce` and `x-celld-egress-mac`
   to `INTERNAL_EGRESS_HEADERS`, so they are stripped before forwarding
   upstream. celld already drops these names from Worker input and replaces
   them, so a Worker cannot smuggle a second set.
6. Keep the rest: `handleHttpRequest` already accepts the absolute form with an
   `https://` scheme (`resolveTargetUrl`), `handleWebSocketUpgrade` accepts
   `ws://` and `wss://` absolute targets (`websocketPolicyUrlFor`), and
   `handleConnect` accepts `host:port` (`parseConnectAuthority`). celld sends
   plain-text absolute-form requests for `https://` destinations and the proxy
   performs the upstream TLS, so credential injection keeps working. `connect()`
   with `secure: true` is end-to-end TLS inside the tunnel, so there is nothing
   to inject.
7. celld follows redirects itself, so `forwardProxyFetch`'s own redirect loop
   is not involved: each hop arrives as its own request. A WebSocket upgrade
   that celld sends carries the handshake headers the Worker set in the upgrade
   request. The `__vibestudio_ws_headers` query parameter is not used.
8. A proxy that wants "no network" for an internal Durable Object does not need
   a port per class. The node refuses `globalOutbound: null` before anything
   leaves; the proxy only has to refuse the identities it does not know.

The secret is per node: set `CELLD_EGRESS_SECRET_FILE` (or the equivalent
flag) to the value `startShared(secret)` already receives.

## Limits

- The proxy gets no identity for traffic that is not `fetch`, outbound
  WebSocket or `connect()`.
- A `fetch()` through the proxy applies `CELLD_FETCH_TIMEOUT_S` to the wait for
  the response head, not to the whole body, which the reader bounds.
- A `307` or `308` redirect of a request whose streamed body already left is
  returned to the Worker rather than followed, as the body cannot be replayed.
- Each `fetch()` hop opens a connection to the proxy; celld does not pool
  them. A proxy on the same machine makes this cheap.
- The proxy is trusted. Whatever it allows leaves the node.
