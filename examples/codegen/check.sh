#!/bin/sh
# Asserts codegen confinement through a real `celld dev` node.
#
#   examples/codegen/check.sh /path/to/celld [celld dev flags...]
#
# Needs node 18+ (for fetch) and curl. Uses a private port and a throwaway
# .celld directory, and removes both.
set -eu

celld=${1:?usage: check.sh /path/to/celld [celld dev flags...]}
shift
here=$(cd "$(dirname "$0")" && pwd)
port=${CODEGEN_CHECK_PORT:-19871}
log=$(mktemp)
cd "$here"

"$celld" dev --clean --port "$port" "$@" >"$log" 2>&1 &
node_pid=$!
cleanup() {
  kill "$node_pid" 2>/dev/null || true
  wait "$node_pid" 2>/dev/null || true
  rm -rf "$here/.celld"
  rm -f "$log"
}
trap cleanup EXIT

tries=0
until curl -fs "http://127.0.0.1:$port/stateless" >/dev/null 2>&1; do
  tries=$((tries + 1))
  if [ "$tries" -gt 120 ] || ! kill -0 "$node_pid" 2>/dev/null; then
    echo "celld dev did not become ready" >&2
    cat "$log" >&2
    exit 1
  fi
  sleep 0.5
done

BASE="http://127.0.0.1:$port" node --input-type=module -e '
const base = process.env.BASE;
const get = async (path) => (await fetch(base + path)).json();
let failed = 0;
const check = (label, actual, expected) => {
  const ok = JSON.stringify(actual) === JSON.stringify(expected);
  console.log(`${ok ? "ok  " : "FAIL"} ${label}: ${JSON.stringify(actual)}`);
  if (!ok) failed++;
};

const refused = { error: "EvalError" };
const denied = {
  eval: refused,
  indirectEval: refused,
  Function: refused,
  AsyncFunction: refused,
  "WebAssembly.Module": { error: "CompileError" },
  "WebAssembly.compile": { error: "CompileError" },
  importedWasm: { ok: "5" },
};
const allowed = {
  eval: { ok: "2" },
  indirectEval: { ok: "4" },
  Function: { ok: "42" },
  AsyncFunction: { ok: "7" },
  "WebAssembly.Module": { ok: "true" },
  "WebAssembly.compile": { ok: "true" },
  importedWasm: { ok: "5" },
};

check("stateless handler is confined", await get("/stateless"), denied);
check("undeclared class is confined", await get("/confined/probe"), denied);
check("declared class may generate code", await get("/trusted/probe"), allowed);

const confinedLoader = await get("/confined/loader");
check("loader without codegen: loaded worker is confined",
  confinedLoader.withoutGrant, refused);
check("loader without codegen cannot grant it",
  /does not hold code generation/.test(confinedLoader.requested?.refused ?? ""), true);

const trustedLoader = await get("/trusted/loader");
check("loader with codegen: loaded worker is confined by default",
  trustedLoader.withoutGrant, refused);
check("loader with codegen may grant it", trustedLoader.requested, { ok: "42" });
process.exit(failed === 0 ? 0 : 1);
'
