#!/bin/sh
# Scripted run of the dolt-remotes example under `celld dev`: decentralized
# work on one graph. Two agents fork the shared graph, commit in parallel (one
# row conflicts), and merge back with fetch, pull (merge with a resolver) and
# push; the graph and both agents end on the same commit hash. Then the
# refusals (no grant, wrong branch, a branch the grant hides, a class without
# the capability, revoke, expiry, a non-fast-forward), a carrier of the
# application's own, a fork that inherits no remote, and everything again after
# a restart and after a restore from the bucket with the node's cell files
# deleted.
#
#   examples/dolt-remotes/demo.sh [path/to/celld]
#
# Needs curl and jq. Exits non-zero on the first failed check.
set -eu
CELLD=${1:-celld}
case $CELLD in */*) CELLD=$(cd "$(dirname "$CELLD")" && pwd)/$(basename "$CELLD") ;; esac
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
cp "$HERE/index.js" "$HERE/wrangler.jsonc" "$WORK/"
PORT=${PORT:-19897}
BASE="http://127.0.0.1:$PORT"
LOG="$WORK/celld.log"
PID=

start() {
  (cd "$WORK" && "$CELLD" dev --port "$PORT" --no-watch --logs >>"$LOG" 2>&1) &
  PID=$!
  for _ in $(seq 1 300); do
    if curl -fsS "$BASE/id?g=probe" >/dev/null 2>&1; then return 0; fi
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
  if printf "%s\n" "$2" | jq -e "$3" >/dev/null 2>&1; then
    echo "ok   $1"
  else
    echo "FAIL $1: $2 (log: $LOG)" >&2
    exit 1
  fi
}
state() { get "/state?g=$1"; }
head_of() { get "/hash?g=$1&ref=${2:-main}" | jq -r .hash; }

start
echo "-- the shared graph, and two agents forked from it"
get "/set?g=gad&id=1&body=one" >/dev/null
get "/set?g=gad&id=2&body=two" >/dev/null
BASE_HASH=$(get "/commit?g=gad&message=base" | jq -r .hash)
check "base commit"            "{\"h\":\"$BASE_HASH\"}"                  '.h | length == 40'
for agent in agent-a agent-b; do
  check "fork $agent"          "$(get "/fork?from=gad&to=$agent")"       '.id | length == 64'
done
check "forks start at the same commit" \
  "{\"g\":\"$(head_of gad)\",\"a\":\"$(head_of agent-a)\",\"b\":\"$(head_of agent-b)\"}" \
  ".g == \"$BASE_HASH\" and .a == .g and .b == .g"
check "a fork holds no remote and no grant" "$(state agent-a)"           '.remotes == [] and .grants == [] and .tracking == []'

echo "-- a grant is the only authority"
check "no remote api on a plain class" "$(get "/plain")"                 '.remote == false'
check "a class that did not declare it answers 403" "$(get "/plain-serve")" '.status == 403 and .code == "ERR_DOLT_REMOTE_FORBIDDEN"'
GAD_ID=$(get "/id?g=gad" | jq -r .id)
check "a remote needs a token"  "$(get "/remote-add-raw?g=agent-a&name=x&ref=Graph:$GAD_ID&token=")" '.error != null'
get "/remote-add-raw?g=agent-a&name=stranger&ref=Graph:$GAD_ID&token=not-a-grant" >/dev/null
check "a made-up token is refused" "$(get "/fetch?g=agent-a&remote=stranger")" '.code == "ERR_DOLT_REMOTE_FORBIDDEN"'
check "a grant must allow something" "$(get "/grant?g=gad&fetch=0")"     '.error != null'
for agent in agent-a agent-b; do
  check "link $agent"          "$(get "/link?source=gad&holder=$agent&label=$agent&push=main&branches=main")" '.grant | length == 16'
done
check "grants are listed without tokens" "$(state gad)"                   '.grants == ["agent-a","agent-b"]'
check "remotes are listed"     "$(state agent-a)"                         '.remotes == [{"name":"origin","ref":"Graph:'"$GAD_ID"'"},{"name":"stranger","ref":"Graph:'"$GAD_ID"'"}]'
check "a name is not a branch" "$(get "/fetch?g=agent-a&branches=nope")"  '.code == "ERR_DOLT_REMOTE_NOT_FOUND"'

echo "-- parallel work: nobody waits for anybody"
get "/checkout?g=gad&name=private&create=1" >/dev/null
get "/set?g=gad&id=99&body=private" >/dev/null
get "/commit?g=gad&message=private-work" >/dev/null
get "/checkout?g=gad&name=main" >/dev/null
( get "/set?g=agent-a&id=1&body=a-one"   >/dev/null
  get "/set?g=agent-a&id=3&body=a-three" >/dev/null
  get "/commit?g=agent-a&message=a-work" >/dev/null ) &
A=$!
( get "/set?g=agent-b&id=1&body=b-one"   >/dev/null
  get "/set?g=agent-b&id=4&body=b-four"  >/dev/null
  get "/commit?g=agent-b&message=b-work" >/dev/null ) &
B=$!
wait "$A" "$B"
A_WORK=$(head_of agent-a)
B_WORK=$(head_of agent-b)
check "the agents diverged"    "{\"a\":\"$A_WORK\",\"b\":\"$B_WORK\"}"   '.a != .b'
check "the graph did not move" "{\"g\":\"$(head_of gad)\"}"               ".g == \"$BASE_HASH\""

echo "-- fetch brings the work, changing nothing else"
check "fetch"                  "$(get "/fetch?g=agent-a&branches=main")"  '.branches.main | length == 40'
check "a fetch moved no branch" "$(state agent-a)"                        ".head == \"$A_WORK\" and (.tracking | index(\"origin/main=$(printf %.8s "$BASE_HASH")\") != null) and (.notes | length == 3)"
check "an unchanged fetch"     "$(get "/fetch?g=agent-a")"                '.branches.main | length == 40'

echo "-- push is a fast-forward the target accepts"
check "agent-a pushes"         "$(get "/push?g=agent-a&branch=main")"     ".hash == \"$A_WORK\" and .unchanged == false"
check "the graph is at agent-a's commit" "$(state gad)"                   ".branches.main == \"$A_WORK\" and .head == \"$A_WORK\" and (.notes | map(.body) | contains([\"a-one\",\"a-three\"]))"
check "pushing again changes nothing" "$(get "/push?g=agent-a&branch=main")" '.unchanged == true'
check "a stale push is refused" "$(get "/push?g=agent-b&branch=main")"    '.code == "ERR_DOLT_NON_FAST_FORWARD"'
check "a push to a branch the grant does not name" "$(get "/push?g=agent-b&branch=main&to=elsewhere")" '.code == "ERR_DOLT_REMOTE_FORBIDDEN"'

echo "-- pull: fetch and merge with a resolver, then push the merge"
check "agent-b pulls"          "$(get "/pull?g=agent-b&branch=main&message=merge-a-into-b")" '.fastForward == false and .conflicts == [] and .resolved.conflicts == 1'
check "the conflict was resolved by what both wrote" "$(state agent-b)"   '(.notes | map(.body)) == ["a-one + b-one","two","a-three","b-four"]'
M=$(head_of agent-b)
check "agent-b pushes the merge" "$(get "/push?g=agent-b&branch=main")"   ".hash == \"$M\" and .unchanged == false"
check "the graph holds the merge" "$(state gad)"                          ".head == \"$M\" and (.notes | length == 4)"
check "agent-a pulls and fast-forwards" "$(get "/pull?g=agent-a&branch=main")" ".fastForward == true"

echo "-- convergence"
check "one commit hash everywhere" \
  "{\"g\":\"$(head_of gad)\",\"a\":\"$(head_of agent-a)\",\"b\":\"$(head_of agent-b)\"}" \
  ".g == \"$M\" and .a == .g and .b == .g"
check "one state everywhere"   "{\"g\":$(state gad | jq -c .notes),\"a\":$(state agent-a | jq -c .notes),\"b\":$(state agent-b | jq -c .notes)}" '.g == .a and .a == .b'
check "history is the same everywhere" "{\"g\":$(state gad | jq -c .log),\"a\":$(state agent-a | jq -c .log)}" '.g == .a and (.g | index("merge-a-into-b") != null)'
check "the private branch was never revealed" "$(state agent-a)"        '(.tracking | map(select(test("private"))) | length) == 0 and (.branches | has("private") | not)'
check "pushes are recorded as tracking branches" "$(state gad)"           '(.tracking | map(select(test("^pushed-"))) | length) == 2'
check "the active branch had no uncommitted change" "$(state gad)"       '.status == []'

echo "-- a carrier of the application's own (what a P2P stream would carry)"
get "/set?g=agent-a&id=5&body=a-five" >/dev/null
L1=$(get "/commit?g=agent-a&message=a-five" | jq -r .hash)
check "loopback fetch of the graph" "$(get "/fetch?g=agent-b&transport=loopback&source=gad")" '.branches.main | length == 40'
check "loopback push to the graph" "$(get "/push?g=agent-a&branch=main&transport=loopback&source=gad")" ".hash == \"$L1\""
check "pull from the graph"    "$(get "/pull?g=agent-b&branch=main")"     '.fastForward == true'

echo "-- revoke and expiry"
ID_A=$(get "/link?source=gad&holder=agent-a&label=temp&push=tmp&name=temp" | jq -r .grant)
check "a new remote works"     "$(get "/fetch?g=agent-a&remote=temp")"    '.branches.main | length == 40'
check "revoke"                 "$(get "/revoke?g=gad&id=$ID_A")"          ".id == \"$ID_A\""
check "a revoked token is refused" "$(get "/fetch?g=agent-a&remote=temp")" '.code == "ERR_DOLT_REMOTE_FORBIDDEN"'
get "/link?source=gad&holder=agent-b&label=old&name=old&expiresAt=1" >/dev/null
check "an expired grant is refused" "$(get "/fetch?g=agent-b&remote=old")" '.code == "ERR_DOLT_REMOTE_FORBIDDEN"'
check "remove a remote"        "$(get "/remote-remove?g=agent-b&name=old")" '.name == "old"'

echo "-- a fork is a new identity"
check "fork of an agent"       "$(get "/fork?from=agent-a&to=agent-c")"   '.id | length == 64'
check "no remote, no grant, no tracking branch inherited" "$(state agent-c)" '.remotes == [] and .grants == [] and .tracking == []'
check "it has the history"     "{\"a\":\"$(head_of agent-a)\",\"c\":\"$(head_of agent-c)\"}" '.a == .c'

echo "-- a Dolt facet of a supervisor is a holder too"
HUB_TOKEN=$(get "/grant?g=gad&label=hub&push=hub/*&branches=main" | jq -r .token)
check "the facet has the capability"  "$(get "/hub/has-remote-api")"      '.remote == true and .fetch == "function"'
check "the facet adds a remote"       "$(get "/hub/remote-add-raw?name=origin&ref=Graph:$GAD_ID&token=$HUB_TOKEN")" '.name == "origin"'
check "the facet fetches the graph"   "$(get "/hub/fetch?branches=main")"  '.branches.main | length == 40'
get "/hub/set?id=7&body=from-the-facet" >/dev/null
FACET=$(get "/hub/commit?message=facet-work" | jq -r .hash)
check "the facet pushes a branch of its own" "$(get "/hub/push?branch=main&to=hub/main")" ".hash == \"$FACET\" and .created == true"
check "the graph holds it"            "$(state gad)"                      ".branches[\"hub/main\"] == \"$FACET\""
check "and nothing else moved"        "$(state gad)"                      ".branches.main == \"$(head_of agent-a)\""

echo "-- restart"
H=$(head_of gad)
stop
start
check "head after a restart"   "{\"g\":\"$(head_of gad)\"}"               ".g == \"$H\""
check "remotes survive"        "$(state agent-a)"                         '(.remotes | length) == 3'
check "fetch after a restart"  "$(get "/fetch?g=agent-b")"                '.branches.main | length == 40'

echo "-- restore from replication (the node's cell files deleted)"
get "/pull?g=agent-b&branch=main" >/dev/null
HG=$(head_of gad)
HA=$(head_of agent-a)
HB=$(head_of agent-b)
check "converged before the restore" "{\"g\":\"$HG\",\"a\":\"$HA\",\"b\":\"$HB\"}" '.g == .a and .a == .b'
TRACK_A=$(state agent-a | jq -c .tracking)
stop
find "$WORK/.celld/dev" -type d -name 'Graph:*' -prune -exec rm -rf {} +
start
check "restored graph"         "{\"h\":\"$(head_of gad)\"}"               ".h == \"$HG\""
check "restored agent-a"       "{\"h\":\"$(head_of agent-a)\"}"           ".h == \"$HA\""
check "restored agent-b"       "{\"h\":\"$(head_of agent-b)\"}"           ".h == \"$HB\""
check "replicas agree"         "{\"g\":\"$(head_of gad)\",\"a\":\"$(head_of agent-a)\",\"b\":\"$(head_of agent-b)\"}" '.g == .a and .a == .b'
check "restored tracking branches" "$(state agent-a)"                     ".tracking == $TRACK_A"
check "restored grants"        "$(state gad)"                             '.grants | length == 4'
get "/set?g=agent-a&id=6&body=after-restore" >/dev/null
L2=$(get "/commit?g=agent-a&message=after-restore" | jq -r .hash)
check "push after a restore"   "$(get "/push?g=agent-a&branch=main")"     ".hash == \"$L2\""
check "the graph has it"       "$(state gad)"                             ".head == \"$L2\""
echo "all checks passed"
