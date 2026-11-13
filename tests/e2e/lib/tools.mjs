// Optional external toolchains of some examples (Docker, Python Workers,
// workers-rs). Each is probed, and where it can be fetched it is bootstrapped
// into a hermetic cache under the scratch root (`<scratch>/tools`); nothing
// outside the scratch tree (~/.cargo, ~/.rustup, global npm) is modified.
// Set CELLD_E2E_BOOTSTRAP=0 to forbid downloads: scenarios then skip.
import { execFile } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { toolsDir, which } from "./env.mjs";

export class Skip extends Error {}

export const bootstrapAllowed = () => process.env.CELLD_E2E_BOOTSTRAP !== "0";

export function sh(cmd, args, opts = {}) {
  return new Promise((resolve) => {
    execFile(
      cmd,
      args,
      { encoding: "utf8", timeout: 900_000, maxBuffer: 1 << 26, ...opts },
      (err, stdout, stderr) => resolve({ ok: !err, code: err?.code ?? 0, stdout, stderr, err }),
    );
  });
}

const memo = new Map();
export const once = (key, fn) => {
  if (!memo.has(key)) memo.set(key, fn());
  return memo.get(key);
};

// Hash of file contents keyed by path relative to `base` (never absolute paths).
const hashFiles = (files, base) => {
  const h = crypto.createHash("sha256");
  for (const f of files) h.update(path.relative(base, f) + "\0").update(fs.readFileSync(f));
  return h.digest("hex").slice(0, 16);
};
const listFiles = (dir, exts) => {
  const out = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (!exts || exts.some((x) => e.name.endsWith(x))) out.push(p);
    }
  };
  walk(dir);
  return out.sort();
};

// ------------------------------------------------------------------ docker
const dockerCli = () => which("docker") ?? which("podman");
async function dockerReady() {
  const cli = dockerCli();
  if (!cli) return "no docker or podman CLI on PATH";
  const r = await sh(cli, ["info"], { timeout: 20000 });
  return r.ok ? null : "container engine daemon not reachable";
}

/** Remove any container labelled by a celld node of this run (safety net). */
export async function removeNodeContainers(nodeIds) {
  const cli = dockerCli();
  if (!cli) return;
  for (const id of nodeIds) {
    const r = await sh(cli, ["ps", "-aq", "--filter", `label=celld.node=${id}`], { timeout: 20000 });
    const ids = r.stdout.split(/\s+/).filter(Boolean);
    if (ids.length) await sh(cli, ["rm", "-f", ...ids], { timeout: 60000 });
  }
}
export async function containersOfNode(nodeId) {
  const cli = dockerCli();
  const r = await sh(cli, ["ps", "-q", "--filter", `label=celld.node=${nodeId}`], { timeout: 20000 });
  return r.stdout.split(/\s+/).filter(Boolean);
}
export async function dockerImagePresent(ref) {
  const r = await sh(dockerCli(), ["image", "inspect", ref], { timeout: 20000 });
  return r.ok;
}
export async function dockerPull(ref) {
  return sh(dockerCli(), ["pull", ref], { timeout: 900_000 });
}

// --------------------------------------------------------------------- npm
/** `npm install` in `dir`, caching node_modules by package.json hash. */
export async function npmInstall(dir, extraArgs = []) {
  const key = crypto
    .createHash("sha256")
    .update(fs.readFileSync(path.join(dir, "package.json")))
    .update(extraArgs.join(" "))
    .digest("hex")
    .slice(0, 16);
  const cache = path.join(toolsDir(), `node_modules-${path.basename(dir).replace(/\W/g, "_")}-${key}`);
  await once(`npm:${cache}`, async () => {
    if (fs.existsSync(path.join(cache, ".ok"))) return;
    if (!bootstrapAllowed()) throw new Skip("npm install needed and CELLD_E2E_BOOTSTRAP=0");
    const tmp = fs.mkdtempSync(path.join(toolsDir(), "npm-"));
    fs.copyFileSync(path.join(dir, "package.json"), path.join(tmp, "package.json"));
    const r = await sh("npm", ["install", "--no-audit", "--no-fund", ...extraArgs], { cwd: tmp });
    if (!r.ok) {
      fs.rmSync(tmp, { recursive: true, force: true });
      throw new Skip(`npm install failed: ${(r.stderr || String(r.err)).split("\n").slice(-3).join(" ")}`);
    }
    fs.rmSync(cache, { recursive: true, force: true });
    fs.renameSync(path.join(tmp, "node_modules"), cache);
    fs.writeFileSync(path.join(cache, ".ok"), "");
    fs.rmSync(tmp, { recursive: true, force: true });
  });
  fs.cpSync(cache, path.join(dir, "node_modules"), { recursive: true });
  fs.rmSync(path.join(dir, "node_modules", ".ok"), { force: true });
}

