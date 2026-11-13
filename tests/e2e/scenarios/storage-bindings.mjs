import crypto from "node:crypto";
import { get, put, del, post, restart, crash, restore, run, M } from "../lib/dsl.mjs";

// kv: KV namespace through the Worker.
const bigValue = "v".repeat(1_000_000);
const kv = {
  example: "kv",
  title: "KV put/get/delete, overwrite, nested keys, 1 MB value; persistence",
  steps: [
    get("/", { status: 400, text: "Use /KEY." }),
    get("/k1", { status: 404, text: "Not found." }),
    put("/k1", 204, { body: "hello" }),
    get("/k1", { status: 200, text: "hello" }),
    put("/k1", 204, { body: "hello again" }),
    get("/k1", { status: 200, text: "hello again" }),
    put("/a/b/c", 204, { body: "nested ✓" }),
    get("/a/b/c", { status: 200, text: "nested ✓" }),
    put("/big", 204, { body: bigValue }),
    get("/big", { status: 200, text: bigValue }),
    put("/empty", 204, { body: "" }),
    del("/empty", 204),
    get("/empty", 404),
    del("/never-existed", 204, { name: "delete of a missing key is not an error" }),
    restart(),
    get("/k1", { status: 200, text: "hello again" }),
    del("/k1", 204),
    get("/k1", 404),
    crash(),
    get("/a/b/c", { status: 200, text: "nested ✓" }),
    get("/k1", 404),
    restore(),
    get("/a/b/c", { status: 200, text: "nested ✓" }),
    get("/big", { status: 200, text: bigValue }),
    get("/k1", 404),
  ],
};

// r2: object storage; binary bodies, content-type, etag.
const blob = crypto.randomBytes(300_000);
const md5 = (b) => crypto.createHash("md5").update(b).digest("hex");
const getBytes = (path, bytes, ct) =>
  get(path, { status: 200, bytes, headers: { "content-type": ct, etag: /^"[^"]+"$/ } });
const r2 = {
  example: "r2",
  title: "R2 put/get/delete with content-type, etag and binary bodies; persistence",
  steps: [
    get("/", { status: 400, text: "Use /KEY." }),
    get("/o1", { status: 404, text: "Not found." }),
    put("/o1", 204, { body: "object-body", headers: { "content-type": "text/plain" } }),
    getBytes("/o1", Buffer.from("object-body"), "text/plain"),
    // R2 returns the MD5 of a singlepart object as its etag. celld dev answers
    // a store-wide counter instead (observed "16", "17", "18"... even for
    // identical content written twice), so the value is not content-derived.
    get("/o1", { status: 200, headers: { etag: `"${md5(Buffer.from("object-body"))}"` } }, {
      name: "etag is the MD5 of the content (R2 contract)",
      knownFail: "dev-mode R2 etag is a store-wide counter, not the content MD5",
    }),
    put("/bin/blob.dat", 204, { body: blob, headers: { "content-type": "application/x-custom" } }),
    getBytes("/bin/blob.dat", blob, "application/x-custom"),
    put("/o1", 204, { body: "v2", headers: { "content-type": "text/markdown" } }),
    getBytes("/o1", Buffer.from("v2"), "text/markdown"),
    put("/noct", 204, { body: "x" }),
    run("a put with no content-type is stored as octet-stream or the default", async (t) => {
      const r = await t.fetch("/noct");
      t.expect(r, { status: 200, text: "x" });
    }),
    { ...post("/o1", 405, { body: "x" }), name: "POST -> 405" },
    restart(),
    getBytes("/bin/blob.dat", blob, "application/x-custom"),
    del("/o1", 204),
    get("/o1", 404),
    crash(),
    getBytes("/bin/blob.dat", blob, "application/x-custom"),
    get("/o1", 404),
    restore(),
    getBytes("/bin/blob.dat", blob, "application/x-custom"),
    get("/o1", 404),
  ],
};

// d1: SQL through the D1 binding.
const entries = (t, list) => list.sort((a, b) => a.id - b.id);
const d1 = {
  example: "d1",
  title: "D1 guestbook: schema, prepared statements, count; persistence",
  steps: [
    get("/count", { status: 200, json: { count: 0 } }),
    post("/", { status: 201, json: { id: 1, changes: 1 } }, { json: { name: "a", message: "hi" } }),
    post("/", { status: 201, json: { id: 2, changes: 1 } }, { json: { name: "b", message: "yo" } }),
    post("/", { status: 201, json: { id: 3, changes: 1 } }, { json: { name: "q'); DROP TABLE entries;--", message: "✓ ünï" }, name: "SQL metacharacters are bound, not interpolated" }),
    get("/count", { status: 200, json: { count: 3 } }),
    run("list returns every entry", async (t) => {
      const r = await t.fetch("/");
      t.expect(r, { status: 200, json: M.partial({ count: 3 }) });
      const rows = entries(t, [...r.json.entries]);
      t.expectValue(rows, [
        { id: 1, name: "a", message: "hi", at: M.epochMs() },
        { id: 2, name: "b", message: "yo", at: M.epochMs() },
        { id: 3, name: "q'); DROP TABLE entries;--", message: "✓ ünï", at: M.epochMs() },
      ]);
      // ORDER BY at DESC: timestamps are non-increasing.
      const ats = r.json.entries.map((e) => e.at);
      t.assert(ats.every((x, i) => i === 0 || ats[i - 1] >= x), "entries ordered by at DESC");
    }),
    post("/", 500, { json: { name: null, message: "x" }, name: "NOT NULL violation -> 500" }),
    get("/count", { status: 200, json: { count: 3 } }),
    restart(),
    get("/count", { status: 200, json: { count: 3 } }),
    post("/", { status: 201, json: { id: 4, changes: 1 } }, { json: { name: "c", message: "after restart" } }),
    crash(),
    get("/count", { status: 200, json: { count: 4 } }),
    post("/", { status: 201, json: { id: 5, changes: 1 } }, { json: { name: "d", message: "after crash" } }),
    restore(),
    get("/count", { status: 200, json: { count: 5 } }),
    post("/", { status: 201, json: { id: 6, changes: 1 } }, { json: { name: "e", message: "after restore" } }),
  ],
};

export default [kv, r2, d1];
