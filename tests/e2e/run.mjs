#!/usr/bin/env node
// End-to-end suite for celld: runs `celld dev` against every project in
// examples/ and asserts behaviour. See tests/e2e/README.md.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { E2E_DIR, EXAMPLES_DIR, resolveBin, resolveEsbuild, scratchRoot } from "./lib/env.mjs";
import { runScenario } from "./lib/runner.mjs";

const USAGE = `usage: tests/e2e/run [options] [filter...]

  filter        scenario ids or globs (e.g. counter, queues*, 'ws*'); default: all
  --list        list scenarios and exit
  --jobs N      scenarios in parallel (default: half the CPUs, at most 8)
  --keep        keep each scenario's working directory
  --json FILE   write machine-readable results
  --bin PATH    binary under test (default: $CELLD_BIN or target/release/celld)
  --no-slow     skip scenarios tagged slow (cron waits up to a minute)
  --selftest    run the harness's own tests (matchers, negative controls) and exit
  -h, --help

env: CELLD_BIN, CELLD_ESBUILD, CELLD_E2E_TMP (disk-backed scratch dir)
`;

const args = process.argv.slice(2);
const opts = { jobs: Math.max(2, Math.min(8, os.cpus().length >> 1)), keep: false, list: false, json: null, slow: true, selftest: false };
const filters = [];
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === "-h" || a === "--help") {
    process.stdout.write(USAGE);
    process.exit(0);
  } else if (a === "--list") opts.list = true;
  else if (a === "--keep") opts.keep = true;
  else if (a === "--no-slow") opts.slow = false;
  else if (a === "--selftest") opts.selftest = true;
  else if (a === "--jobs") opts.jobs = Math.max(1, Number(args[++i]));
  else if (a === "--json") opts.json = args[++i];
  else if (a === "--bin") process.env.CELLD_BIN = args[++i];
  else if (a.startsWith("-")) {
    process.stderr.write(`unknown option ${a}\n${USAGE}`);
    process.exit(2);
  } else filters.push(a);
}

async function loadScenarios() {
  const dir = path.join(E2E_DIR, "scenarios");
  const out = [];
  for (const f of fs.readdirSync(dir).filter((f) => f.endsWith(".mjs")).sort()) {
    const mod = await import(path.join(dir, f));
    for (const sc of [].concat(mod.default)) {
      sc.id ??= sc.example;
      sc.file = f;
      out.push(sc);
    }
  }
  return out;
}

const globToRe = (g) => new RegExp("^" + g.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*") + "$");

const all = await loadScenarios();
const ids = new Set();
for (const sc of all) {
  if (ids.has(sc.id)) throw new Error(`duplicate scenario id ${sc.id}`);
  ids.add(sc.id);
}
// Every example must be covered by a scenario (possibly one that skips).
const examples = fs.readdirSync(EXAMPLES_DIR, { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name);
const uncovered = examples.filter((e) => !all.some((s) => s.example === e));

let selected = all;
if (filters.length) {
  const res = filters.map(globToRe);
  selected = all.filter((s) => res.some((r) => r.test(s.id) || r.test(s.example)));
  if (!selected.length) {
    process.stderr.write(`no scenario matches ${filters.join(", ")}\n`);
    process.exit(2);
  }
}
if (!opts.slow) selected = selected.filter((s) => !s.slow);

if (opts.list) {
  for (const s of all) {
    const flags = [s.slow ? "slow" : "", s.skip ? "skip" : "", s.requires?.length ? "requires:" + s.requires.map((r) => r.name ?? r).join(",") : ""].filter(Boolean);
    process.stdout.write(`${s.id.padEnd(28)} ${s.title ?? ""} ${flags.length ? `[${flags.join(" ")}]` : ""}\n`);
  }
  process.exit(0);
}

const bin = resolveBin();
const esbuild = resolveEsbuild();
const root = scratchRoot();
const runDir = fs.mkdtempSync(path.join(root, "run-"));
const artifactsDir = process.env.CELLD_E2E_ARTIFACTS || path.join(E2E_DIR, ".artifacts");
fs.mkdirSync(artifactsDir, { recursive: true });
let version = "?";
try {
  version = execFileSync(bin, ["--version"], { encoding: "utf8" }).trim();
} catch {}
const env = { bin, esbuild, runDir, artifactsDir };
// Aborted runs (SIGINT/SIGTERM) still remove their scratch dir; lib/celld.mjs kills the processes first.
if (!opts.keep) process.on("exit", () => fs.rmSync(runDir, { recursive: true, force: true }));

if (opts.selftest) {
  const { selftest } = await import("./lib/selftest.mjs");
  let bad = 1;
  try {
    bad = await selftest(env);
  } finally {
    fs.rmSync(runDir, { recursive: true, force: true });
  }
  process.exit(bad ? 1 : 0);
}

const log = (s) => process.stdout.write(s + "\n");
log(`celld e2e: ${selected.length} scenario(s), ${opts.jobs} parallel`);
log(`  binary    ${bin} (${version})`);
log(`  esbuild   ${esbuild}`);
log(`  scratch   ${runDir}`);

const t0 = Date.now();
const results = [];
let next = 0;
const sym = { pass: "PASS", fail: "FAIL", skip: "SKIP", xfail: "XFAIL" };

async function worker() {
  while (next < selected.length) {
    const sc = selected[next++];
    const res = await runScenario(sc, env, opts).catch((e) => ({ id: sc.id, example: sc.example, status: "fail", ms: 0, steps: [{ name: "runner", status: "fail", error: String(e?.stack ?? e) }] }));
    res.title = sc.title;
    results.push(res);
    const sec = (res.ms / 1000).toFixed(1).padStart(5);
    log(`${(sym[res.status] ?? res.status).padEnd(5)} ${res.id.padEnd(26)} ${sec}s${res.reason ? `  (${res.reason})` : ""}`);
    for (const s of res.steps) {
      if (s.status === "fail" || s.status === "xpass") log(`        ${s.status.toUpperCase()}: ${s.name}\n          ${String(s.error).split("\n").join("\n          ")}`);
      else if (s.status === "xfail") log(`        XFAIL: ${s.name}  [${s.known}]\n          ${String(s.error).split("\n")[0]}`);
    }
    if (res.log) log(`        celld log: ${res.log}`);
    if (res.kept) log(`        kept: ${res.kept}`);
  }
}

try {
  await Promise.all(Array.from({ length: Math.min(opts.jobs, selected.length) }, worker));
} finally {
  if (!opts.keep) fs.rmSync(runDir, { recursive: true, force: true });
}

const count = (s) => results.filter((r) => r.status === s).length;
const failed = results.filter((r) => r.status === "fail" || r.steps.some((s) => s.status === "xpass"));
log("");
log(`${count("pass")} passed, ${count("xfail")} known-failing, ${count("skip")} skipped, ${count("fail")} failed in ${((Date.now() - t0) / 1000).toFixed(1)}s`);
const warned = results.filter((r) => r.warnings?.length);
if (warned.length) {
  log("");
  log("celld WARN/ERROR log lines (informational):");
  for (const r of warned) for (const w of r.warnings) log(`  ${r.id}: ${w}`);
}
if (uncovered.length && !filters.length) {
  log(`ERROR: examples without a scenario: ${uncovered.join(", ")}`);
}
if (opts.json) {
  fs.writeFileSync(opts.json, JSON.stringify({ bin, version, results: results.sort((a, b) => a.id.localeCompare(b.id)) }, null, 2));
}
process.exit(failed.length || (uncovered.length && !filters.length) ? 1 : 0);
