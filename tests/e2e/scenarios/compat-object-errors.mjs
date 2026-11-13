// Fork, restore and import errors carry a stable `code`, and a fork or import
// that stopped before it completed can be retried over or deleted
// (docs/fork/design.md section 3b.14, docs/services/object-fork.md). Uses the
// object-fork example. The interrupted publications are written into the local
// bucket of `celld dev` while the node is stopped, in the shape `publish_new`
// leaves when it stops after claiming its target: a record that still says
// `publishing`, and a stale object beside it.
import { execFile } from "node:child_process";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { get, run, stop, start, M } from "../lib/dsl.mjs";

const unfinish = (t, ...scopes) => {
  const db = new DatabaseSync(path.join(t.celld.dir, ".celld/dev/objects.sqlite3"));
  try {
    const put = db.prepare(
      "INSERT OR REPLACE INTO objects (key, body, etag, modified_ms, attributes) " +
        "VALUES (?, ?, (SELECT next_etag FROM store_sequence), ?, '[]')",
    );
    const bump = db.prepare("UPDATE store_sequence SET next_etag = next_etag + 1 WHERE singleton = 1");
    for (const scope of scopes) {
      const record = JSON.stringify({ node: "", epoch: 1, publishing: "interrupted" });
      for (const [key, body] of [
        [`cells/${scope}/own.json`, record],
        [`cells/${scope}/ltx/e1/0000/stale.ltx`, "stale"],
      ]) {
        put.run(key, Buffer.from(body), Date.now());
        bump.run();
      }
    }
  } finally {
    db.close();
  }
};

const cli = (t, args) =>
  new Promise((resolve) => {
    execFile(t.celld.bin, [...args, "--dev", t.celld.dir, "--json"], { encoding: "utf8" }, (error, stdout, stderr) => {
      resolve({ exit: error ? error.code : 0, stdout, stderr });
    });
  });

export default {
  example: "object-fork",
  id: "object-errors",
  title: "fork, restore and import errors have codes; an unfinished fork target can be replaced or deleted",
  steps: [
    get("/w/src/add?to=root&body=a1", { status: 200, json: { n: 1 } }),
    get("/id?name=unf", { status: 200, json: { id: M.capture("unf", M.re(/^[0-9a-f]{64}$/)) } }),
    get("/id?name=unf2", { status: 200, json: { id: M.capture("unf2", M.re(/^[0-9a-f]{64}$/)) } }),
    get("/id?name=unf3", { status: 200, json: { id: M.capture("unf3", M.re(/^[0-9a-f]{64}$/)) } }),
    get("/fork?from=src&to=done", { status: 200, json: { id: M.re(/^[0-9a-f]{64}$/) } }),
    stop("stop to leave interrupted forks in the bucket"),
    run("interrupted publications of three targets", async (t) => {
      unfinish(t, `Workspace:${t.vars.unf}`, `Workspace:${t.vars.unf2}`, `Workspace:${t.vars.unf3}`);
    }),
    start(),
    // The codes of the errors.
    get("/fork?from=src&to=done", { status: 409, json: { error: M.re(/already exists/), code: "ERR_OBJECT_EXISTS" } }),
    get("/fork?from=src&to=unf", {
      status: 409,
      json: { error: M.re(/unfinished publication/), code: "ERR_OBJECT_UNFINISHED" },
    }, { name: "an unfinished target is refused, with its own code" }),
    get("/fork?from=src&to=src", { status: 409, json: { error: M.re(/invalid fork target/), code: "ERR_OBJECT_INVALID_TARGET" } }),
    // Retry over it.
    get("/fork?from=src&to=unf&replace=1", { status: 200, json: { id: "{$unf}" } }, { name: "replaceUnfinished takes the target over" }),
    get("/w/unf/state", { status: 200, json: M.partial({ items: ["a1"] }) }, { name: "the fork's object works" }),
    get("/fork?from=src&to=done&replace=1", { status: 409, json: { error: M.re(/already exists/), code: "ERR_OBJECT_EXISTS" } }, {
      name: "replaceUnfinished never takes over an object that finished",
    }),
    // Delete one.
    get("/delete-unfinished?name=unf2", { status: 200, json: { deleted: 2 } }, { name: "an unfinished publication is deleted" }),
    get("/delete-unfinished?name=unf2", { status: 409, json: { error: M.re(/does not exist/), code: "ERR_OBJECT_NOT_FOUND" } }),
    get("/delete-unfinished?name=done", {
      status: 409,
      json: { error: M.re(/not an unfinished publication/), code: "ERR_OBJECT_NOT_UNFINISHED" },
    }, { name: "an object that finished is not deleted" }),
    get("/fork?from=src&to=unf2", { status: 200, json: { id: "{$unf2}" } }, { name: "a deleted target is free again" }),
    // The CLI, with the node stopped (it opens the same local bucket).
    stop(),
    run("celld cell delete-unfinished: JSON and exit status", async (t) => {
      const args = ["cell", "delete-unfinished", "object-fork", "Workspace"];
      let result = await cli(t, [...args, "unf3"]);
      t.assert(result.exit === 0, `exit ${result.exit}: ${result.stderr}`);
      t.expectValue(JSON.parse(result.stdout), M.partial({ scope: `Workspace:${t.vars.unf3}`, deleted: 2 }));
      result = await cli(t, [...args, "unf3"]);
      t.assert(result.exit === 13, `exit ${result.exit}, expected 13 (ERR_OBJECT_NOT_FOUND)`);
      t.expectValue(JSON.parse(result.stdout), { error: { code: "ERR_OBJECT_NOT_FOUND", message: M.re(/does not exist/) } });
      result = await cli(t, [...args, "done"]);
      t.assert(result.exit === 12, `exit ${result.exit}, expected 12 (ERR_OBJECT_NOT_UNFINISHED)`);
      t.expectValue(JSON.parse(result.stdout).error.code, "ERR_OBJECT_NOT_UNFINISHED");
    }),
    run("celld cell import: an existing object, an unfinished one", async (t) => {
      unfinish(t, `Workspace:${t.vars.unf3}`);
      const root = path.join(t.celld.dir, "root.sqlite");
      const db = new DatabaseSync(root);
      db.exec("CREATE TABLE items (id INTEGER PRIMARY KEY, body TEXT); INSERT INTO items(body) VALUES ('imported');");
      db.close();
      const args = ["cell", "import", "object-fork", "Workspace"];
      let result = await cli(t, [...args, "done", "--file", root]);
      t.assert(result.exit === 10, `exit ${result.exit}, expected 10 (ERR_OBJECT_EXISTS)`);
      t.expectValue(JSON.parse(result.stdout).error.code, "ERR_OBJECT_EXISTS");
      result = await cli(t, [...args, "unf3", "--file", root]);
      t.assert(result.exit === 11, `exit ${result.exit}, expected 11 (ERR_OBJECT_UNFINISHED)`);
      t.expectValue(JSON.parse(result.stdout).error.code, "ERR_OBJECT_UNFINISHED");
      result = await cli(t, [...args, "unf3", "--file", root, "--replace-unfinished"]);
      t.assert(result.exit === 0, `exit ${result.exit}: ${result.stderr}`);
    }),
  ],
};
