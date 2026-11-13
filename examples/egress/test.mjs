#!/usr/bin/env node
// Scripted run of the egress example against a real `celld dev` node.
//
//   node test.mjs [--celld PATH] [--port N]
//
// Three runs of the same worker:
//   1. proxy configured: fetch, redirects, outbound WebSocket and connect() from
//      a deployed Durable Object, a plain Worker and a dynamic worker all
//      arrive at the proxy with the right identity, to destinations that do not
//      resolve (so nothing can have gone direct); `globalOutbound: null` blocks
//      everything.
//   2. proxy configured with the wrong secret: the proxy refuses.
//   3. no proxy configured: traffic goes straight to the origin, with no
//      identity headers, and Deny still blocks.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { startServer } from "./fake-net.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const option = (name, fallback) => {
  const at = args.indexOf(name);
  return at >= 0 ? args[at + 1] : fallback;
};
const celld = option("--celld", process.env.CELLD ?? "celld");
const port = Number(option("--port", "19876"));
const SECRET = "egress-example-secret-0123456789";
const base = `http://127.0.0.1:${port}`;

let failures = 0;
async function check(name, fn) {
  try {
    await fn();
    console.log(`ok    ${name}`);
  } catch (error) {
    failures++;
    console.log(`FAIL  ${name}\n      ${String(error.stack ?? error).replaceAll("\n", "\n      ")}`);
  }
}

