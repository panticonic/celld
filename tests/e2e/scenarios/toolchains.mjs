import crypto from "node:crypto";
import { get, post, restart, crash, restore, run, M } from "../lib/dsl.mjs";
import { Skip, preparePython, prepareWasm, npmInstall } from "../lib/tools.mjs";

// ---------------------------------------------------------------- python
// Python Worker (Pyodide) with a vendored package (beautifulsoup4) and KV.
// Toolchain: uv >= 0.13.4 and `uv run pywrangler sync` (bootstrapped into the
// tool cache, see lib/tools.mjs). The first build also downloads the Pyodide
// runtime into ~/.cache/celld.
const html = `<!doctype html><html><head><title>  Hi  There </title></head>
<body><h1>Head ✓</h1><h2>Sub</h2><a href="/a">a</a><a href="https://x.example/b?c=d">b</a><a>no href</a></body></html>`;
const sha = crypto.createHash("sha256").update(html).digest("hex");
const python = {
  example: "python",
  title: "Python Worker: BeautifulSoup summary, KV round trip, persistence",
  slow: true,
  setup: async (t) => preparePython(t.workdir),
  steps: [
    get("/", { status: 404, text: "POST HTML to /summarize\n" }),
    post("/summarize", { status: 200, headers: { "content-type": /^application\/json/ }, json: { sha256: sha, title: "Hi  There", headings: ["Head ✓", "Sub"], links: ["/a", "https://x.example/b?c=d"] } }, { body: html }),
    get(`/summary/${sha}`, { status: 200, headers: { "content-type": /^application\/json/ }, json: { title: "Hi  There", headings: ["Head ✓", "Sub"], links: ["/a", "https://x.example/b?c=d"] } }),
    get("/summary/0000", { status: 404, text: "not found" }),
    post("/summarize", { status: 200, json: { sha256: M.re(/^[0-9a-f]{64}$/), title: null, headings: [], links: [] } }, { body: "<p>no title</p>", name: "page without a title" }),
    post("/other", 404, { name: "POST to another path is 404", body: "x" }),
    restart(),
    get(`/summary/${sha}`, { status: 200, json: M.partial({ title: "Hi  There" }) }),
    crash(),
    get(`/summary/${sha}`, { status: 200, json: M.partial({ title: "Hi  There" }) }),
    restore(),
    get(`/summary/${sha}`, { status: 200, json: M.partial({ title: "Hi  There" }) }),
  ],
};

// ------------------------------------------------------------------ wasm
// A workers-rs Durable Object compiled to Wasm (worker-build). The toolchain
// (rustup + wasm32 target + worker-build) is bootstrapped privately into the
// tool cache the first time; the build output is cached by source hash.
const rust = (name, n) => get(`/c/${name}`, { status: 200, json: { name, n, lang: "rust" } });
const wasm = {
  example: "wasm",
  title: "workers-rs Durable Object in Wasm: counters; restart/crash/restore",
  slow: true,
  setup: async (t) => prepareWasm(t.workdir),
  steps: [
    get("/", { status: 200, text: "celld rust demo. Try: curl http://localhost:8080/c/hello\n" }),
    get("/other-path", { status: 404 }),
    rust("hello", 1),
    rust("hello", 2),
    rust("other", 1),
    restart(),
    rust("hello", 3),
    crash(),
    rust("hello", 4),
    rust("other", 2),
    restore(),
    rust("hello", 5),
    rust("other", 3),
  ],
};

// ------------------------------------------------------------- container
// A DO supervising a container (Docker/Podman). The Dockerfile is built by
// celld dev; python:3.12-alpine is pulled on first use. `npm install` brings
// @cloudflare/containers (cached in the tool cache).
const NODE_RE = /node=(dev-[0-9a-f]+)/;
const cjson = (path) => ({ hostname: M.capture("h" + path.replace(/\W/g, "_"), M.re(/^[0-9a-f]{12}$/)), path, message: "passed in by the Container class" });
const container = {
  example: "container",
  title: "DO supervising a Docker container: per-name containers, load balancing, cleanup",
  slow: true,
  requires: ["docker"],
  setup: async (t) => npmInstall(t.workdir),
  steps: [
    run("remember the node id so leftover containers are removed in cleanup", async (t) => {
      const { removeNodeContainers } = await import("../lib/tools.mjs");
      t.nodeIds = new Set();
      t.onCleanup(() => removeNodeContainers(t.nodeIds));
    }),
    get("/", { status: 200, text: "Try /container/<name>, /lb, or /singleton\n" }),
    // The first request builds/starts a container; allow time for it.
    get("/container/alpha", { status: 200, json: cjson("/container/alpha") }, { timeoutMs: 180000 }),
    get("/container/beta", { status: 200, json: cjson("/container/beta") }, { timeoutMs: 120000 }),
    run("each name has its own container and the same name reaches the same one", async (t) => {
      t.assert(t.vars.h_container_alpha !== t.vars.h_container_beta, "alpha and beta share a hostname");
      const again = await t.fetch("/container/alpha");
      t.expect(again, { status: 200, json: { hostname: t.vars.h_container_alpha, path: "/container/alpha", message: "passed in by the Container class" } });
    }),
    run("/lb spreads requests over at most three containers", async (t) => {
      const hosts = new Set();
      for (let i = 0; i < 12; i++) {
        const r = await t.fetch("/lb", { timeoutMs: 120000 });
        t.expect(r, { status: 200, json: { hostname: M.re(/^[0-9a-f]{12}$/), path: "/lb", message: "passed in by the Container class" } });
        hosts.add(r.json.hostname);
      }
      t.assert(hosts.size >= 1 && hosts.size <= 3, `expected 1-3 containers, saw ${hosts.size}`);
    }),
    run("lifecycle hook ran (onStart logged)", async (t) => {
      t.assert(t.logLines(/cell_console: container started/).length >= 2, "onStart was not logged");
      for (const l of t.logLines(NODE_RE)) t.nodeIds.add(l.match(NODE_RE)[1]);
    }),
    run("graceful stop removes the containers", async (t) => {
      const { containersOfNode } = await import("../lib/tools.mjs");
      for (const l of t.logLines(NODE_RE)) t.nodeIds.add(l.match(NODE_RE)[1]);
      await t.celld.stop();
      for (const id of t.nodeIds) t.expectValue(await containersOfNode(id), []);
    }),
    run("after a restart the object starts a fresh container and still answers", async (t) => {
      await t.celld.start();
      const r = await t.fetch("/container/alpha", { timeoutMs: 120000 });
      t.expect(r, { status: 200, json: { hostname: M.re(/^[0-9a-f]{12}$/), path: "/container/alpha", message: "passed in by the Container class" } });
      t.assert(r.json.hostname !== t.vars.h_container_alpha, "expected a new container after the restart");
    }),
  ],
};

