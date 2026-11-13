import { get, restart, waitLog, run } from "../lib/dsl.mjs";

// A Worker is loaded from a source string at runtime (Worker Loader); its
// console output is delivered to a Tail Worker (`DynamicWorkerTail`) after the
// response, which re-logs it with the `workerId` prop it got from ctx.props.
const body = { status: 200, text: "Hello from a Dynamic Worker!\n" };
const tailLine = /cell_console: dynamic-worker-tail: hello from loaded code \{"workerId":"hello-v1","level":"log"\}\s*$/;
const loadedLine = /cell_console: hello from loaded code\s*$/;

export default {
  example: "dynamic-worker-tails",
  title: "Worker Loader: run loaded code, tail receives its console record with props",
  steps: [
    get("/", body),
    // The tail runs after the response is available; the wait is bounded.
    waitLog(tailLine, { timeoutMs: 10000, session: true, why: "tail event is delivered after the response", name: "tail logs the loaded worker's record with props" }),
    run("the loaded worker's own console.log reached the node log once", async (t) => {
      t.expectValue(t.logLines(loadedLine).length, 1);
    }),
    get("/", body),
    run("every invocation produces exactly one tail record", async (t) => {
      await t.poll(async () => t.expectValue(t.logLines(tailLine).length, 2), { timeoutMs: 10000, intervalMs: 200, why: "second tail event" });
    }),
    restart(),
    get("/", body),
    waitLog(tailLine, { timeoutMs: 10000, session: true, why: "tail event after restart", name: "tail works after restart (loader re-runs the factory)" }),
  ],
};
