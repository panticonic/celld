import { get, put, post, M } from "../lib/dsl.mjs";

// Web Platform APIs inside the isolate: URL, TextEncoder, btoa/atob, Headers.
const view = (path, query_foo) =>
  get(path, {
    status: 200,
    headers: { "content-type": /^application\/json/ },
    json: {
      pathname: path.split("?")[0],
      search: path.includes("?") ? "?" + path.split("?")[1] : "",
      query_foo,
      encoded_len: 5,
      b64: "aGVsbG8=",
      back: "hello",
      header: "1, 2", // Headers.append joins with ", "
      protocol: "http:",
      host: "{host}",
    },
  });

export default {
  example: "webapi",
  title: "URL, TextEncoder, base64 and Headers",
  steps: [
    view("/p/q?foo=bar", "bar"),
    view("/", null),
    view("/%E2%9C%93?foo=a%20b", "a b"),
    // Same handler for every method.
    post("/x?foo=1", { status: 200, json: M.partial({ pathname: "/x", query_foo: "1" }) }),
    put("/y", { status: 200, json: M.partial({ pathname: "/y", query_foo: null }) }),
  ],
};
