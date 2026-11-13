import { get, post, restart, crash, restore, run, waitLog, noLog, M } from "../lib/dsl.mjs";

// ------------------------------------------------------------ example
// examples/queues as shipped. The only observable output is the consumer's
// console log. Timing is inherent to celld: a batch is delivered when it is
// full or after max_batch_timeout (5 s) and a retry also waits for a batch
// window, so the full retry -> dead-letter path takes ~25 s. Waits are
// bounded at 45 s.
const okLine = /cell_console: processing job \{"path":"\/ok"/;
const failLine = (n) => new RegExp(`cell_console: retrying job \\{"path":"/a/fail",[^}]*\\} ${n}\\s*$`);
const deadLine = /cell_console: dead job \{"path":"\/a\/fail"/;
const example = {
  example: "queues",
  title: "producer -> consumer, retries (max_retries=2) -> dead-letter queue",
  slow: true,
  steps: [
    get("/ok", { status: 200, json: { queued: { path: "/ok", createdAt: M.isoDate() } } }),
    get("/a/fail", { status: 200, json: { queued: { path: "/a/fail", createdAt: M.isoDate() } } }),
    waitLog(okLine, { timeoutMs: 20000, session: false, why: "batch window max_batch_timeout=5 s", name: "ok job is processed" }),
    waitLog(failLine(1), { timeoutMs: 20000, session: false, why: "first delivery shares the 5 s batch window", name: "failing job: attempt 1 retried" }),
    waitLog(failLine(2), { timeoutMs: 30000, session: false, why: "retry delay 1 s + next batch window", name: "failing job: attempt 2 retried" }),
    waitLog(failLine(3), { timeoutMs: 30000, session: false, why: "retry delay 1 s + next batch window", name: "failing job: attempt 3 (max_retries=2 exhausted) retried" }),
    waitLog(deadLine, { timeoutMs: 30000, session: false, why: "moved to example-jobs-dead-letter after exhausting retries", name: "failing job reaches the dead-letter queue" }),
    run("the acked job ran once; the failing job was delivered exactly 3 times and dead-lettered once", async (t) => {
      t.expectValue(t.logLines(okLine).length, 1);
      t.expectValue([1, 2, 3].map((n) => t.logLines(failLine(n)).length), [1, 1, 1]);
      t.expectValue(t.logLines(deadLine).length, 1);
    }),
    restart(),
    get("/ok", 200),
    waitLog(okLine, { timeoutMs: 20000, session: true, why: "batch window 5 s", name: "producer works after restart" }),
    run("settled messages are not redelivered after a restart (6 s window > the 5 s batch window)", async (t) => {
      await t.sleep(6000);
      t.expectValue(t.logLines(deadLine).length, 1);
      t.expectValue(t.logLines(/cell_console: retrying job/).length, 3);
    }),
  ],
};

// ------------------------------------------------------------ probe
// tests/e2e/fixtures/queues-probe replaces the example's Worker with one that
// records every delivery in a Durable Object (queue, batch size, attempts,
// message ids, decoded bodies), so batching, delays, content types and
// dead-lettering are asserted exactly instead of through logs.
const records = async (t) => (await t.fetch("/records")).json;
const msgs = (recs, queue = "probe-q") =>
  recs.filter((r) => r.queue === queue).flatMap((r) => r.messages.map((m) => ({ ...m, batchSize: r.size, at: r.at })));
const ofKind = (ms, kind) => ms.filter((m) => m.shape.value?.kind === kind);
const WAIT = (why, timeoutMs = 20000) => ({ timeoutMs, intervalMs: 250, why });

const probe = {
  example: "queues",
  id: "queues-probe",
  fixture: "queues-probe",
  title: "batching, delaySeconds, content types, retry/throw redelivery, DLQ, durability",
  slow: true,
  steps: [
    run("sendBatch of 25 arrives as batches of at most max_batch_size=10, each message once", async (t) => {
      t.expect(await t.fetch("/send?n=25&kind=batch", { method: "POST" }), { status: 200, json: { sent: 25, kind: "batch", sentAt: M.epochMs() } });
      await t.poll(async () => t.assert(ofKind(msgs(await records(t)), "batch").length >= 25, "not all delivered"), WAIT("batch window 2 s"));
      const recs = (await records(t)).filter((r) => r.messages.some((m) => m.shape.value?.kind === "batch"));
      const all = ofKind(msgs(await records(t)), "batch");
      t.expectValue(all.length, 25);
      t.expectValue(new Set(all.map((m) => m.shape.value.i)).size, 25);
      t.expectValue(new Set(all.map((m) => m.id)).size, 25);
      t.assert(recs.every((r) => r.size <= 10), `a batch exceeded max_batch_size: ${recs.map((r) => r.size)}`);
      t.assert(recs.length < 25 && recs.some((r) => r.size > 1), `messages were not batched: ${recs.map((r) => r.size)}`);
      t.assert(all.every((m) => m.attempts === 1), "unexpected redelivery");
    }),
    run("delaySeconds=3 holds the message back at least ~3 s", async (t) => {
      await t.fetch("/send?n=1&kind=delayed&delay=3", { method: "POST" });
      await t.poll(async () => t.assert(ofKind(msgs(await records(t)), "delayed").length === 1, "not delivered"), WAIT("3 s delay + 2 s batch window"));
      const [m] = ofKind(msgs(await records(t)), "delayed");
      t.assert(m.at - m.shape.value.sentAt >= 2800, `delivered ${m.at - m.shape.value.sentAt} ms after send`);
    }),
    run("content types round-trip: text, json, bytes, v8", async (t) => {
      for (const ct of ["text", "json", "bytes", "v8"]) await t.fetch(`/send-typed?ct=${ct}`, { method: "POST" });
      await t.poll(async () => t.assert(msgs(await records(t)).filter((m) => m.shape.type !== "object" || m.shape.value?.ct).length >= 4, "not all delivered"), WAIT("batch window"));
      const all = msgs(await records(t));
      const byType = (ty) => all.filter((m) => m.shape.type === ty).map((m) => m.shape);
      t.expectValue(byType("string").filter((s) => s.value === "plain text ✓").length, 1);
      t.expectValue(byType("Uint8Array").filter((s) => s.value.join() === "1,2,3,250").length, 1);
      t.expectValue(byType("v8"), [{ type: "v8", when: 0, set: [1, 2], nested: { a: [1, { b: 2 }] } }]);
      t.expectValue(byType("object").filter((s) => s.value?.ct === "json" && s.value.hello === "world").length, 1);
    }),
    run("a throwing consumer redelivers the same message id with attempts=2", async (t) => {
      await t.fetch("/send?n=1&kind=throw-once", { method: "POST" });
      await t.poll(async () => t.assert(ofKind(msgs(await records(t)), "throw-once").length >= 2, "not redelivered"), WAIT("batch window + retry (no backoff added)", 30000));
      const ms = ofKind(msgs(await records(t)), "throw-once");
      t.expectValue(ms.map((m) => m.attempts), [1, 2]);
      t.expectValue(new Set(ms.map((m) => m.id)).size, 1);
    }),
    run("retry({delaySeconds:2}) redelivers no sooner than 2 s later", async (t) => {
      await t.fetch("/send?n=1&kind=retry-once", { method: "POST" });
      await t.poll(async () => t.assert(ofKind(msgs(await records(t)), "retry-once").length >= 2, "not redelivered"), WAIT("batch window + 2 s retry delay", 30000));
      const ms = ofKind(msgs(await records(t)), "retry-once");
      t.expectValue(ms.map((m) => m.attempts), [1, 2]);
      t.assert(ms[1].at - ms[0].at >= 1800, `redelivered after ${ms[1].at - ms[0].at} ms`);
    }),
    run("exhausting max_retries=1 dead-letters the message exactly once", async (t) => {
      await t.fetch("/send?n=1&kind=retry", { method: "POST" });
      await t.poll(async () => t.assert(ofKind(msgs(await records(t), "probe-dlq"), "retry").length >= 1, "not dead-lettered"), WAIT("2 deliveries + retry delay + DLQ batch window", 40000));
      const recs = await records(t);
      t.expectValue(ofKind(msgs(recs), "retry").map((m) => m.attempts), [1, 2]);
      t.expectValue(ofKind(msgs(recs, "probe-dlq"), "retry").length, 1);
    }),
    // Durability: a delayed message must survive the node going away before it is due.
    run("send a message due in 6 s, then SIGKILL immediately", async (t) => {
      await t.fetch("/send?n=1&kind=dur-crash&delay=6", { method: "POST" });
      t.assert(ofKind(msgs(await records(t)), "dur-crash").length === 0, "delivered before it was due");
    }),
    crash("SIGKILL and restart"),
    run("the message is delivered after the restart", async (t) => {
      await t.poll(async () => t.assert(ofKind(msgs(await records(t)), "dur-crash").length === 1, "not delivered"), WAIT("6 s delay + 2 s batch window, from startup", 30000));
    }),
    run("send another message due in 6 s", async (t) => {
      await t.fetch("/send?n=1&kind=dur-restore&delay=6", { method: "POST" });
    }),
    restore(),
    run("the message survives loss of local state and is delivered", async (t) => {
      await t.poll(async () => t.assert(ofKind(msgs(await records(t)), "dur-restore").length === 1, "not delivered"), WAIT("6 s delay + 2 s batch window, from startup", 30000));
    }),
    run("nothing was delivered twice (ack is final)", async (t) => {
      const all = msgs(await records(t));
      for (const kind of ["batch", "delayed", "dur-crash", "dur-restore"]) {
        const ms = ofKind(all, kind);
        t.expectValue(new Set(ms.map((m) => m.id + ":" + m.attempts)).size, ms.length);
        t.assert(ms.every((m) => m.attempts === 1), `${kind} redelivered after ack`);
      }
    }),
  ],
};

export default [example, probe];
