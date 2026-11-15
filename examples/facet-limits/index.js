import { DurableObject } from "cloudflare:workers";

// The code of the facet: a stand-in for a kernel that runs the caller's code.
// It keeps a heap that must survive a call the node ended, and it can spin.
const KERNEL = `
  import { DurableObject } from "cloudflare:workers";
  export class Kernel extends DurableObject {
    born = String(Math.random());
    calls = 0;
    spin() {
      this.calls++;
      let i = 0;
      for (;;) i++;
    }
    work(n) {
      this.calls++;
      let x = 0;
      for (let i = 0; i < n; i++) x += Math.sqrt(i);
      return { worked: n, x };
    }
    info() {
      return { born: this.born, calls: this.calls };
    }
    // Subrequests to a closed port: each is refused fast, and each counts.
    async subs(n) {
      const out = [];
      for (let i = 0; i < n; i++) {
        try {
          await fetch("http://127.0.0.1:9/");
          out.push("ok");
        } catch (error) {
          out.push(String(error?.message ?? error));
        }
      }
      return out;
    }
    // What a kernel's start does (the product's EvalKernel): the call answers
    // at once and the run goes on under waitUntil, after one timer turn. The
    // run arms its deadline, joins the kernel's serial chain and runs the
    // caller's code; a spin there is ended by the call's CPU limit, in a
    // continuation of the start call. The deadline settles the run and
    // writes its terminal row; a run that timed out orphans the chain, as a
    // kernel's force reset does, so the next run is not queued behind it.
    chain = Promise.resolve();
    start(id, timeoutMs, spin, tx = false) {
      this.calls++;
      const sql = this.ctx.storage.sql;
      sql.exec("CREATE TABLE IF NOT EXISTS runs (id TEXT PRIMARY KEY, status TEXT)");
      sql.exec("INSERT OR REPLACE INTO runs VALUES (?, 'running')", id);
      this.ctx.waitUntil(
        new Promise((resolve) => setTimeout(resolve, 0)).then(() => this.execute(id, timeoutMs, spin, tx)),
      );
      return { started: id };
    }
    // With \`tx\`, the terminal row is written by an asynchronous transaction
    // (a product kernel commits it). The deadline that settles a spinning run
    // fires in a turn of the call the limit ended, which still drives the
    // object's pending I/O: the transaction must run there, not be refused
    // as the block of an event that has ended. A refusal is recorded.
    async execute(id, timeoutMs, spin, tx = false) {
      let timer;
      const deadline = new Promise((resolve) => {
        timer = setTimeout(() => resolve("timed-out"), timeoutMs);
      });
      const ran = this.chain.then(() => {
        if (spin) for (;;);
        return "done";
      });
      this.chain = ran.catch(() => {});
      const status = await Promise.race([ran, deadline]);
      clearTimeout(timer);
      if (status === "timed-out") this.chain = Promise.resolve();
      const record = (value) => this.ctx.storage.sql.exec("UPDATE runs SET status = ? WHERE id = ?", value, id);
      if (!tx) return record(status);
      const commit = this.ctx.storage.dolt ? { commit: { message: id } } : undefined;
      try {
        await this.ctx.storage.transaction(async () => record(status), commit);
      } catch (error) {
        record("refused: " + String(error?.message ?? error));
      }
    }
    run(id) {
      const sql = this.ctx.storage.sql;
      sql.exec("CREATE TABLE IF NOT EXISTS runs (id TEXT PRIMARY KEY, status TEXT)");
      return sql.exec("SELECT status FROM runs WHERE id = ?", id).toArray()[0]?.status ?? null;
    }
    // A spin inside a storage transaction: the write before it must roll back,
    // and the object must take the next transaction.
    txSpin(kind) {
      this.calls++;
      const storage = this.ctx.storage;
      storage.sql.exec("CREATE TABLE IF NOT EXISTS rows (v TEXT)");
      const write = () => storage.sql.exec("INSERT INTO rows VALUES ('spun')");
      // A Dolt cell commits each transaction, as a product kernel does.
      const commit = storage.dolt ? { commit: { message: "spun" } } : undefined;
      if (kind === "sync") {
        return storage.transactionSync(() => {
          write();
          for (;;);
        }, commit);
      }
      if (kind === "async") {
        return storage.transaction(async () => {
          write();
          await null;
          for (;;);
        }, commit);
      }
      return this.ctx.blockConcurrencyWhile(async () => {
        write();
        await null;
        for (;;);
      });
    }
    txWrite(value) {
      const storage = this.ctx.storage;
      storage.sql.exec("CREATE TABLE IF NOT EXISTS rows (v TEXT)");
      const commit = storage.dolt ? { commit: { message: value } } : undefined;
      storage.transactionSync(() => storage.sql.exec("INSERT INTO rows VALUES (?)", value), commit);
      return storage.sql.exec("SELECT v FROM rows ORDER BY rowid").toArray().map((row) => row.v);
    }
    async fetch(request) {
      const url = new URL(request.url);
      if (url.pathname === "/spin") return Response.json(this.spin());
      if (url.pathname === "/subs")
        return Response.json(await this.subs(Number(url.searchParams.get("n"))));
      if (url.pathname === "/work")
        return Response.json(this.work(Number(url.searchParams.get("n"))));
      return Response.json(this.info());
    }
  }
`;

