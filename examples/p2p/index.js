// A line-oriented echo over Iroh. Every node runs the same code: the Durable
// Object `Node` accepts inbound streams (`onp2pstream`) and also dials out, so
// two nodes can talk in both directions.
//
// A stream belongs to the Durable Object, not to the event that opened it: it
// lives until it is closed, both directions finish, or the object leaves the
// node, and while it is open the object stays in memory. This example uses
// that three ways: a request dials and finishes inside the request (`/dial`),
// an alarm dials and the stream is used by later requests (`/arm`, `/send`),
// and an inbound handler hands its stream over to later requests (`keep:`).
//
//   GET /id                      this node's id, and `born`: it changes when
//                                the object is evicted and built again
//   GET /dial?peer=ID&addr=IP:PORT&msgs=a,b,c&finish=close|done|bye|none
//                                connect, send each message and read its
//                                reply, then finish as asked; the answer
//                                includes how `stream.closed` settled
//   GET /arm?peer=ID&addr=IP:PORT&delay=MS
//                                set an alarm that dials the peer and keeps
//                                the stream
//   GET /session                 the alarm's stream: open, `closed`, and what
//                                the alarm read
//   GET /send?msg=TEXT&reply=0|1 write a line on the alarm's stream; read one
//                                line back unless reply=0
//   GET /recv                    read one line from the alarm's stream
//   GET /hangup?how=close|finish|reset
//                                end the alarm's stream
//   GET /open?peer=ID&addr=IP:PORT&n=N
//                                open N more streams and keep them; shows
//                                where the node refuses (the per-cell cap)
//   GET /open?close=1            close those streams
//   GET /pull?name=NAME          read one line from the inbound stream the
//                                peer kept with the line `keep:NAME`
//   GET /push?name=NAME&msg=TEXT write a line on that stream
//   GET /drop?name=NAME          close that stream
//   GET /abort                   reset this object: every stream it holds is
//                                reset with code 5
//   GET /seen                    the streams this node accepted, durably

const encoder = new TextEncoder();
const decoder = new TextDecoder();

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

// One reader and one writer for a stream, so several events can take turns
// with it.
class Lines {
  constructor(stream) {
    this.stream = stream;
    this.reader = stream.readable.getReader();
    this.writer = stream.writable.getWriter();
    this.buffer = "";
    this.closed = "pending";
    stream.closed.then(
      () => { this.closed = "resolved"; },
      (error) => { this.closed = "rejected: " + error.message; },
    );
  }

  async send(line) {
    await this.writer.write(encoder.encode(line + "\n"));
  }

  // The next line, or null once the peer has finished.
  async readLine() {
    for (;;) {
      const newline = this.buffer.indexOf("\n");
      if (newline >= 0) {
        const line = this.buffer.slice(0, newline);
        this.buffer = this.buffer.slice(newline + 1);
        return line;
      }
      const { value, done } = await this.reader.read();
      if (done) return null;
      this.buffer += decoder.decode(value, { stream: true });
    }
  }

  // How `stream.closed` settled, waiting a moment for it to.
  async settled() {
    await Promise.race([this.stream.closed.catch(() => {}), sleep(2000)]);
    return this.closed;
  }
}

