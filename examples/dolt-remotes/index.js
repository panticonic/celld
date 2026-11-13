// Dolt remotes between cells: decentralized work on one graph.
//
// A `Graph` object is a Dolt cell. One of them is the shared graph ("gad").
// Agents work on forks of it, in parallel and without asking anyone, and merge
// back with fetch, pull (merge with a resolver) and push. The shared graph
// never serializes their work: it only moves a branch when a push is a
// fast-forward.
//
//   GET /fork?from=gad&to=agent-a         a new cell that starts from `from`
//   GET /link?source=gad&holder=agent-a   the source grants, the holder adds the remote
//   GET /<op>?g=<name>&...                an operation on one cell (see `ops`)
//
// `Plain` is a Dolt class that did not declare `doltRemotes`: it has no
// `ctx.storage.dolt.remote`, and it is never fetched from.
const AUTHOR = "Agent <agent@example.com>";
const DATE = 1700000000000;

import { DurableObject } from "cloudflare:workers";

export class Graph {
  constructor(ctx, env) {
    this.init(ctx, env);
  }

  init(ctx, env) {
    this.ctx = ctx;
    this.env = env;
    this.sql = ctx.storage.sql;
    this.dolt = ctx.storage.dolt;
    this.sql.exec("CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)");
  }

  notes() {
    return this.sql.exec("SELECT id, body FROM notes ORDER BY id").toArray();
  }

  state() {
    return {
      branch: this.dolt.branch.current(),
      head: this.dolt.hashOf("HEAD"),
      branches: Object.fromEntries(this.dolt.branch.list().map((b) => [b.name, b.hash])),
      tracking: this.dolt.remote.branches().map((row) => row.name + "=" + row.hash.slice(0, 8)),
      notes: this.notes(),
      log: this.dolt.log({ limit: 20 }).map((c) => c.message),
      remotes: this.dolt.remote.list(),
      grants: this.dolt.remote.grants().map((g) => g.label),
      status: this.dolt.status(),
    };
  }

  async fetch(request) {
    const url = new URL(request.url);
    const q = (name, fallback = null) => url.searchParams.get(name) ?? fallback;
    const list = (name) => (q(name) === null || q(name) === "" ? [] : q(name).split(","));
    const dolt = this.dolt;
    try {
      switch (url.pathname.slice(1)) {
        case "state":
          return Response.json(this.state());
        case "set":
          this.sql.exec(
            "INSERT INTO notes(id, body) VALUES (?, ?) ON CONFLICT(id) DO UPDATE SET body = excluded.body",
            Number(q("id")), q("body", ""),
          );
          return Response.json({ notes: this.notes() });
        case "commit":
          return Response.json({ hash: dolt.commit({ message: q("message", "work"), author: AUTHOR, date: DATE }) });
        case "branch":
          dolt.branch.create(q("name"), { from: q("from") ?? undefined });
          return Response.json({ branches: dolt.branch.list().map((b) => b.name) });
        case "checkout":
          dolt.checkout(q("name"), { create: q("create") === "1" });
          return Response.json({ branch: dolt.branch.current() });
        case "hash":
          return Response.json({ hash: dolt.hashOf(q("ref", "HEAD")) });
        // The source's side: let a holder fetch and push.
        case "grant":
          return Response.json(dolt.remote.grant({
            label: q("label", ""), fetch: q("fetch", "1") === "1",
            push: list("push"), branches: list("branches"),
            expiresAt: q("expiresAt") === null ? undefined : Number(q("expiresAt")),
          }));
        case "revoke":
          return Response.json(dolt.remote.revoke(q("id")));
        // The holder's side.
        case "remote-add":
          return Response.json(dolt.remote.add(q("name", "origin"), {
            id: this.env.GRAPH.idFromString(q("source")), token: q("token"),
          }));
        case "remote-add-raw":
          return Response.json(dolt.remote.add(q("name", "origin"), { id: q("ref"), token: q("token") }));
        case "remote-remove":
          return Response.json(dolt.remote.remove(q("name", "origin")));
        case "fetch":
          return Response.json(await dolt.fetch(q("remote", "origin"), {
            branches: list("branches"),
            transport: q("transport") === "loopback" ? this.loopback(q("source")) : undefined,
          }));
        case "push":
          return Response.json(await dolt.push(q("remote", "origin"), q("branch", "main"), {
            to: q("to") ?? undefined,
            transport: q("transport") === "loopback" ? this.loopback(q("source")) : undefined,
          }));
        case "pull": {
          // A text merge of the two sides of a conflicting note, so that the
          // conflict is resolved by what both agents wrote.
          const mode = q("resolve", "text");
          const result = await dolt.pull(q("remote", "origin"), q("branch", "main"), {
            message: q("message", "merge"), author: AUTHOR, date: DATE,
            resolve: mode === "none" ? undefined : (merge) => {
              for (const conflict of merge.conflicts) {
                if (mode === "ours") conflict.keepOurs();
                else if (mode === "theirs") conflict.takeTheirs();
                else {
                  this.sql.exec(
                    "UPDATE notes SET body = ? WHERE id = ?",
                    [conflict.ours.body, conflict.theirs.body].sort().join(" + "), conflict.key.id,
                  );
                  conflict.resolve();
                }
              }
            },
          });
          return Response.json(result);
        }
        // The source's handler for a carrier of the application's own: this
        // is all a P2P stream handler would do.
        case "serve": {
          const bytes = new Uint8Array(await request.arrayBuffer());
          return new Response(dolt.remote.serve(bytes, { peer: q("peer") ?? undefined }));
        }
        case "has-remote-api":
          return Response.json({ remote: dolt.remote !== undefined, fetch: typeof dolt.fetch });
        default:
          return new Response("unknown op", { status: 404 });
      }
    } catch (error) {
      return Response.json({ error: String(error.message ?? error), code: error.code ?? null }, { status: 200 });
    }
  }

