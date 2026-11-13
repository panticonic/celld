import { get, restart } from "../lib/dsl.mjs";

const hello = (path) => get(path, { status: 200, text: `Hello from cells! url={origin}${path}` });

export default {
  example: "hello",
  title: "stateless fetch handler",
  steps: [
    hello("/"),
    hello("/a/b?c=d"),
    // workerd gives `new Response("text")` a `content-type: text/plain;charset=UTF-8`
    // header. celld sends none (verified with curl -i against dev 0.6.3 / unified-fork).
    get("/", { status: 200, headers: { "content-type": /^text\/plain/ } }, {
      name: "string body gets default content-type text/plain",
      knownFail: "celld omits the default text/plain content-type for string bodies (workerd sets it)",
    }),
    restart(),
    hello("/after-restart"),
  ],
};
