import { DurableObject } from "cloudflare:workers";

// A WebSocket the object opens with `fetch(url, { headers: { Upgrade:
// "websocket" } })`, from an alarm handler or from a request. The socket
// belongs to the object: it stays open and is used after the event that made
// it has returned.
export class Dialer extends DurableObject {
  socket = null;

  // Opens the socket, sends "hello" and stores each reply under `<mode>:<n>`.
  async dial(mode, url) {
    const response = await fetch(url, { headers: { Upgrade: "websocket" } });
    const socket = response.webSocket;
    if (!socket) throw new Error(`no upgrade: ${response.status}`);
    socket.accept();
    let n = 0;
    socket.addEventListener("message", (event) => {
      this.ctx.storage.kv.put(`${mode}:${++n}`, String(event.data));
    });
    socket.addEventListener("close", (event) => {
      this.ctx.storage.kv.put(`${mode}:closed`, `${event.code}`);
    });
    this.socket = socket;
    socket.send("hello");
  }

  // Writes, dials, sends and waits for the reply inside the alarm, as a model
  // transport does: the frame trails the alarm's own write.
  async dialAndWait(mode, url, { transaction = false } = {}) {
    const write = async () => {
      if (transaction) await this.ctx.storage.transaction(async (txn) => txn.put("before", mode));
      else this.ctx.storage.kv.put("before", mode);
    };
    await write();
    const response = await fetch(url, { headers: { Upgrade: "websocket" } });
    const socket = response.webSocket;
    if (!socket) throw new Error(`no upgrade: ${response.status}`);
    socket.accept();
    this.socket = socket;
    let n = 0;
    const first = new Promise((resolve, reject) => {
      socket.addEventListener("message", (event) => {
        this.ctx.storage.kv.put(`${mode}:${++n}`, String(event.data));
        resolve();
      });
      socket.addEventListener("close", () => reject(new Error("closed before a reply")));
    });
    socket.send("hello");
    await first;
    await write();
  }

  async alarm() {
    const { mode, url } = this.ctx.storage.kv.get("dial");
    if (mode === "alarm-write" || mode === "alarm-transaction") {
      try {
        await this.dialAndWait(mode, url, { transaction: mode === "alarm-transaction" });
      } catch (error) {
        this.ctx.storage.kv.put(`${mode}:error`, String(error?.message ?? error));
      }
      return;
    }
    const dial = () => this.dial(mode, url).catch((error) => {
      this.ctx.storage.kv.put(`${mode}:error`, String(error?.message ?? error));
    });
    // "alarm-block" dials inside a critical section; "alarm-detached" returns
    // at once and lets the dial finish on its own, so its continuation runs
    // after the alarm event has returned.
    if (mode === "alarm-block") await this.ctx.blockConcurrencyWhile(dial);
    else if (mode === "alarm-detached") dial();
    else await dial();
  }

  async fetch(request) {
    const url = new URL(request.url);
    switch (url.pathname) {
      // Dials from this request.
      case "/connect":
        try {
          await this.dial("fetch", url.searchParams.get("url"));
        } catch (error) {
          this.ctx.storage.kv.put("fetch:error", String(error?.message ?? error));
        }
        return new Response(null, { status: 202 });
      // Dials from the alarm, set for 300 ms from now.
      case "/arm":
        this.ctx.storage.kv.put("dial", { mode: url.searchParams.get("mode") ?? "alarm", url: url.searchParams.get("url") });
        await this.ctx.storage.setAlarm(Date.now() + 300);
        return new Response(null, { status: 202 });
      // Dials from a continuation that a later request releases, after this
      // request has ended.
      case "/arm-orphan": {
        const mode = "orphan";
        const target = url.searchParams.get("url");
        this.release = null;
        new Promise((resolve) => (this.release = resolve))
          .then(() => this.dial(mode, target))
          .catch((error) => this.ctx.storage.kv.put(`${mode}:error`, String(error?.message ?? error)));
        return new Response(null, { status: 202 });
      }
      case "/release":
        this.release();
        return new Response(null, { status: 202 });
      // Sends a message on the open socket, from a later request.
      case "/send":
        this.socket.send(url.searchParams.get("text") ?? "");
        return new Response(null, { status: 202 });
      case "/close":
        this.socket.close(1000, "done");
        return new Response(null, { status: 202 });
      case "/state": {
        const state = {};
        for (const [key, value] of this.ctx.storage.kv.list()) if (key !== "dial") state[key] = value;
        return Response.json(state);
      }
    }
    return new Response("not found", { status: 404 });
  }
}

export default {
  fetch(request, env) {
    return env.DIALER.getByName("only").fetch(request);
  },
};
