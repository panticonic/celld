import http from "node:http";
import { get, restart, crash, restore, run, waitLog, M } from "../lib/dsl.mjs";

// ---------------------------------------------------------------- alarm
// The example arms an alarm 2 s ahead; the handler counts fires. Waits are
// bounded: celld fires an alarm within ~1 s of its due time on a quiet node,
// so a firing that takes > 15 s is a failure, not slowness.
const ALARM_WAIT = { timeoutMs: 15000, intervalMs: 250, why: "alarm is due 2 s after /arm; bound is generous for startup" };
const idle = (fires) => get("/", { status: 200, json: { fires, pendingAlarm: null } });
const alarm = {
  example: "alarm",
  title: "DO alarm: arm, fire once, survive restart, SIGKILL and restore while pending",
  steps: [
    idle(0),
    get("/arm", { status: 200, json: { armed: true, at: M.capture("at", M.epochMs(10_000)) } }),
    get("/", { status: 200, json: { fires: 0, pendingAlarm: "{$at}" } }, { name: "getAlarm() reports the armed time" }),
    get("/", { status: 200, json: { fires: 1, pendingAlarm: null } }, { poll: ALARM_WAIT, name: "alarm fires and clears getAlarm()" }),
    run("an alarm fires exactly once (3 s quiet window)", async (t) => {
      // A negative claim needs a window; 3 s is > the 2 s arm delay of the alarm that just fired.
      await t.sleep(3000);
      t.expect(await t.fetch("/"), { status: 200, json: { fires: 1, pendingAlarm: null } });
    }),
    restart(),
    idle(1),
    // Arm, then SIGKILL before it is due: the alarm is part of the durable state.
    get("/arm", { status: 200, json: { armed: true, at: M.any() } }),
    crash(),
    get("/", { status: 200, json: { fires: 2, pendingAlarm: null } }, { poll: ALARM_WAIT, name: "alarm armed before SIGKILL fires after restart" }),
    // Arm, then lose every local database: the alarm must come back with the restore.
    get("/arm", { status: 200, json: { armed: true, at: M.any() } }),
    restore(),
    get("/", { status: 200, json: { fires: 3, pendingAlarm: null } }, { poll: ALARM_WAIT, name: "alarm armed before restore fires after restore" }),
  ],
};

// ----------------------------------------------------------------- cron
// `* * * * *` fires on the minute boundary, so the first tick is up to 60 s
// away; the wait is bounded at 75 s. After a restart the next tick must
// follow, again at most ~60 s later.
const tickRe = /cell_console: cron (\d+)\s*$/;
const ticks = (t) => t.logLines(tickRe).map((l) => Number(l.match(tickRe)[1]));
const cron = {
  example: "cron",
  title: "cron trigger: minute-aligned ticks, one per minute, continues after restart",
  slow: true,
  steps: [
    get("/", { status: 200, text: "The cron runs once a minute.\n" }),
    waitLog(tickRe, { timeoutMs: 75_000, session: false, why: "first minute boundary is <= 60 s away", name: "first cron tick" }),
    run("tick is minute-aligned and recent", async (t) => {
      const [first] = ticks(t);
      t.assert(first % 60_000 === 0, `scheduledTime ${first} is not on a minute boundary`);
      t.assert(Math.abs(Date.now() - first) < 90_000, `scheduledTime ${first} is not recent`);
      t.vars.first = first;
    }),
    restart(),
    run("a tick after the restart, one minute (or more) after the first, no duplicates", async (t) => {
      await t.poll(
        async () => {
          const ts = ticks(t);
          const after = ts.filter((x) => x > t.vars.first);
          t.assert(after.length >= 1, "no later tick yet");
        },
        { timeoutMs: 75_000, intervalMs: 500, why: "next minute boundary is <= 60 s away" },
      );
      const ts = ticks(t);
      t.assert(new Set(ts).size === ts.length, `a scheduled time ran twice: ${ts}`);
      t.assert(ts.every((x) => x % 60_000 === 0), `unaligned tick in ${ts}`);
    }),
    get("/", 200),
  ],
};

// ------------------------------------------------------------- workflow
// The workflow fetches a document in one durable step. A local HTTP server
// stands in for https://example.com so the test needs no internet.
function startDocServer() {
  const hits = { n: 0 };
  const srv = http.createServer((req, res) => {
    hits.n++;
    res.writeHead(200, { "content-type": "text/plain" });
    res.end("line one\nline two\nline three\n");
  });
  return new Promise((resolve) => srv.listen(0, "127.0.0.1", () => resolve({ srv, hits, url: `http://127.0.0.1:${srv.address().port}/doc` })));
}
const workflow = {
  example: "workflow",
  title: "Workflow: create, run a durable step, status, persistence across restart/crash/restore",
  steps: [
    run("start a local document server", async (t) => {
      const d = await startDocServer();
      t.doc = d;
      t.onCleanup(() => new Promise((r) => d.srv.close(r)));
    }),
    get("/", 404, { name: "unknown route -> 404" }),
    run("create an instance and wait for completion", async (t) => {
      const r = await t.fetch(`/create?url=${encodeURIComponent(t.doc.url)}`);
      t.expect(r, { status: 200, json: { id: M.capture("id", M.type("string")) } });
      t.vars.id = r.json.id;
      await t.poll(
        async () => {
          const s = await t.fetch(`/status?id=${t.vars.id}`);
          t.expect(s, { status: 200, json: M.partial({ status: "complete", output: { bytes: 29, lines: 4 } }) });
        },
        { timeoutMs: 20000, intervalMs: 250, why: "one fetch step; runs on the first replay" },
      );
      t.assert(t.doc.hits.n === 1, `source fetched ${t.doc.hits.n} times, expected 1`);
    }),
    restart(),
    get("/status?id={$id}", { status: 200, json: M.partial({ status: "complete", output: { bytes: 29, lines: 4 } }) }, { name: "finished instance and its step output survive a restart" }),
    crash(),
    get("/status?id={$id}", { status: 200, json: M.partial({ status: "complete", output: { bytes: 29, lines: 4 } }) }, { name: "...and SIGKILL" }),
    restore(),
    get("/status?id={$id}", { status: 200, json: M.partial({ status: "complete", output: { bytes: 29, lines: 4 } }) }, { name: "...and restore from the replication log" }),
    run("the step did not run again after replays", async (t) => {
      t.assert(t.doc.hits.n === 1, `source fetched ${t.doc.hits.n} times`);
    }),
    get("/status?id=does-not-exist", { status: 500 }, { name: "status of an unknown instance is an error" }),
  ],
};

export default [alarm, cron, workflow];
