# Examples

These small Wrangler projects demonstrate the Workers platform that `celld`
supports, one service or API at a time:

- `hello/` — a stateless Worker `fetch` handler
- `webapi/` — common Web Platform APIs
- `static-assets/` — an asset-only website with headers and redirects
- `counter/` — a SQLite-backed Durable Object
- `dolt/` — a Dolt-backed Durable Object: commits, branches, merge, fork,
  commit at the end of a transaction, merge with a resolver, local tables,
  reads of history (fork-only feature; `demo.sh` runs it end to end under
  `celld dev`)
- `dolt-facets/` — Dolt facets of a SQLite supervisor, from a Worker Loader
  class: versioning, `execute`, local tables, facet fork, restart and restore
  (fork-only feature; `demo.sh` runs it end to end under `celld dev`)
- `dolt-remotes/` — Dolt remotes between cells, for work that is decentralized:
  agents fork a shared graph, commit in parallel, and merge back with fetch,
  pull (merge with a resolver) and push, under grants the source issues
  (fork-only feature; `demo.sh` runs it under `celld dev`, `cluster.sh` on two
  nodes of one fleet)
- `vectordb/` — nearest-color search with a per-object `vec0` index
- `d1/` — a guestbook on a D1 database
- `r2/` — object reads, writes, and deletes on an R2 bucket
- `kv/` — key reads, writes, and deletes in a KV namespace
- `queues/` — a producer and a consumer, with retries and a dead-letter queue
- `async/` — a timer and asynchronous Durable Object storage
- `body/` — request and response bodies
- `router/` — Worker-to-Durable-Object routing
- `wsecho/` — WebSocket echo with hibernation
- `wsclient/` — outbound WebSocket client from a Durable Object
- `p2p/` — two nodes exchanging streams over Iroh, dialed by node id
- `alarm/` — a Durable Object alarm handler, including one set for the year 2100
- `secrets/` — a secret that one Durable Object class sees and nothing else does,
  from `celld.durable_objects.<Class>.secrets` (fork-only feature)
- `cron/` — a cron trigger that logs each tick
- `workflow/` — a Workflow that builds a report in one durable step
- `rpc/` — Durable Object method calls through JS RPC
- `dynamic-worker-tails/` — a runtime-loaded Worker with a Tail Worker
- `residency/` — an in-memory counter kept resident by `ctx.residency.hold()`
  past idle eviction, and evicted after `release()`; `run.sh` scripts it
  against `celld dev`
- `egress/` — outbound traffic through a node egress proxy, with a test that
  checks the identity each request carries (`node test.mjs`)
- `facet-abort/` — a request in flight to a facet ends when the facet is aborted,
  and a root handler another request settles answers; driven by
  `tests/e2e/scenarios/compat-facet-abort.mjs`
- `facets/` — a Durable Object facet over a class the Worker Loader starts
- `codegen/` — `eval`, `new Function` and WebAssembly compilation are refused
  everywhere except a Durable Object class that declares `codegen`; no build
  step, and `check.sh` asserts the answers
- `continuations/` — work a promise continuation starts after the event that
  created it has ended: `blockConcurrencyWhile`, storage, transactions, alarms,
  fetch and RPC, all allowed while the object is alive; driven by
  `tests/e2e/scenarios/compat.mjs`
- `deadlines/` — `blockConcurrencyWhile` keeps its 30 s limit by default and
  drops it under `--no-implicit-deadlines`; `check.sh` asserts both
- `container/` — a Durable Object that supervises a container with
  `@cloudflare/containers`; needs `npm install` and a Docker or Podman CLI
- `sandbox/` — the Cloudflare Sandbox SDK: commands, files, and processes
  in a container per sandbox; needs `npm install` and a Docker or Podman CLI
- `pi/` — the PiHarness agent loop in a Durable Object
- `opencode/` — the OpenCode Workerd SDK in a Durable Object
- `python/` — a Python Worker that summarizes HTML with a vendored package;
  needs `uv run pywrangler sync` first (see [Python Workers](../docs/services/workers.md#python-workers))
- `wasm/` — a Durable Object counter in Rust, compiled to Wasm with
  [workers-rs](https://github.com/cloudflare/workers-rs); needs a build step
  first (see its [README](wasm/README.md))

Deploy an example from its directory to the same bucket the nodes use:

```sh
celld deploy . --bucket s3://my-cells-bucket
```

The counter example uses the `name` query parameter as the Durable Object
name. It uses `default` when the parameter is absent. Send requests for
different names to demonstrate independent cells:

```sh
curl 'http://127.0.0.1:8080/increment?name=alpha'
curl 'http://127.0.0.1:8080/increment?name=beta'
curl 'http://127.0.0.1:8080/increment?name=alpha'
```

A fleet can place the named cells on different nodes, and each cell keeps an
independent counter.

They are examples, not the complete compatibility test suite.
