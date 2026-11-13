// One `celld dev` process per project directory, restartable.
import { spawn } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";

const live = new Set();

// Last-resort cleanup so an aborted run never leaks celld processes.
function killAllSync() {
  for (const c of live) {
    try {
      c.child?.kill("SIGKILL");
    } catch {}
  }
}
process.on("exit", killAllSync);
for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(sig, () => {
    killAllSync();
    process.exit(130);
  });
}

export function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.once("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export class Celld {
  /**
   * @param {{bin:string, dir:string, esbuild:string, env?:Record<string,string>, name:string}} o
   */
  constructor(o) {
    this.bin = o.bin;
    this.dir = o.dir;
    this.esbuild = o.esbuild;
    this.extraEnv = o.env ?? {};
    this.name = o.name;
    this.port = null;
    this.child = null;
    this.exit = null; // {code, signal} of the last process
    this.lines = []; // every log line of every session, in order
    this.sessionStart = 0; // index into lines where the current session began
    this.sessions = 0;
    this.waiters = new Set();
    this.logFile = path.join(this.dir, "celld.e2e.log");
    fs.writeFileSync(this.logFile, "");
  }

  get origin() {
    return `http://127.0.0.1:${this.port}`;
  }
  get running() {
    return this.child !== null && this.exit === null;
  }
  get stateDir() {
    return path.join(this.dir, ".celld", "dev");
  }

  /** Log text of the whole run, or only the current process (`session`). */
  logText({ session = false } = {}) {
    return this.lines.slice(session ? this.sessionStart : 0).join("\n");
  }

  _onData(chunk, tail) {
    const text = tail.buf + chunk.toString("utf8");
    const parts = text.split("\n");
    tail.buf = parts.pop();
    for (const line of parts) this._pushLine(line);
  }
  _pushLine(line) {
    this.lines.push(line);
    fs.appendFileSync(this.logFile, line + "\n");
    for (const w of [...this.waiters]) w(line);
  }

  /**
   * Resolve when a log line matching `re` appears (searching lines already
   * seen from `fromIndex`, then new ones). Bounded: rejects after `timeoutMs`.
   */
  waitLog(re, { timeoutMs = 10000, fromIndex = this.sessionStart, what = String(re) } = {}) {
    for (let i = fromIndex; i < this.lines.length; i++) {
      if (re.test(this.lines[i])) return Promise.resolve(this.lines[i]);
    }
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.waiters.delete(w);
        reject(new Error(`timed out after ${timeoutMs}ms waiting for log line ${what}`));
      }, timeoutMs);
      const w = (line) => {
        if (re.test(line)) {
          clearTimeout(timer);
          this.waiters.delete(w);
          resolve(line);
        }
      };
      this.waiters.add(w);
    });
  }

  /** Start `celld dev`; resolves when it prints its `ready` line. */
  async start({ args = [], readyTimeoutMs = 60000, port } = {}) {
    if (this.running) throw new Error("celld already running");
    if (port) this.port = port;
    else if (!this.port) this.port = await freePort();
    this.sessions++;
    this.sessionStart = this.lines.length;
    this._pushLine(`--- e2e: start #${this.sessions} port=${this.port} ---`);
    this.exit = null;
    const env = {
      ...process.env,
      CELLD_ESBUILD: this.esbuild,
      NO_COLOR: "1",
      ...this.extraEnv,
    };
    const child = spawn(
      this.bin,
      ["dev", ".", "--host", "127.0.0.1", "--port", String(this.port), "--no-watch", "--logs", ...args],
      { cwd: this.dir, env, stdio: ["ignore", "pipe", "pipe"] },
    );
    this.child = child;
    live.add(this);
    const out = { buf: "" };
    const err = { buf: "" };
    child.stdout.on("data", (c) => this._onData(c, out));
    child.stderr.on("data", (c) => this._onData(c, err));
    const exited = new Promise((resolve) => {
      child.on("exit", (code, signal) => {
        if (out.buf) this._pushLine(out.buf);
        if (err.buf) this._pushLine(err.buf);
        this.exit = { code, signal };
        live.delete(this);
        this._pushLine(`--- e2e: exit code=${code} signal=${signal} ---`);
        resolve();
      });
      child.on("error", (e) => {
        this.exit = { code: null, signal: null, error: e };
        resolve();
      });
    });
    this._exited = exited;
    const ready = this.waitLog(/^\s*ready\s+http:\/\//, { timeoutMs: readyTimeoutMs, what: "`ready`" });
    const died = exited.then(() => {
      throw new Error(
        `celld exited during startup (code=${this.exit?.code} signal=${this.exit?.signal})\n` +
          this.logText({ session: true }).split("\n").slice(-30).join("\n"),
      );
    });
    try {
      await Promise.race([ready, died]);
    } catch (e) {
      await this.stop({ signal: "SIGKILL" });
      throw e;
    }
  }

  /** Graceful stop (SIGTERM); escalates to SIGKILL after `graceMs`. */
  async stop({ signal = "SIGTERM", graceMs = 45000 } = {}) {
    if (!this.child) return;
    if (this.exit === null) {
      this.child.kill(signal);
      const t = setTimeout(() => this.child?.kill("SIGKILL"), graceMs);
      await this._exited;
      clearTimeout(t);
    }
  }

  /** SIGKILL: an unclean crash. */
  kill() {
    return this.stop({ signal: "SIGKILL" });
  }
}
