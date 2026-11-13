#!/bin/bash
# End-to-end check of the vibestore:// bucket: each example runs on it, survives
# a restart and a wipe of the node's local data directory, and a Vibestudio-style
# GC never takes a live object. See docs/fork/vibestore.md.
#
#   cargo build -p celld
#   CELLD_ESBUILD=/path/to/esbuild crates/vibestore/e2e.sh counter kvex r2ex d1ex killtest diag
#
# Needs curl and the sqlite3 CLI. BIN, ROOT (scratch directory) and PORT can be
# overridden.
set -u
REPO=$(cd "$(dirname "$0")/../.." && pwd)
BIN=${BIN:-${CARGO_TARGET_DIR:-$REPO/target}/debug/celld}
ROOT=${ROOT:-$(mktemp -d)}
PORT=${PORT:-18081}
PASS=0; FAIL=0
ok() { echo "  ok   $*"; PASS=$((PASS+1)); }
bad() { echo "  FAIL $*"; FAIL=$((FAIL+1)); }

start_node() {
  CELLD_WATCH="$T/data" CELLD_NODE=vs-1 RUST_LOG=error "$BIN" --no-control-plane \
    --bucket "$SPEC" --listen 127.0.0.1:$PORT --internal-listen 127.0.0.1:0 \
    >"$T/node.log" 2>&1 &
  NODE=$!
  for _ in $(seq 1 120); do
    if curl -s -o /dev/null "http://127.0.0.1:$PORT/"; then return 0; fi
    kill -0 $NODE 2>/dev/null || { echo "node exited"; cat "$T/node.log" | tail -20; return 1; }
    sleep 0.5
  done
  echo "node did not come up"; tail -20 "$T/node.log"; return 1
}
stop_node() { kill -TERM $NODE 2>/dev/null; wait $NODE 2>/dev/null; }
kill_node() { kill -KILL $NODE 2>/dev/null; wait $NODE 2>/dev/null; }

# Vibestudio's GC, in sqlite3 + shell: unlink every blob no retention row names.
gc() {
  local min_age=${1:-0}
  local reachable
  reachable=$(sqlite3 "$T/blobs/retentions.db" "select distinct digest from blob_retentions" 2>/dev/null)
  local removed=0
  while IFS= read -r f; do
    d=$(echo "$f" | sed -E 's#.*/sha256/(..)/(..)/(.*)#\1\2\3#')
    if ! grep -qx "$d" <<<"$reachable"; then rm -f "$f"; removed=$((removed+1)); fi
  done < <(find "$T/blobs/sha256" -type f 2>/dev/null)
  echo $removed
}

setup() {
  EX=$1
  T="$ROOT/$EX"
  rm -rf "$T"; mkdir -p "$T/data"
  SPEC="vibestore://$T/blobs?index=$T/index.sqlite"
  echo "== $EX  ($SPEC)"
  "$BIN" deploy "$REPO/examples/$EX" --bucket "$SPEC" >"$T/deploy.log" 2>&1 \
    && ok "deploy to vibestore" || { bad "deploy"; cat "$T/deploy.log"; return 1; }
}

counter() {
  setup counter || return
  start_node || { bad "node start"; return; }
  for i in 1 2 3; do curl -s "http://127.0.0.1:$PORT/?name=alpha" >/dev/null; done
  curl -s "http://127.0.0.1:$PORT/?name=beta" >/dev/null
  r=$(curl -s "http://127.0.0.1:$PORT/?name=alpha")
  [[ "$r" == *'"n":4'* ]] && ok "alpha counted to 4" || bad "alpha: $r"
  stop_node
  start_node || { bad "restart"; return; }
  r=$(curl -s "http://127.0.0.1:$PORT/?name=alpha")
  [[ "$r" == *'"n":5'* ]] && ok "restart: alpha continues at 5" || bad "restart alpha: $r"
  stop_node
  rm -rf "$T/data"; mkdir -p "$T/data"
  start_node || { bad "restart after wipe"; return; }
  r=$(curl -s "http://127.0.0.1:$PORT/?name=alpha"); r2=$(curl -s "http://127.0.0.1:$PORT/?name=beta")
  [[ "$r" == *'"n":6'* ]] && ok "wipe: alpha restored from the store, now 6" || bad "wipe alpha: $r"
  [[ "$r2" == *'"n":2'* ]] && ok "wipe: beta restored, now 2" || bad "wipe beta: $r2"
  kill_node
  g=$(gc); r=$(sqlite3 "$T/index.sqlite" "select count(*) from objects")
  echo "  info index rows=$r blobs=$(find $T/blobs/sha256 -type f | wc -l) gc_removed=$g"
  rm -rf "$T/data"; mkdir -p "$T/data"
  start_node || { bad "start after gc"; return; }
  r=$(curl -s "http://127.0.0.1:$PORT/?name=alpha")
  [[ "$r" == *'"n":7'* ]] && ok "after GC and another wipe: alpha 7" || bad "after gc alpha: $r"
  stop_node
}

