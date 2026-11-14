// A per-call CPU limit on a facet stub: `ctx.facets.get(name, startup,
// { limits: { cpuMs } })` (docs/services/dynamic-workers.md, "Limits of one
// call"). The kernel facet comes from a loaded Worker whose own limit is 3000
// ms (UNIT_CPU_MS in examples/facet-limits); a spinning call is ended by the
// lower of that and the call's limit, and the facet, its root and a sibling
// object keep working.
import { run } from "../lib/dsl.mjs";

const UNIT_MS = 3000;
const EXCEEDED = /Worker exceeded CPU limit of (\d+) ms/;

// Calls that must end: bounded by the request timeout, never by a sleep.
const call = (t, path) => t.fetch(path, { timeoutMs: 30_000 });

export default {
  example: "facet-limits",
  title: "a per-call CPU limit on a facet stub ends a spinning call, over fetch and RPC, below and above the loaded worker's own limit",
  steps: [
    run("the kernel facet answers, and so does a call within its limit", async (t) => {
      const info = (await call(t, "/kernel/info")).json;
      t.assert(info.ok === true && typeof info.body.born === "string", JSON.stringify(info));
      t.vars.born = info.body.born;
      const work = await call(t, "/kernel/work?n=1000&cpu=2000");
      t.assert(work.json.ok === true && work.json.body.worked === 1000, JSON.stringify(work.json));
    }),

    run("a spin through fetch ends at the call's limit, not the loaded worker's", async (t) => {
      const spin = await call(t, "/kernel/spin?cpu=500");
      t.assert(spin.status === 500, `status ${spin.status}: ${JSON.stringify(spin.json)}`);
      const m = EXCEEDED.exec(spin.json.error);
      t.assert(m && m[1] === "500", `the error: ${spin.json.error}`);
      t.assert(spin.json.ms < UNIT_MS - 500, `ended after ${spin.json.ms} ms, the unit limit is ${UNIT_MS}`);
    }),

    run("a spin through RPC ends the same way", async (t) => {
      const spin = await call(t, "/rpc/spin?cpu=500");
      t.assert(spin.status === 500, `status ${spin.status}: ${JSON.stringify(spin.json)}`);
      const m = EXCEEDED.exec(spin.json.error);
      t.assert(m && m[1] === "500", `the error: ${spin.json.error}`);
      t.assert(spin.json.ms < UNIT_MS - 500, `ended after ${spin.json.ms} ms`);
    }),

    run("the root, the facet and a sibling object keep working, and the facet keeps its heap", async (t) => {
      t.assert((await call(t, "/root")).json.rootAlive === true, "the root");
      const info = (await call(t, "/kernel/info")).json;
      t.assert(info.ok === true, JSON.stringify(info));
      t.assert(info.body.born === t.vars.born, "the facet was rebuilt by the ended call");
      const sibling = (await call(t, "/kernel/info?object=other")).json;
      t.assert(sibling.ok === true, `a sibling object: ${JSON.stringify(sibling)}`);
      const spin = (await call(t, "/kernel/spin?cpu=300&object=other")).json;
      t.assert(spin.ok === false && EXCEEDED.test(spin.error), `a sibling's own spin: ${JSON.stringify(spin)}`);
      t.assert((await call(t, "/kernel/info")).json.ok === true, "the first object after a sibling's spin");
    }),

    run("the limit belongs to one call: calls under it, one after another, all complete", async (t) => {
      // Each call is 300 million iterations, about 0.4 s of CPU here; six of
      // them together spend more than the 1500 ms limit.
      for (let i = 0; i < 6; i++) {
        const work = (await call(t, "/kernel/work?n=300000000&cpu=1500")).json;
        t.assert(work.ok === true && work.body.worked === 300000000, `call ${i}: ${JSON.stringify(work)}`);
      }
      const unlimited = (await call(t, "/kernel/work?n=20000000")).json;
      t.assert(unlimited.ok === true, JSON.stringify(unlimited));
    }),

    run("a limit above the loaded worker's own cannot raise it", async (t) => {
      const spin = (await call(t, "/kernel/spin?cpu=600000")).json;
      t.assert(spin.ok === false, JSON.stringify(spin));
      const m = EXCEEDED.exec(spin.error);
      t.assert(m && m[1] === String(UNIT_MS), `the error: ${spin.error}`);
      t.assert(spin.ms >= UNIT_MS - 500 && spin.ms < 15_000, `ended after ${spin.ms} ms`);
    }),

    run("with no limit on the call the loaded worker's own limit still applies", async (t) => {
      const spin = (await call(t, "/rpc/spin")).json;
      t.assert(spin.ok === false, JSON.stringify(spin));
      const m = EXCEEDED.exec(spin.error);
      t.assert(m && m[1] === String(UNIT_MS), `the error: ${spin.error}`);
      t.assert((await call(t, "/kernel/info")).json.ok === true, "the facet after its own limit");
    }),

    run("a per-call subrequest limit counts that call's subrequests only", async (t) => {
      const limited = (await call(t, "/kernel/subs?n=4&sub=2")).json;
      t.assert(limited.ok === true, JSON.stringify(limited));
      const refused = limited.body.filter((m) => /Worker exceeded subrequest limit of 2/.test(m));
      t.assert(limited.body.length === 4 && refused.length === 2, `the limit of 2: ${JSON.stringify(limited.body)}`);
      // The next call has a count of its own, over RPC this time.
      const again = (await call(t, "/rpc/subs?n=2&sub=2")).json;
      t.assert(again.ok === true && !again.body.some((m) => /subrequest limit/.test(m)), JSON.stringify(again));
      const free = (await call(t, "/kernel/subs?n=4")).json;
      t.assert(free.ok === true && !free.body.some((m) => /subrequest limit/.test(m)), JSON.stringify(free));
    }),

    run("an invalid limit is refused at the call", async (t) => {
      for (const bad of ["0", "-5", "1.5", '"fast"']) {
        const r = (await call(t, `/bad?cpu=${encodeURIComponent(bad)}`)).json;
        t.assert(r.ok === false && /cpuMs must be a positive integer|options\.limits/.test(r.error), `${bad}: ${JSON.stringify(r)}`);
      }
    }),
  ],
};
