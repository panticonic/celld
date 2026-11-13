// WebSocket helpers: a client wrapper over Node's global WebSocket and a
// minimal RFC 6455 server (text frames only) used as a target for wsclient.
import crypto from "node:crypto";
import http from "node:http";

export class WsConn {
  constructor(sock) {
    this.sock = sock;
    this.queue = [];
    this.waiters = [];
    this.closeInfo = null;
    this.closedWaiters = [];
    sock.binaryType = "arraybuffer";
    sock.addEventListener("message", (ev) => {
      const d = typeof ev.data === "string" ? ev.data : Buffer.from(ev.data);
      const w = this.waiters.shift();
      if (w) w.resolve(d);
      else this.queue.push(d);
    });
    sock.addEventListener("close", (ev) => {
      this.closeInfo = { code: ev.code, reason: ev.reason };
      for (const w of this.waiters.splice(0)) w.reject(new Error(`socket closed (${ev.code}) while waiting for a message`));
      for (const w of this.closedWaiters.splice(0)) w(this.closeInfo);
    });
  }
  send(data) {
    this.sock.send(data);
  }
  /** Next message; bounded by `timeoutMs`. */
  next({ timeoutMs = 10000 } = {}) {
    if (this.queue.length) return Promise.resolve(this.queue.shift());
    if (this.closeInfo) return Promise.reject(new Error(`socket already closed (${this.closeInfo.code})`));
    return new Promise((resolve, reject) => {
      const w = {
        resolve: (v) => {
          clearTimeout(t);
          resolve(v);
        },
        reject: (e) => {
          clearTimeout(t);
          reject(e);
        },
      };
      const t = setTimeout(() => {
        this.waiters = this.waiters.filter((x) => x !== w);
        reject(new Error(`no websocket message within ${timeoutMs}ms`));
      }, timeoutMs);
      this.waiters.push(w);
    });
  }
  close(code = 1000, reason = "") {
    try {
      this.sock.close(code, reason);
    } catch {}
  }
  closed({ timeoutMs = 10000 } = {}) {
    if (this.closeInfo) return Promise.resolve(this.closeInfo);
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => reject(new Error(`socket not closed within ${timeoutMs}ms`)), timeoutMs);
      this.closedWaiters.push((i) => {
        clearTimeout(t);
        resolve(i);
      });
    });
  }
}

export function openWs(url, { protocols, timeoutMs = 10000 } = {}) {
  return new Promise((resolve, reject) => {
    const sock = protocols ? new WebSocket(url, protocols) : new WebSocket(url);
    const t = setTimeout(() => {
      try {
        sock.close();
      } catch {}
      reject(new Error(`websocket ${url} did not open within ${timeoutMs}ms`));
    }, timeoutMs);
    sock.addEventListener("open", () => {
      clearTimeout(t);
      resolve(new WsConn(sock));
    });
    sock.addEventListener("error", () => {
      clearTimeout(t);
      reject(new Error(`websocket ${url} failed to open`));
    });
  });
}

/**
 * Minimal WebSocket server. For each text message `m` it replies
 * `reply(m)` (default `"pong:" + m`). Records what it saw.
 */
export async function startWsServer({ reply = (m) => "pong:" + m } = {}) {
  const seen = { connections: 0, messages: [], closes: [], paths: [] };
  const sockets = new Set();
  const server = http.createServer((_, res) => {
    res.writeHead(426);
    res.end("upgrade required");
  });
  server.on("upgrade", (req, socket) => {
    seen.connections++;
    seen.paths.push(req.url);
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
    const key = req.headers["sec-websocket-key"];
    const accept = crypto.createHash("sha1").update(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest("base64");
    socket.write(
      "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
        `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
    );
    let buf = Buffer.alloc(0);
    const frame = (op, payload) => {
      const len = payload.length;
      const head = len < 126 ? Buffer.from([0x80 | op, len]) : Buffer.from([0x80 | op, 126, len >> 8, len & 255]);
      socket.write(Buffer.concat([head, payload]));
    };
    socket.on("data", (d) => {
      buf = Buffer.concat([buf, d]);
      for (;;) {
        if (buf.length < 2) return;
        const op = buf[0] & 0x0f;
        let len = buf[1] & 0x7f;
        let off = 2;
        if (len === 126) {
          if (buf.length < 4) return;
          len = buf.readUInt16BE(2);
          off = 4;
        } else if (len === 127) {
          if (buf.length < 10) return;
          len = Number(buf.readBigUInt64BE(2));
          off = 10;
        }
        const masked = (buf[1] & 0x80) !== 0;
        if (buf.length < off + (masked ? 4 : 0) + len) return;
        const mask = masked ? buf.subarray(off, off + 4) : null;
        if (masked) off += 4;
        const payload = Buffer.from(buf.subarray(off, off + len));
        if (mask) for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i % 4];
        buf = buf.subarray(off + len);
        if (op === 0x1) {
          const m = payload.toString("utf8");
          seen.messages.push(m);
          frame(0x1, Buffer.from(reply(m)));
        } else if (op === 0x8) {
          const code = payload.length >= 2 ? payload.readUInt16BE(0) : 1005;
          const reason = payload.length > 2 ? payload.subarray(2).toString("utf8") : "";
          seen.closes.push({ code, reason });
          frame(0x8, payload.subarray(0, 2));
          socket.end();
        } else if (op === 0x9) {
          frame(0xa, payload);
        }
      }
    });
    socket.on("error", () => {});
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const port = server.address().port;
  return {
    port,
    url: `ws://127.0.0.1:${port}/`,
    seen,
    async close() {
      for (const s of sockets) s.destroy();
      await new Promise((r) => server.close(r));
    },
  };
}
