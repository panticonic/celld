import { get, http, restart, run, M } from "../lib/dsl.mjs";

// Asset-only project: no Worker script. `_headers` and `_redirects` apply.
const index = { status: 200, headers: { "content-type": /^text\/html/, "x-served-by": "celld", etag: /^"[0-9a-f]{64}"$/ } };

export default {
  example: "static-assets",
  title: "asset-only site: files, _headers, _redirects, etag/304, HEAD, 404",
  steps: [
    get("/", { ...index, text: /<h1>This file is served by celld\.<\/h1>/ }),
    get("/home", { status: 301, headers: { location: "/" } }, { name: "_redirects: /home -> / (301)" }),
    // The html_handling default redirects /index.html to /. celld answers 307.
    get("/index.html", { status: 307, headers: { location: "/" } }),
    get("/nope", 404),
    get("/_headers", 404, { name: "_headers is not served" }),
    get("/_redirects", 404, { name: "_redirects is not served" }),
    http("HEAD", "/", { ...index, text: "" }),
    run("conditional GET: If-None-Match -> 304", async (t) => {
      const first = await t.fetch("/");
      const etag = first.headers.etag;
      t.assert(etag, "etag present");
      const r = await t.fetch("/", { headers: { "if-none-match": etag } });
      t.expect(r, { status: 304, text: "", headers: { etag, "x-served-by": "celld" } });
      const other = await t.fetch("/", { headers: { "if-none-match": '"deadbeef"' } });
      t.expect(other, { status: 200 });
    }),
    restart(),
    get("/", { ...index, text: /celld/ }),
  ],
};
