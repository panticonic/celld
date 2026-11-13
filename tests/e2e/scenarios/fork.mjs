// Scenarios for the fork-only examples: codegen confinement, deadlines,
// residency, the P2P binding, and the three examples whose own scripts are
// larger than a scenario (dolt, dolt-facets, egress), which run here as they
// are. Each script keeps working standalone, as its README says.
import { execFile } from "node:child_process";
import dgram from "node:dgram";
import fs from "node:fs";
import path from "node:path";
import { Celld, freePort } from "../lib/celld.mjs";
import { EXAMPLES_DIR } from "../lib/env.mjs";
import { get, run, M } from "../lib/dsl.mjs";

// ------------------------------------------------------------- codegen
// `eval`, `new Function` and WebAssembly compilation are refused everywhere
// except a class that declared `codegen`; an imported Wasm module still runs.
const refused = { error: "EvalError" };
const denied = {
  eval: refused,
  indirectEval: refused,
  Function: refused,
  AsyncFunction: refused,
  "WebAssembly.Module": { error: "CompileError" },
  "WebAssembly.compile": { error: "CompileError" },
  importedWasm: { ok: "5" },
};
const allowed = {
  eval: { ok: "2" },
  indirectEval: { ok: "4" },
  Function: { ok: "42" },
  AsyncFunction: { ok: "7" },
  "WebAssembly.Module": { ok: "true" },
  "WebAssembly.compile": { ok: "true" },
  importedWasm: { ok: "5" },
};
const codegen = {
  example: "codegen",
  title: "codegen confinement: refused by default, allowed for a declaring class, grantable through a loader that holds it",
  steps: [
    get("/stateless", { status: 200, json: denied }, { name: "a stateless handler is confined" }),
    get("/confined/probe", { status: 200, json: denied }, { name: "an undeclared class is confined" }),
    get("/trusted/probe", { status: 200, json: allowed }, { name: "the declared class may generate code" }),
    get("/confined/loader", {
      status: 200,
      json: M.partial({
        withoutGrant: refused,
        requested: M.partial({ refused: M.re(/does not hold code generation/) }),
      }),
    }, { name: "a loader without codegen: its worker is confined and cannot be granted it" }),
    get("/trusted/loader", {
      status: 200,
      json: M.partial({ withoutGrant: refused, requested: { ok: "42" } }),
    }, { name: "a loader with codegen: confined by default, grantable on request" }),
  ],
};

// ----------------------------------------------------------- deadlines
// `blockConcurrencyWhile` keeps its 30 s limit and resets the object; a node
// without implicit deadlines lets a 35 s callback finish. Each takes ~35 s.
const DEADLINE_WAIT = { timeoutMs: 90_000 };
const deadlinesDefault = {
  example: "deadlines",
  id: "deadlines",
  title: "blockConcurrencyWhile past 30 s resets the object by default",
  slow: true,
  steps: [
    get("/", { status: 200, json: { outcome: "reset", message: M.re(/./) } }, { ...DEADLINE_WAIT, name: "a 35 s callback is reset" }),
  ],
};
const deadlinesOff = {
  example: "deadlines",
  id: "deadlines-off",
  title: "with CELLD_NO_IMPLICIT_DEADLINES=1 the same callback completes",
  slow: true,
  env: { CELLD_NO_IMPLICIT_DEADLINES: "1" },
  steps: [
    get("/", { status: 200, json: { outcome: "completed", seconds: M.between(34, 60) } }, { ...DEADLINE_WAIT, name: "a 35 s callback completes" }),
  ],
};

// ----------------------------------------------------------- residency
// With CELLD_IDLE_EVICT_S=2, an idle object loses its in-memory counter; one
// that holds `ctx.residency.hold()` keeps it far past the idle age; after
// `release()` it is evicted like any other. The waits are negative claims
// ("is still there"), so they are fixed windows: the idle age plus a few of the
// once-a-second load samples that run eviction.
const IDLE_WINDOW_MS = 7000;
const residency = {
  example: "residency",
  title: "a residency hold keeps an object past idle eviction until it is released",
  slow: true,
  env: { CELLD_IDLE_EVICT_S: "2" },
  steps: [
    run("without a hold the counter is lost to idle eviction", async (t) => {
      for (let i = 0; i < 3; i++) await t.fetch("/inc?name=plain");
      const before = (await t.fetch("/inc?name=plain")).json;
      t.assert(before.counter === 4, `counter ${before.counter}`);
      await t.sleep(IDLE_WINDOW_MS);
      const after = (await t.fetch("/inc?name=plain")).json;
      t.assert(after.counter === 1, `the counter survived idle eviction: ${JSON.stringify(after)}`);
      t.assert(after.bornAt !== before.bornAt, "the object was not rebuilt");
    }),
    run("with a hold it survives well past the idle age", async (t) => {
      for (let i = 0; i < 3; i++) await t.fetch("/inc?name=kernel");
      const held = (await t.fetch("/hold?name=kernel")).json;
      t.assert(held.held === true, `hold refused: ${JSON.stringify(held)}`);
      await t.sleep(2 * IDLE_WINDOW_MS);
      const after = (await t.fetch("/get?name=kernel")).json;
      t.assert(after.counter === 3, `the held counter was lost: ${JSON.stringify(after)}`);
      t.assert(after.bornAt === held.bornAt, "the held object was rebuilt");
    }),
    run("after release it is evicted like any other", async (t) => {
      const released = (await t.fetch("/release?name=kernel")).json;
      t.assert(released.held === false, `still held: ${JSON.stringify(released)}`);
      await t.sleep(IDLE_WINDOW_MS);
      const fresh = (await t.fetch("/get?name=kernel")).json;
      t.assert(fresh.counter === 0, `the released object was not evicted: ${JSON.stringify(fresh)}`);
    }),
  ],
};

