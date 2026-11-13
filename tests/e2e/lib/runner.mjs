// Scenario execution: copies an example to scratch, runs `celld dev`, executes
// steps, and always cleans up.
import fs from "node:fs";
import path from "node:path";
import { Celld, sleep } from "./celld.mjs";
import { EXAMPLES_DIR, E2E_DIR } from "./env.mjs";
import { match, matchResponse, subst, normString, normValue } from "./match.mjs";
import { openWs } from "./ws.mjs";
import { checkRequirement, Skip } from "./tools.mjs";



class TestContext {
  constructor(celld, workdir, scenario) {
    this.celld = celld;
    this.workdir = workdir;
    this.scenario = scenario;
    this.vars = {};
    this.restoreCheck = null;
    this.cleanups = [];
  }
  get origin() {
    return this.celld.origin;
  }
  get port() {
    return this.celld.port;
  }
  get wsOrigin() {
    return `ws://127.0.0.1:${this.celld.port}`;
  }
  skip(reason) {
    throw new Skip(reason);
  }
  assert(cond, msg) {
    if (!cond) throw new Error(`assertion failed: ${msg}`);
  }
  sleep(ms) {
    return sleep(ms);
  }
  /** Register cleanup (runs in the finally path, in reverse order). */
  onCleanup(fn) {
    this.cleanups.push(fn);
  }
  /** Raw HTTP request; returns {status, headers, text, json?, bytes}. */
  async fetch(pathOrUrl, { method = "GET", headers = {}, body, json, timeoutMs = 30000 } = {}) {
    const url = pathOrUrl.startsWith("http") ? pathOrUrl : this.origin + pathOrUrl;
    const h = { ...headers };
    let payload = body;
    if (json !== undefined) {
      payload = JSON.stringify(json);
      if (!Object.keys(h).some((k) => k.toLowerCase() === "content-type")) h["content-type"] = "application/json";
    }
    const res = await fetch(url, {
      method,
      headers: h,
      body: payload,
      redirect: "manual",
      signal: AbortSignal.timeout(timeoutMs),
    });
    const bytes = Buffer.from(await res.arrayBuffer());
    const text = bytes.toString("utf8");
    let parsed;
    try {
      parsed = JSON.parse(text);
    } catch {}
    return { status: res.status, headers: Object.fromEntries(res.headers), text, json: parsed, bytes };
  }
  /** Assert a response against an expectation; throws with all mismatches. */
  expect(res, expectation, what = "response") {
    const errs = matchResponse(res, expectation, { port: this.port }, this.vars);
    if (errs.length) {
      throw new Error(`${what}: ${errs.join("; ")}\n  actual: status=${res.status} body=${JSON.stringify(res.text.slice(0, 400))}`);
    }
  }
  /** Deep-match an arbitrary value (normalised) against an expectation. */
  expectValue(value, expected, what = "value") {
    const errs = match(normValue(value, { port: this.port }), expected, "$", this.vars);
    if (errs.length) throw new Error(`${what}: ${errs.join("; ")}`);
  }
  async ws(pathOrUrl, opts) {
    const url = pathOrUrl.startsWith("ws") ? pathOrUrl : this.wsOrigin + pathOrUrl;
    const conn = await openWs(url, opts);
    this.onCleanup(() => conn.close());
    return conn;
  }
  /** Retry `fn` until it stops throwing. Bounded; say why in `why`. */
  async poll(fn, { timeoutMs = 10000, intervalMs = 250, why = "" } = {}) {
    const deadline = Date.now() + timeoutMs;
    let last;
    for (;;) {
      try {
        return await fn();
      } catch (e) {
        last = e;
      }
      if (Date.now() + intervalMs > deadline) {
        throw new Error(`condition not met within ${timeoutMs}ms${why ? ` (${why})` : ""}: ${last?.message ?? last}`);
      }
      await sleep(intervalMs);
    }
  }
  waitLog(re, o) {
    return this.celld.waitLog(re, o);
  }
  logText(o) {
    return this.celld.logText(o);
  }
  /** Lines of the whole run matching `re`. */
  logLines(re) {
    return this.celld.lines.filter((l) => re.test(l));
  }
}

