import { get, post, put, del, restart, crash, restore, run, M } from "../lib/dsl.mjs";

// async: timers and storage awaits inside a DO.
const asyncDo = {
  example: "async",
  title: "setTimeout + async storage; concurrent requests are serialised",
  steps: [
    get("/", { status: 200, json: { count: 1 } }),
    get("/", { status: 200, json: { count: 2 } }),
    run("5 concurrent requests get 5 distinct counts (no lost update)", async (t) => {
      const rs = await Promise.all(Array.from({ length: 5 }, () => t.fetch("/")));
      const counts = rs.map((r) => r.json.count).sort((a, b) => a - b);
      t.expectValue(counts, [3, 4, 5, 6, 7]);
    }),
    restart(),
    get("/", { status: 200, json: { count: 8 } }),
    crash(),
    get("/", { status: 200, json: { count: 9 } }),
    restore(),
    get("/", { status: 200, json: { count: 10 } }),
  ],
};

// body: request/response bodies through a Worker -> DO hop.
const big = "x".repeat(1_000_000);
const bodyDo = {
  example: "body",
  title: "JSON, large and empty request bodies; per-name state",
  steps: [
    post("/a", { status: 200, json: { echoed: { n: 3 }, sum: 3 } }, { json: { n: 3 } }),
    put("/a", { status: 200, json: { echoed: { n: 4 }, sum: 7 } }, { json: { n: 4 } }),
    post("/b", { status: 200, json: { echoed: { n: 1 }, sum: 1 } }, { json: { n: 1 } }),
    post("/a", { status: 200, json: { echoed: { s: "héllo ✓ 日本" }, sum: 7 } }, { json: { s: "héllo ✓ 日本" } }),
    post("/a", { status: 200, json: { echoed: { n: 1, pad: big }, sum: 8 } }, { json: { n: 1, pad: big }, name: "POST /a with a 1 MB body" }),
    del("/a", { status: 200, json: { method: "DELETE", bodyLen: 5 } }, { body: "hello", name: "DELETE with a text body" }),
    get("/a", { status: 200, json: { method: "GET", bodyLen: 0 } }),
    post("/a", 500, { body: "{not json", headers: { "content-type": "application/json" }, name: "invalid JSON -> 500" }),
    // The failed request must not have changed state.
    post("/a", { status: 200, json: { echoed: { n: 0 }, sum: 8 } }, { json: { n: 0 }, name: "state intact after a failed request" }),
    restart(),
    post("/a", { status: 200, json: { echoed: { n: 2 }, sum: 10 } }, { json: { n: 2 } }),
    crash(),
    post("/b", { status: 200, json: { echoed: { n: 1 }, sum: 2 } }, { json: { n: 1 } }),
    restore(),
    post("/a", { status: 200, json: { echoed: { n: 0 }, sum: 10 } }, { json: { n: 0 } }),
    post("/b", { status: 200, json: { echoed: { n: 0 }, sum: 2 } }, { json: { n: 0 } }),
  ],
};

// router: Worker routes to a DO chosen from the path.
const routerDo = {
  example: "router",
  title: "Worker -> DO routing by path",
  steps: [
    get("/", { status: 200, json: { name: "default", n: 1 } }),
    get("/", { status: 200, json: { name: "default", n: 2 } }),
    get("/x", { status: 200, json: { name: "x", n: 1 } }),
    get("/y", { status: 200, json: { name: "y", n: 1 } }),
    get("/x?ignored=1", { status: 200, json: { name: "x", n: 2 } }),
    restart(),
    get("/x", { status: 200, json: { name: "x", n: 3 } }),
    crash(),
    get("/y", { status: 200, json: { name: "y", n: 2 } }),
    restore(),
    get("/", { status: 200, json: { name: "default", n: 3 } }),
    get("/x", { status: 200, json: { name: "x", n: 4 } }),
    get("/y", { status: 200, json: { name: "y", n: 3 } }),
  ],
};

export default [asyncDo, bodyDo, routerDo];