kvex() {
  setup kv || return
  start_node || { bad "node start"; return; }
  head -c 300000 /dev/urandom | base64 -w0 >"$T/big.bin"
  want=$(md5sum <"$T/big.bin")
  curl -s -X PUT --data-binary "hello world" "http://127.0.0.1:$PORT/small" -o /dev/null
  curl -s -X PUT --data-binary @"$T/big.bin" "http://127.0.0.1:$PORT/big" -o /dev/null
  [[ "$(curl -s http://127.0.0.1:$PORT/small)" == "hello world" ]] && ok "kv small round trip" || bad "kv small"
  [[ "$(curl -s http://127.0.0.1:$PORT/big | md5sum)" == "$want" ]] && ok "kv 400 KB value round trip" || bad "kv big"
  curl -s -X DELETE "http://127.0.0.1:$PORT/small" -o /dev/null
  [[ "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:$PORT/small)" == 404 ]] && ok "kv delete" || bad "kv delete"
  stop_node
  start_node || { bad "restart"; return; }
  [[ "$(curl -s http://127.0.0.1:$PORT/big | md5sum)" == "$want" ]] && ok "restart: kv big value" || bad "restart kv big"
  stop_node
  rm -rf "$T/data"; mkdir -p "$T/data"
  start_node || { bad "after wipe"; return; }
  [[ "$(curl -s http://127.0.0.1:$PORT/big | md5sum)" == "$want" ]] && ok "wipe: kv big value from the store" || bad "wipe kv big"
  kill_node
  echo "  info blobs=$(find $T/blobs/sha256 -type f | wc -l) retention_rows=$(sqlite3 $T/blobs/retentions.db 'select count(*) from blob_retentions')"
  g=$(gc); echo "  info gc_removed=$g (live blobs must survive)"
  start_node || { bad "after gc"; return; }
  [[ "$(curl -s http://127.0.0.1:$PORT/big | md5sum)" == "$want" ]] && ok "after GC: kv big value intact" || bad "after gc kv big"
  stop_node
}

r2ex() {
  setup r2 || return
  start_node || { bad "node start"; return; }
  head -c 3000000 /dev/urandom >"$T/obj.bin"
  curl -s -X PUT -H 'content-type: application/x-test' --data-binary @"$T/obj.bin" "http://127.0.0.1:$PORT/file.bin" -o /dev/null
  curl -s "http://127.0.0.1:$PORT/file.bin" -D "$T/h.txt" -o "$T/got.bin"
  cmp -s "$T/obj.bin" "$T/got.bin" && ok "r2 3 MB object round trip" || bad "r2 round trip"
  grep -qi 'content-type: application/x-test' "$T/h.txt" && ok "r2 content-type kept" || bad "r2 content-type"
  curl -s -r 1000-1999 "http://127.0.0.1:$PORT/file.bin" -o "$T/range.bin" -w '%{http_code}\n' >"$T/range.code"
  stop_node
  start_node || { bad "restart"; return; }
  curl -s "http://127.0.0.1:$PORT/file.bin" -o "$T/got2.bin"
  cmp -s "$T/obj.bin" "$T/got2.bin" && ok "restart: r2 object intact" || bad "restart r2"
  stop_node
  rm -rf "$T/data"; mkdir -p "$T/data"
  start_node || { bad "after wipe"; return; }
  curl -s "http://127.0.0.1:$PORT/file.bin" -o "$T/got3.bin"
  cmp -s "$T/obj.bin" "$T/got3.bin" && ok "wipe: r2 object from the store" || bad "wipe r2"
  curl -s -X DELETE "http://127.0.0.1:$PORT/file.bin" -o /dev/null
  [[ "$(curl -s -o /dev/null -w '%{http_code}' http://127.0.0.1:$PORT/file.bin)" == 404 ]] && ok "r2 delete" || bad "r2 delete"
  kill_node
  g=$(gc)
  echo "  info after delete: gc_removed=$g blobs_left=$(find $T/blobs/sha256 -type f | wc -l)"
}

