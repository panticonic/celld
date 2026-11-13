import { get, put, post, restart, crash, restore, run, M } from "../lib/dsl.mjs";

// vectordb: sqlite-vec `vec0` virtual table inside a DO's SQLite database.
// The vec0 shadow tables must survive LTX capture, restart and restore.
// Expected distance is the Euclidean distance between the unit-scaled vectors.
const COLORS = { red: [255, 0, 0], green: [0, 255, 0], blue: [0, 0, 255], "dark grey": [30, 30, 30], yellow: [255, 255, 0] };
const near = (rgb, name) => {
  const q = rgb.split(",").map(Number);
  const dist = Math.hypot(...q.map((v, i) => (v - COLORS[name][i]) / 255));
  return get(`/${rgb}`, { status: 200, json: { name, distance: M.near(dist, 1e-4) } }, { name: `nearest to ${rgb} is ${name}` });
};
const vectordb = {
  example: "vectordb",
  title: "sqlite-vec nearest-colour search; vec0 table survives restart/crash/restore",
  steps: [
    get("/10,10,10", { status: 404, text: "The palette is empty.\n" }),
    get("/300,0,0", 400),
    get("/1,2", 400),
    put("/red", 201, { json: [255, 0, 0] }),
    put("/green", 201, { json: [0, 255, 0] }),
    put("/blue", 201, { json: [0, 0, 255] }),
    put("/dark%20grey", 201, { json: [30, 30, 30] }),
    put("/bad", 400, { json: [1, 2, 3, 4] }),
    near("250,10,10", "red"),
    near("10,240,10", "green"),
    near("10,10,240", "blue"),
    near("20,20,20", "dark grey"),
    restart(),
    near("250,10,10", "red"),
    put("/yellow", 201, { json: [255, 255, 0] }),
    crash(),
    near("250,250,10", "yellow"),
    near("10,10,240", "blue"),
    restore(),
    near("250,250,10", "yellow"),
    near("20,20,20", "dark grey"),
    near("250,10,10", "red"),
  ],
};

// rpc: DO methods called over JS RPC from the Worker.
const rpc = {
  example: "rpc",
  title: "DO RPC methods (increment/value); persistence",
  steps: [
    get("/", { status: 200, json: { count: 0 } }),
    post("/", { status: 200, json: { count: 1 } }),
    post("/", { status: 200, json: { count: 2 } }),
    get("/", { status: 200, json: { count: 2 } }),
    run("concurrent RPC calls do not lose updates", async (t) => {
      const rs = await Promise.all(Array.from({ length: 6 }, () => t.fetch("/", { method: "POST" })));
      t.expectValue(rs.map((r) => r.json.count).sort((a, b) => a - b), [3, 4, 5, 6, 7, 8]);
    }),
    restart(),
    get("/", { status: 200, json: { count: 8 } }),
    post("/", { status: 200, json: { count: 9 } }),
    crash(),
    get("/", { status: 200, json: { count: 9 } }),
    restore(),
    get("/", { status: 200, json: { count: 9 } }),
    post("/", { status: 200, json: { count: 10 } }),
  ],
};

// facets: a Worker Loader class run as a facet of a DO, with its own SQLite
// (synchronous KV API), replicated with the parent. The facet gets a
// parameterised ctx.exports binding (GREETER) from the parent.
const facetCount = (n) => get("/", { status: 200, text: `${n}:hello, facet` });
const facets = {
  example: "facets",
  title: "DO facet from the Worker Loader: own SQLite, service binding, replication",
  steps: [
    facetCount(0),
    facetCount(1),
    facetCount(2),
    restart(),
    facetCount(3),
    crash(),
    facetCount(4),
    restore(),
    facetCount(5),
    facetCount(6),
  ],
};

export default [vectordb, rpc, facets];
