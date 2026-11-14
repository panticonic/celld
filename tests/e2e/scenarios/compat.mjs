// Scenarios for workerd-compatibility gaps the Vibestudio cutover found
// (docs/fork/design.md, "Compatibility fixes"). One example per gap, each
// driven through HTTP only.
import { get, run, stop, start, M } from "../lib/dsl.mjs";

// -------------------------------------------------------- continuations
// Work that starts from a promise continuation after the event that created
// it has ended. See examples/continuations.
const settled = { timeoutMs: 10000, intervalMs: 200, why: "the continuation runs in the turn of the request that releases it" };
// Arm a continuation, release it from another request, wait for its result.
const orphan = (op, ok, name = op) => [
  get(`/arm?op=${op}`, { status: 200, json: { armed: op } }, { name: `arm ${name}` }),
  get(`/go?op=${op}`, { status: 200, json: { released: op } }, { name: `release ${name}` }),
  get(`/result?op=${op}`, { status: 200, json: { ok } }, { poll: settled, name: `${name} from an orphaned continuation` }),
];

// The same, but the arming request leaves a timer pending: it has answered and
// is still running (it keeps its pending I/O) when another request's turn runs
// the continuation. Its block must start as the object's own, not be refused
// because the answered request no longer originates cross-event work.
const lingering = (op, ok, name = op) => [
  get(`/arm?op=${op}&linger=4000`, { status: 200, json: { armed: op } }, { name: `arm ${name} (lingering)` }),
  get(`/go?op=${op}`, { status: 200, json: { released: op } }, { name: `release ${name} (lingering)` }),
  get(`/result?op=${op}`, { status: 200, json: { ok } }, { poll: settled, name: `${name} from a continuation of an answered, running request` }),
];

const continuations = {
  example: "continuations",
  title: "continuations that outlive their event may use the object: blockConcurrencyWhile, storage, transactions, alarms, fetch and RPC",
  steps: [
    ...orphan("bcw", "ran:block", "blockConcurrencyWhile"),
    ...orphan("bcwOrder", "first:start,first:end,second", "two blocks queue in order"),
    ...orphan("storage", JSON.stringify({ got: [["a", 1], ["b", 2], ["c", 3]], listed: ["a", "b", "bcw"] }), "async storage"),
    ...orphan("sql", '[{"v":7}]', "SQL storage"),
    ...orphan("transactionSync", "inside", "transactionSync"),
    ...orphan("transaction", "inside", "an async transaction"),
    ...lingering("transaction", "inside", "an async transaction"),
    ...lingering("bcw", "ran:block", "blockConcurrencyWhile"),
    ...orphan("alarm", "armed", "setAlarm, getAlarm, deleteAlarm"),
    ...orphan("fetch", "200:pong", "fetch"),
    ...orphan("stub", "200:peer", "a Durable Object stub fetch"),
    ...orphan("rpc", "echo:hi", "an RPC call"),
    ...orphan("effect", "200:written", "a write, then an outbound effect"),
    // A block that throws resets the object, as a block in a request does: the
    // object that answers afterwards is a new instance, and the block ran.
    get("/instance", { status: 200, json: { instance: M.capture("before", M.type("string")) } }),
    ...orphan("bcwThrows", "block ran", "a block that throws"),
    run("the object was reset", async (t) => {
      const res = await t.fetch("/instance");
      t.expect(res, { status: 200, json: { instance: M.type("string") } });
      if (res.json.instance === t.vars.before) throw new Error("the instance did not change");
    }),
  ],
};

// An exclusive section that never ends (the 30 s limit is off) holds its object,
// and must not hold the node: a request queued behind it is released and the
// node stops and starts again.
const stuck = {
  example: "continuations",
  id: "continuations-stuck",
  title: "a block that never ends does not keep the node from stopping",
  env: { CELLD_NO_IMPLICIT_DEADLINES: "1" },
  steps: [
    get("/arm?op=bcwStuck", { status: 200, json: { armed: "bcwStuck" } }),
    get("/go?op=bcwStuck", { status: 200, json: { released: "bcwStuck" } }),
    run("a request behind the block waits", async (t) => {
      const res = await t.fetch("/instance", { timeoutMs: 1500 }).then(() => "answered", () => "waiting");
      if (res !== "waiting") throw new Error("a request was served while the block held the object");
    }),
    stop("stop with the block running"),
    start("start again"),
    get("/instance", { status: 200, json: { instance: M.type("string") } }, { name: "the object serves again" }),
  ],
};

export default [continuations, stuck];