// ----------------------------------------------------------------- p2p
// Two nodes of the example over Iroh with no relay and no discovery, each bound
// to a loopback UDP port and dialing the other by direct address. The Rust test
// `crates/celld/tests/p2p_two_nodes.rs` covers restarts and the cell-ownership
// lifecycle in more depth; this checks the example end to end through the
// runner: a dial from a request, `closed` on a clean end and on a reset, an
// alarm that dials and a stream that later requests keep using, and an
// inbound stream a handler hands to later requests.
async function freeUdpPort() {
  const socket = dgram.createSocket("udp4");
  await new Promise((resolve) => socket.bind(0, "127.0.0.1", resolve));
  const { port } = socket.address();
  await new Promise((resolve) => socket.close(resolve));
  return port;
}

const p2pEnv = (udp) => ({
  CELLD_P2P_RELAY: "disabled",
  CELLD_P2P_DISCOVERY: "none",
  CELLD_P2P_BIND: `127.0.0.1:${udp}`,
});

const p2p = {
  example: "p2p",
  title: "p2p: dial, closed on a clean end and a reset, an alarm's stream used by later requests, an inbound stream kept",
  async setup(t) {
    const a = await freeUdpPort();
    const b = await freeUdpPort();
    t.vars.udp = { a, b };
    Object.assign(t.celld.extraEnv, p2pEnv(a));
    // The second node: the same project in a directory of its own.
    const dir = `${t.workdir}-b`;
    fs.mkdirSync(dir, { recursive: true });
    fs.cpSync(path.join(EXAMPLES_DIR, "p2p"), dir, { recursive: true });
    const node = new Celld({
      bin: t.celld.bin,
      dir,
      esbuild: t.celld.esbuild,
      env: p2pEnv(b),
      name: "p2p-b",
    });
    t.vars.b = node;
    t.onCleanup(async () => {
      await node.kill(); // returns once the process has exited and logged its last line
      fs.rmSync(dir, { recursive: true, force: true });
    });
  },
  steps: [
    run("both nodes are up with their own ids", async (t) => {
      await t.vars.b.start();
      const a = (await t.fetch("/id")).json;
      const b = (await t.fetch(`${t.vars.b.origin}/id`)).json;
      t.assert(/^[0-9a-f]{64}$/.test(a.nodeId) && /^[0-9a-f]{64}$/.test(b.nodeId), "ids");
      t.assert(a.nodeId !== b.nodeId, "the nodes share an id");
      t.vars.aId = a.nodeId;
      t.vars.bId = b.nodeId;
      t.vars.toB = `peer=${b.nodeId}&addr=127.0.0.1:${t.vars.udp.b}`;
      t.vars.toA = `peer=${a.nodeId}&addr=127.0.0.1:${t.vars.udp.a}`;
    }),
    run("a request dials, exchanges lines and closes: closed resolves", async (t) => {
      const r = (await t.fetch(`/dial?${t.vars.toB}&msgs=one,two&finish=close`)).json;
      t.assert(JSON.stringify(r.replies) === JSON.stringify(["echo:one", "echo:two"]), JSON.stringify(r));
      t.assert(r.remoteNodeId === t.vars.bId, "the peer's authenticated id");
      t.assert(r.end === "closed" && r.closed === "resolved", JSON.stringify(r));
    }),
    run("both directions finishing resolves closed with no close()", async (t) => {
      const r = (await t.fetch(`/dial?${t.vars.toB}&msgs=x&finish=done`)).json;
      t.assert(JSON.stringify(r.replies) === JSON.stringify(["echo:x", "done:ok"]), JSON.stringify(r));
      t.assert(r.closed === "resolved", JSON.stringify(r));
    }),
    run("a reset by the peer rejects closed with its code", async (t) => {
      const r = (await t.fetch(`/dial?${t.vars.toB}&msgs=y&finish=bye`)).json;
      t.assert(/stream reset by peer \(code 300\)/.test(r.end), JSON.stringify(r));
      t.assert(/^rejected: stream reset by peer \(code 300\)/.test(r.closed), JSON.stringify(r));
    }),
    run("an alarm dials, and later requests keep using its stream", async (t) => {
      await t.fetch(`/arm?${t.vars.toB}&delay=200`);
      await t.poll(async () => {
        const s = (await t.fetch("/session")).json;
        t.assert(s.dialed?.reply === "echo:hello from the alarm", `the alarm has not dialed: ${JSON.stringify(s)}`);
        t.assert(s.open === true && s.closed === "pending", JSON.stringify(s));
      }, { timeoutMs: 30_000, why: "the alarm fires 200 ms after /arm, and dials" });
      for (const msg of ["ping", "pong"]) {
        const r = (await t.fetch(`/send?msg=${msg}`)).json;
        t.assert(r.reply === `echo:${msg}`, JSON.stringify(r));
      }
    }),
    run("an inbound stream outlives its handler: later requests read and write it", async (t) => {
      await t.fetch("/send?msg=keep:one&reply=0");
      await t.fetch("/send?msg=from-a&reply=0");
      const b = t.vars.b.origin;
      await t.poll(async () => {
        const pulled = (await t.fetch(`${b}/pull?name=one`)).json;
        t.assert(pulled.line === "from-a", JSON.stringify(pulled));
      }, { timeoutMs: 15_000, why: "the kept stream registers when the handler runs, on the first line after the hello" });
      await t.fetch(`${b}/push?name=one&msg=from-b`);
      const received = (await t.fetch("/recv")).json;
      t.assert(received.line === "pushed:from-b", JSON.stringify(received));
    }),
    run("closing the streams releases both objects", async (t) => {
      const hangup = (await t.fetch("/hangup?how=close")).json;
      t.assert(hangup.closed === "resolved", JSON.stringify(hangup));
      await t.fetch(`${t.vars.b.origin}/drop?name=one`);
      await t.poll(async () => {
        const a = (await t.fetch("/session")).json;
        t.assert(a.open === false, JSON.stringify(a));
      }, { timeoutMs: 15_000, why: "close() is applied by the node's own tasks" });
    }),
  ],
};

