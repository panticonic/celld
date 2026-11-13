import { get, restart, crash, restore } from "../lib/dsl.mjs";

// One SQLite-backed DO per `name`; independent counters; durable across a
// graceful restart, a SIGKILL, and loss of every local cell database.
const inc = (name, n) =>
  get(`/increment${name ? `?name=${name}` : ""}`, {
    status: 200,
    headers: { "content-type": /^application\/json/ },
    json: { n, url: `{origin}/increment${name ? `?name=${name}` : ""}` },
  });

export default {
  example: "counter",
  title: "per-name SQLite DO counters; restart, crash and restore persistence",
  steps: [
    inc("alpha", 1),
    inc("beta", 1),
    inc("alpha", 2),
    inc("", 1), // no `name` -> "default"
    restart(),
    inc("alpha", 3),
    inc("beta", 2),
    crash(),
    inc("alpha", 4),
    inc("", 2),
    restore(),
    inc("alpha", 5),
    inc("beta", 3),
    inc("", 3),
  ],
};
