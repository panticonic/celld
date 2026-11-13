#!/bin/sh
# Asserts that blockConcurrencyWhile keeps its 30 s limit by default and drops
# it under CELLD_NO_IMPLICIT_DEADLINES=1 (the environment form of
# --no-implicit-deadlines, which `celld dev` hands to its node), through a
# real `celld dev` node.
#
#   examples/deadlines/check.sh /path/to/celld
#
# Takes about 80 s: each of the two runs waits out a 35 s callback.
set -eu

celld=${1:?usage: check.sh /path/to/celld}
here=$(cd "$(dirname "$0")" && pwd)
port=${DEADLINES_CHECK_PORT:-19872}
cd "$here"

run() {
  expected=$1
  shift
  log=$(mktemp)
  "$celld" dev --clean --port "$port" "$@" >"$log" 2>&1 &
  node_pid=$!
  tries=0
  until curl -s -o /dev/null "http://127.0.0.1:$port/"; do
    tries=$((tries + 1))
    if [ "$tries" -gt 120 ] || ! kill -0 "$node_pid" 2>/dev/null; then
      echo "celld dev did not become ready" >&2
      cat "$log" >&2
      exit 1
    fi
    sleep 0.5
  done
  body=$(curl -s -m 90 "http://127.0.0.1:$port/")
  kill "$node_pid" 2>/dev/null || true
  wait "$node_pid" 2>/dev/null || true
  rm -rf "$here/.celld"
  rm -f "$log"
  case $body in
    *"\"outcome\":\"$expected\""*) echo "ok   $* -> $body" ;;
    *) echo "FAIL $* -> $body (wanted $expected)" >&2; exit 1 ;;
  esac
}

run reset
(export CELLD_NO_IMPLICIT_DEADLINES=1; run completed)
