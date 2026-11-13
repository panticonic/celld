// A versioned notes store on a Dolt cell. Each Repo object is a DoltLite
// prolly store: ctx.storage.sql reads and writes the active branch's working
// set, and ctx.storage.dolt commits, branches, merges and travels history.
//
//   GET /<op>?repo=<name>&...   (see `routes` below)
export class Repo {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.sql = ctx.storage.sql;
    this.dolt = ctx.storage.dolt;
    this.sql.exec(
      "CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)",
    );
  }

  notes() {
    return this.sql.exec("SELECT id, body FROM notes ORDER BY id").toArray();
  }

  async fetch(request) {
    const url = new URL(request.url);
    const q = (name, fallback = null) => url.searchParams.get(name) ?? fallback;
    const op = url.pathname.slice(1);
    const dolt = this.dolt;
    switch (op) {
      case "engine":
        return Response.json({ dolt: dolt !== undefined, branch: dolt.branch.current() });
      case "add": {
        this.sql.exec("INSERT INTO notes(body) VALUES (?)", q("body", "note"));
        return Response.json({ branch: dolt.branch.current(), notes: this.notes() });
      }
      case "edit": {
        this.sql.exec("UPDATE notes SET body = ? WHERE id = ?", q("body", ""), Number(q("id", "1")));
        return Response.json({ branch: dolt.branch.current(), notes: this.notes() });
      }
      case "notes":
        return Response.json({ branch: dolt.branch.current(), notes: this.notes() });
      case "commit": {
        const hash = dolt.commit({
          message: q("message", "commit"),
          author: q("author", undefined) ?? undefined,
          date: q("date", undefined) ?? undefined,
        });
        return Response.json({ hash });
      }
      case "status":
        return Response.json(dolt.status());
      case "branch": {
        const name = q("name");
        if (name) dolt.branch.create(name, { from: q("from", undefined) ?? undefined });
        return Response.json(dolt.branch.list());
      }
      case "delete-branch":
        dolt.branch.delete(q("name"), { force: q("force") === "1" });
        return Response.json(dolt.branch.list());
      case "checkout":
        dolt.checkout(q("name"), { create: q("create") === "1" });
        return Response.json({ branch: dolt.branch.current(), notes: this.notes() });
      case "merge":
        return Response.json(dolt.merge(q("name")));
      case "reset":
        return Response.json({
          head: dolt.reset({ hard: q("hard") === "1", to: q("to", undefined) ?? undefined }),
          notes: this.notes(),
        });
      case "log":
        return Response.json(dolt.log({ limit: Number(q("limit", "20")) }));
      case "diff":
        return Response.json(
          dolt.diff({ from: q("from", "HEAD~1"), to: q("to", "HEAD"), table: q("table", undefined) ?? undefined }),
        );
      case "hash":
        return Response.json({ hash: dolt.hashOf(q("ref", "HEAD")) });
      case "kv": {
        // celld's own runtime tables (KV, alarms) are not versioned: they
        // survive checkout, reset and merge, and never enter a commit.
        const value = q("set");
        if (value != null) await this.ctx.storage.put("kv", value);
        return Response.json({ kv: (await this.ctx.storage.get("kv")) ?? null });
      }
      case "gc":
        return Response.json({ gc: dolt.gc() });
      case "sql-versioning": {
        // Application SQL cannot reach the versioning surface.
        try {
          this.sql.exec("SELECT dolt_commit('-A', '-m', 'sneaky')");
          return Response.json({ denied: false });
        } catch (error) {
          return Response.json({ denied: true, error: String(error.message) });
        }
      }
      default:
        return new Response("unknown op " + op, { status: 404 });
    }
  }
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const name = url.searchParams.get("repo") ?? "default";
    if (url.pathname === "/fork") {
      // A new object whose state starts from `repo` (optionally at a commit).
      const at = url.searchParams.get("at") ?? undefined;
      const id = await env.REPO.fork(env.REPO.idFromName(name), { at });
      const notes = await env.REPO.get(id).fetch("https://repo/notes");
      return Response.json({ id: id.toString(), fork: await notes.json() });
    }
    if (url.pathname.startsWith("/byid/")) {
      const [, , id, op] = url.pathname.split("/");
      return env.REPO.get(env.REPO.idFromString(id)).fetch(
        new Request("https://repo/" + op + url.search, request),
      );
    }
    return env.REPO.get(env.REPO.idFromName(name)).fetch(request);
  },
};
