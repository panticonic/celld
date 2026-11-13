#!/bin/sh
# Scripted run of the Dolt example under `celld dev`: commit, branch, merge,
# conflicts, reset, diff, fork, then a restart (local resume) and a restore
# from replication (the node's cell files deleted, the local bucket kept).
#
#   examples/dolt/demo.sh [path/to/celld]
#
# Exits non-zero on the first failed check.
set -eu
CELLD=${1:-celld}
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
cp "$HERE/index.js" "$HERE/wrangler.jsonc" "$WORK/"
PORT=${PORT:-19893}
BASE="http://127.0.0.1:$PORT"
LOG="$WORK/celld.log"
PID=

start() {
  (cd "$WORK" && "$CELLD" dev --port "$PORT" --no-watch --logs >>"$LOG" 2>&1) &
  PID=$!
  for _ in $(seq 1 300); do
    if curl -fsS "$BASE/engine?repo=probe" >/dev/null 2>&1; then return 0; fi
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
post() { curl -sS -X POST -H 'content-type: application/json' --data "$2" "$BASE$1"; }
check() { # check <label> <json> <jq-expression that must be true>
  if echo "$2" | jq -e "$3" >/dev/null; then
    echo "ok   $1"
  else
    echo "FAIL $1: $2 (log: $LOG)" >&2
    exit 1
  fi
}

start
R="repo=demo"
check "dolt engine"          "$(get "/engine?$R")"                       '.dolt == true and .branch == "main"'
get "/add?$R&body=first" >/dev/null
C1=$(get "/commit?$R&message=first&author=Ann%20%3Cann%40example.com%3E" | jq -r .hash)
check "commit hash"          "{\"h\":\"$C1\"}"                            '.h | length == 40'
check "clean status"         "$(get "/status?$R")"                        'length == 0'
check "kv write"             "$(get "/kv?$R&set=runtime-state")"          '.kv == "runtime-state"'
check "branch create"        "$(get "/branch?$R&name=feature")"           'map(.name) | index("feature") != null'
check "checkout feature"     "$(get "/checkout?$R&name=feature")"         '.branch == "feature"'
get "/add?$R&body=on-feature" >/dev/null
C2=$(get "/commit?$R&message=feature" | jq -r .hash)
check "kv survives checkout" "$(get "/kv?$R")"                            '.kv == "runtime-state"'
check "checkout main"        "$(get "/checkout?$R&name=main")"            '.branch == "main" and (.notes | length) == 1'
check "fast-forward merge"   "$(get "/merge?$R&name=feature")"            '.hash != null and (.conflicts | length) == 0'
check "merged rows"          "$(get "/notes?$R")"                         '(.notes | length) == 2'
check "log"                  "$(get "/log?$R&limit=5")"                   'map(.message) | index("feature") != null'
check "diff"                 "$(get "/diff?$R&from=$C1&to=$C2")"          '(.patch | length) == 1 and .patch[0].table == "notes"'
check "hashOf"               "$(get "/hash?$R&ref=feature")"              ".hash == \"$C2\""
check "sql cannot version"   "$(get "/sql-versioning?$R")"                '.denied == true'
# A conflict never commits.
get "/branch?$R&name=other" >/dev/null
get "/edit?$R&id=1&body=x" >/dev/null && get "/commit?$R&message=main-x" >/dev/null
get "/checkout?$R&name=other" >/dev/null
get "/edit?$R&id=1&body=y" >/dev/null && get "/commit?$R&message=other-y" >/dev/null
get "/checkout?$R&name=main" >/dev/null
check "conflict reported"    "$(get "/merge?$R&name=other")"              '.hash == null and (.conflicts | length) >= 1'
check "reset hard"           "$(get "/reset?$R&hard=1&to=$C2")"           ".head == \"$C2\" and (.notes | length) == 2"
# Leave the object on a non-main branch to prove the checkout persists.
check "checkout other"       "$(get "/checkout?$R&name=other")"           '.branch == "other"'
FORK=$(get "/fork?repo=demo&at=$C1")
check "fork at first commit" "$FORK"                                      '(.fork.notes | length) == 1'
FORK_ID=$(echo "$FORK" | jq -r .id)
check "fork keeps the kv data" "$(get "/byid/$FORK_ID/kv")"            '.kv == "runtime-state"'

echo "-- deterministic execution (ctx.storage.dolt.execute)"
CTX='"context":{"nowMs":"1700000000000","seed":"42","actor":"AAEC","schemaAdmin":true}'
PROG='"program":[{"sql":"CREATE TABLE ledger (id INTEGER PRIMARY KEY, at TEXT, r INTEGER, who BLOB, big INTEGER, x REAL)"},{"sql":"INSERT INTO ledger(at, r, who, big, x) VALUES (datetime('"'"'now'"'"'), random(), chronolog_actor(), ?, ?)","bindings":[{"index":1,"value":{"t":"int","v":"9007199254740993"}},{"index":2,"value":{"t":"real","v":1}}]}]'
PROBE='"probe":"SELECT at, r, hex(who) AS who, CAST(big AS TEXT) AS big, typeof(x) AS xt FROM ledger"'
E1=$(post "/exec?repo=ex1" "{$PROG,$CTX,$PROBE}")
E2=$(post "/exec?repo=ex2" "{$PROG,$CTX,$PROBE}")
check "exec accepted"        "$E1"                                        '.outcome.outcome == "accepted"'
check "pinned now"           "$E1"                                        '.rows[0].at == "2023-11-14 22:13:20"'
check "exact int and real"   "$E1"                                        '.rows[0].big == "9007199254740993" and .rows[0].xt == "real" and .rows[0].who == "000102"'
check "same state twice"     "{\"a\":$E1,\"b\":$E2}"                    '.a.rows == .b.rows and .a.digest == .b.digest'
REJ='"program":[{"sql":"INSERT INTO ledger(at) VALUES (1)"},{"sql":"SELECT chronolog_assert(0, '"'"'nope'"'"')"}]'
check "assert rejects"       "$(post "/exec?repo=ex1" "{$REJ,$CTX,\"probe\":\"SELECT count(*) AS n FROM ledger\"}")" \
  '.outcome.outcome == "rejected" and .outcome.code == "assert:nope" and .outcome.statementIndex == 1 and .rows[0].n == 1'
HOST='"program":[{"sql":"SELECT * FROM _cf_KV"}]'
check "host tables denied"   "$(post "/exec?repo=ex1" "{$HOST,$CTX}")"    '.outcome.outcome == "rejected" and .outcome.code == "denied"'
H1=$(get "/commit?repo=ex1&message=m&author=A%20%3Ca%40x%3E&date=1700000000000" | jq -r .hash)
H2=$(get "/commit?repo=ex2&message=m&author=A%20%3Ca%40x%3E&date=1700000000000" | jq -r .hash)
check "replica commit hashes" "{\"a\":\"$H1\",\"b\":\"$H2\"}"        '.a == .b and (.a | length) == 40'

check "exec with log row"    "$(post "/exec?repo=ex1&log=1" "{$REJ,$CTX,\"probe\":\"SELECT outcome, code, idx FROM exec_log\"}")" \
  '.outcome.outcome == "rejected" and .rows[0].outcome == "rejected" and .rows[0].code == "assert:nope" and .rows[0].idx == 1'
check "exact local query"    "$(get "/query?repo=ex1&sql=SELECT%20big,%20x%20FROM%20ledger")" \
  '.rows[0][0].int == "9007199254740993" and .rows[0][1] == 1'
check "query is read-only"   "$(get "/query?repo=ex1&sql=DELETE%20FROM%20ledger")"       '.error == "SQL_PROFILE_VIOLATION"'
check "begin/commit/rollback" "$(get "/txn?repo=ex1")"                     '.rows == [{"x":1}]'

echo "-- F2: commit at the end of a SQL transaction"
T="repo=tx"
get "/add?$T&body=base" >/dev/null
get "/commit?$T&message=base" >/dev/null
TXHEAD=$(get "/hash?$T" | jq -r .hash)
check "tx writes and commits"  "$(get "/tx?$T&body=in-tx&message=one%20step&author=Ann%20%3Cann%40example.com%3E")" \
  '.result == 2 and .status == [] and (.notes | length) == 2 and .head != "'"$TXHEAD"'"'
check "tx commit in the log"   "$(get "/log?$T&limit=2")"                '.[0].message == "one step" and .[0].committer == "Ann" and .[1].message == "base"'
TXHEAD=$(get "/hash?$T" | jq -r .hash)
check "tx throw rolls back both" "$(get "/tx?$T&body=doomed&fail=after-write")" \
  '.error == "declined" and (.notes | length) == 2 and .status == [] and .head == "'"$TXHEAD"'"'
check "tx message from result" "$(get "/tx?$T&body=three&fn=1")"        '.result == 3 and .status == []'
check "tx message log"         "$(get "/log?$T&limit=1")"               '.[0].message == "3 notes"'
check "async transaction"      "$(get "/tx-async?$T")"                  '.result == "ran" and .log[0].message == "async transaction ran" and .status == []'
check "nested commit refused"  "$(get "/tx-nested?$T")"                 '(.error | test("outermost")) and .log[0].message == "async transaction ran"'
check "tx commit with a bad author fails whole" "$(get "/tx?$T&body=bad&author=nobody")" \
  '(.error | test("author")) and (.notes | length) == 4 and .status == []'

echo "-- restart (local resume)"
stop
start
check "branch after restart" "$(get "/engine?$R")"                        '.branch == "other"'
check "kv after restart"     "$(get "/kv?$R")"                            '.kv == "runtime-state"'
check "head after restart"   "$(get "/hash?$R&ref=main")"                 ".hash == \"$C2\""
check "fork after restart"   "$(get "/byid/$FORK_ID/notes")"              '(.notes | length) == 1'

echo "-- restore from replication (node cell files deleted)"
stop
DATA=$(find "$WORK/.celld/dev" -type d -name 'Repo:*' -prune | head -1 | xargs dirname)
find "$WORK/.celld/dev" -type d -name 'Repo:*' -prune -exec rm -rf {} +
echo "     removed cell files under $DATA"
start
check "restored log"         "$(get "/log?$R&limit=50")"                  'map(.message) | (index("first") != null and index("feature") != null)'
check "restored branch"      "$(get "/engine?$R")"                        '.branch == "other"'
check "restored kv"          "$(get "/kv?$R")"                            '.kv == "runtime-state"'
check "restored main"        "$(get "/hash?$R&ref=main")"                 ".hash == \"$C2\""
check "restored fork"        "$(get "/byid/$FORK_ID/notes")"              '(.notes | length) == 1'
check "restored tx commit"   "$(get "/log?repo=tx&limit=10")"             'map(.message) | (index("one step") != null and index("3 notes") != null)'
check "restored tx rows"     "$(get "/notes?repo=tx")"                    '(.notes | length) == 4'
get "/add?$R&body=after-restore" >/dev/null
check "commit after restore" "$(get "/commit?$R&message=after")"          '.hash | length == 40'
check "gc"                   "$(get "/gc?$R")"                            '.gc != null'
get "/add?$R&body=after-gc" >/dev/null
check "commit after gc"      "$(get "/commit?$R&message=after-gc")"       '.hash | length == 40'
stop
start
check "log after gc restart" "$(get "/log?$R&limit=50")"                  'map(.message) | index("after-gc") != null'
HEAD=$(get "/hash?$R&ref=HEAD" | jq -r .hash)
echo "-- restore the post-gc generation from replication"
stop
find "$WORK/.celld/dev" -type d -name 'Repo:*' -prune -exec rm -rf {} +
start
check "post-gc restore head" "$(get "/hash?$R&ref=HEAD")"                 ".hash == \"$HEAD\""
check "post-gc restore log"  "$(get "/log?$R&limit=50")"                  'map(.message) | (index("first") != null and index("after-gc") != null)'
grep -c "restored remote replica" "$LOG" | sed 's/^/     remote restores in log: /'
echo "all checks passed"