d1ex() {
  setup d1 || return
  start_node || { bad "node start"; return; }
  for n in a b c; do
    curl -s -X POST -H 'content-type: application/json' -d "{\"name\":\"$n\",\"message\":\"hi $n\"}" "http://127.0.0.1:$PORT/" -o /dev/null
  done
  [[ "$(curl -s http://127.0.0.1:$PORT/count)" == *'"count":3'* ]] && ok "d1 three rows" || bad "d1 rows"
  stop_node
  start_node || { bad "restart"; return; }
  [[ "$(curl -s http://127.0.0.1:$PORT/count)" == *'"count":3'* ]] && ok "restart: d1 rows" || bad "restart d1"
  curl -s -X POST -H 'content-type: application/json' -d '{"name":"d","message":"after restart"}' "http://127.0.0.1:$PORT/" -o /dev/null
  stop_node
  rm -rf "$T/data"; mkdir -p "$T/data"
  start_node || { bad "after wipe"; return; }
  [[ "$(curl -s http://127.0.0.1:$PORT/count)" == *'"count":4'* ]] && ok "wipe: d1 4 rows restored from the store" || bad "wipe d1: $(curl -s http://127.0.0.1:$PORT/count)"
  stop_node
  echo "  info index rows=$(sqlite3 $T/index.sqlite 'select count(*) from objects') blobs=$(find $T/blobs/sha256 -type f 2>/dev/null | wc -l)"
}

killtest() {
  setup r2 || return
  start_node || { bad "node start"; return; }
  head -c 1500000 /dev/urandom >"$T/seed.bin"
  ( i=0; while true; do
      { cat "$T/seed.bin"; echo $i; } | curl -s -X PUT --data-binary @- "http://127.0.0.1:$PORT/obj-$((i%7))" -o /dev/null 2>/dev/null
      i=$((i+1)); done ) &
  WRITER=$!
  sleep 6
  kill_node
  kill $WRITER 2>/dev/null; wait $WRITER 2>/dev/null
  echo "  info SIGKILLed the node mid-write"
  [[ "$(sqlite3 "$T/index.sqlite" 'pragma integrity_check')" == ok ]] && ok "index integrity_check after SIGKILL" || bad "index integrity"
  missing=0
  for d in $(sqlite3 "$T/index.sqlite" "select digest from objects where digest is not null"); do
    f="$T/blobs/sha256/${d:0:2}/${d:2:2}/${d:4}"
    if [[ ! -f "$f" ]] || [[ "$(sha256sum <"$f" | cut -d' ' -f1)" != "$d" ]]; then missing=$((missing+1)); fi
  done
  [[ $missing == 0 ]] && ok "every blob the index names exists and matches its address" || bad "$missing index entries lack a valid blob"
  unret=0
  for d in $(sqlite3 "$T/index.sqlite" "select digest from objects where digest is not null"); do
    [[ "$(sqlite3 "$T/blobs/retentions.db" "select count(*) from blob_retentions where digest='$d'")" -ge 1 ]] || unret=$((unret+1))
  done
  [[ $unret == 0 ]] && ok "every live blob is retained at the moment of the kill" || bad "$unret live blobs unretained"
  echo "  info before restart: index_blobs=$(sqlite3 $T/index.sqlite 'select count(distinct digest) from objects where digest is not null') files=$(find $T/blobs/sha256 -type f | wc -l) retention_rows=$(sqlite3 $T/blobs/retentions.db 'select count(*) from blob_retentions') staging=$(ls $T/blobs/tmp-celld | wc -l)"
  start_node || { bad "restart after SIGKILL"; return; }
  okc=0
  for i in 0 1 2 3 4 5 6; do
    code=$(curl -s -o "$T/o.bin" -w '%{http_code}' "http://127.0.0.1:$PORT/obj-$i")
    [[ $code == 200 || $code == 404 ]] && okc=$((okc+1))
  done
  [[ $okc == 7 ]] && ok "restart after SIGKILL serves every object (200/404, no 500)" || bad "restart reads: $okc/7 ok"
  stop_node
  rows=$(sqlite3 "$T/blobs/retentions.db" "select count(*) from blob_retentions")
  live=$(sqlite3 "$T/index.sqlite" "select count(*) from objects where digest is not null")
  [[ $rows == $live ]] && ok "retentions reconciled to the index on restart ($rows rows)" || bad "retentions $rows vs index $live"
  g=$(gc)
  echo "  info after restart GC removed $g orphan blobs; files_left=$(find $T/blobs/sha256 -type f | wc -l)"
}

diag() {
  setup counter || return
  out=$("$BIN" diagnose --bucket "$SPEC" 2>&1); echo "$out" | head -20
  echo "$out" | grep -qi "conditional" && ok "diagnose ran the conditional-write probe" || bad "diagnose"
}

for f in "$@"; do $f; done
echo "passed=$PASS failed=$FAIL"
