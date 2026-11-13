// Self-tests of the harness: the matcher semantics, and negative controls that
// prove a wrong expectation (or an unexpected pass of a knownFail step) fails.
// Run with `tests/e2e/run --selftest`.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { M, match, subst, normString } from "./match.mjs";
import { runScenario } from "./runner.mjs";
import { get, put } from "./dsl.mjs";
import { startWsServer, openWs } from "./ws.mjs";

const results = [];
const test = async (name, fn) => {
  try {
    await fn();
    results.push([name, null]);
  } catch (e) {
    results.push([name, e]);
  }
};

export async function selftest(env) {
  await test("match: exact objects reject extra and missing keys", () => {
    assert.equal(match({ a: 1 }, { a: 1 }).length, 0);
    assert.ok(match({ a: 1, b: 2 }, { a: 1 }).length > 0);
    assert.ok(match({ a: 1 }, { a: 1, b: 2 }).length > 0);
    assert.ok(match({ a: 1 }, { a: 2 }).length > 0);
    assert.ok(match([1, 2], [1]).length > 0);
  });
  await test("match: partial, re, near, between, oneOf, absent", () => {
    assert.equal(match({ a: 1, b: 2 }, M.partial({ a: 1 })).length, 0);
    assert.ok(match({ a: 2 }, M.partial({ a: 1 })).length > 0);
    assert.equal(match("abc", M.re(/^a/)).length, 0);
    assert.ok(match("xbc", M.re(/^a/)).length > 0);
    assert.equal(match(1.0001, M.near(1, 0.001)).length, 0);
    assert.ok(match(1.1, M.near(1, 0.001)).length > 0);
    assert.equal(match(5, M.between(1, 5)).length, 0);
    assert.ok(match(6, M.between(1, 5)).length > 0);
    assert.equal(match(2, M.oneOf(1, 2)).length, 0);
    assert.ok(match(3, M.oneOf(1, 2)).length > 0);
    assert.equal(match({}, { a: M.absent() }).length, 0);
    assert.ok(match({ a: 1 }, { a: M.absent() }).length > 0);
  });
  await test("match: capture and typed substitution", () => {
    const vars = {};
    assert.equal(match({ id: 7 }, { id: M.capture("id", M.type("number")) }, "$", vars).length, 0);
    assert.equal(vars.id, 7);
    assert.equal(subst("{$id}", vars), 7);
    assert.equal(subst("/x/{$id}", vars), "/x/7");
    assert.deepEqual(subst({ a: ["{$id}"], r: /x/ }, vars), { a: [7], r: /x/ });
    assert.throws(() => subst("{$nope}", vars));
  });
  await test("normalisation replaces this run's origin only", () => {
    assert.equal(normString("http://127.0.0.1:123/x 127.0.0.1:123", { port: 123 }), "{origin}/x {host}");
    assert.equal(normString("http://127.0.0.1:124/x", { port: 123 }), "http://127.0.0.1:124/x");
  });
  await test("ws server + client round trip", async () => {
    const srv = await startWsServer();
    const ws = await openWs(srv.url);
    ws.send("hi");
    assert.equal(await ws.next(), "pong:hi");
    ws.close(1000, "bye");
    await ws.closed();
    await new Promise((r) => setTimeout(r, 50));
    assert.deepEqual(srv.seen.closes, [{ code: 1000, reason: "bye" }]);
    await srv.close();
  });

  // Negative controls against a real celld: the harness must be able to fail.
  const runDir = fs.mkdtempSync(path.join(env.runDir, "selftest-"));
  const sub = { ...env, runDir, artifactsDir: path.join(runDir, "artifacts") };
  const sc = (id, steps) => ({ id: `selftest-${id}`, example: "hello", steps });
  await test("negative control: a wrong expectation fails the scenario", async () => {
    const r = await runScenario(sc("wrong", [get("/", { status: 200, text: "something else" })]), sub, {});
    assert.equal(r.status, "fail");
  });
  await test("negative control: a wrong status fails the scenario", async () => {
    const r = await runScenario(sc("status", [put("/", 204)]), sub, {});
    assert.equal(r.status, "fail");
  });
  await test("negative control: a passing knownFail step is reported (xpass)", async () => {
    const r = await runScenario(sc("xpass", [get("/", { status: 200 }, { knownFail: "x" })]), sub, {});
    assert.ok(r.steps.some((s) => s.status === "xpass"));
    assert.equal(r.status, "fail");
  });
  await test("positive control: a failing knownFail step is xfail", async () => {
    const r = await runScenario(sc("xfail", [get("/", { status: 500 }, { knownFail: "x" })]), sub, {});
    assert.equal(r.status, "xfail");
  });
  await test("positive control: the right expectation passes", async () => {
    const r = await runScenario(sc("ok", [get("/", { status: 200, text: "Hello from cells! url={origin}/" })]), sub, {});
    assert.equal(r.status, "pass", JSON.stringify(r.steps));
  });

  let bad = 0;
  for (const [name, err] of results) {
    process.stdout.write(`${err ? "FAIL" : "PASS"}  ${name}\n`);
    if (err) {
      bad++;
      process.stdout.write(`      ${String(err.stack ?? err).split("\n").slice(0, 6).join("\n      ")}\n`);
    }
  }
  fs.rmSync(runDir, { recursive: true, force: true });
  return bad;
}
