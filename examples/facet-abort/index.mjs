import { DurableObject } from "cloudflare:workers";

// A request in flight to a facet when the facet is aborted. workerd rejects the
// pending call with the abort reason, so the root's own request ends: with the
// error it let through, or with the response it chose when it caught it. celld
// did not reject the call, and the client's request stayed open.
const CODE = `
  import { DurableObject } from "cloudflare:workers";
  export class App extends DurableObject {
    async fetch(request) {
      const path = new URL(request.url).pathname;
      if (path === "/hold") await new Promise(() => {});
      this.ctx.storage.kv.put("n", (this.ctx.storage.kv.get("n") ?? 0) + 1);
      return new Response("facet " + path);
    }
  }
`;

export class Root extends DurableObject {
  facet() {
    const worker = this.env.LOADER.get("app-v1", () => ({
      compatibilityDate: "2025-01-01",
      mainModule: "app.js",
      modules: { "app.js": CODE },
    }));
    return this.ctx.facets.get("do", () => ({ class: worker.getDurableObjectClass("App") }));
  }

  async fetch(request) {
    const url = new URL(request.url);
    switch (url.pathname) {
      // Forwards to the facet and returns whatever it does. If the facet is
      // aborted meanwhile, the call rejects and the root answers 500 itself.
      case "/hold":
        try {
          return await this.facet().fetch(request);
        } catch (error) {
          return new Response(`root caught: ${error.message}`, { status: 500 });
        }
      // Forwards and lets a rejection through, uncaught.
      case "/hold-uncaught":
        return this.facet().fetch(new Request(new URL("/hold", request.url)));
      // Gives up on the facet's answer after 300 ms and answers by itself; the
      // call is still pending when the facet is aborted later.
      case "/race": {
        const call = this.facet().fetch(new Request(new URL("/hold", request.url)));
        call.catch(() => {});
        const timeout = new Promise((resolve) =>
          setTimeout(() => resolve(new Response("root gave up", { status: 500 })), 300));
        return Promise.race([call, timeout]);
      }
      // Waits for `/wake` with a call into the facet still pending. Another
      // request settles this handler, and the facet call never ends.
      case "/hold2": {
        const call = this.facet().fetch(new Request(new URL("/hold", request.url)));
        call.catch(() => {});
        await new Promise((resolve) => (this.wake = resolve));
        return new Response("hold2 done");
      }
      case "/wake": {
        const wake = this.wake;
        if (!wake) return new Response("not waiting", { status: 409 });
        this.wake = null;
        wake();
        return new Response("woken");
      }
      case "/abort":
        this.ctx.facets.abort("do", new Error("x"));
        return new Response("aborted");
      // The facet works again after an abort.
      case "/ok":
        return this.facet().fetch(new Request(new URL("/ok", request.url)));
    }
    return new Response("not found", { status: 404 });
  }
}

export default {
  fetch(request, env) {
    return env.ROOT.getByName("only").fetch(request);
  },
};
