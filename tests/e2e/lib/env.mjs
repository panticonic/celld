// Environment discovery: the binary under test, esbuild, scratch space.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const E2E_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const REPO_ROOT = path.resolve(E2E_DIR, "..", "..");
export const EXAMPLES_DIR = path.join(REPO_ROOT, "examples");

const TMPFS_MAGIC = 0x01021994;
const RAMFS_MAGIC = 0x858458f6;

export function resolveBin() {
  const bin = path.resolve(process.env.CELLD_BIN || path.join(REPO_ROOT, "target/release/celld"));
  try {
    fs.accessSync(bin, fs.constants.X_OK);
  } catch {
    throw new Error(
      `celld binary not found or not executable: ${bin}\n` +
        `Set CELLD_BIN to the binary under test (default: target/release/celld).`,
    );
  }
  return bin;
}

export function which(cmd) {
  for (const dir of (process.env.PATH || "").split(path.delimiter)) {
    if (!dir) continue;
    const p = path.join(dir, cmd);
    try {
      fs.accessSync(p, fs.constants.X_OK);
      if (fs.statSync(p).isFile()) return p;
    } catch {}
  }
  return null;
}

// `celld deploy` and `celld dev` need an esbuild executable. Order: the
// CELLD_ESBUILD override, esbuild on PATH, then the copy that a globally
// installed wrangler or esbuild ships (`npm root -g`).
export function resolveEsbuild() {
  if (process.env.CELLD_ESBUILD) return process.env.CELLD_ESBUILD;
  const onPath = which("esbuild");
  if (onPath) return onPath;
  const roots = [];
  try {
    roots.push(execFileSync("npm", ["root", "-g"], { encoding: "utf8", timeout: 20000 }).trim());
  } catch {}
  roots.push("/usr/local/lib/node_modules", "/usr/lib/node_modules");
  const arch = `${process.platform}-${process.arch}`;
  for (const r of roots) {
    for (const rel of [
      `wrangler/node_modules/@esbuild/${arch}/bin/esbuild`,
      `esbuild/node_modules/@esbuild/${arch}/bin/esbuild`,
      `esbuild/bin/esbuild`,
    ]) {
      const p = path.join(r, rel);
      if (fs.existsSync(p)) return p;
    }
  }
  throw new Error(
    "esbuild not found. Put esbuild on PATH, or set CELLD_ESBUILD=/path/to/esbuild " +
      "(e.g. `npm i -g esbuild`, or the binary inside a wrangler install).",
  );
}

// All run state lives under a disk-backed scratch directory. A RAM-backed
// tmpfs would hide fsync behaviour, so it is rejected.
export function scratchRoot() {
  const root = path.resolve(
    process.env.CELLD_E2E_TMP ||
      path.join(process.env.XDG_CACHE_HOME || path.join(os.homedir(), ".cache"), "celld-e2e"),
  );
  fs.mkdirSync(root, { recursive: true });
  const t = fs.statfsSync(root).type;
  if ((t === TMPFS_MAGIC || t === RAMFS_MAGIC) && process.env.CELLD_E2E_ALLOW_TMPFS !== "1") {
    throw new Error(
      `scratch dir ${root} is on tmpfs/ramfs. Point CELLD_E2E_TMP at a disk-backed directory ` +
        `(or set CELLD_E2E_ALLOW_TMPFS=1 to override).`,
    );
  }
  return root;
}

// Persistent (across runs) cache for bootstrapped toolchains. Never deleted.
export function toolsDir() {
  const d = path.join(scratchRoot(), "tools");
  fs.mkdirSync(d, { recursive: true });
  return d;
}
