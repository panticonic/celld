# V8 inspector

celld can serve the V8 inspector (the Chrome DevTools Protocol) for every
isolate it runs. Chrome DevTools, `node --inspect`-style clients, VS Code and
scripted clients (profilers, heap-snapshot tools) attach to an isolate and
evaluate code, set breakpoints, take CPU profiles and take heap snapshots.

The endpoint has the shape of workerd's `--inspector-addr`, so a relay written
for workerd (such as Vibestudio's `workerdInspectorBridge`) works unchanged.

It is off by default.

## Turning it on

```sh
celld --bucket s3://NAME --inspector-addr 127.0.0.1:9229
celld dev --inspector-addr 127.0.0.1:9229
CELLD_INSPECTOR_ADDR=127.0.0.1:9229 celld dev
```

`--inspector-addr HOST:PORT` (or `CELLD_INSPECTOR_ADDR`) takes an IP literal or
`localhost`. Port 0 picks a free port; the node logs the bound address
(`inspector_listening`).

### Security

**An inspector connection is full code execution.** It runs arbitrary
JavaScript in an isolate, and an isolate hosts many cells. Through
`Runtime.evaluate` a client reads and writes every cell's storage, calls every
binding and secret in the Worker's `env`, and can break the deployment's
confinement (for example it can `eval` in a class that did not declare
`codegen`). The inspector has no authentication.

- The listener accepts only a loopback address. A non-loopback address is
  refused at startup unless `--inspector-allow-non-loopback` (or
  `CELLD_INSPECTOR_ALLOW_NON_LOOPBACK=1`) is also given. Do that only on a
  network you control, and prefer an SSH or authenticated tunnel to a loopback
  listener.
- A request is refused with 403 unless its `Host` header is an IP literal or
  `localhost` (Node's defence against DNS rebinding from a browser).
- Only GET and WebSocket upgrades are served. No `/json/new`, `/json/close`
  or `/json/activate`.
- It is a per-node tool. In a fleet, each node serves its own isolates only.
- `celld dev` forwards `--inspector-addr` and `--inspector-allow-non-loopback`
  to its node.

## Endpoints

| Request | Answer |
| --- | --- |
| `GET /json/version` | `{"Browser": "celld/<version>", "Protocol-Version": "1.3", "V8-Version": ...}` |
| `GET /json/list`, `GET /json` | one entry per live isolate |
| `GET /<target-id>` with a WebSocket upgrade | a CDP session on that isolate |

An entry of `/json/list` is:

```json
{
  "id": "core:user:my-app:isolate-3",
  "title": "my-app (celld isolate; classes Counter; shared by 2 cells: Counter:a, Counter:b)",
  "type": "node",
  "description": "celld V8 isolate. An isolate runs every cell placed in it ...",
  "url": "file:///my-app",
  "webSocketDebuggerUrl": "ws://127.0.0.1:9229/core:user:my-app:isolate-3",
  "devtoolsFrontendUrl": "devtools://devtools/bundled/js_app.html?experiments=true&v8only=true&ws=127.0.0.1:9229/core:user:my-app:isolate-3"
}
```

The `Host` the request used is what `webSocketDebuggerUrl` carries.

## Targets

There is one target per live V8 isolate, and no others.

- `core:user:<script>:isolate-<n>`: an isolate of a deployment's pool. It
  hosts the Durable Object cells placed in it, or serves the script's
  stateless requests, or both.
- `core:dynamic:<script>:isolate-<n>`: the isolate of one dynamic Worker (a
  Worker Loader's `LOADER.get(...)`). It hosts exactly that Worker.

Targets appear when an isolate is created and disappear when it is retired.
Pools build isolates lazily, so a node that has served nothing lists nothing,
and a cell's isolate is created when the cell is first placed.

**Isolates are shared across cells.** celld places many cells in one isolate
(one heap, one realm); the Worker's module is compiled once and shared. A
target is therefore an isolate, not a cell, and the title says who is in it:
`shared by N cells: ...` (first six, then `+M more`) or `no cells now, serves
stateless requests`. Whatever you do to the target (a breakpoint, a pause, a
profile, `Runtime.evaluate`) applies to all of them. A paused isolate is
marked ` [paused]` in its title. Cells move between isolates over time, so
re-read `/json/list` before attaching.

## What works

Every CDP domain V8 implements itself works, because celld hands the protocol
to V8's own `V8Inspector`. The ones Vibestudio's profiling and a debugger need:

| Domain | Methods |
| --- | --- |
| `Runtime` | `enable`, `evaluate`, `callFunctionOn`, `getProperties`, `runIfWaitingForDebugger`, ... |
| `Debugger` | `enable`, `setBreakpoint`, `setBreakpointByUrl`, `pause`, `resume`, `stepOver/Into/Out`, `evaluateOnCallFrame`, `getScriptSource`, ... |
| `Profiler` | `enable`, `start`, `stop` (a CPU profile) |
| `HeapProfiler` | `enable`, `takeHeapSnapshot` (streams `addHeapSnapshotChunk`), `startSampling` / `stopSampling`, `collectGarbage` |

`Runtime.runIfWaitingForDebugger` is accepted and does nothing: celld never
holds an isolate back to wait for a debugger. Attach, then trigger the code.

`Network`, `Page`, `DOM` and `Target` are not V8 domains and are not served.

Things to know:

- **Script names.** The Worker bundle's scripts are reported with the names the
  bundler gave them; a source map in the bundle works in DevTools as usual.
- **`console`.** celld installs its own `console` that writes to the node log,
  so `Runtime.consoleAPICalled` is not emitted for a Worker's `console.log`.
  Use the node log.
- **Evaluation runs outside a request.** `Runtime.evaluate` runs in the
  isolate's realm on a turn of its own, with no request or Durable Object event
  active. It can read globals, bundle state and anything reachable from them
  (`Debugger.evaluateOnCallFrame` in a paused request frame reaches that
  request's variables), but an API that needs an active event (for example
  `fetch` from a Durable Object's storage turn) may refuse. An evaluation that
  loops forever is not stopped by the CPU limit.

## Threading, and pausing

celld runs isolates on Tokio worker threads; an isolate belongs to no thread
and each turn locks it on whichever worker holds the isolate's turn permit.
The inspector follows that:

- A protocol message queues for the isolate and schedules one **turn** on it.
  That turn takes the same permit any request would, so a message waits its
  turn behind requests already running in that isolate, and the isolate sees
  the debugger as one more turn. Other isolates are unaffected.
- **Pausing.** A breakpoint stops JavaScript inside a running turn. V8 asks the
  embedder for a nested message loop, and celld runs it on the worker that is
  executing the turn. That worker blocks, and the turn keeps the isolate's
  permit, so **the whole isolate is frozen, with every cell in it** (their
  requests queue behind the permit) until the debugger resumes. This is the
  same as stopping a shared process under a debugger, and is why a target's
  title lists its cells.
  - Other isolates keep serving. Their cells, stateless requests and dynamic
    Workers take their own permits; the inspector's own HTTP and WebSocket
    handling runs as ordinary tasks, so `/json/list`, other sessions and other
    isolates' debuggers work while one isolate is paused.
  - The pause loop dispatches this isolate's inspector messages itself, so
    `Debugger.resume`, `stepOver`, `evaluateOnCallFrame` and `Runtime.evaluate`
    reach a paused isolate.
  - **At most workers - 1 isolates may be paused at once**, where workers is the
    Tokio worker count (`CELLD_TOKIO_THREADS`, default: the CPU count). One
    worker must stay free to run the listener and socket tasks, or a paused
    debugger could not be reached. A pause beyond that is *declined*: celld
    logs `inspector_pause_declined` and execution continues as if the debugger
    had resumed. With one Tokio worker no pause is possible.
  - **If the last debugger session on a paused isolate disconnects, the pause
    ends** and the isolate resumes, so a client that crashed cannot wedge it.
  - `Debugger.pause` on an idle isolate takes effect when the isolate next runs
    JavaScript, since there is no thread to interrupt.
- **Wall-clock deadlines keep running while paused.** The paused turn
  stays inside one request, and celld's timers for it do not know about the
  debugger: the 300 s handler budget, the 15 s operation deadline and an
  outbound request's timeout can fire and cancel the paused request. `celld
  dev` disarms them (`CELLD_NO_IMPLICIT_DEADLINES`); on a node, pass
  `--no-implicit-deadlines` when debugging. CPU limits count thread CPU time
  and do not advance while paused.
- A **heap snapshot** holds the isolate's lock for as long as V8 takes to walk
  the heap (seconds for a heap near the 128 MiB default limit), so that
  isolate serves nothing meanwhile. The same holds for `Profiler.stop` on a long
  profile, but briefly.

### CPU profiles and thread migration

V8 samples an isolate by signalling the thread that started the profile. celld
may run consecutive turns of one isolate on different Tokio workers, and a
sample taken while the isolate is on another thread is lost. A profile of a busy
isolate on a many-worker node can therefore have fewer samples than the
isolate's real CPU use, and shows the work in proportion rather than in
absolute time. For a complete profile set `CELLD_TOKIO_THREADS=1` (with
`celld dev`, in the environment) so the isolate never migrates; heap snapshots
are unaffected.

## Vibestudio

Vibestudio's `WorkerdInspectorBridge` lists `/json/list`, takes each row's `id`,
`title` and `type`, derives the path from `webSocketDebuggerUrl`, and proxies a
WebSocket to `<base>/<path>`. That works as is with `--inspector-addr` in place
of workerd's: ids are URL-path safe, and the relay needs no changes. The
profiling testkit's CPU profile (`Profiler.start`/`stop`) and heap snapshot
(`HeapProfiler.takeHeapSnapshot`) go straight through.

## Testing

`tests/inspector/run.mjs` starts `celld dev --inspector-addr` on
`tests/inspector/fixture`, and with Node 22's built-in WebSocket checks
discovery and the Host check, `Runtime.evaluate`, a CPU profile, a heap
snapshot, a breakpoint (with `evaluateOnCallFrame`, and with the dynamic Worker
and `/json/list` answering while the isolate is paused), a client that
disconnects while paused, and a second session afterwards.

```sh
CELLD_BIN=target/release/celld node tests/inspector/run.mjs
```

Unit tests in `crates/celld/inspector.rs` cover the address policy, the Host
check and target naming.

## Limits

- One `V8Inspector` per isolate costs a little memory on every isolate, so
  the node creates them only when the inspector is configured.
- A session cannot be attached to a cell. A cell has no isolate of its own.
- Python Workers (Pyodide) run in a V8 isolate and are targets too, but their
  Python frames are not debuggable through CDP.
