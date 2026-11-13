#!/bin/sh
# Scripted run of the object-fork example under `celld dev`: whole-object
# fork (a SQLite root with SQLite and Dolt facets, one of them nested), a
# fork while the source is busy, restore into an inactive target, refusal on
# an active one and the stop that quiesces it, reset from a pristine object,
# a restore interrupted by a crash and rolled forward (debug binaries), and
# `celld cell import` from files made by stock sqlite3 and a DoltLite store.
# Then everything again after a restart and after a restore from the bucket
# with the node's cell files deleted.
#
#   examples/object-fork/demo.sh [path/to/celld]
#
# Needs curl, jq and sqlite3. Exits non-zero on the first failed check.
set -eu
CELLD=${1:-celld}
case $CELLD in */*) CELLD=$(cd "$(dirname "$CELLD")" && pwd)/$(basename "$CELLD") ;; esac
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
cp "$HERE/index.js" "$HERE/wrangler.jsonc" "$WORK/"
PORT=${PORT:-19895}
BASE="http://127.0.0.1:$PORT"
LOG="$WORK/celld.log"
PID=

start() {
  (cd "$WORK" && "$CELLD" dev --port "$PORT" --no-watch --logs >>"$LOG" 2>&1) &
  PID=$!
  for _ in $(seq 1 300); do
    if curl -fsS "$BASE/id?name=probe" >/dev/null 2>&1; then return 0; fi
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
state() { get "/w/$1/state"; }
same() { # same <label> <a> <b>: two workspaces hold the same state
  A=$(state "$2" | jq -S 'del(.alarm)')
  B=$(state "$3" | jq -S 'del(.alarm)')
  check "$1" "{\"a\":$A,\"b\":$B}" '.a == .b'
}
scope() { echo "Workspace:$(get "/id?name=$1" | jq -r .id)"; }
wipe() { # delete every cell file of the node; the dev bucket stays
  find "$WORK/.celld/dev" -type d -name 'Workspace:*' -prune -exec rm -rf {} +
}

start
echo "-- a source with every kind of component"
get "/w/src/add?to=root&body=a1" >/dev/null
get "/w/src/add?to=notes&body=n1" >/dev/null
get "/w/src/add?to=attachments&body=t1" >/dev/null
get "/w/src/add?to=ledger&body=l1" >/dev/null
C1=$(get "/w/src/commit?message=c1" | jq -r .hash)
get "/w/src/add?to=ledger&body=l2" >/dev/null
get "/w/src/commit?message=c2" >/dev/null
check "kv"                   "$(get "/w/src/kv?set=v1")"                  '.kv == "v1"'
check "alarm"                "$(get "/w/src/alarm?in=86400000")"          '.alarm > 0'
check "source state"         "$(state src)"                               '.items == ["a1"] and .notes == ["n1"] and .attachments == ["t1"] and .ledger.engine == "dolt" and .ledger.rows == ["l1","l2"]'

echo "-- fork"
check "fork"                 "$(get "/fork?from=src&to=f1")"              '.id | length == 64'
same "fork holds everything" f1 src
check "fork has no alarm"    "$(state f1)"                                '.alarm == null and .kv == "v1"'
check "fork at a ledger commit" "$(get "/fork?from=src&to=f2&ledgerAt=$C1")" '.id | length == 64'
check "ledger reset to c1"   "$(state f2)"                                '.ledger.rows == ["l1"] and .ledger.log[0] == "c1" and .notes == ["n1"]'
check "fork never overwrites" "$(get "/fork?from=src&to=f1")"             '.error | test("already exists")'
check "root at refused"      "$(get "/fork?from=src&to=fx&rootAt=$C1")"   '.error | test("SQLite database")'
check "bad ledger commit refused" "$(get "/fork?from=src&to=fx&ledgerAt=nope")" '.error != null'
for to in root notes attachments ledger; do get "/w/f1/add?to=$to&body=f1-$to" >/dev/null; done
get "/w/src/add?to=notes&body=src-later" >/dev/null
check "fork is independent"  "$(state f1)"                                '.items == ["a1","f1-root"] and .notes == ["n1","f1-notes"] and .attachments == ["t1","f1-attachments"] and .ledger.rows == ["l1","l2","f1-ledger"]'
check "source is independent" "$(state src)"                              '.items == ["a1"] and .notes == ["n1","src-later"] and .attachments == ["t1"] and .ledger.rows == ["l1","l2"]'

echo "-- fork while the source is busy"
ACKS="$WORK/acks"
: >"$ACKS"
( for i in $(seq 1 60); do
    get "/w/src/add?to=notes&body=b$i" | jq -e '.n > 0' >/dev/null && echo "b$i" >>"$ACKS"
  done ) &
WRITER=$!
for _ in $(seq 1 600); do [ "$(wc -l <"$ACKS")" -ge 10 ] && break; sleep 0.05; done
BEFORE=$(wc -l <"$ACKS")
check "busy fork"            "$(get "/fork?from=src&to=busy")"            '.id | length == 64'
wait "$WRITER"
BUSY=$(state busy)
SRC=$(state src)
# Every write acknowledged before the fork began is in the fork, and the
# fork's notes are a prefix of the source's: a state the source was in.
check "acked writes are in the fork" "$BUSY" "[.notes[] | select(startswith(\"b\"))] | length >= $BEFORE"
check "fork is a prefix of the source" "{\"f\":$BUSY,\"s\":$SRC}" '.s.notes[0:(.f.notes | length)] == .f.notes'
TARGET=$(scope busy)
SOURCE=$(scope src)
ORDER=$(grep -n -e "fork export: root read in its turn.*target=$TARGET" -e "fork export: facet read and proved root=\"$SOURCE\"" \
  -e "fork export: root proved; publishing.*target=$TARGET" -e "published object image target=\"$TARGET\"" "$LOG" \
  | awk -F: '{print $1}' | tr '\n' ' ')
READ=$(grep -n "fork export: root read in its turn.*target=$TARGET" "$LOG" | cut -d: -f1)
PROVED=$(grep -n "fork export: root proved; publishing.*target=$TARGET" "$LOG" | cut -d: -f1)
PUBLISHED=$(grep -n "published object image target=\"$TARGET\"" "$LOG" | cut -d: -f1)
FACETS=$(awk -v a="$READ" -v b="$PROVED" 'NR > a && NR < b && /fork export: facet read and proved/' "$LOG" | wc -l)
check "log order: read, 3 facets proved, root proved, published" \
  "{\"r\":$READ,\"p\":$PROVED,\"u\":$PUBLISHED,\"f\":$FACETS}" '.r < .p and .p < .u and .f == 3'
echo "     log lines: $ORDER"

echo "-- restore"
check "backup by fork"       "$(get "/fork?from=f1&to=bk")"               '.id | length == 64'
check "restore an inactive target" "$(get "/restore?target=bk&from=src")" '.restored == true'
same "restored state" bk src
check "restore into an active one is refused" "$(get "/restore?target=f1&from=src")" '.code == "ERR_RESTORE_TARGET_ACTIVE"'
check "f1 untouched by the refusal" "$(state f1)"                         '.items == ["a1","f1-root"]'
check "restore with stop"    "$(get "/restore?target=f1&from=src&stop=1")" '.restored == true'
same "restored over a running object" f1 src
check "restored target keeps no alarm" "$(state f1)"                      '.alarm == null'
get "/w/f1/add?to=ledger&body=after-restore" >/dev/null
check "write after restore"  "$(state f1)"                                '.ledger.rows[-1] == "after-restore"'
check "reset from a pristine object" "$(get "/restore?target=f2&from=pristine&stop=1")" '.restored == true'
check "reset state"          "$(state f2)"                                '.items == [] and .notes == [] and .attachments == [] and .ledger.rows == []'

echo "-- a restore interrupted between its facets and its root"
check "victim"               "$(get "/fork?from=src&to=victim")"          '.id | length == 64'
stop
export CELLD_TEST_RESTORE_CRASH=1
start
unset CELLD_TEST_RESTORE_CRASH
get "/restore?target=victim&from=f1" >/dev/null 2>&1 || true
wait "$PID" 2>/dev/null || true
PID=
CRASHED=$(grep -n "injected crash between a restore's streams and its root" "$LOG" | cut -d: -f1 | tail -1)
start
same "rolled forward on the next activation" victim f1
if [ -n "$CRASHED" ]; then
  # The root was installed only after the restart, by the roll-forward.
  INSTALLED=$(grep -n "restore: root installed cell=$(scope victim)" "$LOG" | cut -d: -f1 | tail -1)
  check "the crash came first, the root after the restart" "{\"c\":$CRASHED,\"i\":${INSTALLED:-0}}" '.i > .c'
else
  echo "     (release binary: no injected crash; the restore completed)"
fi

echo "-- import"
IMP="$WORK/import"
mkdir -p "$IMP"
sqlite3 "$IMP/root.sqlite" "PRAGMA journal_mode=WAL;
  CREATE TABLE items(id INTEGER PRIMARY KEY, body TEXT NOT NULL);
  INSERT INTO items(body) VALUES ('imported-1'), ('imported-2');
  CREATE TABLE _cf_KV (key TEXT PRIMARY KEY, value BLOB) WITHOUT ROWID;
  INSERT INTO _cf_KV VALUES ('greeting', X'FF0F220568656C6C6F');
  CREATE TABLE _cf_METADATA (key INTEGER PRIMARY KEY, value BLOB);
  INSERT INTO _cf_METADATA VALUES (1, X'00');" >/dev/null
sqlite3 "$IMP/notes.sqlite" "CREATE TABLE rows (id INTEGER PRIMARY KEY, body TEXT NOT NULL);
  INSERT INTO rows(body) VALUES ('old-note');"
# A DoltLite store: the ledger facet's file of `src`, as the node keeps it.
LEDGER=$(find "$WORK/.celld/dev" -path "*$(scope src)/facets/$(printf ledger | sha256sum | cut -c1-32)/ltx/*/db.sqlite" | sort | tail -1)
cp "$LEDGER" "$IMP/ledger.db"
"$CELLD" cell import object-fork Workspace imported --file "$IMP/root.sqlite" \
  --facet notes="$IMP/notes.sqlite" --facet ledger="$IMP/ledger.db:dolt" --dev "$WORK" >/dev/null 2>&1
