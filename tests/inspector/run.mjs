// V8 inspector end-to-end test. Runs `celld dev --inspector-addr` on
// tests/inspector/fixture and drives it with a Chrome DevTools Protocol client
// built on Node 22's WebSocket: target discovery, Runtime.evaluate, a CPU
// profile, a heap snapshot, a breakpoint (and a client that vanishes while the
// isolate is paused), checking after each that the node still serves.
//
//   CELLD_BIN=/path/to/celld node tests/inspector/run.mjs
//
// Needs Node 22 and esbuild (see tests/e2e/README.md for how it is found).
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Celld, freePort } from "../e2e/lib/celld.mjs";
import { resolveBin, resolveEsbuild, scratchRoot } from "../e2e/lib/env.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const failures = [];

async function step(name, fn) {
  const started = Date.now();
  try {
    await fn();
    console.log(`ok   ${name} (${Date.now() - started} ms)`);
  } catch (error) {
    failures.push(name);
    console.log(`FAIL ${name}: ${error?.stack ?? error}`);
  }
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function poll(fn, { timeoutMs = 15000, why }) {
  const deadline = Date.now() + timeoutMs;
  let last;
  for (;;) {
    try {
      const value = await fn();
      if (value) return value;
    } catch (error) {
      last = error;
    }
    if (Date.now() > deadline) throw new Error(`timed out: ${why}${last ? ` (${last})` : ""}`);
    await sleep(100);
  }
}

/** A minimal CDP client. */
class Cdp {
  constructor(url) {
    this.url = url;
    this.nextId = 1;
    this.pending = new Map();
    this.events = [];
    this.waiters = new Set();
  }
  open() {
    return new Promise((resolve, reject) => {
      this.ws = new WebSocket(this.url);
      this.ws.onopen = () => resolve(this);
      this.ws.onerror = (event) => reject(new Error(`websocket error: ${event.message ?? event}`));
      this.ws.onclose = () => {
        this.closed = true;
        for (const { reject } of this.pending.values()) reject(new Error("socket closed"));
        this.pending.clear();
      };
      this.ws.onmessage = ({ data }) => {
        const message = JSON.parse(data);
        if (message.id !== undefined) {
          const call = this.pending.get(message.id);
          this.pending.delete(message.id);
          if (message.error) call.reject(new Error(`${call.method}: ${JSON.stringify(message.error)}`));
          else call.resolve(message.result);
        } else {
          this.events.push(message);
          for (const waiter of [...this.waiters]) waiter(message);
        }
      };
    });
  }
  send(method, params = {}) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject, method });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }
  /** The next (or an already received, unconsumed) event named `method`. */
  event(method, { timeoutMs = 15000, match = () => true } = {}) {
    const index = this.events.findIndex((e) => e.method === method && match(e.params));
    if (index >= 0) return Promise.resolve(this.events.splice(index, 1)[0].params);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.waiters.delete(waiter);
        reject(new Error(`timed out waiting for ${method}`));
      }, timeoutMs);
      const waiter = (message) => {
        if (message.method === method && match(message.params)) {
          clearTimeout(timer);
          this.waiters.delete(waiter);
          this.events.splice(this.events.indexOf(message), 1);
          resolve(message.params);
        }
      };
      this.waiters.add(waiter);
    });
  }
  close() {
    this.ws.close();
  }
}

const bin = resolveBin();
const esbuild = resolveEsbuild();
const work = fs.mkdtempSync(path.join(scratchRoot(), "inspector-"));
fs.cpSync(path.join(here, "fixture"), work, { recursive: true });
const httpPort = await freePort();
const inspectorPort = await freePort();
const node = new Celld({ bin, dir: work, esbuild, name: "inspector", env: {} });
node.port = httpPort;
const origin = `http://127.0.0.1:${httpPort}`;
const inspector = `http://127.0.0.1:${inspectorPort}`;

async function get(pathname, { timeoutMs = 20000 } = {}) {
  const response = await fetch(origin + pathname, { signal: AbortSignal.timeout(timeoutMs) });
  return { status: response.status, text: await response.text() };
}

async function listTargets() {
  const response = await fetch(`${inspector}/json/list`);
  assert(response.ok, `/json/list answered ${response.status}`);
  return response.json();
}

