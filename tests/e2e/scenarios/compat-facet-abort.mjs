// A request in flight to a facet when the facet is aborted ends: workerd
// rejects the pending call with the abort reason (docs/fork/design.md section
// 3b.14). See examples/facet-abort.
import { run } from "../lib/dsl.mjs";

const bounded = (t, path, ms = 8000) => t.fetch(path, { timeoutMs: ms });

// Starts `path`, which parks in the facet; once the facet is running (`/ok`
// answers only then) aborts it, and resolves with the parked request's answer.
const parkedThenAborted = async (t, path) => {
  const pending = t.fetch(path, { timeoutMs: 8000 }).catch((error) => {
    throw new Error(`the request to ${path} did not end: ${error}`);
  });
  pending.catch(() => {});
  t.expect(await bounded(t, "/ok"), { status: 200, text: "facet /ok" });
  t.expect(await bounded(t, "/abort"), { status: 200, text: "aborted" });
  return pending;
};

export default {
  example: "facet-abort",
  title: "a request in flight to a facet ends when the facet is aborted",
  steps: [
    run("the root catches the rejection and answers", async (t) => {
      t.expect(await parkedThenAborted(t, "/hold"), { status: 500, text: "root caught: x" });
    }),
    run("a rejection the root lets through fails the request, which still ends", async (t) => {
      t.expect(await parkedThenAborted(t, "/hold-uncaught"), { status: 500 });
    }),
    run("a root that gave up on the call answers at once, and the abort leaves it alone", async (t) => {
      t.expect(await bounded(t, "/race"), { status: 500, text: "root gave up" });
      t.expect(await bounded(t, "/abort"), { status: 200, text: "aborted" });
    }),
    run("a handler another request settles answers although a facet call of its own is still pending", async (t) => {
      const pending = t.fetch("/hold2", { timeoutMs: 8000 });
      pending.catch(() => {});
      await t.poll(async () => t.expect(await bounded(t, "/wake"), { status: 200, text: "woken" }), {
        timeoutMs: 5000, intervalMs: 100, why: "the request has to reach the handler and start its facet call",
      });
      t.expect(await pending, { status: 200, text: "hold2 done" });
      t.expect(await bounded(t, "/abort"), { status: 200, text: "aborted" });
    }),
    run("the facet runs again after an abort", async (t) => {
      t.expect(await bounded(t, "/ok"), { status: 200, text: "facet /ok" });
    }),
  ],
};
