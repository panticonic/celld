#!/bin/sh
# The dolt-remotes example on two nodes of one fleet: the shared graph lives on
# node 1 and the agents on node 2, so every fetch, pull and push crosses the
# inter-node channel (the same dispatch as any call to a cell on another node).
# The two nodes share a vibestore bucket on the local disk (no network, no
# containers); the same checks as demo.sh hold, and the log shows that the
# graph answered the agents from another node.
#
#   examples/dolt-remotes/cluster.sh [path/to/celld]
#
# Needs curl and jq. Exits non-zero on the first failed check.
set -eu
CELLD=${1:-celld}
case $CELLD in */*) CELLD=$(cd "$(dirname "$CELLD")" && pwd)/$(basename "$CELLD") ;; esac
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
mkdir -p "$WORK/app"
cp "$HERE/index.js" "$HERE/wrangler.jsonc" "$WORK/app/"
BUCKET="vibestore://$WORK/blobs?index=$WORK/index.db"
BASEPORT=$((20000 + $$ % 20000))
P1=${P1:-$BASEPORT}; Q1=${Q1:-$((BASEPORT + 1))}; P2=${P2:-$((BASEPORT + 2))}; Q2=${Q2:-$((BASEPORT + 3))}
N1="http://127.0.0.1:$P1"
N2="http://127.0.0.1:$P2"
PID1=
PID2=

node() { # node <n> <public> <internal>
  mkdir -p "$WORK/n$1"
  CELLD_WATCH="$WORK/n$1" "$CELLD" --bucket "$BUCKET" --listen "127.0.0.1:$2" \
    --internal-listen "127.0.0.1:$3" --advertise "127.0.0.1:$3" \
    --lease-ttl-ms 4000 >"$WORK/n$1.log" 2>&1 &
  echo $!
}
stop() {
  for pid in $PID1 $PID2; do
    [ -n "$pid" ] && kill "$pid" 2>/dev/null && wait "$pid" 2>/dev/null || true
  done
  PID1=; PID2=
}
trap 'stop' EXIT
ready() { # ready <base>
  for _ in $(seq 1 300); do
    if curl -fsS "$1/id?g=probe" >/dev/null 2>&1; then return 0; fi
    sleep 0.1
  done
  echo "a node did not become ready; logs in $WORK" >&2
  tail -30 "$WORK"/n*.log >&2
  exit 1
}

check() { # check <label> <json> <jq-expression that must be true>
  if printf "%s\n" "$2" | jq -e "$3" >/dev/null 2>&1; then
    echo "ok   $1"
  else
    echo "FAIL $1: $2 (logs: $WORK)" >&2
    exit 1
  fi
}
# Every request about the graph goes to node 1, every one about an agent to
# node 2, so each cell is activated, and owned, by that node.
g() { curl -sS "$N1$1"; }
a() { curl -sS "$N2$1"; }
H() { curl -sS "$N1/hash?g=gad&ref=main" | jq -r .hash; }
HA() { curl -sS "$N2/hash?g=$1&ref=main" | jq -r .hash; }

echo "-- deploy once, start two nodes of one fleet"
(cd "$WORK/app" && "$CELLD" deploy . --bucket "$BUCKET" >"$WORK/deploy.log" 2>&1) || { cat "$WORK/deploy.log" >&2; exit 1; }
PID1=$(node 1 "$P1" "$Q1")
ready "$N1"
PID2=$(node 2 "$P2" "$Q2")
ready "$N2"

echo "-- the graph on node 1, forks (each owned by node 2) and links"
g "/set?g=gad&id=1&body=one" >/dev/null
g "/set?g=gad&id=2&body=two" >/dev/null
BASE_HASH=$(g "/commit?g=gad&message=base" | jq -r .hash)
for agent in agent-a agent-b; do
  check "fork $agent" "$(a "/fork?from=gad&to=$agent")" '.id | length == 64'
  a "/state?g=$agent" >/dev/null
  check "link $agent" "$(a "/link?source=gad&holder=$agent&label=$agent&push=main&branches=main")" '.grant | length == 16'
done
check "the agents start at the graph's commit" "{\"a\":\"$(HA agent-a)\",\"b\":\"$(HA agent-b)\"}" ".a == \"$BASE_HASH\" and .b == .a"

echo "-- parallel work on node 2, merged back across the nodes"
a "/set?g=agent-a&id=1&body=a-one" >/dev/null
a "/set?g=agent-a&id=3&body=a-three" >/dev/null
a "/commit?g=agent-a&message=a-work" >/dev/null
a "/set?g=agent-b&id=1&body=b-one" >/dev/null
a "/set?g=agent-b&id=4&body=b-four" >/dev/null
a "/commit?g=agent-b&message=b-work" >/dev/null
check "fetch across nodes"       "$(a "/fetch?g=agent-a&branches=main")"  '.branches.main | length == 40'
A_WORK=$(HA agent-a)
check "push across nodes"        "$(a "/push?g=agent-a&branch=main")"     ".hash == \"$A_WORK\" and .unchanged == false"
check "the graph on node 1 moved" "{\"g\":\"$(H)\"}"                       ".g == \"$A_WORK\""
check "a stale push is refused"  "$(a "/push?g=agent-b&branch=main")"     '.code == "ERR_DOLT_NON_FAST_FORWARD"'
check "pull with a resolver"     "$(a "/pull?g=agent-b&branch=main&message=merge-a-into-b")" '.fastForward == false and .resolved.conflicts == 1'
M=$(HA agent-b)
check "push the merge"           "$(a "/push?g=agent-b&branch=main")"     ".hash == \"$M\""
check "pull fast-forwards"       "$(a "/pull?g=agent-a&branch=main")"     '.fastForward == true'
check "one hash on both nodes"   "{\"g\":\"$(H)\",\"a\":\"$(HA agent-a)\",\"b\":\"$(HA agent-b)\"}" ".g == \"$M\" and .a == .g and .b == .g"
GAD_ID=$(g "/id?g=gad" | jq -r .id)
a "/remote-add-raw?g=agent-a&name=stranger&ref=Graph:$GAD_ID&token=not-a-grant" >/dev/null
check "a grant is still the only authority across nodes" "$(a "/fetch?g=agent-a&remote=stranger")" '.code == "ERR_DOLT_REMOTE_FORBIDDEN"'

echo "-- the requests really crossed nodes"
# The graph is activated by node 1 only; node 2 never opened its file.
AGENT_ID=$(a "/id?g=agent-a" | jq -r .id)
if [ -z "$(find "$WORK/n2" -type d -name "Graph:$AGENT_ID" 2>/dev/null)" ]; then
  echo "FAIL node 2 does not hold agent-a's cell files" >&2; exit 1
fi
echo "ok   node 2 owns the agents"
if [ -n "$(find "$WORK/n2" -type d -name "Graph:$GAD_ID" 2>/dev/null)" ]; then
  echo "FAIL node 2 holds a copy of the graph's cell files" >&2; exit 1
fi
echo "ok   node 2 never opened the graph's file"
if [ -z "$(find "$WORK/n1" -type d -name "Graph:$GAD_ID" 2>/dev/null)" ]; then
  echo "FAIL node 1 does not hold the graph's cell files" >&2; exit 1
fi
echo "ok   node 1 owns the graph"
echo "all checks passed"
