#!/usr/bin/env bash
# Scripted `celld dev` run: an in-memory counter survives past the idle-eviction
# age while a residency hold stands, and is evicted once the hold is released.
#
#   CELLD=path/to/celld examples/residency/run.sh
#
# Exits non-zero if the counter does not behave as described.
set -euo pipefail

CELLD=${CELLD:-celld}
HERE=$(cd "$(dirname "$0")" && pwd)
PORT=${PORT:-19876}
IDLE_S=2
# Idle eviction runs on the once-a-second load sample, so wait a few ticks past
# the idle age.
WAIT_S=$((IDLE_S + 5))
URL="http://127.0.0.1:$PORT"

state=$(mktemp -d)
trap 'kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; rm -rf "$state"' EXIT

(cd "$state" && CELLD_IDLE_EVICT_S=$IDLE_S "$CELLD" dev "$HERE" --port "$PORT" --logs \
  >"$state/node.log" 2>&1) &
pid=$!

for _ in $(seq 60); do
  curl -fsS "$URL/inc?name=probe" >/dev/null 2>&1 && break
  sleep 1
done

field() { sed -n "s/.*\"$1\":\([^,}]*\).*/\1/p"; }
get() { curl -fsS "$URL$1?name=$2"; }

echo "== without a hold: the counter is lost to idle eviction"
for _ in 1 2 3; do get /inc plain >/dev/null; done
plain_born=$(get /inc plain | field bornAt)
sleep "$WAIT_S"
plain=$(get /inc plain)
echo "$plain"
[ "$(echo "$plain" | field counter)" = 1 ] || { echo "FAIL: expected a fresh counter" >&2; exit 1; }
[ "$(echo "$plain" | field bornAt)" != "$plain_born" ] || { echo "FAIL: expected a new object" >&2; exit 1; }

echo "== with a hold: the counter survives well past CELLD_IDLE_EVICT_S=$IDLE_S"
for _ in 1 2 3; do get /inc kernel >/dev/null; done
held=$(get /hold kernel)
echo "$held"
[ "$(echo "$held" | field held)" = true ] || { echo "FAIL: hold refused: $held" >&2; exit 1; }
born=$(echo "$held" | field bornAt)
sleep "$WAIT_S"
sleep "$WAIT_S"
after=$(get /get kernel)
echo "$after"
[ "$(echo "$after" | field counter)" = 3 ] || { echo "FAIL: held counter was lost" >&2; exit 1; }
[ "$(echo "$after" | field bornAt)" = "$born" ] || { echo "FAIL: held object was rebuilt" >&2; exit 1; }

echo "== after release: the object is evicted like any other"
released=$(get /release kernel)
echo "$released"
[ "$(echo "$released" | field held)" = false ] || { echo "FAIL: still held" >&2; exit 1; }
sleep "$WAIT_S"
fresh=$(get /get kernel)
echo "$fresh"
[ "$(echo "$fresh" | field counter)" = 0 ] || { echo "FAIL: released object was not evicted" >&2; exit 1; }

echo "ok"