// The loaded worker's own limit. A per-call limit lower than this ends a call
// sooner; one higher than it changes nothing.
const UNIT_CPU_MS = 3000;

export class Owner extends DurableObject {
  // `dolt`: the kernel's storage is a Dolt facet (a product kernel's is).
  kernel(cpu, subRequests = null, dolt = false) {
    const worker = this.env.LOADER.get("kernel-v1", () => ({
      compatibilityDate: "2025-01-01",
      mainModule: "kernel.js",
      modules: { "kernel.js": KERNEL },
      limits: { cpuMs: UNIT_CPU_MS },
    }));
    const name = dolt ? "dolt-kernel" : "kernel";
    const startup = () => ({
      class: worker.getDurableObjectClass("Kernel"),
      ...(dolt ? { celld: { storage: "dolt" } } : {}),
    });
    // A call's own limit rides on the stub: `get(name, startup, { limits })`.
    if (cpu === null && subRequests === null) return this.ctx.facets.get(name, startup);
    const limits = {};
    if (cpu !== null) limits.cpuMs = cpu;
    if (subRequests !== null) limits.subRequests = subRequests;
    return this.ctx.facets.get(name, startup, { limits });
  }

  async fetch(request) {
    const url = new URL(request.url);
    const cpuParam = url.searchParams.get("cpu");
    const [, via, rest] = url.pathname.split("/");
    const started = Date.now();
    try {
      if (via === "root") return Response.json({ rootAlive: true });
      if (via === "bad") {
        // An invalid limit is refused at the call, and nothing runs.
        this.kernel(JSON.parse(url.searchParams.get("cpu")));
        return Response.json({ ok: true });
      }
      const cpu = cpuParam === null ? null : Number(cpuParam);
      const subParam = url.searchParams.get("sub");
      const kernel = this.kernel(
        cpu,
        subParam === null ? null : Number(subParam),
        url.searchParams.get("dolt") === "1",
      );
      if (via === "kernel") {
        const response = await kernel.fetch(`http://kernel/${rest}${url.search}`);
        return Response.json({ ok: true, ms: Date.now() - started, body: await response.json() });
      }
      if (via === "rpc") {
        const result =
          rest === "spin" ? await kernel.spin()
            : rest === "work" ? await kernel.work(Number(url.searchParams.get("n")))
              : rest === "subs" ? await kernel.subs(Number(url.searchParams.get("n")))
                : rest === "start"
                  ? await kernel.start(
                    url.searchParams.get("id"),
                    Number(url.searchParams.get("timeout")),
                    url.searchParams.get("spin") === "1",
                    url.searchParams.get("tx") === "1",
                  )
                  : rest === "run" ? await kernel.run(url.searchParams.get("id"))
                    : rest === "tx-spin" ? await kernel.txSpin(url.searchParams.get("kind"))
                      : rest === "tx-write" ? await kernel.txWrite(url.searchParams.get("v"))
                        : await kernel.info();
        return Response.json({ ok: true, ms: Date.now() - started, body: result });
      }
      return new Response("not found", { status: 404 });
    } catch (error) {
      return Response.json(
        { ok: false, ms: Date.now() - started, error: String(error?.message ?? error) },
        { status: 500 },
      );
    }
  }
}

export default {
  fetch(request, env) {
    const object = new URL(request.url).searchParams.get("object") ?? "owner";
    return env.OWNER.getByName(object).fetch(request);
  },
};
