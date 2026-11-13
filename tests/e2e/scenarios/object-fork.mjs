// object-fork: env.NS.fork() and env.NS.restore() of a SQLite Durable Object
// with a SQLite facet (which has a facet of its own) and a Dolt facet. The
// fork and the restored object hold every component, a running target is
// refused unless the restore stops it, and everything survives a graceful
// restart, SIGKILL, and a restore from the replication log. The scripted
// examples/object-fork/demo.sh covers more (busy source, import, a crash
// mid-restore).
import { get, restart, crash, restore, M } from "../lib/dsl.mjs";

const SOURCE = {
  items: ["a1"],
  kv: "v1",
  greeting: null,
  alarm: null,
  notes: ["n1"],
  attachments: ["t1"],
  ledger: { engine: "dolt", rows: ["l1"], log: ["c1", M.re(/^Initialize data repository/)] },
};
const state = (name, json) => get(`/w/${name}/state`, { status: 200, json }, { name: `state of ${name}` });
const count = { status: 200, json: { n: 1 } };

export default {
  example: "object-fork",
  title: "whole-object fork and restore across SQLite and Dolt facets",
  steps: [
    get("/w/src/add?to=root&body=a1", count),
    get("/w/src/add?to=notes&body=n1", count),
    get("/w/src/add?to=attachments&body=t1", count),
    get("/w/src/add?to=ledger&body=l1", count),
    get("/w/src/commit?message=c1", { status: 200, json: { hash: M.re(/^[0-9a-f]{40}$/) } }),
    get("/w/src/kv?set=v1", { status: 200, json: { kv: "v1" } }),
    state("src", SOURCE),
    get("/fork?from=src&to=f1", { status: 200, json: { id: M.re(/^[0-9a-f]{64}$/) } }),
    state("f1", SOURCE),
    get("/fork?from=src&to=f1", { status: 409, json: { error: M.re(/already exists/), code: null } }),
    get("/w/f1/add?to=notes&body=f1-only", { status: 200, json: { n: 2 } }),
    get("/fork?from=f1&to=backup", { status: 200, json: { id: M.re(/^[0-9a-f]{64}$/) } }),
    get("/restore?target=f1&from=src", {
      status: 409,
      json: { error: M.re(/not quiesced: it is running/), code: "ERR_RESTORE_TARGET_ACTIVE" },
    }),
    get("/restore?target=f1&from=src&stop=1", { status: 200, json: { restored: true } }),
    state("f1", SOURCE),
    restart(),
    state("f1", SOURCE),
    state("backup", { ...SOURCE, notes: ["n1", "f1-only"] }),
    crash(),
    state("f1", SOURCE),
    restore(),
    state("f1", SOURCE),
    state("backup", { ...SOURCE, notes: ["n1", "f1-only"] }),
    get("/restore?target=backup&from=f1&stop=1", { status: 200, json: { restored: true } }),
    state("backup", SOURCE),
  ],
};