function listRuntimeCells(celld) {
  const rt = path.join(celld.stateDir, "runtime");
  try {
    // Cells are directories; `.clean-reload.json` is a shutdown marker.
    return fs.readdirSync(rt, { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name);
  } catch {
    return [];
  }
}

async function execStep(t, step, opts) {
  const c = t.celld;
  switch (step.kind) {
    case "req": {
      const req = subst(step.req, t.vars);
      const attempt = async () => {
        const res = await t.fetch(req.path, req);
        const errs = matchResponse(res, subst(step.expect, t.vars), { port: t.port }, t.vars);
        if (errs.length) {
          throw new Error(`${errs.join("; ")}\n  actual: status=${res.status} body=${JSON.stringify(res.text.slice(0, 400))}`);
        }
        return res;
      };
      if (step.poll) await t.poll(attempt, step.poll);
      else await attempt();
      if (t.restoreCheck) {
        const { n } = t.restoreCheck;
        t.restoreCheck = null;
        await c.waitLog(/event="restore_plan"/, { timeoutMs: 5000, what: `restore_plan log for ${n} wiped cell(s)` });
      }
      return;
    }
    case "restart":
      await c.stop();
      await c.start();
      return;
    case "crash":
      await c.kill();
      await c.start();
      return;
    case "stop":
      await c.stop();
      return;
    case "start":
      await c.start();
      return;
    case "restore": {
      await c.stop();
      const cells = listRuntimeCells(c);
      fs.rmSync(path.join(c.stateDir, "runtime"), { recursive: true, force: true });
      if (!fs.existsSync(path.join(c.stateDir, "objects.sqlite3"))) {
        throw new Error("no replication log (objects.sqlite3) to restore from");
      }
      if (fs.existsSync(path.join(c.stateDir, "runtime"))) throw new Error("local runtime state was not wiped");
      await c.start();
      if (cells.length) t.restoreCheck = { n: cells.length };
      return;
    }
    case "waitLog":
      await c.waitLog(step.re, {
        timeoutMs: step.timeoutMs,
        fromIndex: step.session ? c.sessionStart : 0,
        what: `${step.re}${step.why ? ` (${step.why})` : ""}`,
      });
      return;
    case "noLog": {
      const hit = c.lines.find((l) => step.re.test(l));
      if (hit) throw new Error(`unexpected log line: ${hit.slice(0, 300)}`);
      return;
    }
    case "fn":
      await step.fn(t);
      return;
    default:
      throw new Error(`unknown step kind ${step.kind}`);
  }
}

/**
 * Run one scenario. Returns {id, status: pass|fail|skip, reason?, steps, ms}.
 * `status` is "xfail" when every failure was a declared knownFail.
 */
export async function runScenario(sc, env, opts = {}) {
  const t0 = Date.now();
  const id = sc.id;
  const result = { id, example: sc.example, status: "pass", steps: [], ms: 0 };
  const finish = (status, extra = {}) => {
    result.status = status;
    Object.assign(result, extra);
    result.ms = Date.now() - t0;
    return result;
  };

  if (sc.skip) return finish("skip", { reason: sc.skip });
  for (const req of sc.requires ?? []) {
    const why = await checkRequirement(req, env);
    if (why) return finish("skip", { reason: why });
  }

  const workdir = path.join(env.runDir, id.replaceAll("/", "__"));
  let celld = null;
  let t = null;
  try {
    fs.mkdirSync(workdir, { recursive: true });
    const src = path.join(EXAMPLES_DIR, sc.example);
    if (!fs.existsSync(src)) throw new Error(`no such example: ${sc.example}`);
    fs.cpSync(src, workdir, { recursive: true });
    if (sc.fixture) fs.cpSync(path.join(E2E_DIR, "fixtures", sc.fixture), workdir, { recursive: true });
    celld = new Celld({ bin: env.bin, dir: workdir, esbuild: env.esbuild, env: sc.env, name: id });
    t = new TestContext(celld, workdir, sc);
    try {
      if (sc.setup) await sc.setup(t);
    } catch (e) {
      if (e instanceof Skip) return finish("skip", { reason: e.message });
      throw e;
    }
    if (sc.autostart !== false) await celld.start();
    result.steps.push({ name: "start", status: "pass" });
    let failed = false;
    let xfailed = 0;
    for (const step of sc.steps) {
      const s0 = Date.now();
      const rec = { name: step.name, status: "pass" };
      try {
        await execStep(t, step, opts);
        if (step.knownFail) {
          rec.status = "xpass";
          rec.error = `known-failing step passed (${step.knownFail}); remove knownFail`;
          failed = true;
        }
      } catch (e) {
        if (e instanceof Skip) {
          rec.status = "skip";
          rec.error = e.message;
          result.steps.push(rec);
          return finish("skip", { reason: e.message });
        }
        if (step.knownFail) {
          rec.status = "xfail";
          rec.error = String(e.message ?? e);
          rec.known = step.knownFail;
          xfailed++;
        } else {
          rec.status = "fail";
          rec.error = String(e.message ?? e);
          failed = true;
        }
      }
      rec.ms = Date.now() - s0;
      result.steps.push(rec);
      if (rec.status === "fail") break;
    }
    // Panics are failures regardless of what the steps saw.
    const panic = celld.lines.find((l) => /panicked at|thread '.*' panicked|\bFATAL\b/.test(l));
    if (panic && !sc.allowPanic) {
      result.steps.push({ name: "no panics in celld log", status: "fail", error: panic.slice(0, 300) });
      failed = true;
    }
    // WARN/ERROR lines are reported (not failed on): they are leads for triage.
    result.warnings = [...new Set(celld.lines.filter((l) => /\b(WARN|ERROR)\b/.test(l)).map((l) => l.replace(/^\S+\s+/, "").slice(0, 260)))].slice(0, 12);
    return finish(failed ? "fail" : xfailed ? "xfail" : "pass");
  } catch (e) {
    result.steps.push({ name: "scenario", status: "fail", error: String(e?.stack ?? e) });
    return finish("fail");
  } finally {
    if (t) {
      for (const fn of t.cleanups.reverse()) {
        try {
          await fn();
        } catch {}
      }
    }
    if (celld) {
      await celld.stop({ signal: "SIGTERM", graceMs: 20000 }).catch(() => {});
      if (result.status === "fail" || result.status === "xfail") {
        const dest = path.join(env.artifactsDir, id.replaceAll("/", "__"));
        try {
          fs.rmSync(dest, { recursive: true, force: true });
          fs.mkdirSync(dest, { recursive: true });
          fs.copyFileSync(celld.logFile, path.join(dest, "celld.log"));
          result.log = path.join(dest, "celld.log");
        } catch {}
      }
    }
    if (!opts.keep) fs.rmSync(workdir, { recursive: true, force: true });
    else result.kept = workdir;
  }
}
