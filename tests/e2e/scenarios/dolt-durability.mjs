// dolt-durability: every operation that changes a Dolt cell is durable once its
// request was answered. After each one the node is SIGKILLed and its local
// runtime state deleted, so the next start restores the cell from the
// replication log alone: what the answer promised must be there. (A graceful
// stop hides a lost operation, because stopping makes everything durable; and
// a restart over the surviving local file hides it too.)
//
// The first version of this suite found that an acknowledged
// `dolt.reset({ hard: true })` was lost: a Dolt cell's schema cookie is
// derived from its catalog, a reset to a commit of another schema gave it a
// lower value, the committed-write position (a sum that included it) went
// down, and the output gate released the answer with no durability proof.
//
// Everything runs through examples/dolt (`celld dev`). The remote operations
// (grant, fetch, push, accepted push) are in dolt-remotes-durability.
import fs from "node:fs";
import path from "node:path";
import { run } from "../lib/dsl.mjs";

const repo = "repo=kill";
const call = async (t, route, options) => {
  const res = await t.fetch(`${route}${route.includes("?") ? "&" : "?"}${repo}`, options);
  if (res.status !== 200) throw new Error(`${route} answered ${res.status}: ${res.text.slice(0, 300)}`);
  return res.json;
};
const bodies = (notes) => notes.map((note) => note.body);

export const killAndRestore = run(
  "SIGKILL, delete the local runtime state, start: the cell is restored from the replication log",
  async (t) => {
    const c = t.celld;
    await c.kill();
    fs.rmSync(path.join(c.stateDir, "runtime"), { recursive: true, force: true });
    await c.start();
  },
);