export class Node {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env;
    // Changes when this object is evicted and constructed again.
    this.born = crypto.randomUUID();
    this.session = null; // the stream the alarm dialed
    this.dialed = null;
    this.extra = []; // streams `/open` keeps
    this.kept = new Map(); // inbound streams a peer asked this node to keep
  }

  // An inbound stream. The peer's node id is authenticated by Iroh and arrives
  // as `stream.remoteNodeId`; whether to serve it is this method's decision.
  // The stream outlives this method: it ends when it is closed, when both
  // directions finish, or when this object leaves the node.
  async onp2pstream(stream) {
    const session = { remote: stream.remoteNodeId, messages: [], end: null };
    const lines = new Lines(stream);
    let keep = false;
    try {
      for (;;) {
        const line = await lines.readLine();
        if (line === null) {
          session.end = "eof";
          return;
        }
        session.messages.push(line);
        if (line === "bye") {
          // Say authorization failed: reset the stream with a code.
          session.end = "closed-by-handler";
          await stream.close(300);
          return;
        }
        if (line === "done") {
          session.end = "finished";
          await lines.send("done:ok");
          await lines.writer.close();
          return;
        }
        if (line.startsWith("keep:")) {
          // Hand the stream to later requests: this handler returns and the
          // stream stays open, owned by this object.
          session.end = "kept";
          this.kept.set(line.slice("keep:".length), lines);
          keep = true;
          return;
        }
        await lines.send("echo:" + line);
      }
    } catch (error) {
      session.end = "error: " + error.message;
    } finally {
      const seen = (await this.ctx.storage.get("seen")) ?? [];
      seen.push(session);
      await this.ctx.storage.put("seen", seen);
      // A stream this handler is done with is closed, or it stays open (and
      // keeps this object in memory) until the peer does.
      if (!keep) await stream.close();
    }
  }

  // The alarm dials. The stream it opens is this object's, so it is still open
  // when the alarm returns and later requests use it.
  async alarm() {
    const target = await this.ctx.storage.get("target");
    if (!target) return;
    const stream = await this.env.P2P.connect(target.peer, { addrs: [target.addr] });
    this.session = new Lines(stream);
    await this.session.send("hello from the alarm");
    this.dialed = { reply: await this.session.readLine(), remote: stream.remoteNodeId };
  }

  async fetch(request) {
    const url = new URL(request.url);
    const param = (name, fallback = null) => url.searchParams.get(name) ?? fallback;
    switch (url.pathname) {
      case "/id":
        return Response.json({ nodeId: this.env.P2P.nodeId, born: this.born });
      case "/seen":
        return Response.json((await this.ctx.storage.get("seen")) ?? []);
      case "/dial":
        return this.dial(url);
      case "/arm": {
        this.dialed = null;
        await this.ctx.storage.put("target", { peer: param("peer"), addr: param("addr") });
        await this.ctx.storage.setAlarm(Date.now() + Number(param("delay", "200")));
        return Response.json({ armed: true });
      }
      case "/session": {
        const session = this.session;
        return Response.json({
          dialed: this.dialed,
          open: session !== null && session.closed === "pending",
          closed: session?.closed ?? null,
          born: this.born,
        });
      }
      case "/send": {
        if (!this.session) return Response.json({ error: "no session" }, { status: 409 });
        await this.session.send(param("msg"));
        if (param("reply", "1") === "0") return Response.json({ sent: true });
        return Response.json({ reply: await this.session.readLine() });
      }
      case "/recv": {
        if (!this.session) return Response.json({ error: "no session" }, { status: 409 });
        return Response.json({ line: await this.session.readLine() });
      }
      case "/hangup": {
        if (!this.session) return Response.json({ error: "no session" }, { status: 409 });
        const how = param("how", "close");
        if (how === "finish") await this.session.writer.close();
        else if (how === "reset") await this.session.stream.close(301);
        else await this.session.stream.close();
        return Response.json({ closed: await this.session.settled() });
      }
      case "/open": {
        if (param("close") !== null) {
          for (const lines of this.extra) await lines.stream.close();
          const count = this.extra.length;
          this.extra = [];
          return Response.json({ closed: count });
        }
        const outcomes = [];
        for (let index = 0; index < Number(param("n", "1")); index++) {
          try {
            const stream = await this.env.P2P.connect(param("peer"), { addrs: [param("addr")] });
            this.extra.push(new Lines(stream));
            outcomes.push("open");
          } catch (error) {
            outcomes.push("refused: " + error.message);
          }
        }
        return Response.json({ outcomes });
      }
      case "/pull": {
        const lines = this.kept.get(param("name"));
        if (!lines) return Response.json({ error: "no such stream" }, { status: 404 });
        return Response.json({ line: await lines.readLine() });
      }
      case "/abort":
        // Reset this object, as a crash would: its streams end with the
        // cell-ended code (5) and the peer's reads fail with it.
        this.ctx.abort("reset on request");
        return new Response("unreachable");
      case "/drop": {
        const lines = this.kept.get(param("name"));
        if (!lines) return Response.json({ error: "no such stream" }, { status: 404 });
        this.kept.delete(param("name"));
        await lines.stream.close();
        return Response.json({ dropped: true });
      }
      case "/push": {
        const lines = this.kept.get(param("name"));
        if (!lines) return Response.json({ error: "no such stream" }, { status: 404 });
        await lines.send("pushed:" + param("msg"));
        return Response.json({ pushed: true });
      }
      default:
        return new Response("not found", { status: 404 });
    }
  }

  // Connect, exchange lines and finish, all inside one request.
  async dial(url) {
    const peer = url.searchParams.get("peer");
    const addr = url.searchParams.get("addr");
    const msgs = (url.searchParams.get("msgs") ?? "hello").split(",");
    const finish = url.searchParams.get("finish") ?? "close";
    const result = { replies: [], end: null };
    let stream;
    let lines;
    try {
      stream = await this.env.P2P.connect(peer, addr ? { addrs: [addr] } : {});
      result.remoteNodeId = stream.remoteNodeId;
      lines = new Lines(stream);
      for (const message of msgs) {
        await lines.send(message);
        result.replies.push(await lines.readLine());
      }
      if (finish === "done" || finish === "bye") {
        await lines.send(finish);
        result.replies.push(await lines.readLine());
        result.end = "eof-after-" + finish;
        if (finish === "done") {
          // The peer finished its side (that was the reply and then
          // end-of-stream); finish ours, and the stream is over cleanly.
          await lines.readLine();
          await lines.writer.close();
        }
      } else if (finish === "close") {
        await stream.close();
        result.end = "closed";
      }
    } catch (error) {
      result.end = "error: " + error.message;
    } finally {
      if (lines) result.closed = await lines.settled();
      // `none` leaves the stream open on purpose: it belongs to this object
      // and stays until something closes it. Everything else is done.
      if (finish !== "none") await stream?.close();
    }
    return Response.json(result);
  }
}

export default {
  fetch(request, env) {
    return env.NODE.getByName("node").fetch(request);
  },
};