async function withNode(env, run) {
  const child = spawn(celld, ["dev", here, "--port", String(port), "--no-watch", "--clean"], {
    env: { ...process.env, ...env },
    stdio: ["ignore", "inherit", "inherit"],
  });
  const exited = new Promise((resolve) => child.once("exit", resolve));
  try {
    for (let attempt = 0; ; attempt++) {
      if (child.exitCode !== null) throw new Error(`celld dev exited with ${child.exitCode}`);
      try {
        if ((await fetch(`${base}/`)).ok) break;
      } catch {}
      if (attempt > 600) throw new Error("celld dev did not become ready");
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
    await run();
  } finally {
    child.kill("SIGTERM");
    await Promise.race([exited, new Promise((resolve) => setTimeout(resolve, 10_000))]);
    child.kill("SIGKILL");
  }
}

const call = async (path) => (await fetch(`${base}${path}`)).json();
const probe = (where, op, params) =>
  call(`${where}?op=${op}&${new URLSearchParams(params)}`);

async function run() {
  const proxy = await startServer({ role: "proxy", secret: SECRET });
  const origin = await startServer({ role: "origin", secret: SECRET });
  const proxyEnv = {
    CELLD_EGRESS_PROXY: `http://127.0.0.1:${proxy.port}`,
    CELLD_EGRESS_SECRET: SECRET,
  };
  const take = () => proxy.records.splice(0);

  console.log("== 1. proxy configured");
  await withNode(proxyEnv, async () => {
    let deployment;
    // One table drives all three isolate kinds, so each is held to the same
    // checks: where the code runs, and what its identity must say.
    const kinds = [
      {
        name: "deployed Durable Object",
        where: "/do",
        identity: (id) => {
          assert.equal(id.class, "Probe");
          assert.match(id.cell, /^Probe:/);
          assert.equal(id.loaded, false);
          assert.deepEqual(id.egressIdentity, []);
        },
      },
      {
        name: "deployed plain Worker",
        where: "/plain",
        identity: (id) => {
          assert.equal(id.cell, null);
          assert.equal(id.class, null);
          assert.equal(id.loaded, false);
          assert.deepEqual(id.egressIdentity, []);
        },
      },
      {
        name: "dynamic worker",
        where: "/dynamic",
        identity: (id) => {
          assert.equal(id.loaded, true);
          assert.equal(id.cell, null);
          assert.deepEqual(id.egressIdentity, ["tenant-a"]);
        },
      },
    ];
    for (const kind of kinds) {
      const common = (id) => {
        assert.equal(id.v, 1);
        assert.ok(id.script, "script");
        assert.ok(id.deployment, "deployment");
        deployment ??= id.deployment;
        assert.equal(id.deployment, deployment);
        kind.identity(id);
      };
      await check(`${kind.name}: fetch (http and https) carries identity`, async () => {
        take();
        for (const url of ["http://origin.test/hello?x=1", "https://secure.test/path"]) {
          const result = await probe(kind.where, "fetch", { url });
          assert.deepEqual(result, { ok: true, status: 200, body: `served:${url}` });
        }
        const records = take();
        assert.deepEqual(
          records.map((r) => [r.kind, r.method, r.target, r.authenticated]),
          [
            ["fetch", "GET", "http://origin.test/hello?x=1", true],
            ["fetch", "GET", "https://secure.test/path", true],
          ],
        );
        records.forEach((r) => common(r.identity));
      });
      await check(`${kind.name}: redirects go back through the proxy`, async () => {
        take();
        const result = await probe(kind.where, "fetch", { url: "http://origin.test/redirect" });
        assert.equal(result.body, "served:http://landing.test/landed");
        const records = take();
        assert.deepEqual(records.map((r) => r.target), [
          "http://origin.test/redirect",
          "http://landing.test/landed",
        ]);
        records.forEach((r) => common(r.identity));
      });
      await check(`${kind.name}: a Worker cannot forge identity headers`, async () => {
        take();
        const result = await probe(kind.where, "fetch", { url: "http://origin.test/f", forge: "1" });
        assert.equal(result.ok, true);
        const [record] = take();
        assert.equal(record.authenticated, true);
        common(record.identity);
      });
      for (const [op, label, url] of [
        ["ws", "outbound WebSocket (constructor)", "ws://origin.test/socket"],
        ["wsfetch", "outbound WebSocket (fetch upgrade)", "http://origin.test/socket2"],
      ]) {
        await check(`${kind.name}: ${label}`, async () => {
          take();
          const result = await probe(kind.where, op, { url });
          assert.deepEqual(result, { ok: true, body: "echo:hello" });
          const [record] = take();
          assert.equal(record.kind, "websocket");
          assert.equal(record.target, url.replace("http://", "ws://"));
          assert.equal(record.authenticated, true);
          common(record.identity);
        });
      }
      await check(`${kind.name}: connect()`, async () => {
        take();
        const result = await probe(kind.where, "tcp", { host: "db.test", port: "5432" });
        assert.deepEqual(result, { ok: true, body: "echo:ping" });
        const [record] = take();
        assert.equal(record.kind, "connect");
        assert.equal(record.target, "db.test:5432");
        assert.equal(record.authenticated, true);
        common(record.identity);
      });
    }

    await check("deny: a dynamic worker with globalOutbound null reaches nothing", async () => {
      take();
      for (const [op, params] of [
        ["fetch", { url: "http://origin.test/denied" }],
        ["ws", { url: "ws://origin.test/denied" }],
        ["wsfetch", { url: "http://origin.test/denied" }],
        ["tcp", { host: "db.test", port: "5432" }],
      ]) {
        const result = await probe("/dynamic", op, { ...params, deny: "1" });
        assert.equal(result.ok, false, op);
        assert.match(result.error, /not permitted to access the internet/, op);
      }
      assert.deepEqual(take(), []);
    });
  });

  console.log("== 2. proxy configured with the wrong secret");
  await withNode({ ...proxyEnv, CELLD_EGRESS_SECRET: "a-different-secret-0123456789" }, async () => {
    await check("the proxy refuses an identity it cannot authenticate", async () => {
      take();
      const result = await probe("/do", "fetch", { url: "http://origin.test/hello" });
      assert.equal(result.status, 403);
      assert.equal(result.body, "egress: unattributed request");
      const [record] = take();
      assert.equal(record.authenticated, false);
    });
  });

  console.log("== 3. no proxy configured");
  await withNode({ CELLD_EGRESS_PROXY: "", CELLD_EGRESS_SECRET: "" }, async () => {
    const o = `127.0.0.1:${origin.port}`;
    await check("direct fetch, redirect, WebSocket and connect() are unchanged", async () => {
      origin.records.length = 0;
      for (const where of ["/do", "/plain", "/dynamic"]) {
        assert.deepEqual(await probe(where, "fetch", { url: `http://${o}/hello` }), {
          ok: true,
          status: 200,
          body: `served:http://${o}/hello`,
        });
        assert.equal((await probe(where, "fetch", { url: `http://${o}/redirect` })).body, `served:http://${o}/landed`);
        assert.equal((await probe(where, "ws", { url: `ws://${o}/socket` })).body, "echo:hello");
        assert.equal((await probe(where, "wsfetch", { url: `http://${o}/socket` })).body, "echo:hello");
        assert.equal(
          (await probe(where, "tcp", { host: "127.0.0.1", port: String(origin.tcpPort) })).body,
          "echo:ping",
        );
      }
      // Nothing about identity appears on a direct request.
      assert.ok(origin.records.length >= 15);
      assert.ok(origin.records.every((r) => r.identityHeaderSeen !== true));
      assert.deepEqual(take(), []);
    });
    await check("deny still blocks without a proxy", async () => {
      origin.records.length = 0;
      for (const [op, params] of [
        ["fetch", { url: `http://${o}/denied` }],
        ["ws", { url: `ws://${o}/denied` }],
        ["wsfetch", { url: `http://${o}/denied` }],
        ["tcp", { host: "127.0.0.1", port: String(origin.tcpPort) }],
      ]) {
        const result = await probe("/dynamic", op, { ...params, deny: "1" });
        assert.equal(result.ok, false, op);
        assert.match(result.error, /not permitted to access the internet/, op);
      }
      assert.deepEqual(origin.records, []);
    });
  });

  proxy.close();
  origin.close();
}

await run();
console.log(failures === 0 ? "\nall checks passed" : `\n${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