check "imported state"       "$(state imported)"                          '.items == ["imported-1","imported-2"] and .greeting == "hello" and .notes == ["old-note"] and .ledger.engine == "dolt" and .ledger.rows == ["l1","l2"]'
get "/w/imported/add?to=notes&body=new-note" >/dev/null
check "write after import"   "$(state imported)"                          '.notes == ["old-note","new-note"]'
refused() { # refused <label> <pattern> <celld cell import args...>
  label=$1; pattern=$2; shift 2
  if out=$("$CELLD" cell import object-fork Workspace "$@" --dev "$WORK" 2>&1); then
    echo "FAIL $label: the import succeeded" >&2; exit 1
  fi
  check "$label" "$(jq -n --arg e "$out" '{error: $e}')" ".error | test(\"$pattern\")"
}
refused "import refuses an existing object" "already exists" imported --file "$IMP/root.sqlite"
refused "import refuses a facet of the wrong engine" "must be a dolt database" other --file "$IMP/root.sqlite" --facet ledger="$IMP/notes.sqlite:dolt"
refused "import refuses a root of the wrong engine" "must be a sqlite database" other --file "$IMP/ledger.db"

for phase in "restart" "restore from the bucket (cell files deleted)"; do
  echo "-- $phase"
  SNAP=""
  for w in src f1 f2 busy bk victim imported; do SNAP="$SNAP $(state $w | jq -c 'del(.alarm)')"; done
  stop
  [ "$phase" = "restart" ] || wipe
  start
  AFTER=""
  for w in src f1 f2 busy bk victim imported; do AFTER="$AFTER $(state $w | jq -c 'del(.alarm)')"; done
  check "every object survives the $phase" "{\"a\":\"$(echo $SNAP | md5sum | cut -c1-32)\",\"b\":\"$(echo $AFTER | md5sum | cut -c1-32)\"}" '.a == .b'
done
check "source alarm survives" "$(state src)"                              '.alarm > 0'
grep -c "restored remote replica" "$LOG" | sed 's/^/     remote restores in log: /'
echo "all checks passed"