export const eq = (t, actual, expected, what) => t.assert(
  JSON.stringify(actual) === JSON.stringify(expected),
  `${what}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
);

/** Run `act`, kill and restore, then `check`. */
export const survives = (name, act, check) => [
  run(`${name}: acknowledged`, async (t) => { await act(t); }),
  killAndRestore,
  run(`${name}: survived the kill`, async (t) => { await check(t); }),
];

const program = (sql) => ({
  program: sql.map((text) => ({ sql: text })),
  context: { nowMs: "1700000000000", seed: "42", actor: "AAEC", schemaAdmin: true },
});

const steps = [
  ...survives(
    "commit",
    async (t) => {
      await call(t, "/add?body=one");
      t.vars.c1 = (await call(t, "/commit?message=c1")).hash;
    },
    async (t) => {
      const log = await call(t, "/log?limit=2");
      eq(t, log[0].hash, t.vars.c1, "head");
      eq(t, bodies((await call(t, "/notes")).notes), ["one"], "rows");
      eq(t, await call(t, "/status"), [], "status");
    },
  ),
  ...survives(
    "commit in a transaction (F2)",
    async (t) => {
      t.vars.c2 = (await call(t, "/tx?body=two&message=c2")).head;
    },
    async (t) => {
      eq(t, (await call(t, "/hash?ref=HEAD")).hash, t.vars.c2, "head");
      eq(t, bodies((await call(t, "/notes")).notes), ["one", "two"], "rows");
    },
  ),
  ...survives(
    "a write to a table declared local (doltLocal)",
    async (t) => { await call(t, "/outbox?add=m1"); },
    async (t) => { eq(t, (await call(t, "/outbox")).outbox.map((row) => row.body), ["m1"], "local rows"); },
  ),
  ...survives(
    "branch create",
    async (t) => { await call(t, "/branch?name=feature"); },
    async (t) => {
      eq(t, (await call(t, "/branch")).map((branch) => branch.name), ["feature", "main"], "branches");
    },
  ),
  ...survives(
    "checkout, and a commit on the other branch",
    async (t) => {
      await call(t, "/checkout?name=feature");
      await call(t, "/add?body=on-feature");
      t.vars.feature = (await call(t, "/commit?message=feature")).hash;
    },
    async (t) => {
      const now = await call(t, "/notes");
      eq(t, now.branch, "feature", "the checked-out branch");
      eq(t, bodies(now.notes), ["one", "two", "on-feature"], "rows");
      eq(t, (await call(t, "/hash?ref=feature")).hash, t.vars.feature, "feature");
    },
  ),
  ...survives(
    "merge",
    async (t) => {
      await call(t, "/checkout?name=main");
      t.vars.merged = (await call(t, "/merge?name=feature")).hash;
    },
    async (t) => {
      eq(t, (await call(t, "/hash?ref=HEAD")).hash, t.vars.merged, "head");
      eq(t, bodies((await call(t, "/notes")).notes), ["one", "two", "on-feature"], "rows");
    },
  ),
  ...survives(
    "merge with a resolver (F1)",
    async (t) => {
      await call(t, "/branch?name=left");
      await call(t, "/branch?name=right");
      await call(t, "/checkout?name=left");
      await call(t, "/edit?id=1&body=left");
      await call(t, "/commit?message=left");
      await call(t, "/checkout?name=right");
      await call(t, "/edit?id=1&body=right");
      await call(t, "/commit?message=right");
      await call(t, "/checkout?name=left");
      const merged = await call(t, "/merge-resolve?name=right&mode=theirs&message=resolved");
      t.assert(merged.result && merged.result.conflicts.length === 0, JSON.stringify(merged));
      t.vars.resolved = (await call(t, "/hash?ref=HEAD")).hash;
    },
    async (t) => {
      const now = await call(t, "/notes");
      eq(t, now.branch, "left", "the checked-out branch");
      eq(t, now.notes.find((note) => note.id === 1).body, "right", "the resolution");
      eq(t, (await call(t, "/hash?ref=HEAD")).hash, t.vars.resolved, "head");
      eq(t, (await call(t, "/log?limit=1"))[0].message, "resolved", "message");
    },
  ),
  ...survives(
    "branch delete",
    async (t) => {
      await call(t, "/checkout?name=main");
      await call(t, "/delete-branch?name=right&force=1");
    },
    async (t) => {
      eq(t, (await call(t, "/branch")).map((branch) => branch.name).sort(),
        ["feature", "left", "main"], "branches");
    },
  ),
  ...survives(
    "reset soft",
    async (t) => {
      await call(t, "/add?body=three");
      await call(t, "/commit?message=three");
      t.vars.parent = (await call(t, "/hash?ref=HEAD~1")).hash;
      eq(t, (await call(t, "/reset?to=HEAD~1")).head, t.vars.parent, "the answer");
    },
    async (t) => {
      eq(t, (await call(t, "/hash?ref=HEAD")).hash, t.vars.parent, "head");
      // The rows stay, uncommitted, as a soft reset leaves them.
      eq(t, bodies((await call(t, "/notes")).notes), ["one", "two", "on-feature", "three"], "rows");
      t.assert((await call(t, "/status")).some((entry) => entry.table === "notes"), "the rows are still uncommitted");
    },
  ),
  ...survives(
    "reset hard",
    async (t) => {
      await call(t, "/commit?message=three-again");
      await call(t, "/add?body=four");
      await call(t, "/commit?message=four");
      t.vars.before = (await call(t, "/hash?ref=HEAD~1")).hash;
      eq(t, (await call(t, "/reset?hard=1&to=HEAD~1")).head, t.vars.before, "the answer");
    },
    async (t) => {
      eq(t, (await call(t, "/hash?ref=HEAD")).hash, t.vars.before, "head");
      eq(t, bodies((await call(t, "/notes")).notes), ["one", "two", "on-feature", "three"], "rows");
      eq(t, await call(t, "/status"), [], "status");
    },
  ),
  ...survives(
    "reset hard of the working set",
    async (t) => {
      await call(t, "/add?body=scratch");
      await call(t, "/reset?hard=1");
    },
    async (t) => {
      eq(t, bodies((await call(t, "/notes")).notes), ["one", "two", "on-feature", "three"], "rows");
    },
  ),
  ...survives(
    "gc",
    async (t) => { await call(t, "/gc"); },
    async (t) => {
      eq(t, bodies((await call(t, "/notes")).notes), ["one", "two", "on-feature", "three"], "rows");
      eq(t, (await call(t, "/branch")).map((branch) => branch.name).sort(),
        ["feature", "left", "main"], "branches");
    },
  ),
  ...survives(
    "execute (deterministic program, with DDL)",
    async (t) => {
      const res = await call(t, "/exec", {
        method: "POST",
        json: {
          ...program([
            "CREATE TABLE ledger (id INTEGER PRIMARY KEY, v TEXT)",
            "INSERT INTO ledger(v) VALUES ('x')",
          ]),
          probe: "SELECT v FROM ledger",
        },
      });
      t.assert(res.outcome.outcome === "accepted", JSON.stringify(res));
    },
    async (t) => {
      const res = await call(t, "/exec", {
        method: "POST",
        json: { ...program(["SELECT 1"]), probe: "SELECT v FROM ledger" },
      });
      eq(t, res.rows, [{ v: "x" }], "the program's rows");
    },
  ),
  // The cell is on `main` with notes; checking out another branch and coming
  // back, then resetting hard across a schema change, was the failing shape.
  ...survives(
    "reset hard across a schema change",
    async (t) => {
      // Committed schema changes: each reset lands on a different table set.
      await call(t, "/exec", { method: "POST", json: program(["CREATE TABLE a (x INTEGER PRIMARY KEY)"]) });
      await call(t, "/commit?message=a");
      await call(t, "/exec", { method: "POST", json: program(["CREATE TABLE b (x INTEGER PRIMARY KEY)"]) });
      await call(t, "/commit?message=b");
      await call(t, "/exec", { method: "POST", json: program(["CREATE TABLE c (x INTEGER PRIMARY KEY)"]) });
      await call(t, "/commit?message=c");
      await call(t, "/reset?hard=1&to=HEAD~1");
      await call(t, "/reset?hard=1&to=HEAD~1");
      t.vars.head = (await call(t, "/hash?ref=HEAD")).hash;
    },
    async (t) => {
      eq(t, (await call(t, "/hash?ref=HEAD")).hash, t.vars.head, "head");
      eq(t, (await call(t, "/log?limit=1"))[0].message, "a", "the commit it landed on");
      const tables = (await call(t, "/query?sql=" + encodeURIComponent(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('a','b','c') ORDER BY name",
      ))).rows.map((row) => row[0]);
      eq(t, tables, ["a"], "tables");
    },
  ),
];

export default {
  example: "dolt",
  id: "dolt-durability",
  title: "every acknowledged Dolt operation survives a SIGKILL and a restore from the replication log",
  steps,
};
