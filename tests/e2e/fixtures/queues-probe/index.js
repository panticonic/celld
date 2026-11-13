import { DurableObject } from "cloudflare:workers";

export class Recorder extends DurableObject {
  async add(rec) {
    const recs = (await this.ctx.storage.get("recs")) ?? [];
    recs.push(rec);
    await this.ctx.storage.put("recs", recs);
  }
  async all() {
    return (await this.ctx.storage.get("recs")) ?? [];
  }
}

const rec = (env) => env.REC.getByName("r");

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const q = url.searchParams;

    // GET /records -> everything the consumer recorded, in order of receipt.
    if (url.pathname === "/records") return Response.json(await rec(env).all());

    // POST /send?n=N&kind=K  (K goes into every body); ?delay=S sets delaySeconds.
    if (url.pathname === "/send") {
      const n = Number(q.get("n") ?? 1);
      const kind = q.get("kind") ?? "ok";
      const delaySeconds = q.has("delay") ? Number(q.get("delay")) : undefined;
      const sentAt = Date.now();
      const msgs = Array.from({ length: n }, (_, i) => ({
        body: { kind, i, sentAt },
        ...(delaySeconds !== undefined ? { delaySeconds } : {}),
      }));
      if (n === 1) await env.Q.send(msgs[0].body, delaySeconds !== undefined ? { delaySeconds } : undefined);
      else await env.Q.sendBatch(msgs);
      return Response.json({ sent: n, kind, sentAt });
    }

    // POST /send-typed?ct=text|json|bytes|v8
    if (url.pathname === "/send-typed") {
      const ct = q.get("ct");
      const body =
        ct === "text" ? "plain text ✓"
        : ct === "bytes" ? new Uint8Array([1, 2, 3, 250])
        : ct === "v8" ? { when: new Date(0), set: new Set([1, 2]), nested: { a: [1, { b: 2 }] } }
        : { ct, hello: "world" };
      await env.Q.send(body, { contentType: ct });
      return Response.json({ sent: ct });
    }
    return new Response("not found", { status: 404 });
  },

  async queue(batch, env) {
    const at = Date.now();
    const messages = batch.messages.map((m) => {
      const b = m.body;
      let shape;
      if (typeof b === "string") shape = { type: "string", value: b };
      else if (b instanceof Uint8Array) shape = { type: "Uint8Array", value: Array.from(b) };
      else if (b && b.set instanceof Set) shape = { type: "v8", when: b.when.getTime(), set: [...b.set], nested: b.nested };
      else shape = { type: "object", value: b };
      return { id: m.id, attempts: m.attempts, shape };
    });
    await rec(env).add({ queue: batch.queue, size: batch.messages.length, at, messages });

    for (const m of batch.messages) {
      const kind = m.body?.kind;
      if (batch.queue === "probe-dlq") m.ack();
      else if (kind === "retry") m.retry({ delaySeconds: 1 }); // always: ends in the DLQ
      else if (kind === "throw-once" && m.attempts === 1) throw new Error("boom once");
      else if (kind === "retry-once" && m.attempts === 1) m.retry({ delaySeconds: 2 });
      else m.ack();
    }
  },
};
