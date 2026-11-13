// Fleet fault-injection test application.
//
// One Durable Object class, `Ledger`: an append-only log in SQLite. Every
// append is a single synchronous SQL write that assigns the next sequence
// number. The HTTP response (the "acknowledgement") is only produced after
// that write, so celld's output gate holds it until the write is durable.
//
//   POST /append/<cell>?op=<unique op id>[&pad=<bytes>]
//        -> 200 {"cell","op","seq","inst","did","ts"}       (the acknowledgement)
//   GET  /head/<cell>   -> {"cell","head","count","inst"}  (a read; also gated)
//   GET  /dump/<cell>   -> {"cell","inst","rows":[[seq,op,inst,ts],...]}
//
// `inst` is a random id minted when the DO object is constructed in a process,
// so the checker can see how many incarnations served a cell.
export class Ledger {
  constructor(state, env) {
    this.state = state;
    this.sql = state.storage.sql;
    this.inst = crypto.randomUUID().slice(0, 8);
    this.sql.exec(
      "CREATE TABLE IF NOT EXISTS ledger (" +
        "seq INTEGER PRIMARY KEY, op TEXT NOT NULL UNIQUE, inst TEXT NOT NULL, " +
        "ts INTEGER NOT NULL, pad BLOB)",
    );
  }

  async fetch(request) {
    const url = new URL(request.url);
    const parts = url.pathname.split("/").filter(Boolean);
    const verb = parts[0];
    const cell = decodeURIComponent(parts[1] ?? "");
    if (verb === "append" && request.method === "POST") {
      const op = url.searchParams.get("op");
      if (!op) return new Response("missing op", { status: 400 });
      const padLen = Math.min(parseInt(url.searchParams.get("pad") ?? "0", 10) || 0, 65536);
      const pad = padLen > 0 ? new Uint8Array(padLen) : null;
      // No await between computing the next seq and writing it, so the append
      // is atomic with respect to other requests on this object. The PRIMARY
      // KEY rejects a duplicate seq.
      const row = this.sql
        .exec(
          "INSERT INTO ledger (seq, op, inst, ts, pad) " +
            "VALUES ((SELECT COALESCE(MAX(seq), 0) + 1 FROM ledger), ?, ?, ?, ?) RETURNING seq",
          op,
          this.inst,
          Date.now(),
          pad,
        )
        .one();
      return Response.json({ cell, op, seq: row.seq, inst: this.inst, did: this.state.id.toString(), ts: Date.now() });
    }
    if (verb === "head") {
      const r = this.sql
        .exec("SELECT COALESCE(MAX(seq),0) AS head, COUNT(*) AS n FROM ledger")
        .one();
      return Response.json({ cell, head: r.head, count: r.n, inst: this.inst, did: this.state.id.toString() });
    }
    if (verb === "dump") {
      const rows = this.sql.exec("SELECT seq, op, inst, ts FROM ledger ORDER BY seq").toArray();
      return Response.json({
        cell,
        inst: this.inst,
        rows: rows.map((r) => [r.seq, r.op, r.inst, r.ts]),
      });
    }
    return new Response("not found", { status: 404 });
  }
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/ping") return new Response("pong");
    const parts = url.pathname.split("/").filter(Boolean);
    if (parts.length < 2) return new Response("not found", { status: 404 });
    const id = env.LEDGER.idFromName(decodeURIComponent(parts[1]));
    return env.LEDGER.get(id).fetch(request);
  },
};
