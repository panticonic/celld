// What a call ended by a CPU limit leaves behind (docs/services/dynamic-workers.md,
// "Limits of one call"). The kernel facet of examples/facet-limits does what
// a product kernel's `start` does: it answers, then runs the caller's code
// under waitUntil behind its deadline timer and its serial chain. A spin
// there is ended by the limit in a continuation of the call. The timer the
// run armed must still fire and settle the run, and the next run must
// complete: a kernel whose deadline never fires never answers its caller
// again, which is the wedge this scenario pins. A spin inside a storage
// transaction, an asynchronous transaction or a critical section must roll
// back what it wrote and leave the object able to take the next one.
import { run } from "../lib/dsl.mjs";

const EXCEEDED = /Worker exceeded CPU limit of (\d+) ms/;

// Calls that must end: bounded by the request timeout, never by a sleep.
const call = (t, path) => t.fetch(path, { timeoutMs: 30_000 });

// Start a run, then wait for its row to reach a terminal status.
async function runToEnd(t, { object, id, timeout, spin, cpu, within }) {
  const limit = cpu === undefined ? "" : `&cpu=${cpu}`;
  const started = (await call(t, `/rpc/start?object=${object}&id=${id}&timeout=${timeout}&spin=${spin ? 1 : 0}${limit}`)).json;
  t.assert(started.ok === true && started.body.started === id, `start ${id}: ${JSON.stringify(started)}`);
  let status = null;
  await t.poll(async () => {
    const r = (await call(t, `/rpc/run?object=${object}&id=${id}`)).json;
    t.assert(r.ok === true, `run ${id}: ${JSON.stringify(r)}`);
    status = r.body;
    t.assert(status !== "running", `run ${id} is still running`);
  }, { timeoutMs: within, why: "the run's deadline timer settles it after the CPU limit ends its code" });
  return status;
}

export default {
  id: "facet-limit-recovery",
  example: "facet-limits",
  title: "a call ended by its CPU limit leaves nothing held: the timers it armed still fire, its transaction rolls back, and the object takes the next call",
  steps: [
    run("a run whose code spins is settled by its deadline after the call's limit ends it, and the next run completes", async (t) => {
      const born = (await call(t, "/rpc/info?object=k1")).json.body.born;
      const first = await runToEnd(t, { object: "k1", id: "r1", timeout: 300, spin: true, cpu: 500, within: 15_000 });
      t.assert(first === "timed-out", `the spinning run: ${first}`);
      const second = await runToEnd(t, { object: "k1", id: "r2", timeout: 5_000, spin: false, cpu: 500, within: 15_000 });
      t.assert(second === "done", `the next run: ${second}`);
      const info = (await call(t, "/rpc/info?object=k1")).json;
      t.assert(info.ok === true && info.body.born === born, `the kernel keeps its heap: ${JSON.stringify(info)}`);
    }),

    run("the same under the loaded worker's own limit, with no limit on the call", async (t) => {
      const first = await runToEnd(t, { object: "k2", id: "u1", timeout: 300, spin: true, within: 20_000 });
      t.assert(first === "timed-out", `the spinning run: ${first}`);
      const second = await runToEnd(t, { object: "k2", id: "u2", timeout: 5_000, spin: false, within: 15_000 });
      t.assert(second === "done", `the next run: ${second}`);
    }),

    run("a run ended twice in a row: each is settled and the third run completes", async (t) => {
      for (const id of ["s1", "s2"]) {
        const status = await runToEnd(t, { object: "k3", id, timeout: 200, spin: true, cpu: 300, within: 15_000 });
        t.assert(status === "timed-out", `${id}: ${status}`);
      }
      const last = await runToEnd(t, { object: "k3", id: "s3", timeout: 5_000, spin: false, cpu: 300, within: 15_000 });
      t.assert(last === "done", `the third run: ${last}`);
    }),

    // A transaction's writes roll back. A critical section is no transaction:
    // what it wrote before the spin is committed, as each statement is, and
    // the object it reset takes the next event.
    // The same on a Dolt facet, whose transactions commit (a product kernel's
    // storage), with `dolt=1`.
    ...[
      ["sync", "transactionSync", [], ""],
      ["async", "transaction()", [], ""],
      ["block", "blockConcurrencyWhile", ["spun"], ""],
      ["sync", "a Dolt facet's transactionSync({ commit })", [], "1"],
      ["async", "a Dolt facet's transaction({ commit })", [], "1"],
    ].map(([kind, what, kept, dolt]) =>
      run(`a spin inside ${what} ${kept.length ? "keeps its committed write" : "rolls back its write"}, and the object takes the next transaction`, async (t) => {
        const object = `tx-${kind}${dolt ? "-dolt" : ""}&dolt=${dolt}`;
        const spun = (await call(t, `/rpc/tx-spin?object=${object}&kind=${kind}&cpu=400`)).json;
        t.assert(spun.ok === false && EXCEEDED.test(spun.error), `the spin: ${JSON.stringify(spun)}`);
        const write = (await call(t, `/rpc/tx-write?object=${object}&v=after`)).json;
        t.assert(write.ok === true, `the next transaction: ${JSON.stringify(write)}`);
        t.assert(JSON.stringify(write.body) === JSON.stringify([...kept, "after"]), `the rows after the spin: ${JSON.stringify(write.body)}`);
        const again = (await call(t, `/rpc/tx-write?object=${object}&v=again`)).json;
        t.assert(again.ok === true && JSON.stringify(again.body) === JSON.stringify([...kept, "after", "again"]), JSON.stringify(again));
      }),
    ),
  ],
};
