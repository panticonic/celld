# Codegen confinement

Status: implemented on `feature/codegen-deadlines`, 2026-11-13.

workerd blocks `eval` and `new Function`, and Vibestudio relies on that: a
userland Durable Object must not be able to compile code it was not deployed
with. Upstream celld applied no restriction. Every isolate now confines code
generation unless the deployment declared otherwise.

## Rule

An isolate generates code from strings only if it holds the `codegen`
capability. It holds it only when one of these is true:

1. It hosts a Durable Object class the deployment declared, in the wrangler
   config:

   ```jsonc
   "celld": { "durable_objects": { "Evaluator": { "codegen": true } } }
   ```

2. It is a loaded Dynamic Worker whose `WorkerCode` has `celld: { codegen: true }`
   **and** whose loading Worker holds `codegen` itself. A loader cannot grant
   more than it holds: a request from a Worker without it is refused when the
   worker loads, with an error that names the missing declaration. It is not
   silently dropped.

Everything else, including stateless `fetch` handlers, queue consumers, cron,
workflows, D1/KV/Queue broker cells, and any facet whose class runs in a confined
isolate, is confined.

## What "confined" means

V8 is told, per context and before the first script runs, not to allow code
generation from strings (`Context::AllowCodeGenerationFromStrings(false)`).
Then:

| Operation | Confined | With `codegen` |
| --- | --- | --- |
| `eval(string)`, indirect `eval` | `EvalError` | works |
| `new Function(...)`, `AsyncFunction`, `GeneratorFunction` constructors | `EvalError` | works |
| `setTimeout(string)` | `TypeError` (celld's timers take functions only, as in Workers) | `TypeError`, unchanged |
| `new WebAssembly.Module(bytes)`, `WebAssembly.compile`, `WebAssembly.instantiate(bytes)` | `CompileError` | works |
| `new WebAssembly.Instance(module)` for a module the bundle imports | works | works |
| `{ wasm: bytes }` in a Worker Loader `modules` map | works, celld compiles it on the host and the worker imports a compiled module | works |

The WebAssembly decision follows Workers: workerd allows instantiating the
modules a bundle imports, and refuses to compile arbitrary bytes at run time.
V8 ties the second check to the same context flag when no separate WebAssembly
callback is installed, so one switch gives both. docs/wasm.md already says celld
refuses bare bytes in a loader's `modules` map; that is unchanged.

celld's own JavaScript (`harness.js` and the Node compatibility modules) was
searched for `eval`, `new Function` and function-constructor access and uses
none. Its host-side snippets compile with `Script::compile`, which the flag does
not govern. Nothing is exempted.

## Isolates are per pool

A flag set on a context belongs to one isolate, and isolates are pooled and
shared between cells. So the isolate pool key now includes the capability:

- A script with no `codegen` class has one cell pool, as before.
- A script with a `codegen` class has a second cell pool. Its codegen classes
  place cells there. Its other classes place cells in the first pool. The two
  never share an isolate (`WorkerConfig::pool_key`).
- The script's stateless pool is never codegen-enabled.
- A loaded worker has its own isolate, so it carries its own answer.
- A facet runs in the isolate of its root class, or of the Worker the loader
  started for it, and takes that isolate's answer. A confined root cannot get an
  evaluating facet from its own bundle. A codegen root's own-bundle facets can
  evaluate: declare `codegen` only for classes whose whole call graph in that
  isolate you trust.

The census at `/state` lists the second pool under the key
`<script>\0codegen`.

A Worker Loader caches a loaded worker by name, so a worker loaded with and
without `celld.codegen` needs two names.

## Deployment checks

`celld deploy` refuses a `celld` block that:

- is not an object, or names a top-level key celld does not model;
- names a class that is not a Durable Object class of the Worker (a misspelt
  name would otherwise leave the capability off while the author believes it
  on), or a runtime class (`__D1Database` and the like);
- sets a key other than `codegen` or `residency`, or a value that is not a
  boolean.

The block travels verbatim in the manifest's metadata, so changing it is a new
deployment version. A node reads it from the manifest, so a deployment made by
the control plane gets the same rule. `celld deploy` adds no required feature:
a node without confinement runs the class unconfined, which is what the
declaration asks for anyway.

## Compatibility

A Worker written for Cloudflare cannot rely on `eval`, because workerd refuses
it, so upstream deployments are unaffected. Python Workers embed Pyodide
0.28.3, whose `pyodide.asm.js` contains `eval(` call sites (emscripten's
`emscripten_run_script` among them). Cloudflare runs the same interpreter with
evaluation disabled, so the paths a Worker normally takes avoid them, but a
Python Worker was **not** run under confinement for this change: the build needs
`pywrangler` and network access. If one needs evaluation, declare
`celld.durable_objects.<Class>.codegen` for its class. A Python `fetch`
handler outside a class cannot be declared, because the contract declares only
classes.

## Verification

`examples/codegen` probes the rows of the table above (except `setTimeout`, which
is a plain `TypeError` in every isolate) from a stateless handler, a confined
class, a codegen class and loaded workers, and `examples/codegen/check.sh`
asserts the answers through `celld dev`. On the base build, the same example
with the `celld` block removed shows every probe succeeding, so the check does
fail without this change. `hello`, `counter`, `facets`, `rpc` and
`dynamic-worker-tails` give identical responses before and after.

Not tested: Python Workers (above), and a facet of a codegen class that comes
from the same bundle.
