// fetch() with an Upgrade header from an alarm, or from a continuation after
// its event ended, gives a socket the object keeps and uses after the event
// returned, as it does from a request (docs/fork/design.md section 3b.14).
// See examples/ws-alarm.
import { run, M } from "../lib/dsl.mjs";
import { startWsServer } from "../lib/ws.mjs";

const dialed = (mode, extra = {}) => ({ [`${mode}:1`]: "pong:hello", ...extra });

// The reply arrives, and a later request sends on the socket and gets a reply.
const roundTrips = async (t, mode, start) => {
  await start();
  await t.poll(async () => {
    const res = await t.fetch("/state");
    if (res.json?.[`${mode}:error`]) throw new Error(`the dial failed: ${res.json[`${mode}:error`]}`);
    t.expect(res, { status: 200, json: M.partial(dialed(mode)) });
  }, { timeoutMs: 15000, intervalMs: 250, why: "handshake and one round trip" });
  t.expect(await t.fetch("/send?text=later"), { status: 202 });
  await t.poll(async () => t.expect(await t.fetch("/state"), { status: 200, json: M.partial({ [`${mode}:2`]: "pong:later" }) }), {
    timeoutMs: 10000, intervalMs: 200, why: "a second round trip from a later request",
  });
};

export default {
  example: "ws-alarm",
  title: "a WebSocket opened with fetch() from an alarm or a later continuation stays usable after the event returns",
  steps: [
    run("start a local WebSocket server", async (t) => {
      t.srv = await startWsServer();
      t.onCleanup(() => t.srv.close());
    }),
    run("from a request", (t) => roundTrips(t, "fetch", async () => {
      t.expect(await t.fetch(`/connect?url=${encodeURIComponent(t.srv.url)}`), { status: 202 });
    })),
    run("from a continuation another request releases", (t) => roundTrips(t, "orphan", async () => {
      t.expect(await t.fetch(`/arm-orphan?url=${encodeURIComponent(t.srv.url)}`), { status: 202 });
      t.expect(await t.fetch("/release"), { status: 202 });
    })),
    ...["alarm", "alarm-block", "alarm-detached"].map((mode) =>
      run(`from an alarm that ${{ alarm: "awaits the dial", "alarm-block": "dials in a critical section", "alarm-detached": "returns before the dial ends" }[mode]}`, (t) =>
        roundTrips(t, mode, async () => {
          t.expect(await t.fetch(`/arm?mode=${mode}&url=${encodeURIComponent(t.srv.url)}`), { status: 202 });
        }))),
  ],
};
