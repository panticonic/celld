#!/bin/sh
# Regression check that SQLite cells are unaffected by the Dolt engine:
# examples/counter and examples/facets, each across a restart and a restore
# from replication (node cell files deleted, local bucket kept).
#
#   examples/dolt/sqlite-regression.sh [path/to/celld]
set -eu
CELLD=${1:-celld}
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PORT=${PORT:-19897}
BASE="http://127.0.0.1:$PORT"
PID=
start() {
  (cd "$WORK" && "$CELLD" dev --port "$PORT" --no-watch --logs >>"$WORK/celld.log" 2>&1) &
  PID=$!
  for _ in $(seq 1 300); do
    if curl -s -o /dev/null "$BASE/"; then return 0; fi
    sleep 0.1
  done
  tail -40 "$WORK/celld.log" >&2
  exit 1
}
stop() { [ -n "$PID" ] && kill "$PID" 2>/dev/null && wait "$PID" 2>/dev/null || true; PID=; }
trap 'stop' EXIT
wipe() { find "$WORK/.celld/dev" -type d -name "$1:*" -prune -exec rm -rf {} +; }

for example in counter facets; do
  WORK=$(mktemp -d)
  cp -r "$ROOT/examples/$example/." "$WORK/"
  start
  case $example in
    counter)
      for _ in 1 2 3; do curl -fsS "$BASE/?name=a" >/dev/null; done
      before=$(curl -fsS "$BASE/?name=a" | jq .n) ;;
    facets)
      for _ in 1 2 3; do curl -fsS "$BASE/" >/dev/null; done
      before=$(curl -fsS "$BASE/") ;;
  esac
  stop; start
  case $example in
    counter) after=$(curl -fsS "$BASE/?name=a" | jq .n); want=$((before + 1)) ;;
    facets) after=$(curl -fsS "$BASE/"); want=$after; [ "${after%%:*}" -gt "${before%%:*}" ] || want=lost ;;
  esac
  echo "$example: before=$before after-restart=$after"
  [ "$after" = "$want" ] || { echo "FAIL $example restart"; exit 1; }
  stop
  case $example in counter) wipe Counter ;; facets) wipe Root 2>/dev/null || true; find "$WORK/.celld/dev/runtime" -mindepth 1 -maxdepth 1 -type d -exec rm -rf {} + ;; esac
  start
  case $example in
    counter) restored=$(curl -fsS "$BASE/?name=a" | jq .n); want=$((after + 1)) ;;
    facets) restored=$(curl -fsS "$BASE/"); want=$restored; [ "${restored%%:*}" -gt "${after%%:*}" ] || want=lost ;;
  esac
  echo "$example: after-restore=$restored remote-restores=$(grep -c 'restored remote replica' "$WORK/celld.log")"
  [ "$restored" = "$want" ] || { echo "FAIL $example restore"; exit 1; }
  stop
done
echo "sqlite regression passed"
