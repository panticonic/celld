// Fixture for tests/inspector/run.mjs: a Durable Object, a stateless route
// and a dynamic Worker, so a node has all three kinds of inspector target.
export class Counter {
  constructor(state, env) {
    this.state = state;
  }
  async fetch(request) {
    let n = (await this.state.storage.get("n")) ?? 0;
    n++; // BREAKPOINT_LINE
    await this.state.storage.put("n", n);
    // A little CPU so a profile has samples to show.
    let acc = 0;
    for (let i = 0; i < 2000000; i++) acc += Math.sqrt(i);
    return Response.json({ n, acc: Math.round(acc) });
  }
}

const DYNAMIC_SOURCE = `
  export default {
    fetch() { return new Response("dynamic ok"); }
  };
`;

export default {
  async fetch(request, env, ctx) {
    const url = new URL(request.url);
    if (url.pathname === "/ping") return new Response("pong");
    if (url.pathname === "/dyn") {
      const worker = env.LOADER.get("inspector-dyn-v1", () => ({
        compatibilityDate: "2025-01-01",
        mainModule: "worker.js",
        modules: { "worker.js": DYNAMIC_SOURCE },
      }));
      return worker.getEntrypoint().fetch(request);
    }
    const name = url.searchParams.get("name") ?? "default";
    return env.COUNTER.get(env.COUNTER.idFromName(name)).fetch(request);
  },
};
