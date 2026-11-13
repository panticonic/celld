// Step constructors for scenario files. A scenario is a list of steps; the
// runner (runner.mjs) executes them in order against one `celld dev`.
export { M } from "./match.mjs";

/**
 * HTTP request step.
 *  req:    {method, path, headers?, body?, json?}   (strings may use {$capture})
 *  expect: status number or {status, headers, text, json, bytes}
 *  opts:   {name, poll:{timeoutMs, intervalMs, why}, knownFail:"reason"}
 * `poll` retries the request until the expectation holds. Use it only where
 * celld timing is inherent (alarms, queues, cron) and state `why`.
 */
export function http(method, path, expect, opts = {}) {
  const { name, poll, knownFail, ...req } = opts;
  return { kind: "req", name: name ?? `${method} ${path}`, req: { method, path, ...req }, expect, poll, knownFail };
}
export const get = (path, expect, opts) => http("GET", path, expect, opts);
export const del = (path, expect, opts) => http("DELETE", path, expect, opts);
export const post = (path, expect, opts) => http("POST", path, expect, opts);
export const put = (path, expect, opts) => http("PUT", path, expect, opts);

/** Graceful stop (SIGTERM) then start on the same state directory. */
export const restart = (name = "restart (graceful)") => ({ kind: "restart", name });
/** SIGKILL then start: an unclean crash. Acknowledged writes must survive. */
export const crash = (name = "crash (SIGKILL) and restart") => ({ kind: "crash", name });
/**
 * Stop, delete every local cell database (`.celld/dev/runtime`), start.
 * The replication log in `.celld/dev/objects.sqlite3` is the only copy left,
 * so the next access to each cell must restore from it. The runner asserts
 * that celld logged a restore plan for the wiped cells.
 */
export const restore = (name = "wipe local runtime state, restore from replication log") => ({ kind: "restore", name });
/** Graceful stop without restarting (pair with `start()`). */
export const stop = (name = "stop") => ({ kind: "stop", name });
export const start = (name = "start") => ({ kind: "start", name });

/** Wait for a celld log line (bounded). `session:true` = current process only. */
export const waitLog = (re, { timeoutMs = 10000, why = "", session = true, name } = {}) => ({
  kind: "waitLog",
  re,
  timeoutMs,
  why,
  session,
  name: name ?? `log ~ ${re}`,
});
/** Assert that no log line matches (checked over the whole run). */
export const noLog = (re, name) => ({ kind: "noLog", re, name: name ?? `no log ~ ${re}` });

/** Arbitrary async code. `t` is the TestContext (see runner.mjs). */
export const run = (name, fn, opts = {}) => ({ kind: "fn", name, fn, ...opts });
