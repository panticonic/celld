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
    // A local table (wrangler.jsonc, celld.durable_objects.Repo.doltLocal).
    this.sql.exec(
      "CREATE TABLE IF NOT EXISTS outbox_msgs (id INTEGER PRIMARY KEY, body TEXT NOT NULL)",
    );
  }

  outbox() {
    return this.sql.exec("SELECT id, body FROM outbox_msgs ORDER BY id").toArray();
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
      case "delete": {
        this.sql.exec("DELETE FROM notes WHERE id = ?", Number(q("id", "1")));
        return Response.json({ branch: dolt.branch.current(), notes: this.notes() });
      }
      case "notes":
        return Response.json({ branch: dolt.branch.current(), notes: this.notes() });
      case "history": {
        // Reads of other commits and branches, without a checkout, through
        // ctx.storage.sql and ctx.storage.dolt.query. Nothing of it is writable.
        const other = q("other", "other");
        if (!/^[A-Za-z0-9_-]+$/.test(other)) return new Response("bad branch", { status: 400 });
        const rows = (sql) => this.sql.exec(sql).toArray();
        const text = (result) => result.rows.map((row) => row.map((v) => (typeof v === "bigint" ? String(v) : v)));
        const denied = (sql) => {
          try {
            this.sql.exec(sql).toArray();
            return "allowed";
          } catch (error) {
            return "denied";
          }
        };
        return Response.json({
          asOfParent: rows("SELECT id, body FROM dolt_at_notes('HEAD~1') ORDER BY id"),
          asOfBranch: rows(`SELECT id, body FROM dolt_at_notes('${other}') ORDER BY id`),
          versionsOfRow1: rows("SELECT count(*) AS n FROM dolt_history_notes WHERE id = 1")[0].n,
          diff: rows("SELECT diff_type, from_body, to_body FROM dolt_diff_notes WHERE to_commit = dolt_hashof('HEAD') ORDER BY diff_type"),
          blame: rows("SELECT * FROM dolt_blame_notes WHERE id = 1").map((row) => Object.values(row)),
          mergeBase: rows(`SELECT dolt_merge_base('main', '${other}') AS h`)[0].h,
          query: {
            asOfParent: text(dolt.query("SELECT body FROM dolt_at_notes('HEAD~1') ORDER BY id")),
            mergeBase: text(dolt.query(`SELECT dolt_merge_base('main', '${other}')`)),
            blameRows: text(dolt.query("SELECT count(*) FROM dolt_blame_notes")),
          },
          writes: {
            history: denied("DELETE FROM dolt_history_notes"),
            ignore: denied("INSERT INTO dolt_ignore(pattern, ignored) VALUES ('x_%', 1)"),
            branches: denied("UPDATE dolt_branches SET name = 'x'"),
            commit: denied("SELECT dolt_commit('-A', '-m', 'x')"),
            resolve: denied("SELECT dolt_conflicts_resolve('--ours', 'notes')"),
          },
        });
      }
      case "outbox": {
        // Local tables: written like any table, committed by no commit.
        if (q("add") != null) this.sql.exec("INSERT INTO outbox_msgs(body) VALUES (?)", q("add"));
        return Response.json({ branch: dolt.branch.current(), outbox: this.outbox(), status: dolt.status() });
      }
      case "outbox-fill": {
        // Many local rows in one transaction, for the cost of a checkout.
        const n = Number(q("n", "1000"));
        this.ctx.storage.transactionSync(() => {
          for (let i = 0; i < n; i++)
            this.sql.exec("INSERT INTO outbox_msgs(body) VALUES (?)", `message ${i} ${"x".repeat(32)}`);
        });
        return Response.json({ count: this.sql.exec("SELECT count(*) AS n FROM outbox_msgs").one().n });
      }
      case "outbox-count":
        return Response.json({
          branch: dolt.branch.current(),
          count: this.sql.exec("SELECT count(*) AS n FROM outbox_msgs").one().n,
        });
      case "commit": {
        const hash = dolt.commit({
          message: q("message", "commit"),
          author: q("author", undefined) ?? undefined,
          date: q("date") == null ? undefined : Number(q("date")),
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
      case "query": {
        // Exact local reads: INTEGER comes back as a bigint, so it is
        // rendered as a string here, and typeof shows the storage class.
        let result;
        try {
          result = dolt.query(q("sql"));
        } catch (error) {
          return Response.json({ error: error.code ?? String(error.message) });
        }
        return Response.json({
          columns: result.columns,
          rows: result.rows.map((row) =>
            row.map((v) => (typeof v === "bigint" ? { int: String(v) } : v instanceof Uint8Array ? { blob: v.length } : v))),
        });
      }
      case "txn": {
        // beginTransactionSync(): separate begin and commit/rollback calls.
        this.sql.exec("CREATE TABLE IF NOT EXISTS txn_probe (x INTEGER)");
        const kept = this.ctx.storage.beginTransactionSync();
        this.sql.exec("INSERT INTO txn_probe VALUES (1)");
        kept.commit();
        const dropped = this.ctx.storage.beginTransactionSync();
        this.sql.exec("INSERT INTO txn_probe VALUES (2)");
        dropped.rollback();
        return Response.json({ rows: this.sql.exec("SELECT x FROM txn_probe ORDER BY x").toArray() });
      }
      case "exec": {
        // Deterministic programs (Chronolog Spec 10): POST a JSON body
        // { program, context }; the answer is the outcome plus the rows a
        // probe query reads afterwards.
        // JSON cannot carry a 64-bit integer, an integral REAL or bytes, so
        // the body tags values ({ t: "int" | "real" | "blob" | ..., v }) and
        // they are turned back into the JS values execute() maps exactly.
        const { program, context, probe } = await request.json();
        const value = ({ t, v }) =>
          t === "int" ? BigInt(v)
          : t === "real" ? Float64Array.of(v)
          : t === "blob" ? Uint8Array.from(atob(v.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0))
          : t === "null" ? null
          : v;
        for (const step of program)
          for (const binding of step.bindings ?? []) binding.value = value(binding.value);
        if (typeof context.actor === "string")
          context.actor = value({ t: "blob", v: context.actor });
        const options = {};
        if (url.searchParams.get("log"))
          options.log = {
            sql: "INSERT INTO exec_log(outcome, code, idx) VALUES (:outcome, :rejection_code, :failing_statement_index)",
          };
        if (options.log)
          this.sql.exec("CREATE TABLE IF NOT EXISTS exec_log (outcome TEXT, code TEXT, idx INTEGER)");
        const outcome = dolt.execute(program, context, options);
        const rows = probe ? this.sql.exec(probe).toArray() : [];
        return Response.json({ outcome, rows, digest: dolt.engineDigest() });
      }
      case "tx": {
        // transactionSync(fn, { commit }): the writes and the Dolt commit
        // are one durable step; a throw anywhere rolls both back.
        const body = q("body", "note");
        try {
          const result = this.ctx.storage.transactionSync(
            () => {
              this.sql.exec("INSERT INTO notes(body) VALUES (?)", body);
              if (q("fail") === "after-write") throw new Error("declined");
              return this.notes().length;
            },
            {
              // A function renders the message from what the callback wrote.
              commit: q("fn") === "1"
                ? (count) => ({ message: `${count} notes`, author: q("author", undefined) ?? undefined })
                : {
                    message: q("message", "tx"),
                    author: q("author", undefined) ?? undefined,
                    date: q("date") == null ? undefined : Number(q("date")),
                  },
            },
          );
          return Response.json({ result, head: dolt.hashOf("HEAD"), status: dolt.status(), notes: this.notes() });
        } catch (error) {
          return Response.json({
            error: String(error.message), head: dolt.hashOf("HEAD"), status: dolt.status(), notes: this.notes(),
          });
        }
      }
      case "tx-async": {
        try {
          const result = await this.ctx.storage.transaction(
            async () => {
              this.sql.exec("INSERT INTO notes(body) VALUES (?)", q("body", "async"));
              await Promise.resolve();
              return "ran";
            },
            { commit: (result) => `async transaction ${result}` },
          );
          return Response.json({ result, log: dolt.log({ limit: 1 }), status: dolt.status() });
        } catch (error) {
          return Response.json({ error: String(error.message) });
        }
      }
      case "tx-nested": {
        // { commit } belongs to the outermost transaction.
        try {
          this.ctx.storage.transactionSync(() => {
            this.ctx.storage.transactionSync(() => {}, { commit: { message: "nested" } });
          });
          return Response.json({ error: null });
        } catch (error) {
          return Response.json({ error: String(error.message), log: dolt.log({ limit: 1 }) });
        }
      }
      case "merge-resolve": {
        // dolt.merge(name, { resolve }): conflicts are resolved inside the
        // merge's own transaction, then one merge commit.
        const mode = q("mode", "sql");
        let seen = null;
        try {
          const result = dolt.merge(q("name"), {
            message: q("message", "merge"),
            author: q("author", undefined) ?? undefined,
            date: q("date") == null ? undefined : Number(q("date")),
            fastForward: q("ff") !== "0",
            resolve: (merge) => {
              seen = {
                branch: merge.branch,
                conflicts: merge.conflicts.map(({ table, key, base, ours, theirs, ourChange, theirChange }) =>
                  ({ table, key, base, ours, theirs, ourChange, theirChange })),
                violations: merge.violations.map(({ table, type, row }) => ({ table, type, row })),
                // The conflicts are readable through plain SQL, and not writable.
                sqlConflicts: this.sql.exec("SELECT count(*) AS n FROM dolt_conflicts_notes").one().n,
              };
              try {
                this.sql.exec("DELETE FROM dolt_conflicts_notes");
                seen.sqlWrite = "allowed";
              } catch (error) {
                seen.sqlWrite = "denied";
              }
              if (mode === "throw") {
                this.sql.exec("UPDATE notes SET body = 'half resolved'");
                throw new Error("declined");
              }
              if (mode === "leave") return;
              if (mode === "async") return Promise.resolve();
              for (const conflict of merge.conflicts) {
                if (mode === "theirs") conflict.takeTheirs();
                else if (mode === "ours") conflict.keepOurs();
                else {
                  this.sql.exec(
                    "UPDATE notes SET body = ? WHERE id = ?",
                    `${conflict.ours?.body} + ${conflict.theirs?.body}`, conflict.key.id);
                  conflict.resolve();
                }
              }
              for (const violation of merge.violations) violation.resolve();
            },
          });
          return Response.json({ result, seen, notes: this.notes(), status: dolt.status(), log: dolt.log({ limit: 3 }) });
        } catch (error) {
          return Response.json({
            error: String(error.message),
            seen,
            notes: this.notes(),
            status: dolt.status(),
            head: dolt.hashOf("HEAD"),
            openConflicts: this.sql.exec("SELECT * FROM dolt_conflicts").toArray(),
          });
        }
      }
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
