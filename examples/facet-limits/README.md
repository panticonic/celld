# Facet limits

A fork feature: a CPU limit on one call to a facet. The `Owner` object runs a
`Kernel` class, loaded with the Worker Loader, as a facet. The loaded Worker
has its own limit (`limits: { cpuMs: 3000 }`), which applies to every call. The
owner gets a stub for one call with a lower limit of its own:

```js
this.ctx.facets.get("kernel", startup, { limits: { cpuMs: 500 } }).fetch(request);
```

A spinning call ends after 500 ms with `Worker exceeded CPU limit of 500 ms`.
The kernel keeps its heap, the owner keeps answering and so does a second owner.
A limit above the Worker's own cannot raise it. The documentation is
[Limits of one call](../../docs/services/dynamic-workers.md#limits-of-one-call).

```sh
celld dev .
curl 'http://127.0.0.1:9876/kernel/spin?cpu=500'        # ends at 500 ms (fetch)
curl 'http://127.0.0.1:9876/rpc/spin?cpu=500'           # the same over RPC
curl 'http://127.0.0.1:9876/kernel/spin'                # ends at the Worker's 3000 ms
curl 'http://127.0.0.1:9876/kernel/info'                # the heap survived
```

Driven by `tests/e2e/scenarios/facet-limits.mjs`.
