import { get, restart, crash, restore, run, waitLog } from "../lib/dsl.mjs";
import { startWsServer } from "../lib/ws.mjs";

const reply = (echo, count) => JSON.stringify({ echo, count });

// wsecho: hibernatable WebSockets in a DO (ctx.acceptWebSocket +
// webSocketMessage). The count lives in DO storage, so it must survive
// everything that the storage survives.
async function roundtrip(t, ws, msg, count) {
  ws.send(msg);
  const got = await ws.next();
  t.assert(got === reply(msg, count), `sent ${msg}: expected ${reply(msg, count)}, got ${got}`);
}
const wsecho = {
  example: "wsecho",
  title: "hibernatable WebSocket echo: count in DO storage; restart/crash/restore",
  steps: [
    get("/", { status: 426, text: "websocket upgrade required" }),
    run("a socket echoes with an incrementing count", async (t) => {
      const ws = await t.ws("/");
      await roundtrip(t, ws, "a", 1);
      await roundtrip(t, ws, "b", 2);
      await roundtrip(t, ws, "unicode ✓ 日本", 3);
      await roundtrip(t, ws, "x".repeat(70_000), 4); // > 64 KiB: a multi-frame message
      ws.close(1000, "bye");
      t.vars.closed = await ws.closed();
      t.expectValue(t.vars.closed.code, 1000);
    }),
    run("two sockets share the same DO", async (t) => {
      const a = await t.ws("/");
      const b = await t.ws("/");
      await roundtrip(t, a, "from-a", 5);
      await roundtrip(t, b, "from-b", 6);
      await roundtrip(t, a, "from-a-again", 7);
      // Close before the restart: a graceful stop waits for open sockets (see the drain step below).
      a.close();
      b.close();
      await Promise.all([a.closed(), b.closed()]);
    }),
    restart(),
    run("after a graceful restart the count continues on a new socket", async (t) => {
      const ws = await t.ws("/");
      await roundtrip(t, ws, "r", 8);
      ws.close();
      await ws.closed();
    }),
    crash(),
    run("after SIGKILL", async (t) => {
      const ws = await t.ws("/");
      await roundtrip(t, ws, "c", 9);
      ws.close();
      await ws.closed();
    }),
    restore(),
    run("after wiping local state and restoring", async (t) => {
      const ws = await t.ws("/");
      await roundtrip(t, ws, "s", 10);
      ws.close();
      await ws.closed();
    }),
    // Observed behaviour, asserted as a bound rather than an ideal: SIGTERM with
    // a socket still open waits for the "preserve drain" (25 s deadline, logged as
    // `websockets=1`), then exits 0 and the client sees an abnormal close (1006).
    run("graceful stop with a socket open drains within the shutdown bound and exits 0", async (t) => {
      const ws = await t.ws("/");
      await roundtrip(t, ws, "open", 11);
      const t0 = Date.now();
      await t.celld.stop({ graceMs: 45000 });
      const took = Date.now() - t0;
      t.assert(t.celld.exit.code === 0, `exit ${JSON.stringify(t.celld.exit)}`);
      t.assert(took < 40000, `shutdown took ${took} ms`);
      await ws.closed({ timeoutMs: 5000 });
      await t.celld.start();
      const ws2 = await t.ws("/");
      await roundtrip(t, ws2, "next", 12);
      ws2.close();
    }),
  ],
};

// Hibernation: with CELLD_IDLE_EVICT_S=2 an idle cell is evicted while its
// socket stays open. The next message must wake the DO (a new isolate,
// epoch 2) and be handled with the persisted state.
// Waiting for the eviction is bounded: idle age 2 s + eviction sweep.
const hibernation = {
  example: "wsecho",
  id: "wsecho-hibernation",
  title: "an idle DO is evicted with its WebSocket open and woken by the next message",
  env: { CELLD_IDLE_EVICT_S: "2" },
  steps: [
    run("open a socket and send one message", async (t) => {
      t.ws1 = await t.ws("/");
      await roundtrip(t, t.ws1, "before", 1);
    }),
    waitLog(/event="eviction_durability_barrier"/, { timeoutMs: 20000, why: "idle eviction after CELLD_IDLE_EVICT_S=2 s", name: "cell is evicted while the socket is open" }),
    run("the open socket still works: message wakes the DO, state is intact", async (t) => {
      await roundtrip(t, t.ws1, "after", 2);
      t.assert(t.logLines(/cell isolate startup completed.*scope=W:/).some((l) => /epoch=2\b/.test(l)), "expected a second activation (epoch=2) of the cell");
    }),
    run("the same socket keeps working across a second hibernation", async (t) => {
      const n = t.logLines(/event="eviction_durability_barrier"/).length;
      await t.poll(async () => t.assert(t.logLines(/event="eviction_durability_barrier"/).length > n, "no second eviction yet"), {
        timeoutMs: 20000,
        intervalMs: 250,
        why: "idle eviction again after the wake",
      });
      await roundtrip(t, t.ws1, "again", 3);
    }),
  ],
};

// wsclient: a DO opens an outbound WebSocket to a target, sends "hello",
// stores the first reply and closes with 1000 "done". The target here is a
// local server run by this test.
const wsclient = {
  example: "wsclient",
  title: "outbound WebSocket client from a DO; reply persisted",
  steps: [
    run("start a local WebSocket server", async (t) => {
      t.srv = await startWsServer();
      t.onCleanup(() => t.srv.close());
    }),
    get("/", { status: 200, json: { message: null } }),
    get("/connect", { status: 400, text: "The url parameter is required." }),
    run("connect: the DO sends hello, stores the reply, closes with 1000 done", async (t) => {
      const r = await t.fetch(`/connect?url=${encodeURIComponent(t.srv.url)}`);
      t.expect(r, { status: 202 });
      await t.poll(async () => t.expect(await t.fetch("/"), { status: 200, json: { message: "pong:hello" } }), {
        timeoutMs: 15000,
        intervalMs: 200,
        why: "connect + handshake + one round trip",
      });
      await t.poll(async () => t.assert(t.srv.seen.closes.length === 1, "server saw no close frame"), { timeoutMs: 5000, intervalMs: 100, why: "client closes after storing the reply" });
      t.expectValue(t.srv.seen.messages, ["hello"]);
      t.expectValue(t.srv.seen.closes, [{ code: 1000, reason: "done" }]);
    }),
    restart(),
    get("/", { status: 200, json: { message: "pong:hello" } }),
    crash(),
    get("/", { status: 200, json: { message: "pong:hello" } }),
    restore(),
    get("/", { status: 200, json: { message: "pong:hello" } }),
    run("a second connect overwrites the message", async (t) => {
      t.srv.seen.messages.length = 0;
      const srv2 = await startWsServer({ reply: (m) => `second:${m}` });
      t.onCleanup(() => srv2.close());
      t.expect(await t.fetch(`/connect?url=${encodeURIComponent(srv2.url)}`), { status: 202 });
      await t.poll(async () => t.expect(await t.fetch("/"), { status: 200, json: { message: "second:hello" } }), { timeoutMs: 15000, intervalMs: 200, why: "second round trip" });
    }),
  ],
};

export default [wsecho, hibernation, wsclient];
