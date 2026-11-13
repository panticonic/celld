# celld end-to-end suite

Upstream celld ships no tests, so this suite is the first line of defence for
the fork: it runs `celld dev` against **every project in `examples/`** and
asserts behaviour through the HTTP/WebSocket surface, including restarts,
`SIGKILL` crashes, and restores from the replication log after deleting the
local runtime state.

It is a black box. It needs a built `celld` binary and nothing else of the Rust
workspace, so the same scenarios run unchanged against upstream, the fork, or
any later build (point `CELLD_BIN` at each and compare).

## Run

```sh
CELLD_BIN=/path/to/celld tests/e2e/run              # everything (about 80 s with 8 jobs, 160 s with 4)
tests/e2e/run counter kv 'ws*'                       # by scenario id or glob
tests/e2e/run --no-slow                              # skip cron/queues/sandbox (~1 minute)
tests/e2e/run --list                                 # scenarios, tags, requirements
tests/e2e/run --selftest                             # the harness's own tests
tests/e2e/run --json results.json --jobs 8 --keep
```

Requirements: Node 22 (no npm dependencies) and `esbuild`. The runner takes
esbuild from `CELLD_ESBUILD`, then `PATH`, then a globally installed
`wrangler`/`esbuild` (`npm root -g`). `CELLD_BIN` defaults to
`target/release/celld`.

| Variable | Meaning |
| --- | --- |
| `CELLD_BIN` | binary under test |
| `CELLD_ESBUILD` | esbuild executable |
| `CELLD_E2E_TMP` | scratch root (default `~/.cache/celld-e2e`); must be disk-backed, tmpfs is refused |
| `CELLD_E2E_ARTIFACTS` | where failing scenarios save the celld log (default `tests/e2e/.artifacts/`) |
| `CELLD_E2E_BOOTSTRAP=0` | never download toolchains; scenarios that need one skip instead |

Exit status is 1 if a scenario fails, a `knownFail` step unexpectedly passes,
or an example has no scenario. Skips and known failures do not fail the run.

### State, ports, cleanup

Each scenario copies its example into `<scratch>/run-*/<scenario>/`, picks a free
loopback port, and starts `celld dev --no-watch --logs` there (so state is
`.celld/dev` in that copy). Scenarios run in parallel (`--jobs`, default half the CPUs up to 8)
and never share a directory or port. Every `celld` process is killed and the
directory deleted in a `finally`, on `SIGINT`/`SIGTERM`, and by an `exit`
handler (containers of an aborted run carry the label `celld.node`: `docker rm -f $(docker ps -aq --filter label=celld.node)`). On failure the full celld log is kept in `tests/e2e/.artifacts/<id>/`.

Toolchains for the Python, Wasm, container and sandbox examples are cached in
`<scratch>/tools` and survive between runs (see below); they are never installed
into `~`, `~/.cargo` or global npm.

## Readiness and timing

The runner never sleeps to make something correct. A node is ready when it
prints its `ready  http://...` line. Everything else waits on an observable
condition with an explicit bound:

* `poll` on a request step, `t.poll(fn, {timeoutMs, why})`: retry until an
  expectation holds. `why` names the inherent celld latency.
* `waitLog(re, {timeoutMs, why})`: wait for a celld log line.

Where time is inherent, the scenario says so: alarms fire 2 s after arming; a
queue batch is released when full or after `max_batch_timeout` (5 s in the
example, so its retry and dead-letter path takes ~25 s); `* * * * *` cron fires
on the next minute boundary (up to 60 s, twice, because the test also checks
that ticks continue after a restart). Those are tagged `slow` so `--no-slow`
skips them. There are two deliberate *negative* windows (an alarm must not fire
twice; settled queue messages must not be redelivered after a restart); each
is a fixed bounded `sleep` justified next to it, because "nothing happens"
cannot be observed.

## Scenarios

`scenarios/*.mjs` each export a scenario object or an array of them. A scenario
is data plus a list of steps:

```js
import { get, post, restart, crash, restore, run, waitLog, M } from "../lib/dsl.mjs";

export default {
  example: "counter",            // directory under examples/
  id: "counter",                 // optional; defaults to `example`, needed for variants
  title: "what it checks",
  // fixture: "name",            // overlay tests/e2e/fixtures/<name>/ on the example (e.g. a probe consumer)
  // env: { CELLD_IDLE_EVICT_S: "2" },   // extra environment for celld
  // requires: ["docker"],       // skip when absent
  // setup: async (t) => {},     // before first start: build, npm install, ... (t.skip("why"))
  // slow: true, skip: "reason", allowPanic: false,
  steps: [
    get("/increment?name=a", { status: 200, json: { n: 1, url: "{origin}/increment?name=a" } }),
    post("/x", 201, { json: { k: "v" } }),   // opts: headers, body, json, timeoutMs, name
    restart(),   // SIGTERM, start again on the same state
    crash(),     // SIGKILL, start again: acknowledged writes must survive
    restore(),   // stop, delete .celld/dev/runtime, start: state must come back from the
                 // replication log, and celld must log a `restore_plan` for it
    get("/x", 200, { poll: { timeoutMs: 15000, why: "alarm fires 2 s after arming" } }),
    waitLog(/cell_console: .../, { timeoutMs: 10000, why: "..." }),
    run("custom code", async (t) => { /* t.fetch, t.ws, t.poll, t.expect, t.logLines ... */ }),
  ],
};
```