// ------------------------------------------------------------------ python
const UV_MIN = [0, 13, 4];
const verGe = (v, min) => {
  for (let i = 0; i < 3; i++) if ((v[i] ?? 0) !== min[i]) return (v[i] ?? 0) > min[i];
  return true;
};
async function uvVersion(bin) {
  const r = await sh(bin, ["--version"], { timeout: 20000 });
  const m = r.stdout.match(/(\d+)\.(\d+)\.(\d+)/);
  return r.ok && m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}
/** A uv >= 0.13.4 (pywrangler requires it): on PATH, else pip-installed into the tool cache. */
export function ensureUv() {
  return once("uv", async () => {
    const onPath = which("uv");
    if (onPath) {
      const v = await uvVersion(onPath);
      if (v && verGe(v, UV_MIN)) return path.dirname(onPath);
    }
    const venv = path.join(toolsDir(), "uv-venv");
    const uv = path.join(venv, "bin", "uv");
    if (fs.existsSync(uv)) {
      const v = await uvVersion(uv);
      if (v && verGe(v, UV_MIN)) return path.dirname(uv);
    }
    if (!bootstrapAllowed()) throw new Skip(`uv >= ${UV_MIN.join(".")} is required (found ${onPath ? "an older one" : "none"}) and CELLD_E2E_BOOTSTRAP=0`);
    const py = which("python3");
    if (!py) throw new Skip("python3 not found to bootstrap uv");
    let r = await sh(py, ["-m", "venv", venv]);
    if (r.ok) r = await sh(path.join(venv, "bin", "pip"), ["install", "-q", `uv>=${UV_MIN.join(".")}`]);
    if (!r.ok) throw new Skip(`could not install uv: ${(r.stderr || String(r.err)).split("\n").slice(-2).join(" ")}`);
    return path.dirname(uv);
  });
}

/** Vendor python_modules/ for the Python example (cached by pyproject.toml hash). */
export async function preparePython(workdir) {
  const key = hashFiles([path.join(workdir, "pyproject.toml")], workdir);
  const cache = path.join(toolsDir(), `python-modules-${key}`);
  await once(`py:${key}`, async () => {
    if (fs.existsSync(path.join(cache, ".ok"))) return;
    const uvDir = await ensureUv();
    if (!bootstrapAllowed()) throw new Skip("python_modules not cached and CELLD_E2E_BOOTSTRAP=0");
    const tmp = fs.mkdtempSync(path.join(toolsDir(), "py-"));
    fs.cpSync(workdir, tmp, { recursive: true });
    const r = await sh("uv", ["run", "pywrangler", "sync"], { cwd: tmp, env: { ...process.env, PATH: `${uvDir}${path.delimiter}${process.env.PATH}` } });
    if (!r.ok || !fs.existsSync(path.join(tmp, "python_modules"))) {
      fs.rmSync(tmp, { recursive: true, force: true });
      throw new Skip(`pywrangler sync failed: ${(r.stderr + r.stdout).split("\n").filter(Boolean).slice(-3).join(" | ")}`);
    }
    fs.rmSync(cache, { recursive: true, force: true });
    fs.mkdirSync(cache, { recursive: true });
    fs.renameSync(path.join(tmp, "python_modules"), path.join(cache, "python_modules"));
    if (fs.existsSync(path.join(tmp, "pylock.toml"))) fs.copyFileSync(path.join(tmp, "pylock.toml"), path.join(cache, "pylock.toml"));
    fs.rmSync(tmp, { recursive: true, force: true });
    fs.writeFileSync(path.join(cache, ".ok"), "");
  });
  fs.cpSync(path.join(cache, "python_modules"), path.join(workdir, "python_modules"), { recursive: true });
  if (fs.existsSync(path.join(cache, "pylock.toml"))) fs.copyFileSync(path.join(cache, "pylock.toml"), path.join(workdir, "pylock.toml"));
}

