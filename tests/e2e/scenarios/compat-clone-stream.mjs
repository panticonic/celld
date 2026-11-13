// Response.clone() of a streaming body tees it, as workerd does
// (docs/fork/design.md section 3b.14). See examples/clone-stream.
import { get } from "../lib/dsl.mjs";

export default {
  example: "clone-stream",
  title: "Response.clone() and Request.clone() of a streaming body tee it",
  steps: [
    get("/response", { status: 200, json: { fromClone: "abc", fromOriginal: "abc", status: 201, header: "streamed" } }),
    get("/response-twice", { status: 200, json: { original: "123", second: "123", first: "123" } }, { name: "a clone of a clone, read in the other order" }),
    get("/response-large", { status: 200, json: { original: 32 * 64 * 1024, clone: 32 * 64 * 1024 } }, { name: "the branch read second sees all of 2 MiB" }),
    get("/fetched", { status: 200, json: { same: true, fromClone: "abc" } }, { name: "a fetch() response, a host stream, tees" }),
    get("/used", { status: 200, json: { error: "TypeError" } }, { name: "a body already read is refused" }),
    get("/request", { status: 200, json: { clone: "pq", original: "pq" } }, { name: "a request with a streaming body" }),
  ],
};
