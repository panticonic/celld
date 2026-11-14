import { DurableObject } from "cloudflare:workers";

// Work that starts from a promise continuation after the event that created
// the continuation has ended. workerd allows all of it while the object lives,
// and so does celld. `GET /arm?op=NAME` registers a continuation that waits,
// a later `GET /go?op=NAME` releases it, and `GET /result?op=NAME` reads how
// it ended. The continuation runs after /arm has returned, in the turn of the
// request that released it.
const OPS = {
  // The agent runtimes' pattern: an initialisation block started in the
  // background, after the request that began it.
  async bcw(self) {
    const value = await self.ctx.blockConcurrencyWhile(async () => {
      await self.ctx.storage.put("bcw", "block");
      return "ran";
    });
    return `${value}:${await self.ctx.storage.get("bcw")}`;
  },
  // Two blocks started from one orphaned continuation queue, and run in order.
  async bcwOrder(self) {
    const order = [];
    const first = self.ctx.blockConcurrencyWhile(async () => {
      order.push("first:start");
      await new Promise((resolve) => setTimeout(resolve, 50));
      order.push("first:end");
    });
    const second = self.ctx.blockConcurrencyWhile(async () => {
      order.push("second");
    });
    await Promise.all([first, second]);
    return order.join(",");
  },
  // A block that throws resets the object, as one in a request does, so the
  // continuation that started it never learns: its outcome is what the block
  // itself recorded. The object that answers afterwards is a new one.
  async bcwThrows(self) {
    await self.ctx.blockConcurrencyWhile(async () => {
      self.ctx.storage.kv.put("result:bcwThrows", JSON.stringify({ ok: "block ran" }));
      throw new Error("block failed");
    });
    return "unreachable";
  },
  // An exclusive section that never ends. With the 30 s limit off it holds the
  // object until the node stops, which must not be held up by it.
  async bcwStuck(self) {
    await self.ctx.blockConcurrencyWhile(() => new Promise(() => {}));
    return "unreachable";
  },
  async storage(self) {
    await self.ctx.storage.put("a", 1);
    await self.ctx.storage.put({ b: 2, c: 3 });
    const got = await self.ctx.storage.get(["a", "b", "c"]);
    await self.ctx.storage.delete("c");
    const listed = [...(await self.ctx.storage.list()).keys()].sort();
    return JSON.stringify({ got: [...got.entries()], listed });
  },
  async sql(self) {
    self.ctx.storage.sql.exec("CREATE TABLE IF NOT EXISTS t (v INTEGER)");
    self.ctx.storage.sql.exec("INSERT INTO t (v) VALUES (7)");
    return JSON.stringify(self.ctx.storage.sql.exec("SELECT v FROM t").toArray());
  },
  async transactionSync(self) {
    self.ctx.storage.transactionSync(() => {
      self.ctx.storage.kv.put("tx", "inside");
    });
    return String(self.ctx.storage.kv.get("tx"));
  },
  async transaction(self) {
    await self.ctx.storage.transaction(async (txn) => {
      await txn.put("txn", "inside");
    });
    return String(await self.ctx.storage.get("txn"));
  },
  async alarm(self) {
    await self.ctx.storage.setAlarm(Date.now() + 600_000);
    const armed = await self.ctx.storage.getAlarm();
    await self.ctx.storage.deleteAlarm();
    return typeof armed === "number" ? "armed" : "no alarm";
  },
  async fetch(self, origin) {
    const response = await fetch(`${origin}/ping`);
    return `${response.status}:${await response.text()}`;
  },
  async stub(self, origin, env) {
    const response = await env.AGENT.getByName("peer").fetch(`${origin}/peer`);
    return `${response.status}:${await response.text()}`;
  },
  async rpc(self, origin, env) {
    return await env.AGENT.getByName("peer").echo("hi");
  },
  // A write the continuation makes is durable before the effect that follows
  // it leaves, and the effect completes.
  async effect(self, origin) {
    await self.ctx.storage.put("effect", "written");
    const response = await fetch(`${origin}/ping`);
    return `${response.status}:${await self.ctx.storage.get("effect")}`;
  },
};

export class Agent extends DurableObject {
  results = {};
  releases = new Map();
  // A new instance, as after a reset, has a new id.
  instance = crypto.randomUUID();

  echo(value) {
    return `echo:${value}`;
  }

  async fetch(request) {
    const url = new URL(request.url);
    const op = url.searchParams.get("op") ?? "";
    switch (url.pathname) {
      case "/arm": {
        if (!(op in OPS)) return new Response(`unknown op ${op}`, { status: 400 });
        let release;
        const released = new Promise((resolve) => (release = resolve));
        this.releases.set(op, release);
        this.results[op] = "pending";
        // Nothing here is awaited, and nothing native is pending when this
        // request returns: the continuation outlives the request that made it.
        // With `linger`, the request leaves a timer pending: it has answered
        // but is still running when the continuation starts its block, in the
        // turn of another request (Vibestudio's agent: an alarm pass that
        // returned with I/O in flight, whose queued transaction a later event
        // releases).
        const linger = Number(url.searchParams.get("linger") ?? 0);
        if (linger > 0) setTimeout(() => {}, linger);
        released
          .then(() => OPS[op](this, url.origin, this.env))
          .then(
            (value) => (this.results[op] = { ok: value }),
            (error) => (this.results[op] = { error: String(error?.message ?? error) }),
          );
        return Response.json({ armed: op });
      }
      case "/go":
        this.releases.get(op)?.();
        return Response.json({ released: op });
      case "/result": {
        const recorded = this.ctx.storage.kv.get(`result:${op}`);
        return Response.json(this.results[op] ?? (recorded ? JSON.parse(recorded) : "unknown"));
      }
      // A request that answers in its first turn and leaves a continuation
      // behind, which another request's turn runs while the first is still
      // on its way out, and which starts an op (a timer). The op belongs to
      // the first request until it has gone: that request must poll it.
      // `/late-arm` holds the isolate long enough that `/late-release`,
      // sent just before it, is due by then and runs the continuation in the
      // very next turn.
      case "/late-arm": {
        this.results.late = "pending";
        const released = new Promise((resolve) => (this.lateRelease = resolve));
        released
          .then(() => new Promise((resolve) => setTimeout(resolve, 20)))
          .then(() => (this.results.late = { ok: "timer settled" }));
        // The clock does not move inside a turn, so the hold is counted work.
        let spin = 0;
        for (let i = Number(url.searchParams.get("spin") ?? 2e9); i > 0; i--) spin ^= i;
        this.spin = spin;
        return Response.json({ armed: "late" });
      }
      case "/late-release": {
        await new Promise((resolve) => setTimeout(resolve, Number(url.searchParams.get("after") ?? 100)));
        this.lateRelease?.();
        return Response.json({ released: "late" });
      }
      case "/instance":
        return Response.json({ instance: this.instance });
      case "/peer":
        return new Response("peer");
    }
    return new Response("not found", { status: 404 });
  }
}

export default {
  fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/ping") return new Response("pong");
    return env.AGENT.getByName(url.searchParams.get("name") ?? "main").fetch(request);
  },
};
