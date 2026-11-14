// A code swap does not wait out residency holds (docs/services/residency.md,
// "Generation swaps"). A standalone node serves the residency example from a
// vibestore bucket on disk; a held object is on v1, v2 is deployed and
// `POST /reload` adopts it. The object's next answer comes from v2 within a
// few seconds, with `onrevoked` run ("swap"), where it used to wait for
// CELLD_DEPLOY_MAX_AGE_S (default 60). The node keeps that default here, so a
// swap that waited would take a minute and fail the bound.
//
// The second scenario step is the case that must not change: a request still
// in flight keeps its hold until it has finished, and the holder is told after.
import { execFile, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { freePort } from "../lib/celld.mjs";
import { EXAMPLES_DIR } from "../lib/env.mjs";
import { run } from "../lib/dsl.mjs";

const SWAP_BOUND_MS = 8000;

const sh = (file, args, options) =>
  new Promise((resolve, reject) => {
    execFile(file, args, { timeout: 120_000, ...options }, (error, stdout, stderr) => {
      if (error) reject(new Error(`${path.basename(file)} ${args.join(" ")} failed:\n${stdout}\n${stderr}`));
      else resolve(stdout);
    });
  });

export default {
  example: "residency",
  id: "residency-swap",
  title: "a generation swap revokes a held cell's holds at once and does not wait for the maximum age; a request in flight is still waited for",
  autostart: false,
  async setup(t) {
    const dir = `${t.workdir}-swap`;
    const app = path.join(dir, "app");
    fs.mkdirSync(app, { recursive: true });
    for (const file of ["index.js", "wrangler.jsonc"]) {
      fs.copyFileSync(path.join(EXAMPLES_DIR, "residency", file), path.join(app, file));
    }
    const bucket = `vibestore://${dir}/blobs?index=${dir}/index.db`;
    const env = { ...process.env, CELLD_ESBUILD: t.celld.esbuild, NO_COLOR: "1" };
    const port = await freePort();
    const internal = await freePort();
    const node = { lines: [], child: null };
    t.vars.swap = {
      origin: `http://127.0.0.1:${port}`,
      internal: `http://127.0.0.1:${internal}`,
      app,
      node,
      async deploy(version) {
        const file = path.join(app, "index.js");
        fs.writeFileSync(file, fs.readFileSync(file, "utf8").replace(/const VERSION = "[^"]*";/, `const VERSION = "${version}";`));
        await sh(t.celld.bin, ["deploy", ".", "--bucket", bucket], { cwd: app, env });
      },
      async start() {
        const child = spawn(
          t.celld.bin,
          [
            "--bucket", bucket,
            "--listen", `127.0.0.1:${port}`,
            "--internal-listen", `127.0.0.1:${internal}`,
            "--advertise", `127.0.0.1:${internal}`,
            "--lease-ttl-ms", "4000",
          ],
          { env: { ...env, CELLD_WATCH: path.join(dir, "node"), RUST_LOG: "info" }, stdio: ["ignore", "pipe", "pipe"] },
        );
        node.child = child;
        node.exited = new Promise((resolve) => child.once("exit", resolve));
        let tail = "";
        const onData = (chunk) => {
          const parts = (tail + chunk.toString("utf8")).split("\n");
          tail = parts.pop();
          node.lines.push(...parts);
        };
        child.stdout.on("data", onData);
        child.stderr.on("data", onData);
      },
      async reload() {
        const r = await t.fetch(`http://127.0.0.1:${internal}/reload`, { method: "POST", timeoutMs: 60_000 });
        t.assert(r.status === 200 && r.json?.outcome === "adopted", `reload: ${r.status} ${JSON.stringify(r.json ?? r.text)}`);
      },
    };
    t.onCleanup(async () => {
      // This node is ours: SIGKILL it by pid and wait for the exit.
      if (node.child && node.child.exitCode === null) {
        node.child.kill("SIGKILL");
        await node.exited;
      }
      if (process.env.CELLD_E2E_SWAP_LOG) fs.writeFileSync(process.env.CELLD_E2E_SWAP_LOG, node.lines.join("\n"));
      fs.rmSync(dir, { recursive: true, force: true });
    });
  },
  steps: [
    run("a held object on v1, with state only memory holds", async (t) => {
      const s = t.vars.swap;
      await s.deploy("v1");
      await s.start();
      await t.poll(async () => {
        const r = await t.fetch(`${s.origin}/inc?name=idle`, { timeoutMs: 5000 });
        t.assert(r.status === 200, `status ${r.status}`);
      }, { timeoutMs: 60_000, why: "the node starts and adopts the deployment" });
      for (let i = 0; i < 3; i++) await t.fetch(`${s.origin}/inc?name=idle`);
      const held = (await t.fetch(`${s.origin}/hold?name=idle`)).json;
      t.assert(held.held === true && held.version === "v1" && held.counter >= 3, JSON.stringify(held));
      t.vars.idleBorn = held.bornAt;
      t.vars.idleCounter = held.counter;
    }),

    run("deploy v2 and reload: the swap completes in seconds and the holder was told", async (t) => {
      const s = t.vars.swap;
      await s.deploy("v2");
      const started = Date.now();
      await s.reload();
      // A request during the swap waits for the new code, so its answer is the
      // end of the swap.
      const after = (await t.fetch(`${s.origin}/get?name=idle`, { timeoutMs: 40_000 })).json;
      const swapMs = Date.now() - started;
      t.assert(after.version === "v2", `still on ${after.version}`);
      t.assert(swapMs < SWAP_BOUND_MS, `the swap took ${swapMs} ms; it waited for the maximum age (60 s)`);
      t.assert(after.revoked?.reason === "swap" && after.revoked?.counter === t.vars.idleCounter,
        `onrevoked did not run as the swap's: ${JSON.stringify(after.revoked)}`);
      t.assert(after.held === false && after.bornAt !== t.vars.idleBorn && after.counter === 0,
        `the object was not rebuilt on the new generation: ${JSON.stringify(after)}`);
      console.log(`    swap of a held object: ${swapMs} ms`);
    }),

    run("a request in flight is not cut short, and the holder is told after it", async (t) => {
      const s = t.vars.swap;
      const name = "busy";
      await t.fetch(`${s.origin}/inc?name=${name}`);
      const held = (await t.fetch(`${s.origin}/hold?name=${name}`)).json;
      t.assert(held.held === true && held.version === "v2", JSON.stringify(held));
      const slow = t.fetch(`${s.origin}/slow?name=${name}&ms=3000`, { timeoutMs: 40_000 });
      slow.catch(() => {});
      await t.poll(async () => {
        const r = (await t.fetch(`${s.origin}/get?name=${name}`)).json;
        t.assert(r.slowStarted === 1 && r.slowDone === 0, `the slow request is not in flight: ${JSON.stringify(r)}`);
      }, { timeoutMs: 10_000, why: "the slow request has to reach its handler" });

      await s.deploy("v3");
      const started = Date.now();
      await s.reload();
      const finished = (await slow).json;
      t.assert(finished.version === "v2" && finished.slowDone === 1,
        `the request in flight did not finish on the code it started on: ${JSON.stringify(finished)}`);
      const after = (await t.fetch(`${s.origin}/get?name=${name}`, { timeoutMs: 40_000 })).json;
      const swapMs = Date.now() - started;
      t.assert(after.version === "v3", `still on ${after.version}`);
      // The handler's record carries what the request had done when it ran.
      t.assert(after.revoked?.reason === "swap" && after.revoked?.slowDone === 1,
        `the holder was told before the request had finished: ${JSON.stringify(after.revoked)}`);
      t.assert(swapMs < 3000 + SWAP_BOUND_MS, `the swap took ${swapMs} ms`);
    }),
  ],
};
