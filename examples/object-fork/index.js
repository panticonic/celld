import { DurableObject } from "cloudflare:workers";

// Whole-object fork, restore and import (docs/services/object-fork.md).
//
// A workspace is a SQLite Durable Object with three facets, all of one
// loaded class: "notes" (SQLite) with a facet of its own, "attachments"
// (SQLite), and "ledger" (Dolt). env.WORKSPACE.fork() copies a workspace
// with all of them, env.WORKSPACE.restore() replaces one with a copy of
// another, and `celld cell import` creates one from database files.
//
//   GET /w/<name>/add?to=root|notes|attachments|ledger&body=...
//   GET /w/<name>/state                 everything the workspace holds
//   GET /w/<name>/commit?message=...    a commit of the ledger
//   GET /w/<name>/kv?set=...            the root's key-value API
//   GET /w/<name>/alarm?in=<ms>         set the root's alarm, <ms> from now
//   GET /fork?from=<a>&to=<b>[&ledgerAt=<hash>][&rootAt=<hash>][&replace=1]
//   GET /delete-unfinished?name=<a>     remove what an interrupted fork left
//   GET /restore?target=<a>&from=<b>[&stop=1]

// The facets' code. A real application loads it from a bundle or a
// database; here it is a string so the example is one file.
const STORE = `
import { DurableObject } from "cloudflare:workers";
export class Store extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    this.sql.exec("CREATE TABLE IF NOT EXISTS rows (id INTEGER PRIMARY KEY, body TEXT NOT NULL)");
  }
  add(body) {
    this.sql.exec("INSERT INTO rows(body) VALUES (?)", body);
    return this.sql.exec("SELECT count(*) AS n FROM rows").one().n;
  }
  rows() {
    return this.sql.exec("SELECT body FROM rows ORDER BY id").toArray().map((row) => row.body);
  }
  engine() {
    return this.ctx.storage.dolt ? "dolt" : "sqlite";
  }
  commit(message) {
    return this.ctx.storage.dolt.commit({ message });
  }
  log() {
    return this.ctx.storage.dolt.log({ limit: 20 }).map((entry) => entry.message);
  }
  head() {
    return this.ctx.storage.dolt.hashOf("HEAD");
  }
  // A facet of this facet, of the same class.
  child(name) {
    return this.ctx.facets.get(name, () => ({ class: this.ctx.exports.Store }));
  }
  async addBelow(name, body) {
    return this.child(name).add(body);
  }
  async rowsBelow(name) {
    return this.child(name).rows();
  }
}
`;

export class Workspace extends DurableObject {
  constructor(ctx, env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    this.sql.exec("CREATE TABLE IF NOT EXISTS items (id INTEGER PRIMARY KEY, body TEXT NOT NULL)");
  }

  storeClass() {
    const worker = this.env.LOADER.get("store-v1", () => ({
      compatibilityDate: "2026-01-01",
      mainModule: "store.js",
      modules: { "store.js": STORE },
    }));
    return worker.getDurableObjectClass("Store");
  }

  facet(name) {
    const storage = name === "ledger" ? "dolt" : "sqlite";
    return this.ctx.facets.get(name, () => ({ class: this.storeClass(), celld: { storage } }));
  }

  async state() {
    const items = this.sql.exec("SELECT body FROM items ORDER BY id").toArray().map((row) => row.body);
    const notes = this.facet("notes");
    const ledger = this.facet("ledger");
    return {
      items,
      kv: (await this.ctx.storage.get("kv")) ?? null,
      greeting: (await this.ctx.storage.get("greeting")) ?? null,
      alarm: await this.ctx.storage.getAlarm(),
      notes: await notes.rows(),
      attachments: await notes.rowsBelow("attachments"),
      ledger: { engine: await ledger.engine(), rows: await ledger.rows(), log: await ledger.log() },
    };
  }

  async fetch(request) {
    const url = new URL(request.url);
    const q = (name, fallback = null) => url.searchParams.get(name) ?? fallback;
    const op = url.pathname.split("/")[3] ?? "state";
    const json = (value, status = 200) => Response.json(value, { status });
    switch (op) {
      case "add": {
        const to = q("to", "root");
        const body = q("body", "x");
        if (to === "root") {
          this.sql.exec("INSERT INTO items(body) VALUES (?)", body);
          return json({ n: this.sql.exec("SELECT count(*) AS n FROM items").one().n });
        }
        if (to === "attachments") return json({ n: await this.facet("notes").addBelow("attachments", body) });
        return json({ n: await this.facet(to).add(body) });
      }
      case "state":
        return json(await this.state());
      case "commit":
        return json({ hash: await this.facet("ledger").commit(q("message", "commit")) });
      case "head":
        return json({ hash: await this.facet("ledger").head() });
      case "kv":
        if (q("set") !== null) await this.ctx.storage.put("kv", q("set"));
        return json({ kv: (await this.ctx.storage.get("kv")) ?? null });
      case "alarm":
        await this.ctx.storage.setAlarm(Date.now() + Number(q("in")));
        return json({ alarm: await this.ctx.storage.getAlarm() });
      default:
        return new Response("unknown op " + op, { status: 404 });
    }
  }

  alarm() {}
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const q = (name) => url.searchParams.get(name);
    const ns = env.WORKSPACE;
    const fail = (error) => Response.json(
      { error: String(error?.message ?? error), code: error?.code ?? null }, { status: 409 });
    if (url.pathname === "/fork") {
      const options = { name: q("to") };
      if (q("ledgerAt")) options.facets = { ledger: { at: q("ledgerAt") } };
      if (q("rootAt")) options.at = q("rootAt");
      // Takes over a target that an interrupted fork left unfinished.
      if (q("replace") === "1") options.replaceUnfinished = true;
      try {
        const id = await ns.fork(ns.idFromName(q("from")), options);
        return Response.json({ id: id.toString() });
      } catch (error) {
        return fail(error);
      }
    }
    if (url.pathname === "/restore") {
      try {
        const options = q("stop") === "1" ? { stop: true } : {};
        return Response.json(
          await ns.restore(ns.idFromName(q("target")), ns.idFromName(q("from")), options));
      } catch (error) {
        return fail(error);
      }
    }
    if (url.pathname === "/delete-unfinished") {
      try {
        return Response.json(await ns.deleteUnfinished(ns.idFromName(q("name"))));
      } catch (error) {
        return fail(error);
      }
    }
    if (url.pathname === "/id") return Response.json({ id: ns.idFromName(q("name")).toString() });
    const name = url.pathname.split("/")[2];
    if (!url.pathname.startsWith("/w/") || !name) return new Response("not found", { status: 404 });
    try {
      return await ns.getByName(name).fetch(request);
    } catch (error) {
      return fail(error);
    }
  },
};
