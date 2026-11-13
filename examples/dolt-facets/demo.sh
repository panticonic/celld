#!/bin/sh
# Scripted run of the Dolt-facets example under `celld dev`: Dolt facets of a
# SQLite supervisor, created from a Worker Loader class, with commit, branch,
# merge, deterministic execute, the engine guard, a facet fork, abort, then a
# restart (local resume), a restore from replication (the node's cell files
# deleted, the local bucket kept), gc, and a second restore of the post-gc
# generation.
#
#   examples/dolt-facets/demo.sh [path/to/celld]
#
# Exits non-zero on the first failed check.
set -eu
CELLD=${1:-celld}
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
cp "$HERE/index.js" "$HERE/wrangler.jsonc" "$WORK/"
PORT=${PORT:-19894}
BASE="http://127.0.0.1:$PORT"
LOG="$WORK/celld.log"
PID=

start() {
  (cd "$WORK" && "$CELLD" dev --port "$PORT" --no-watch --logs >>"$LOG" 2>&1) &
  PID=$!
  for _ in $(seq 1 300); do
    if curl -fsS "$BASE/engine?ws=probe" >/dev/null 2>&1; then return 0; fi
    sleep 0.1
  done
  echo "celld did not become ready; log:" >&2
  tail -50 "$LOG" >&2
  exit 1
}
stop() {
  [ -n "$PID" ] && kill "$PID" 2>/dev/null && wait "$PID" 2>/dev/null || true
  PID=
}
trap 'stop' EXIT

get() { curl -sS "$BASE$1"; }
check() { # check <label> <json> <jq-expression that must be true>
  if echo "$2" | jq -e "$3" >/dev/null 2>&1; then
    echo "ok   $1"
  else
    echo "FAIL $1: $2 (log: $LOG)" >&2
    exit 1
  fi
}
wipe() { # delete every cell file of the node; the dev bucket stays
  find "$WORK/.celld/dev" -type d -name 'Supervisor:*' -prune -exec rm -rf {} +
}

start
W="ws=a"
check "dolt facet"           "$(get "/engine?$W")"                       '.dolt == true and .branch == "main"'
check "sqlite facet beside"  "$(get "/plain")"                           '.dolt == false'
get "/add?$W&body=first" >/dev/null
C1=$(get "/commit?$W&message=first" | jq -r .hash)
check "commit hash"          "{\"h\":\"$C1\"}"                            '.h | length == 40'
check "kv write"             "$(get "/kv?$W&set=runtime-state")"          '.kv == "runtime-state"'
check "branch create"        "$(get "/branch?$W&name=feature")"           'map(.name) | index("feature") != null'
check "checkout feature"     "$(get "/checkout?$W&name=feature")"         '.branch == "feature"'
get "/add?$W&body=on-feature" >/dev/null
C2=$(get "/commit?$W&message=feature" | jq -r .hash)
check "kv survives checkout" "$(get "/kv?$W")"                            '.kv == "runtime-state"'
check "checkout main"        "$(get "/checkout?$W&name=main")"            '.branch == "main" and (.notes | length) == 1'
check "merge"                "$(get "/merge?$W&name=feature")"            '.hash != null and (.conflicts | length) == 0'
check "merged rows"          "$(get "/notes?$W")"                         '(.notes | length) == 2'
check "hashOf"               "$(get "/hash?$W&ref=feature")"              ".hash == \"$C2\""
MAIN=$(get "/hash?$W&ref=main" | jq -r .hash)

echo "-- deterministic execution in a facet"
X1=$(get "/exec?ws=x1")
X2=$(get "/exec?ws=x2")
check "exec accepted"        "$X1"                                        '.outcome.outcome == "accepted"'
check "pinned clock"         "$X1"                                        '.rows[0].at == "2023-11-14 22:13:20"'
check "same rows twice"      "{\"a\":$X1,\"b\":$X2}"                    '.a.rows == .b.rows'

echo "-- the engine is fixed"
REFUSED=$(get "/as-sqlite?$W")
check "other engine refused" "$REFUSED"                                   '.refused == true and (.error | test("engine is fixed"))'
echo "     $(echo "$REFUSED" | jq -r .error | cut -c1-160)"
check "data untouched"       "$(get "/notes?$W")"                         '(.notes | length) == 2'