// ------------------------------------------------- scripts run as they are
// dolt and dolt-facets are scripted end to end (commit, branch, merge, fork,
// restart, restore) and egress runs three nodes against a fake network; their
// scripts start their own nodes, so these scenarios start none and run the
// script against the binary under test on a free port.
const needs = (command) => ({
  name: `command:${command}`,
  check: () =>
    new Promise((resolve) =>
      execFile("sh", ["-c", `command -v ${command}`], (error) =>
        resolve(error ? `${command} is not installed` : null),
      ),
    ),
});

function script(t, file, args, env = {}) {
  return new Promise((resolve, reject) => {
    execFile(
      file,
      args,
      {
        env: {
          ...process.env,
          ...env,
          CELLD: t.celld.bin,
          CELLD_ESBUILD: t.celld.esbuild,
        },
        timeout: 280_000,
        maxBuffer: 16 * 1024 * 1024,
      },
      (error, stdout, stderr) => {
        if (error) reject(new Error(`${path.basename(file)} failed:\n${stdout}\n${stderr}`));
        else resolve(stdout);
      },
    );
  });
}

const scripted = (example, title, command, { requires = [], slow = false } = {}) => ({
  example,
  title,
  slow,
  autostart: false,
  requires: [needs("curl"), ...requires],
  steps: [
    run(`the example's own script passes`, async (t) => {
      const port = String(await freePort());
      const { file, args } = command(path.join(EXAMPLES_DIR, example), t, port);
      await script(t, file, args, { PORT: port });
    }),
  ],
});

const dolt = scripted(
  "dolt",
  "dolt: commit, branch, merge, conflicts, reset, diff, fork, restart and restore (examples/dolt/demo.sh)",
  (dir, t) => ({ file: path.join(dir, "demo.sh"), args: [t.celld.bin] }),
  { requires: [needs("jq")] },
);
const doltFacets = scripted(
  "dolt-facets",
  "dolt facets: versioning, execute, facet fork, restart, restore and gc (examples/dolt-facets/demo.sh)",
  (dir, t) => ({ file: path.join(dir, "demo.sh"), args: [t.celld.bin] }),
  { requires: [needs("jq")] },
);
const egress = scripted(
  "egress",
  "egress: identity at the proxy for fetch, redirects, WebSocket, connect(), loaded workers; wrong secret; no proxy (examples/egress/test.mjs)",
  (dir, t, port) => ({
    file: process.execPath,
    args: [path.join(dir, "test.mjs"), "--celld", t.celld.bin, "--port", port],
  }),
);

export default [codegen, deadlinesDefault, deadlinesOff, residency, p2p, dolt, doltFacets, egress];