// -------------------------------------------------------------------- wasm
function wasmEnv() {
  const t = toolsDir();
  const cargoHome = path.join(t, "cargo");
  return {
    ...process.env,
    RUSTUP_HOME: path.join(t, "rustup"),
    CARGO_HOME: cargoHome,
    CARGO_TARGET_DIR: path.join(t, "wasm-target"),
    PATH: `${path.join(cargoHome, "bin")}${path.delimiter}${process.env.PATH}`,
  };
}
/** A private rustup toolchain with wasm32 + worker-build (all inside the tool cache). */
export function ensureWasmToolchain() {
  return once("wasm-toolchain", async () => {
    const env = wasmEnv();
    const bin = path.join(env.CARGO_HOME, "bin");
    // examples/wasm pins worker 0.8.x, which needs worker-build 0.8.x (0.9 refuses it).
    const wbRoot = path.join(toolsDir(), "worker-build-0.8");
    env.WORKER_BUILD = path.join(wbRoot, "bin", "worker-build");
    if (fs.existsSync(env.WORKER_BUILD)) return env;
    if (!bootstrapAllowed()) throw new Skip("wasm toolchain (rustup + wasm32 + worker-build) not cached and CELLD_E2E_BOOTSTRAP=0");
    if (!which("curl")) throw new Skip("curl not found to bootstrap rustup");
    if (!fs.existsSync(path.join(bin, "rustup"))) {
      const init = path.join(toolsDir(), "rustup-init.sh");
      let r = await sh("curl", ["-sSfL", "https://sh.rustup.rs", "-o", init], { timeout: 120000 });
      if (!r.ok) throw new Skip("could not download rustup-init");
      r = await sh("sh", [init, "-y", "--no-modify-path", "--profile", "minimal", "--default-toolchain", "stable", "-t", "wasm32-unknown-unknown"], { env });
      if (!fs.existsSync(path.join(bin, "rustup"))) throw new Skip(`rustup install failed: ${r.stderr.split("\n").slice(-3).join(" ")}`);
    }
    const r = await sh(path.join(bin, "cargo"), ["install", "worker-build@^0.8", "--locked", "--root", wbRoot], { env });
    if (!r.ok) throw new Skip(`cargo install worker-build failed: ${r.stderr.split("\n").slice(-3).join(" ")}`);
    return env;
  });
}

/** Build the wasm example (build/worker/shim.mjs); cached by Cargo.toml + src hash. */
export async function prepareWasm(workdir) {
  const key = hashFiles([path.join(workdir, "Cargo.toml"), ...listFiles(path.join(workdir, "src"), [".rs"])], workdir);
  const cache = path.join(toolsDir(), `wasm-build-${key}`);
  await once(`wasm:${key}`, async () => {
    if (fs.existsSync(path.join(cache, ".ok"))) return;
    const env = await ensureWasmToolchain();
    const tmp = fs.mkdtempSync(path.join(toolsDir(), "wasm-"));
    fs.cpSync(workdir, tmp, { recursive: true });
    const r = await sh(env.WORKER_BUILD, ["--release"], { cwd: tmp, env });
    if (!r.ok || !fs.existsSync(path.join(tmp, "build", "worker", "shim.mjs"))) {
      fs.rmSync(tmp, { recursive: true, force: true });
      throw new Error(`worker-build failed:\n${(r.stderr + r.stdout).split("\n").slice(-25).join("\n")}`);
    }
    fs.rmSync(cache, { recursive: true, force: true });
    fs.mkdirSync(cache, { recursive: true });
    fs.renameSync(path.join(tmp, "build"), path.join(cache, "build"));
    fs.rmSync(tmp, { recursive: true, force: true });
    fs.writeFileSync(path.join(cache, ".ok"), "");
  });
  fs.cpSync(path.join(cache, "build"), path.join(workdir, "build"), { recursive: true });
}

// ------------------------------------------------------------ requirements
const builtin = {
  /** The Docker/Podman daemon must be reachable. */
  docker: () => once("docker", dockerReady),
};

/** req: a string naming a builtin, or {name, check: async (env) => null|reason}. */
export async function checkRequirement(req, env) {
  if (typeof req === "string") {
    const fn = builtin[req];
    if (!fn) return `unknown requirement ${req}`;
    return fn(env);
  }
  return once(`req:${req.name}`, () => req.check(env));
}