echo "-- fork and abort"
FORK=$(get "/fork?$W&to=b&at=$C1")
check "fork at first commit" "$FORK"                                      '(.fork.notes | length) == 1 and .forked.bytes > 0'
check "fork is dolt"         "$(get "/engine?ws=b")"                      '.dolt == true'
check "fork has history"     "$(get "/log?ws=b")"                         'map(.message) | index("first") != null'
check "fork refuses overwrite" "$(get "/fork?$W&to=b")"                   '.error | test("already exists|running")'
get "/add?ws=b&body=only-in-b" >/dev/null
check "fork is independent"  "$(get "/notes?$W")"                         '(.notes | length) == 2'
check "abort"                "$(get "/abort?$W")"                         '.aborted == "a"'
check "reopen after abort"   "$(get "/notes?$W")"                         '(.notes | length) == 2'
# Leave `a` on a non-main branch to prove the checkout persists.
check "checkout feature"     "$(get "/checkout?$W&name=feature")"         '.branch == "feature"'

echo "-- restart (local resume)"
stop
start
check "branch after restart" "$(get "/engine?$W")"                        '.branch == "feature"'
check "kv after restart"     "$(get "/kv?$W")"                            '.kv == "runtime-state"'
check "main after restart"   "$(get "/hash?$W&ref=main")"                 ".hash == \"$MAIN\""
# `b` is not running in this activation: the host refuses the overwrite.
check "fork onto stored facet" "$(get "/fork?$W&to=b")"                   '.error | test("already exists")'
check "fork after restart"   "$(get "/notes?ws=b")"                       '(.notes | length) == 2'

echo "-- restore from replication (node cell files deleted)"
stop
wipe
start
# The first open after the wipe names the wrong engine: the restore of the
# Dolt stream from the bucket is refused before anything captures it.
check "restore as sqlite refused" "$(get "/as-sqlite?$W")"                '.refused == true and (.error | test("engine is fixed"))'
check "restored log"         "$(get "/log?$W")"                           'map(.message) | (index("first") != null and index("feature") != null)'
check "restored branch"      "$(get "/engine?$W")"                        '.branch == "feature"'
check "restored kv"          "$(get "/kv?$W")"                            '.kv == "runtime-state"'
check "restored main"        "$(get "/hash?$W&ref=main")"                 ".hash == \"$MAIN\""
check "restored fork"        "$(get "/notes?ws=b")"                       '(.notes | length) == 2'
check "restored exec facet"  "$(get "/exec?ws=x1")"                       '(.rows | length) == 2'
check "back to main"         "$(get "/checkout?$W&name=main")"            '.branch == "main"'
get "/add?$W&body=after-restore" >/dev/null
check "commit after restore" "$(get "/commit?$W&message=after")"          '.hash | length == 40'
check "gc"                   "$(get "/gc?$W")"                            '.gc != null'
get "/add?$W&body=after-gc" >/dev/null
check "commit after gc"      "$(get "/commit?$W&message=after-gc")"       '.hash | length == 40'
stop
start
check "log after gc restart" "$(get "/log?$W")"                           'map(.message) | index("after-gc") != null'
HEAD=$(get "/hash?$W&ref=HEAD" | jq -r .hash)

echo "-- restore the post-gc generation from replication"
stop
wipe
start
check "post-gc restore head" "$(get "/hash?$W&ref=HEAD")"                 ".hash == \"$HEAD\""
check "post-gc restore log"  "$(get "/log?$W")"                           'map(.message) | (index("first") != null and index("after-gc") != null)'
check "post-gc rows"         "$(get "/notes?$W")"                         '(.notes | length) == 4'

echo "-- delete"
check "delete fork"          "$(get "/delete?ws=b")"                      '.deleted == "b"'
check "recreated empty"      "$(get "/notes?ws=b")"                       '(.notes | length) == 0'
stop
wipe
start
check "delete survives restore" "$(get "/notes?ws=b")"                    '(.notes | length) == 0'
grep -c "restored remote replica" "$LOG" | sed 's/^/     remote restores in log: /'
echo "all checks passed"
