import { DurableObject } from "cloudflare:workers";

// Dolt facets: a supervisor object (an ordinary SQLite class) runs one
// versioned workspace per name as a facet whose class a Worker Loader
// provides, the way an application loads user code without a redeploy.
// `celld: { storage: "dolt" }` in the startup options makes the facet a
// DoltLite prolly store with ctx.storage.dolt; the engine is fixed when the
// facet is created.
//
//   GET /<op>?ws=<name>&...    a workspace op (see WORKSPACE below)
//   GET /fork?ws=<a>&to=<b>[&at=<rev>]   ctx.facets.fork(a, b, { at })
//   GET /abort?ws=<a>  GET /delete?ws=<a>
//   GET /as-sqlite?ws=<a>   restart <a> naming the other engine (refused)
//   GET /plain              an ordinary SQLite facet beside the Dolt ones

// The workspace's code. A real application loads it from a bundle or a
// database; here it is a string so the example is one file.
const WORKSPACE = `
import { DurableObject } from "cloudflare:workers";
export class Workspace extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    this.dolt = ctx.storage.dolt;
    if (this.dolt)
      this.sql.exec("CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)");
  }
  notes() {
    return this.sql.exec("SELECT id, body FROM notes ORDER BY id").toArray();
  }
  async fetch(request) {
    const url = new URL(request.url);
    const q = (name, fallback = null) => url.searchParams.get(name) ?? fallback;
    const op = url.pathname.slice(1);
    const dolt = this.dolt;
    const json = (value) => Response.json(value);
    switch (op) {
      case "engine":
        return json({ dolt: dolt !== undefined, branch: dolt ? dolt.branch.current() : null });
      case "add":
        this.sql.exec("INSERT INTO notes(body) VALUES (?)", q("body", "note"));
        return json({ branch: dolt.branch.current(), notes: this.notes() });
      case "edit":
        this.sql.exec("UPDATE notes SET body = ? WHERE id = ?", q("body", ""), Number(q("id", "1")));
        return json({ notes: this.notes() });
      case "notes":
        return json({ branch: dolt.branch.current(), notes: this.notes() });
      case "commit":
        return json({ hash: dolt.commit({ message: q("message", "commit") }) });
      case "branch":
        if (q("name")) dolt.branch.create(q("name"));
        return json(dolt.branch.list());
      case "checkout":
        dolt.checkout(q("name"), { create: q("create") === "1" });
        return json({ branch: dolt.branch.current(), notes: this.notes() });
      case "merge":
        return json(dolt.merge(q("name")));
      case "log":
        return json(dolt.log({ limit: Number(q("limit", "50")) }));
      case "hash":
        return json({ hash: dolt.hashOf(q("ref", "HEAD")) });
      case "kv": {
        if (q("set") != null) await this.ctx.storage.put("kv", q("set"));
        return json({ kv: (await this.ctx.storage.get("kv")) ?? null });
      }
      case "gc":
        return json({ gc: dolt.gc() });
      case "exec": {
        // A deterministic program: pinned clock and PRNG, so two facets fed
        // the same program reach the same rows.
        const outcome = dolt.execute(
          [
            { sql: "CREATE TABLE IF NOT EXISTS ledger (id INTEGER PRIMARY KEY, at TEXT, r INTEGER)" },
            { sql: "INSERT INTO ledger(at, r) VALUES (datetime('now'), random())" },
          ],
          { nowMs: 1700000000000n, seed: 42n, actor: new Uint8Array([1]), schemaAdmin: true },
        );
        const rows = this.sql.exec("SELECT at, r FROM ledger ORDER BY id").toArray()
          .map((row) => ({ at: row.at, r: String(row.r) }));
        return json({ outcome, rows });
      }
      default:
        return new Response("unknown op " + op, { status: 404 });
    }
  }
}
`;

export class Supervisor extends DurableObject {
  workspaceClass() {
    const worker = this.env.LOADER.get("workspace-v1", () => ({
      compatibilityDate: "2026-01-01",
      mainModule: "workspace.js",
      modules: { "workspace.js": WORKSPACE },
    }));
    return worker.getDurableObjectClass("Workspace");
  }

  workspace(name, storage = "dolt") {
    return this.ctx.facets.get("ws:" + name, () => ({
      class: this.workspaceClass(),
      celld: { storage },
    }));
  }

  async fetch(request) {
    const url = new URL(request.url);
    const ws = url.searchParams.get("ws") ?? "main";
    switch (url.pathname) {
      case "/fork": {
        const to = url.searchParams.get("to");
        const at = url.searchParams.get("at") ?? undefined;
        let forked;
        try {
          forked = await this.ctx.facets.fork("ws:" + ws, "ws:" + to, { at });
        } catch (error) {
          return Response.json({ error: String(error.message ?? error) }, { status: 409 });
        }
        const notes = await this.workspace(to).fetch("https://ws/notes");
        return Response.json({ forked, fork: await notes.json() });
      }
      case "/abort":
        this.ctx.facets.abort("ws:" + ws, new Error("aborted"));
        return Response.json({ aborted: ws });
      case "/delete":
        this.ctx.facets.delete("ws:" + ws);
        return Response.json({ deleted: ws });
      case "/as-sqlite": {
        // The engine is fixed when the facet is created: a start that names
        // the other engine is refused, and the data is untouched.
        this.ctx.facets.abort("ws:" + ws, new Error("restart"));
        try {
          await this.workspace(ws, "sqlite").fetch("https://ws/engine");
          return Response.json({ refused: false });
        } catch (error) {
          return Response.json({ refused: true, error: String(error.message ?? error) });
        } finally {
          this.ctx.facets.abort("ws:" + ws, new Error("restart"));
        }
      }
      case "/plain":
        return this.workspace("plain", "sqlite").fetch("https://ws/engine");
      default:
        return this.workspace(ws).fetch(request);
    }
  }
}

export default {
  fetch(request, env) {
    return env.SUPERVISOR.getByName("only").fetch(request);
  },
};
