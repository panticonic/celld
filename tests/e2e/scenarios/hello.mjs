import { get, restart } from "../lib/dsl.mjs";

const hello = (path) => get(path, { status: 200, text: `Hello from cells! url={origin}${path}` });

export default {
  example: "hello",
  title: "stateless fetch handler",
  steps: [
    hello("/"),
    hello("/a/b?c=d"),
    // Fetch's "extract a body": `new Response("text")` is `text/plain;charset=UTF-8`, as in
    // workerd. (Upstream celld 0.6.3 sent no content-type; fixed in the fork.)
    get("/", { status: 200, headers: { "content-type": "text/plain;charset=UTF-8" } }, {
      name: "string body gets default content-type text/plain",
    }),
    restart(),
    hello("/after-restart"),
  ],
};