Expectations: `expect` is a status number or `{status, headers, text, json, bytes}`.
Objects are compared **exactly** (extra and missing keys fail). Volatile values
use matchers from `M`: `M.any`, `M.type`, `M.re`, `M.isoDate`, `M.epochMs`,
`M.near`, `M.between`, `M.oneOf`, `M.absent`, `M.partial({...})`, and
`M.capture("name", inner)`, which stores the value for `{$name}` in later
requests and expectations (a lone `"{$name}"` keeps its type).

Normalisation: this run's origin is rewritten before comparing, so expected
values say `{origin}` (`http://127.0.0.1:PORT`), `{host}` (`127.0.0.1:PORT`) or
`{wsorigin}` and never contain a port.

`knownFail: "reason"` on a step marks a genuine celld deviation. The step must
fail (reported `XFAIL`); if it starts passing the run fails (`XPASS`) so the
marker gets removed. Put the evidence in a comment next to the step.

To add a scenario for a new example: create `examples/<name>`, add
`scenarios/<name>.mjs`, run `tests/e2e/run <name>`. `tests/e2e/run` fails if
any directory in `examples/` has no scenario. Prefer asserting on responses;
use the celld log (`t.logLines`, `waitLog`) only for effects with no HTTP
surface (cron, queue consumers, tails). If the example cannot show a behaviour
you need to test, add an overlay under `fixtures/` (see `queues-probe`) rather
than editing the example.

## What each example checks

Every stateful scenario ends with the sequence graceful restart, `SIGKILL`,
restore-from-log, with the counters/data continuing across each.

| Scenario | Checks |
| --- | --- |
| hello, webapi, body, router, async | fetch handler; URL/TextEncoder/btoa/Headers; JSON, 1 MB and invalid bodies; Worker to DO routing; concurrent requests are serialised |
| counter, rpc, facets, vectordb | DO storage per name; RPC methods; facet with own SQLite and parameterised service binding; sqlite-vec `vec0` shadow tables across LTX capture and restore |
| kv, r2, d1 | binding semantics (overwrite, delete, 1 MB and binary values, content-type, SQL binding and constraint errors) and durability |
| alarm | arm, `getAlarm`, fire once; an alarm pending at SIGKILL / at local-state loss still fires |
| cron | minute-aligned ticks, continue after restart, never duplicated |
| queues | the example's retry to dead-letter path (log) and `queues-probe`: batch size limit, `delaySeconds`, text/json/bytes/v8 bodies, redelivery with the same id, DLQ, delayed message surviving SIGKILL and restore |
| workflow | durable step with output, survives restart/crash/restore without re-running the step |
| wsecho, wsclient | hibernatable sockets (state survives, idle eviction with an open socket and wake on message), outbound WebSocket to a local server, close code/reason |
| static-assets | assets, `_headers`, `_redirects`, etag/304, HEAD, 404 |
| dynamic-worker-tails | Worker Loader code and its Tail Worker with props |
| python | Pyodide Worker with vendored BeautifulSoup and KV |
| wasm | workers-rs DO compiled to Wasm |
| container, sandbox | Docker containers per DO, load balancing, cleanup at shutdown; Sandbox SDK exec/files/processes |
| pi, opencode | skipped: need a live LLM endpoint (non-deterministic) |

## Toolchains (bootstrapped into `<scratch>/tools`)

* **python**: needs `uv` >= 0.13.4 for `pywrangler sync`; a newer uv is
  `pip install`ed into a private venv if the system one is older. The first
  build also downloads the Pyodide runtime to `~/.cache/celld` (done by celld).
* **wasm**: a private rustup + `wasm32-unknown-unknown` and `worker-build@^0.8`
  (the example pins `worker` 0.8.x, which `worker-build` 0.9 refuses). The first
  run compiles for several minutes; the result is cached by source hash.
* **container, sandbox**: need a reachable Docker/Podman daemon. `sandbox` pulls
  `cloudflare/sandbox` (about 225 MB) once. Containers are labelled with the
  celld node id and removed in cleanup even if celld dies.

## Files

```
run, run.mjs        entry point (run is a shell shim)
lib/celld.mjs       one `celld dev` process: start/stop/kill, ready detection, log buffer
lib/runner.mjs      scenario execution, step semantics, restore verification, cleanup
lib/match.mjs       matchers, normalisation, capture/substitution
lib/dsl.mjs         step constructors used by scenarios
lib/ws.mjs          WebSocket client wrapper and a minimal WebSocket server
lib/tools.mjs       Docker/uv/Rust/npm bootstrap and caches
lib/selftest.mjs    --selftest
scenarios/          the scenarios
fixtures/           overlays for scenarios that need a probe Worker
```