let cellTarget;
try {
  await node.start({ args: ["--inspector-addr", `127.0.0.1:${inspectorPort}`] });

  await step("/json/version names celld and the protocol", async () => {
    const version = await (await fetch(`${inspector}/json/version`)).json();
    assert(/^celld\//.test(version.Browser), JSON.stringify(version));
    assert(version["Protocol-Version"], "no Protocol-Version");
    assert(version["V8-Version"], "no V8-Version");
    const alias = await (await fetch(`${inspector}/json`)).json();
    assert(Array.isArray(alias), "/json is not a list");
  });

  await step("a request with a foreign Host header is refused", async () => {
    const response = await fetch(`${inspector}/json/list`, {
      headers: { host: "evil.example" },
    }).catch(() => null);
    // Node's fetch forbids overriding Host on some versions; fall back to a raw socket.
    if (response && response.status === 200) {
      const net = await import("node:net");
      const status = await new Promise((resolve, reject) => {
        const socket = net.connect(inspectorPort, "127.0.0.1", () =>
          socket.write("GET /json/list HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n"),
        );
        let data = "";
        socket.on("data", (chunk) => (data += chunk));
        socket.on("end", () => resolve(data.split(" ")[1]));
        socket.on("error", reject);
      });
      assert(status === "403", `foreign Host answered ${status}`);
    } else {
      assert(response === null || response.status === 403, "foreign Host was not refused");
    }
  });

  await step("traffic creates targets: cell isolate and dynamic worker", async () => {
    assert((await get("/ping")).text === "pong", "ping");
    for (const name of ["a", "a", "b"]) {
      const answer = await get(`/counter?name=${name}`);
      assert(answer.status === 200, `counter ${name}: ${answer.status} ${answer.text}`);
    }
    assert((await get("/dyn")).text === "dynamic ok", "dynamic worker");
    const targets = await poll(
      async () => {
        const list = await listTargets();
        return list.some((t) => t.id.startsWith("core:dynamic:")) &&
          list.some((t) => /shared by \d+ cell/.test(t.title))
          ? list
          : null;
      },
      { why: "both a cell target and a dynamic-worker target are listed" },
    );
    for (const target of targets) {
      assert(target.id && target.title && target.type, `incomplete target ${JSON.stringify(target)}`);
      const url = new URL(target.webSocketDebuggerUrl);
      assert(url.pathname.slice(1) === target.id, `webSocketDebuggerUrl does not end in the id: ${target.webSocketDebuggerUrl}`);
    }
    cellTarget = targets.find((t) => /shared by \d+ cell/.test(t.title));
    console.log(`     targets: ${targets.map((t) => `\n       ${t.id}  ${t.title}`).join("")}`);
    assert(/isolate/.test(cellTarget.title) && /Counter/.test(cellTarget.title), cellTarget.title);
  });

  await step("an unknown target is 404", async () => {
    const response = await fetch(`${inspector}/core:user:nope:isolate-0`);
    assert(response.status === 404, `status ${response.status}`);
  });

  const cdp = await new Cdp(cellTarget.webSocketDebuggerUrl).open();

  await step("Runtime.evaluate runs in the isolate", async () => {
    await cdp.send("Runtime.enable");
    const result = await cdp.send("Runtime.evaluate", { expression: "6 * 7", returnByValue: true });
    assert(result.result.value === 42, JSON.stringify(result));
    const text = await cdp.send("Runtime.evaluate", { expression: "'héllo ✓'", returnByValue: true });
    assert(text.result.value === "héllo ✓", `non-ASCII round trip: ${JSON.stringify(text)}`);
    const thrown = await cdp.send("Runtime.evaluate", { expression: "(() => { throw new Error('boom'); })()" });
    assert(thrown.exceptionDetails, "no exceptionDetails");
    await cdp.send("Runtime.runIfWaitingForDebugger");
  });

  await step("the node serves while a session is attached", async () => {
    assert((await get("/counter?name=b")).status === 200, "cell b");
    assert((await get("/ping")).text === "pong", "ping");
  });

  await step("Profiler.start/stop returns a CPU profile with samples", async () => {
    await cdp.send("Profiler.enable");
    await cdp.send("Profiler.start");
    for (let i = 0; i < 12; i++) assert((await get("/counter?name=a")).status === 200, "counter under profile");
    const { profile } = await cdp.send("Profiler.stop");
    assert(profile.nodes.length > 1, `profile has ${profile.nodes.length} nodes`);
    assert(profile.samples.length > 0, "profile has no samples");
    const byId = new Map(profile.nodes.map((n) => [n.id, n]));
    const named = new Map();
    for (const id of profile.samples) {
      const fn = byId.get(id).callFrame.functionName || "(anonymous)";
      named.set(fn, (named.get(fn) ?? 0) + 1);
    }
    const top = [...named].sort((a, b) => b[1] - a[1]).slice(0, 5);
    console.log(`     ${profile.samples.length} samples over ${profile.nodes.length} nodes; top: ${JSON.stringify(top)}`);
    const user = profile.nodes.filter((n) => n.callFrame.url && /index|worker/.test(n.callFrame.url));
    assert(user.length > 0, "no sample was attributed to a script frame (samples were taken on the wrong thread?)");
    await cdp.send("Profiler.disable");
  });

  await step("HeapProfiler.takeHeapSnapshot streams a parseable snapshot", async () => {
    await cdp.send("HeapProfiler.enable");
    const chunks = [];
    const collect = ({ data }) => chunks.push(data);
    const onChunk = (event) => {
      if (event.method === "HeapProfiler.addHeapSnapshotChunk") collect(event.params);
    };
    // Chunks arrive as events before the response; drain them from the buffer.
    await cdp.send("HeapProfiler.takeHeapSnapshot", { reportProgress: false });
    for (const event of cdp.events.splice(0)) onChunk(event);
    const snapshot = JSON.parse(chunks.join(""));
    assert(snapshot.snapshot?.meta?.node_fields?.length > 0, "snapshot has no meta");
    assert(snapshot.nodes.length > 1000, `snapshot has ${snapshot.nodes.length} node fields`);
    console.log(`     ${chunks.length} chunks, ${(chunks.join("").length / 1e6).toFixed(1)} MB, ${snapshot.snapshot.node_count} nodes`);
    await cdp.send("HeapProfiler.disable");
  });

  let breakpointLine;
  let scriptId;
  await step("Debugger finds the script and sets a breakpoint", async () => {
    await cdp.send("Debugger.enable");
    const parsed = [];
    for (const event of cdp.events.splice(0)) {
      if (event.method === "Debugger.scriptParsed") parsed.push(event.params);
    }
    // The worker bundle is the script whose source holds the fixture's counter.
    for (const script of parsed) {
      if (!script.url || /^(node:|cloudflare:|internal)/.test(script.url)) continue;
      const { scriptSource } = await cdp.send("Debugger.getScriptSource", { scriptId: script.scriptId });
      const lines = scriptSource.split("\n");
      const line = lines.findIndex((l) => /\bn\+\+/.test(l));
      if (line >= 0 && /Counter/.test(scriptSource)) {
        scriptId = script.scriptId;
        breakpointLine = line;
        break;
      }
    }
    assert(scriptId !== undefined, `no script containing the fixture was reported (${parsed.map((s) => s.url).join(", ")})`);
  });

  await step("a breakpoint pauses the isolate; other isolates and the node keep serving", async () => {
    const { breakpointId } = await cdp.send("Debugger.setBreakpoint", {
      location: { scriptId, lineNumber: breakpointLine },
    });
    const hit = get("/counter?name=a", { timeoutMs: 40000 });
    const paused = await cdp.event("Debugger.paused", { timeoutMs: 20000 });
    assert(paused.callFrames.length > 0, "paused without frames");
    const titles = (await listTargets()).map((t) => t.title);
    assert(titles.some((t) => t.endsWith("[paused]")), `no target is marked paused: ${titles.join(" | ")}`);
    // Evaluate in the paused frame.
    const frame = await cdp.send("Debugger.evaluateOnCallFrame", {
      callFrameId: paused.callFrames[0].callFrameId,
      expression: "typeof n",
      returnByValue: true,
    });
    assert(frame.result.value === "number", `evaluateOnCallFrame: ${JSON.stringify(frame)}`);
    // The dynamic worker has its own isolate: it must answer while this one is paused.
    const dynamic = await get("/dyn", { timeoutMs: 10000 });
    assert(dynamic.text === "dynamic ok", "the dynamic worker stalled while another isolate was paused");
    // The /json endpoints are served by the host, not by the paused isolate.
    assert((await listTargets()).length >= 2, "target list unavailable while paused");
    let resumed = cdp.event("Debugger.resumed");
    await cdp.send("Debugger.removeBreakpoint", { breakpointId });
    await cdp.send("Debugger.resume");
    await resumed;
    const answer = await hit;
    assert(answer.status === 200, `the paused request finished with ${answer.status}`);
  });

  await step("cells in the paused isolate resume after Debugger.resume", async () => {
    await cdp.send("Debugger.disable");
    for (const name of ["a", "b"]) {
      const answer = await get(`/counter?name=${name}`);
      assert(answer.status === 200, `cell ${name}: ${answer.status}`);
    }
  });

  await step("a debugger that disconnects while paused releases the isolate", async () => {
    await cdp.send("Debugger.enable");
    await cdp.send("Debugger.setBreakpoint", { location: { scriptId, lineNumber: breakpointLine } });
    const hit = get("/counter?name=b", { timeoutMs: 40000 });
    await cdp.event("Debugger.paused", { timeoutMs: 20000 });
    cdp.close();
    const answer = await hit;
    assert(answer.status === 200, `request after disconnect: ${answer.status}`);
    assert((await get("/counter?name=a")).status === 200, "cell a after disconnect");
  });

  await step("a second session can attach after the first left", async () => {
    const again = await new Cdp(cellTarget.webSocketDebuggerUrl).open();
    const result = await again.send("Runtime.evaluate", { expression: "1 + 1", returnByValue: true });
    assert(result.result.value === 2, JSON.stringify(result));
    again.close();
  });
} finally {
  await node.stop();
  fs.rmSync(work, { recursive: true, force: true });
}

if (failures.length) {
  console.log(`\n${failures.length} step(s) failed: ${failures.join("; ")}`);
  process.exit(1);
}
console.log("\ninspector: all steps passed");