  // A carrier of the application's own: the source is reached through its
  // `/serve` route. A stream of the P2P binding would carry the same bytes.
  loopback(source) {
    const stub = this.env.GRAPH.get(this.env.GRAPH.idFromName(source));
    return async (bytes) => {
      const response = await stub.fetch("https://graph/serve", { method: "POST", body: bytes });
      return new Uint8Array(await response.arrayBuffer());
    };
  }
}

// The facet's class: the same operations as a Graph, on a class that no binding
// names and that extends DurableObject, so that `ctx.exports` offers it as a
// class to run as a facet.
export class Member extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.init(ctx, env);
  }
}
for (const name of Object.getOwnPropertyNames(Graph.prototype)) {
  if (name !== "constructor") Member.prototype[name] = Graph.prototype[name];
}

// A supervisor (an ordinary SQLite class that declared `doltRemotes`, so that
// its facets have the capability) whose one facet is a Graph: a Dolt facet
// that holds a remote to the shared graph and pushes to it.
export class Hub {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env;
  }

  async fetch(request) {
    const facet = this.ctx.facets.get("member", () => ({
      class: this.ctx.exports.Member,
      celld: { storage: "dolt" },
    }));
    return facet.fetch(request);
  }
}

// A Dolt class that never declared `doltRemotes`.
export class Plain {
  constructor(ctx) {
    this.ctx = ctx;
  }

  async fetch() {
    return Response.json({ remote: this.ctx.storage.dolt.remote !== undefined });
  }
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const named = (name) => env.GRAPH.idFromName(url.searchParams.get(name) ?? "gad");
    if (url.pathname === "/fork") {
      const id = await env.GRAPH.fork(named("from"), { to: named("to") });
      return Response.json({ id: id.toString() });
    }
    if (url.pathname === "/id") {
      return Response.json({ id: named("g").toString() });
    }
    if (url.pathname === "/link") {
      // The source decides what the holder may do and hands over the token.
      const source = named("source");
      const query = new URLSearchParams();
      for (const key of ["label", "fetch", "push", "branches", "expiresAt"]) {
        if (url.searchParams.has(key)) query.set(key, url.searchParams.get(key));
      }
      const granted = await (await env.GRAPH.get(source).fetch("https://graph/grant?" + query)).json();
      if (granted.error) return Response.json(granted);
      const holder = named("holder");
      const remote = await env.GRAPH.get(holder).fetch(
        "https://graph/remote-add?" + new URLSearchParams({
          name: url.searchParams.get("name") ?? "origin", source: source.toString(), token: granted.token,
        }),
      );
      return Response.json({ grant: granted.id, token: granted.token, remote: await remote.json() });
    }
    if (url.pathname.startsWith("/hub/")) {
      // An operation on the hub's facet, which is a Dolt cell of its own.
      return env.HUB.get(env.HUB.idFromName("hub")).fetch(
        new Request("https://graph/" + url.pathname.slice(5) + url.search, request),
      );
    }
    if (url.pathname === "/plain") {
      return env.PLAIN.get(env.PLAIN.idFromName("p")).fetch("https://plain/");
    }
    if (url.pathname === "/plain-serve") {
      // A raw request to a class that did not declare doltRemotes.
      const id = env.PLAIN.idFromName("p");
      const response = await env.PLAIN.get(id).fetch("https://celld.internal/__celld/dolt-remote", {
        method: "POST", body: new Uint8Array([1, 1, 2, 0, 0, 0, 123, 125]),
      });
      return Response.json({ status: response.status, code: response.headers.get("x-celld-code") });
    }
    const id = named("g");
    return env.GRAPH.get(id).fetch(new Request("https://graph" + url.pathname + url.search, request));
  },
};
