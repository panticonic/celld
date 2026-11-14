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
  kernel(cpu, subRequests = null) {
    const worker = this.env.LOADER.get("kernel-v1", () => ({
      compatibilityDate: "2025-01-01",
      mainModule: "kernel.js",
      modules: { "kernel.js": KERNEL },
      limits: { cpuMs: UNIT_CPU_MS },
    }));
    const startup = () => ({ class: worker.getDurableObjectClass("Kernel") });
    // A call's own limit rides on the stub: `get(name, startup, { limits })`.
    if (cpu === null && subRequests === null) return this.ctx.facets.get("kernel", startup);
    const limits = {};
    if (cpu !== null) limits.cpuMs = cpu;
    if (subRequests !== null) limits.subRequests = subRequests;
    return this.ctx.facets.get("kernel", startup, { limits });
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
      const kernel = this.kernel(cpu, subParam === null ? null : Number(subParam));
      if (via === "kernel") {
        const response = await kernel.fetch(`http://kernel/${rest}${url.search}`);
        return Response.json({ ok: true, ms: Date.now() - started, body: await response.json() });
      }
      if (via === "rpc") {
        const result =
          rest === "spin" ? await kernel.spin()
            : rest === "work" ? await kernel.work(Number(url.searchParams.get("n")))
              : rest === "subs" ? await kernel.subs(Number(url.searchParams.get("n")))
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
