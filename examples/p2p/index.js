// A line-oriented echo over Iroh. Every node runs the same code: the Durable
// Object `Node` accepts inbound streams (`onp2pstream`) and also dials out
// (`/dial`), so two nodes can talk in both directions.
//
//   GET /id                      this node's id
//   GET /dial?peer=ID&addr=IP:PORT&msgs=a,b,c&finish=close|done|bye|none
//                                connect to another node, send each message
//                                and read its reply, then finish as asked
//   GET /seen                    the streams this node accepted, durably

const encoder = new TextEncoder();
const decoder = new TextDecoder();

export class Node {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env;
  }

  // An inbound stream. The peer's node id is authenticated by Iroh and arrives
  // as `stream.remoteNodeId`; whether to serve it is this method's decision.
  // The stream lives until this promise settles, then it is closed.
  async onp2pstream(stream) {
    const session = { remote: stream.remoteNodeId, messages: [], end: null };
    const reader = stream.readable.getReader();
    const writer = stream.writable.getWriter();
    let buffer = "";
    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) {
          session.end = "eof";
          return;
        }
        buffer += decoder.decode(value, { stream: true });
        let newline;
        while ((newline = buffer.indexOf("\n")) >= 0) {
          const line = buffer.slice(0, newline);
          buffer = buffer.slice(newline + 1);
          session.messages.push(line);
          if (line === "bye") {
            // Say authorization failed: reset the stream with a code.
            session.end = "closed-by-handler";
            await stream.close(300);
            return;
          }
          if (line === "done") {
            session.end = "finished";
            await writer.write(encoder.encode("done:ok\n"));
            await writer.close();
            return;
          }
          await writer.write(encoder.encode("echo:" + line + "\n"));
        }
      }
    } catch (error) {
      session.end = "error: " + error.message;
    } finally {
      const seen = (await this.ctx.storage.get("seen")) ?? [];
      seen.push(session);
      await this.ctx.storage.put("seen", seen);
    }
  }

  async fetch(request) {
    const url = new URL(request.url);
    if (url.pathname === "/id") {
      return Response.json({ nodeId: this.env.P2P.nodeId });
    }
    if (url.pathname === "/seen") {
      return Response.json((await this.ctx.storage.get("seen")) ?? []);
    }
    if (url.pathname === "/dial") {
      const peer = url.searchParams.get("peer");
      const addr = url.searchParams.get("addr");
      const msgs = (url.searchParams.get("msgs") ?? "hello").split(",");
      const finish = url.searchParams.get("finish") ?? "close";
      const result = { replies: [], end: null };
      let stream;
      try {
        stream = await this.env.P2P.connect(peer, addr ? { addrs: [addr] } : {});
        result.remoteNodeId = stream.remoteNodeId;
        const reader = stream.readable.getReader();
        const writer = stream.writable.getWriter();
        let buffer = "";
        const readLine = async () => {
          for (;;) {
            const newline = buffer.indexOf("\n");
            if (newline >= 0) {
              const line = buffer.slice(0, newline);
              buffer = buffer.slice(newline + 1);
              return line;
            }
            const { value, done } = await reader.read();
            if (done) return null;
            buffer += decoder.decode(value, { stream: true });
          }
        };
        for (const message of msgs) {
          await writer.write(encoder.encode(message + "\n"));
          result.replies.push(await readLine());
        }
        if (finish === "done" || finish === "bye") {
          await writer.write(encoder.encode(finish + "\n"));
          result.replies.push(await readLine());
          result.end = "eof-after-" + finish;
        } else if (finish === "close") {
          await stream.close();
          result.end = "closed";
        }
      } catch (error) {
        result.end = "error: " + error.message;
      } finally {
        await stream?.close();
      }
      return Response.json(result);
    }
    return new Response("not found", { status: 404 });
  }
}

export default {
  fetch(request, env) {
    return env.NODE.getByName("node").fetch(request);
  },
};
