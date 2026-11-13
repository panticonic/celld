// A stand-in network for the egress example and its test.
//
// `role: "proxy"` is a minimal egress proxy. It speaks what a celld node sends
// when `--egress-proxy` is set (see docs/services/egress.md), verifies the HMAC
// on every request, records the caller identity it carries, and answers as if
// it were the destination. `role: "origin"` is the destination itself, for the
// run with no proxy configured. Both serve the same things: HTTP with a
// redirect, a WebSocket echo, and a TCP echo.
import crypto from "node:crypto";
import http from "node:http";
import net from "node:net";

export function verify(secret, kind, method, target, headers) {
  const identity = headers["x-celld-egress-identity"];
  const nonce = headers["x-celld-egress-nonce"];
  const mac = headers["x-celld-egress-mac"];
  if (!identity || !nonce || !mac) return null;
  const expected = crypto
    .createHmac("sha256", secret)
    .update([ "celld-egress/v1", kind, method, target, identity, nonce ].map((part) => `${part}\n`).join(""))
    .digest();
  const got = Buffer.from(mac, "base64url");
  if (got.length !== expected.length || !crypto.timingSafeEqual(got, expected)) return null;
  return JSON.parse(Buffer.from(identity, "base64url").toString());
}

function frame(opcode, payload) {
  const body = Buffer.from(payload);
  return Buffer.concat([Buffer.from([0x80 | opcode, body.length]), body]);
}

// Echo text frames back as `echo:<text>`; answer a close with a close.
function echoWebSocket(socket) {
  let buffered = Buffer.alloc(0);
  socket.on("data", (chunk) => {
    buffered = Buffer.concat([buffered, chunk]);
    for (;;) {
      if (buffered.length < 2) return;
      const opcode = buffered[0] & 0x0f;
      const masked = (buffered[1] & 0x80) !== 0;
      let length = buffered[1] & 0x7f;
      let offset = 2;
      if (length === 126) {
        if (buffered.length < 4) return;
        length = buffered.readUInt16BE(2);
        offset = 4;
      }
      const need = offset + (masked ? 4 : 0) + length;
      if (buffered.length < need) return;
      const mask = masked ? buffered.subarray(offset, offset + 4) : null;
      const data = Buffer.from(buffered.subarray(offset + (masked ? 4 : 0), need));
      if (mask) for (let i = 0; i < data.length; i++) data[i] ^= mask[i % 4];
      buffered = buffered.subarray(need);
      if (opcode === 1) socket.write(frame(1, `echo:${data.toString()}`));
      else if (opcode === 8) {
        socket.write(frame(8, data.subarray(0, 2)));
        socket.end();
        return;
      } else if (opcode === 9) socket.write(frame(10, data));
    }
  });
  socket.on("error", () => {});
}

function acceptWebSocket(req, socket) {
  const accept = crypto
    .createHash("sha1")
    .update(`${req.headers["sec-websocket-key"]}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
    .digest("base64");
  const protocol = req.headers["sec-websocket-protocol"]?.split(",")[0].trim();
  socket.write(
    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n" +
      `Sec-WebSocket-Accept: ${accept}\r\n` +
      (protocol ? `Sec-WebSocket-Protocol: ${protocol}\r\n` : "") +
      "\r\n",
  );
  echoWebSocket(socket);
}

export async function startServer({ role, secret }) {
  const records = [];
  const sockets = new Set();
  const track = (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  };
  const record = (entry) => records.push(entry);

  // Resolve the request's destination and, for a proxy, the caller.
  // `target` is what the request line carried (proxy) or what the Host header
  // and path say (origin): the same URL the Worker asked for.
  const target = (req, scheme) =>
    role === "proxy" ? req.url : `${scheme}://${req.headers.host}${req.url}`;

  const server = http.createServer((req, res) => {
    if (!/^[a-z]+:\/\//.test(req.url) && role === "proxy") {
      // Not proxy traffic: the test's own control channel.
      if (req.url === "/__records") return res.end(JSON.stringify(records));
      if (req.url === "/__reset") {
        records.length = 0;
        return res.end("ok");
      }
      res.statusCode = 400;
      return res.end("not a proxy request");
    }
    if (role === "origin" && req.url === "/__records") return res.end(JSON.stringify(records));
    if (role === "origin" && req.url === "/__reset") {
      records.length = 0;
      return res.end("ok");
    }
    const url = target(req, "http");
    const identity = role === "proxy" ? verify(secret, "fetch", req.method, url, req.headers) : null;
    record({
      kind: "fetch",
      method: req.method,
      target: url,
      identity,
      authenticated: role === "proxy" ? identity !== null : null,
      identityHeaderSeen: "x-celld-egress-identity" in req.headers,
    });
    if (role === "proxy" && !identity) {
      res.statusCode = 403;
      return res.end("egress: unattributed request");
    }
    if (url.endsWith("/redirect")) {
      const next = role === "proxy" ? "http://landing.test/landed" : `http://${req.headers.host}/landed`;
      res.statusCode = 302;
      res.setHeader("location", next);
      return res.end();
    }
    res.end(`served:${url}`);
  });

  server.on("connection", track);
  server.on("upgrade", (req, socket) => {
    const url = target(req, "ws");
    const identity = role === "proxy" ? verify(secret, "websocket", req.method, url, req.headers) : null;
    record({ kind: "websocket", target: url, identity, authenticated: role === "proxy" ? identity !== null : null });
    if (role === "proxy" && !identity) {
      socket.end("HTTP/1.1 403 Forbidden\r\n\r\n");
      return;
    }
    acceptWebSocket(req, socket);
  });
  // CONNECT is proxy-only; the origin's TCP service is the plain server below.
  server.on("connect", (req, socket) => {
    const identity = verify(secret, "connect", req.method, req.url, req.headers);
    record({ kind: "connect", target: req.url, identity, authenticated: identity !== null });
    if (!identity) {
      socket.end("HTTP/1.1 403 Forbidden\r\n\r\n");
      return;
    }
    socket.write("HTTP/1.1 200 Connection Established\r\n\r\n");
    socket.on("data", (data) => socket.write(`echo:${data}`));
    socket.on("error", () => {});
  });

  const tcp = net.createServer((socket) => {
    track(socket);
    record({ kind: "tcp", target: `${socket.localAddress}:${socket.localPort}`, identity: null });
    socket.on("data", (data) => socket.write(`echo:${data}`));
    socket.on("error", () => {});
  });

  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  if (role === "origin") await new Promise((resolve) => tcp.listen(0, "127.0.0.1", resolve));
  return {
    port: server.address().port,
    tcpPort: role === "origin" ? tcp.address().port : null,
    records,
    close() {
      for (const socket of sockets) socket.destroy();
      server.close();
      tcp.close();
    },
  };
}
