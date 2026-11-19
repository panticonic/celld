import { DurableObject } from "cloudflare:workers";

// One wire request and one response per stream. Each writer only closes its
// own half so its peer can read to EOF and reply.
async function readMessage(stream) {
  const chunks = [];
  let size = 0;
  for await (const chunk of stream.readable) {
    size += chunk.length;
    if (size > 1024 * 1024) throw new Error("The example message exceeded one MiB");
    chunks.push(chunk);
  }
  const data = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) {
    data.set(chunk, offset);
    offset += chunk.length;
  }
  return data;
}

async function writeMessage(stream, bytes) {
  const writer = stream.writable.getWriter();
  try {
    await writer.write(bytes);
    await writer.close();
  } finally {
    writer.releaseLock();
  }
}

export class Carrier extends DurableObject {
  async onp2pstream(stream) {
    try {
      const request = await readMessage(stream);
      const response = await this.env.GRAPH.getByName("workspace").serveRemote(
        request, stream.remoteNodeId,
      );
      await writeMessage(stream, response);
    } catch (error) {
      await stream.close(256);
      throw error;
    }
  }
}

export class Graph extends DurableObject {
  serveRemote(bytes, peer) {
    return this.ctx.storage.dolt.remote.serve(bytes, { peer });
  }

  async fetch(request) {
    const { pathname } = new URL(request.url);
    const { dolt, sql } = this.ctx.storage;
    try {
      if (pathname === "/state") {
        return Response.json({
          head: dolt.hashOf("HEAD"),
          notes: sql.exec("SELECT body FROM notes ORDER BY id").toArray().map((row) => row.body),
        });
      }
      const input = await request.json();
      switch (pathname) {
        case "/commit":
          sql.exec("CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)");
          sql.exec("INSERT INTO notes (id, body) VALUES (?, ?)", input.id, input.body);
          return Response.json({ head: dolt.commit({ message: "Write note" }) });
        case "/grant":
          return Response.json(dolt.remote.grant({
            label: input.label, peer: input.peer, fetch: true, branches: ["main"], push: ["main"],
          }));
        case "/revoke":
          return Response.json(dolt.remote.revoke(input.id));
        case "/remote":
          if (dolt.remote.list().some(({ name }) => name === "origin")) dolt.remote.remove("origin");
          return Response.json(dolt.remote.add("origin", { id: "Graph:workspace", token: input.token }));
        case "/fetch":
        case "/push": {
          const transport = async (message) => {
            const stream = await this.env.P2P.connect(input.peer, { addrs: [input.address] });
            try {
              await writeMessage(stream, message);
              const response = await readMessage(stream);
              await stream.closed;
              return response;
            } catch (error) {
              await stream.close(256);
              throw error;
            }
          };
          if (pathname === "/push") return Response.json(await dolt.push("origin", "main", { transport }));
          const fetched = await dolt.fetch("origin", { branches: ["main"], transport });
          dolt.reset({ hard: true, to: fetched.branches.main });
          return Response.json(fetched);
        }
        default:
          return new Response("Not found", { status: 404 });
      }
    } catch (error) {
      return Response.json({ code: error.code ?? null, message: error.message }, { status: 400 });
    }
  }
}

export default {
  fetch(request, env) {
    if (new URL(request.url).pathname === "/id") return Response.json({ nodeId: env.P2P.nodeId });
    return env.GRAPH.getByName("workspace").fetch(request);
  },
};