// --------------------------------------------------------------- sandbox
const IMAGE = "docker.io/cloudflare/sandbox:0.12.9";
// The SDK may or may not keep the trailing newline of a stream.
const out = (text) => M.re(new RegExp("^" + text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "\\n?$"));
const sbx = (id, cmd) => `/exec?${id ? `id=${id}&` : ""}cmd=${encodeURIComponent(cmd)}`;
const sandbox = {
  example: "sandbox",
  title: "Sandbox SDK: exec, files, background processes; one container per sandbox id",
  slow: true,
  requires: ["docker"],
  setup: async (t) => {
    const { dockerImagePresent, dockerPull, bootstrapAllowed } = await import("../lib/tools.mjs");
    if (!(await dockerImagePresent(IMAGE))) {
      if (!bootstrapAllowed()) throw new Skip(`image ${IMAGE} not present and CELLD_E2E_BOOTSTRAP=0`);
      const r = await dockerPull(IMAGE);
      if (!r.ok) throw new Skip(`could not pull ${IMAGE}`);
    }
    await npmInstall(t.workdir);
  },
  steps: [
    run("track node ids for cleanup", async (t) => {
      const { removeNodeContainers } = await import("../lib/tools.mjs");
      t.nodeIds = new Set();
      t.onCleanup(() => removeNodeContainers(t.nodeIds));
    }),
    get(sbx("", "uname -s"), { status: 200, json: { stdout: out("Linux"), stderr: "", exitCode: 0 } }, { timeoutMs: 240000, name: "exec uname (cold start)" }),
    get(sbx("", "false"), { status: 200, json: { stdout: "", stderr: "", exitCode: 1 } }, { timeoutMs: 60000 }),
    get(sbx("", "sh -c 'echo out; echo err 1>&2; exit 3'"), { status: 200, json: { stdout: out("out"), stderr: out("err"), exitCode: 3 } }, { timeoutMs: 60000 }),
    get("/write", { status: 200, json: { content: M.re(/^written at \d{4}-\d\d-\d\dT[\d:.]+Z\n?$/) } }, { timeoutMs: 60000 }),
    get(sbx("", "cat /workspace/note.txt"), { status: 200, json: { stdout: M.re(/^written at /), stderr: "", exitCode: 0 } }, { timeoutMs: 60000, name: "the file is visible to exec" }),
    get(sbx("other", "ls /workspace"), { status: 200, json: { stdout: "", stderr: "", exitCode: 0 } }, { timeoutMs: 240000, name: "another sandbox id has an empty workspace" }),
    run("a background process outlives the request and serves HTTP inside the sandbox", async (t) => {
      const r = await t.fetch("/serve", { timeoutMs: 60000 });
      t.expect(r, { status: 200, json: { id: M.type("string"), status: M.type("string") } });
      await t.poll(
        async () => {
          const l = await t.fetch("/processes", { timeoutMs: 30000 });
          t.expect(l, { status: 200 });
          t.assert(l.json.some((p) => p.id === r.json.id), "process not listed");
        },
        { timeoutMs: 20000, intervalMs: 500, why: "process registration is asynchronous" },
      );
      await t.poll(async () => t.expect(await t.fetch(sbx("", "curl -s localhost:8787"), { timeoutMs: 30000 }), { status: 200, json: { stdout: "served from the sandbox", stderr: "", exitCode: 0 } }), {
        timeoutMs: 20000,
        intervalMs: 500,
        why: "node needs a moment to listen",
      });
    }),
    run("record node ids", async (t) => {
      for (const l of t.logLines(NODE_RE)) t.nodeIds.add(l.match(NODE_RE)[1]);
    }),
  ],
};

// --------------------------------------------------------------- pi, opencode
const pi = {
  example: "pi",
  title: "PiHarness agent loop in a DO",
  skip: "needs `npm install --legacy-peer-deps` of a preview build of Cloudflare Agents PR 2197 (can expire) and a live OpenAI-compatible model endpoint with an API key; answers are non-deterministic",
  steps: [],
};
const opencode = {
  example: "opencode",
  title: "OpenCode Workerd SDK in a DO",
  skip: "needs a live model provider on the internet (free model resolved at run time); the reply is non-deterministic and the SDK needs network access",
  steps: [],
};

export default [python, wasm, container, sandbox, pi, opencode];
