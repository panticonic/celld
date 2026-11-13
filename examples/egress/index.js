// Outbound traffic from a deployed Durable Object, a plain Worker and a
// dynamic worker. Run `node test.mjs` to see what a node's egress proxy
// receives for each; see docs/services/egress.md.
import { connect } from "cloudflare:sockets";

// Open a WebSocket the way a Worker does: `fetch` with an upgrade header, or
// the constructor. Resolves with the first message, so a caller learns
// whether the round trip worked.
async function wsRoundTrip(url, mode) {
  if (mode === "fetch") {
    const response = await fetch(url, { headers: { Upgrade: "websocket" } });
    const socket = response.webSocket;
    if (!socket) throw new Error(`upgrade declined: ${response.status}`);
    socket.accept();
    return await new Promise((resolve, reject) => {
      socket.addEventListener("message", (event) => {
        socket.close(1000, "done");
        resolve(event.data);
      });
      socket.addEventListener("error", () => reject(new Error("websocket error")));
      socket.send("hello");
    });
  }
  return await new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    socket.addEventListener("open", () => socket.send("hello"));
    socket.addEventListener("message", (event) => {
      socket.close(1000, "done");
      resolve(event.data);
    });
    socket.addEventListener("error", () => reject(new Error("websocket error")));
  });
}

async function tcpRoundTrip(hostname, port) {
  const socket = connect({ hostname, port: Number(port) });
  const writer = socket.writable.getWriter();
  await writer.write(new TextEncoder().encode("ping"));
  const reader = socket.readable.getReader();
  const { value } = await reader.read();
  await socket.close();
  return new TextDecoder().decode(value);
}

// One probe: the same code runs in every kind of isolate, and reports what
// happened as JSON so a caller can tell an allowed request from a refused one.
async function probe(request) {
  const url = new URL(request.url);
  const op = url.searchParams.get("op");
  const target = url.searchParams.get("url");
  try {
    if (op === "fetch") {
      const forged = url.searchParams.get("forge");
      const init = forged
        ? { headers: { "x-celld-egress-identity": "forged", "x-celld-egress-mac": "forged" } }
        : undefined;
      const response = await fetch(target, init);
      return Response.json({ ok: true, status: response.status, body: await response.text() });
    }
    if (op === "ws" || op === "wsfetch") {
      return Response.json({ ok: true, body: await wsRoundTrip(target, op === "ws" ? "ctor" : "fetch") });
    }
    if (op === "tcp") {
      return Response.json({
        ok: true,
        body: await tcpRoundTrip(url.searchParams.get("host"), url.searchParams.get("port")),
      });
    }
    return Response.json({ ok: false, error: "unknown op" });
  } catch (error) {
    return Response.json({ ok: false, error: String(error?.message ?? error) });
  }
}

export class Probe {
  constructor(state) {
    this.state = state;
  }
  async fetch(request) {
    return probe(request);
  }
}

const LOADED = `import { connect } from "cloudflare:sockets";\n${wsRoundTrip}\n${tcpRoundTrip}\n${probe}\nexport default { fetch: probe };`;

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    // A deployed Durable Object: identity carries its class and cell.
    if (url.pathname === "/do") {
      return env.PROBE.getByName(url.searchParams.get("name") ?? "probe").fetch(request);
    }
    // A plain deployed Worker: no cell.
    if (url.pathname === "/plain") return probe(request);
    // A dynamic worker. Its loader, this code, sets its identity; the loaded
    // code cannot. `deny=1` loads it with no network at all.
    if (url.pathname === "/dynamic") {
      const deny = url.searchParams.get("deny") === "1";
      const worker = env.LOADER.get(deny ? "probe-deny" : "probe-open", () => ({
        compatibilityDate: "2025-01-01",
        mainModule: "main.js",
        modules: { "main.js": LOADED },
        celld: { egressIdentity: "tenant-a" },
        ...(deny ? { globalOutbound: null } : {}),
      }));
      return worker.getEntrypoint().fetch(request);
    }
    return new Response("egress example: /do, /plain, /dynamic");
  },
};
