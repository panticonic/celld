# Dolt cells

A Dolt cell is a Durable Object whose database is a
[DoltLite](https://github.com/dolthub/doltlite) prolly-tree store instead of a
stock SQLite file. It runs the same `ctx.storage.sql` as any SQLite-backed
object, and adds version control: commits, branches, merge, diff, reset, and
forks of a whole object. celld replicates it, fences it, and proves its writes
durable exactly as it does a SQLite cell.

This is a feature of the celld fork; Cloudflare has no equivalent API.

## Example

The [dolt example](../../examples/dolt) keeps notes in a versioned table and
exposes every operation over HTTP.

`examples/dolt/demo.sh [celld]` runs it end to end under `celld dev`: commit,
branch, merge, a conflict, reset, diff, fork, a restart, and two restores from
replication (before and after `gc()`).

## Declare a Dolt class

Name the class in `new_dolt_classes` instead of `new_sqlite_classes`:

```jsonc
"durable_objects": { "bindings": [{ "name": "REPO", "class_name": "Repo" }] },
"migrations": [{ "tag": "v1", "new_dolt_classes": ["Repo"] }]
```

A class's storage engine is fixed when the class is created. `celld deploy`
refuses a class named in both lists, in one migration or two, and a node
refuses to open a SQLite cell's file as a Dolt cell or the reverse. A
deployment with a Dolt class requires the `dolt-cells-v1` feature, so a node
without the engine refuses the deployment instead of creating stock files.

## API

`ctx.storage.dolt` exists only on a Dolt cell (it is `undefined` elsewhere).
Every method is synchronous, like `ctx.storage.sql`, and throws on failure.

| method | result |
| --- | --- |
| `commit({ message, author?, date?, allowEmpty? })` | the new commit hash. Stages every application table (`dolt commit -Am`). `author` is `"Name <email>"`; `date` is an ISO string, a `Date`, or epoch milliseconds |
| `branch.create(name, { from? })` | creates `name` at `from` (default `HEAD`) |
| `branch.delete(name, { force? })` | deletes a branch; `main` and the checked-out branch cannot be deleted |
| `branch.list()` | `[{ name, hash, committer, date, message }]` |
| `branch.current()` | the checked-out branch |
| `checkout(name, { create? })` | switches the object to `name` (creating it first with `create`) |
| `reset({ hard?, to? })` | resets the current branch to `to` (default `HEAD`); returns the new `HEAD` hash |
| `merge(name, { message? })` | `{ hash, conflicts: [] }`, or `{ hash: null, conflicts: [{ table, conflicts }] }` |
| `merge(name, { message, author?, date?, resolve, fastForward? })` | merges and resolves conflicts inside the merge's own transaction: see [Merge with a resolver](#merge-with-a-resolver). `{ hash, conflicts: [], fastForward, upToDate, resolved }` |
| `log({ limit? })` | `[{ hash, committer, email, date, message }]`, newest first |
| `diff({ from, to?, table? })` | `{ summary, patch }`: the changed tables and the SQL statements that turn `from` into `to` (`to` defaults to the working set) |
| `hashOf(ref)` | the commit hash of a branch, tag, hash, or `HEAD~n` |
| `status()` | the uncommitted changes to application tables |
| `gc()` | compacts the store (see [Storage and replication](#storage-and-replication)) |
| `execute(program, context, { log? })` | runs one deterministic program (Chronolog Spec 10) as its own transaction: `{ outcome: "accepted" }` or `{ outcome: "rejected", code, statementIndex }`. `context` is `{ nowMs, seed, actor?, schemaAdmin?, limits? }`; `'now'`, `random()`, `localtime` and the actor are pinned for the program. With `log: { sql, bindings? }`, that statement runs after the program in the same transaction, as application SQL, and may name `:outcome`, `:rejection_code` and `:failing_statement_index`. An operational failure throws |
| `query(sql, bindings?, { limits?, maxRows? })` | one read-only statement under the local-read profile and a step budget, `{ columns, rows }` with exact storage classes: INTEGER is a `bigint`, REAL a number, BLOB a `Uint8Array`. A refusal throws an error whose `code` is `SQL_PROFILE_VIOLATION`, `SQL_STEP_LIMIT`, `SQL_RESULT_LIMIT`, ... |
| `engineDigest()` | the digest of the engine build and execution profile; it changes whenever execution semantics could |

### Merge with a resolver

`merge(name, { resolve })` merges `name` into the checked-out branch and lets
the application resolve what conflicts, inside one transaction, with no merge
session left between events:

```js
const result = ctx.storage.dolt.merge("feature", {
  message: "Merge feature",
  author: "Ann <ann@example.com>",   // optional
  date: 1700000000000,               // optional, pins the commit time
  resolve(merge) {
    for (const row of merge.conflicts) {
      // row.table, row.key ({ id: 1 }), row.base / row.ours / row.theirs
      // (the row as { column: value }, or null where that side has none),
      // row.ourChange / row.theirChange ("added" | "modified" | "removed")
      if (row.table === "notes" && row.theirs !== null) {
        ctx.storage.sql.exec("UPDATE notes SET body = ? WHERE id = ?",
          merge3(row.base.body, row.ours.body, row.theirs.body), row.key.id);
        row.resolve();            // I wrote the row with SQL
      } else {
        row.keepOurs();           // or row.takeTheirs()
      }
    }
    for (const violation of merge.violations) {
      // violation.table, .type ("unique index", "foreign key", ...), .info, .row
      ctx.storage.sql.exec("UPDATE notes SET slug = slug || '-2' WHERE id = ?", violation.row.id);
      violation.resolve();
    }
  },
});
// { hash, conflicts: [], fastForward: false, upToDate: false,
//   resolved: { conflicts: 2, violations: 1 } }
```

What happens:

1. `dolt_merge --no-commit` runs inside a transaction. Rows that changed on one
   side only are merged by the engine (including different columns of one
   row); a merge Dolt can finish needs no resolver and `resolve` is still
   called once, with empty lists, so the application can add its own rows.
   A **fast-forward** or an **up-to-date** merge makes no merge commit and
   does not call `resolve` (`fastForward: false` forces a merge commit).
2. `resolve(merge)` is called once, synchronously, with `merge.conflicts` (every
   conflicting row of every table), `merge.violations` (every constraint
   violation the merge produced) and the hashes `merge.ours`, `merge.theirs`
   and `merge.base`. It may write rows with `ctx.storage.sql`, read the
   conflict tables (`dolt_conflicts_<table>`), and read the other branch
   with `dolt_at_<table>('branch')` (see [Reading history](#reading-history)).
   Values keep their storage class (an INTEGER beyond 2^53 is a `bigint`, a BLOB
   a `Uint8Array`).
3. It **must answer every row**: `conflict.resolve()` (the row is as it
   should be), `conflict.keepOurs()` (the same, for ours), `conflict.takeTheirs()`
   (their row replaces ours, their deletion deletes ours) and
   `violation.resolve()`. A row left unanswered makes the merge fail.
4. celld then clears the conflict and violation tables, checks that nothing is
   outstanding, and makes **one merge commit with both parents**
   (`message`, `author` and `date` as in `commit`), in the same root record as
   the resolver's writes (see the previous section).

If `resolve` throws, returns a promise, or leaves a row unanswered, or if
anything else fails, **everything rolls back**: the resolver's rows, the merge
state, the conflicts. The branch and its working set are as they were, and the
merge can be run again. Schema conflicts (the two sides changed one table's
schema differently) cannot be resolved by a row resolver and fail the merge.
The resolver sees at most 100 000 conflicting rows and violations; a merge with
more fails. The call is a transaction of its own and throws inside another.

### Commit at the end of a transaction

`ctx.storage.transactionSync(fn, { commit })` and
`ctx.storage.transaction(fn, { commit })` make the transaction's SQL writes and
a Dolt commit one durable step:

```js
const count = ctx.storage.transactionSync(
  () => {
    ctx.storage.sql.exec("INSERT INTO notes(body) VALUES (?)", body);
    return ctx.storage.sql.exec("SELECT count(*) AS n FROM notes").one().n;
  },
  { commit: { message: "add a note", author: "Ann <ann@example.com>", date } },
);
```

`commit` is `{ message, author?, date?, allowEmpty? }`, a bare message, or a
function of the callback's result that returns either (a message rendered
from the facts the callback wrote). A function may return `null` to leave the
writes uncommitted. The call still returns the callback's result; the new
commit is `ctx.storage.dolt.hashOf("HEAD")`.

The writes and the commit are written as **one root record**, so one replicated
unit: a crash can never leave the writes uncommitted, which is the window that
a write followed by `dolt.commit()` has (two root records). If the callback
throws, if the commit cannot be made (nothing to commit without `allowEmpty`,
a malformed author), or if anything fails after it, the whole transaction rolls
back: no rows, no commit. `{ commit }` belongs to the outermost transaction;
on a nested one (a savepoint, which a Dolt commit cannot undo) and on a SQLite
cell it throws.

Values passed to `execute` and `query` keep SQLite's storage classes: a safe
integer or a `bigint` is INTEGER, any other number is REAL, a one-element
`Float64Array` is REAL even when integral, bytes are BLOB. Programs cannot read
or write celld's `_cf_*` tables.

On every cell, `ctx.storage.beginTransactionSync()` returns `{ commit(),
rollback() }`: `transactionSync()` without the callback, for adapters such as a
`node:sqlite` shim that receive BEGIN and COMMIT as separate calls. The handle
must be closed in the same turn; one still open when the turn's microtasks run
is rolled back.

`env.REPO.fork(sourceId, { at? })` returns a new id whose object starts with a
copy of the source: every branch and its history, with the checked-out branch
reset to `at` when it is given, and every facet of the source.
`env.REPO.restore(targetId, fromId)` replaces an object with such a copy. Both
work for any SQLite-backed class too: see
[Object fork, restore and import](object-fork.md).

Application SQL reads and writes the checked-out branch's working set. It can
read the Dolt system tables (`dolt_log`, `dolt_status`, `dolt_diff_<table>`,
`dolt_branches`, ...) and call read-only functions (`dolt_hashof`,
`active_branch`, `dolt_merge_base`, `dolt_version`), but it cannot call
`dolt_commit`, `dolt_checkout` or any other versioning function, and cannot
write a `dolt_*` system table: versioning goes through `ctx.storage.dolt`.
Functions with network or file reach (`dolt_clone`, `dolt_fetch`,
`dolt_push`, `dolt_pull`, `dolt_remote`, credentials, `dolt_backup`) are
denied to every statement; a cell's state leaves the node only through
replication.

## Checkout

Dolt keeps a working set per branch, but celld's own tables (the key-value
API, alarms, bookkeeping) belong to the object and must follow whichever
branch is checked out. `checkout` is `dolt_checkout --carry-ignored` (celld's
DoltLite patch 0003, `vendor/libsqlite3-sys/doltlite/patches`): the engine
moves the session's untracked ignored tables to the target branch **by catalog
entry**, replacing the target's stale copy of the same name, and reads and
writes no row. A branch you left keeps a stale copy that is never read; the
next checkout to it replaces it with the live one. celld then records which
branch is active (a pointer in `main`'s working set, which every open reads
first): a switch to `main`, one write, a switch back.

**The cost of a checkout does not depend on how much local state there is.**
Measured on the release profile (`cargo test -p celld --release --lib
checkout_cost -- --ignored --nocapture`, one local table, a local write before
each checkout, 200 checkouts alternating two branches, this machine's NVMe,
whose `fsync` takes about 6.4 ms):

| `synchronous` | local rows | checkout after a local write, p50 / p95 | checkout with nothing written, p50 / p95 |
| --- | ---: | ---: | ---: |
| OFF (no device flush) | 0 | 0.67 / 0.93 ms | 0.70 / 0.75 ms |
| OFF | 1 000 | 0.66 / 0.77 ms | 0.71 / 0.77 ms |
| OFF | 10 000 | 0.74 / 0.81 ms | 0.73 / 0.78 ms |
| NORMAL (celld's setting) | 0 | 13.78 / 15.07 ms | 7.50 / 8.19 ms |
| NORMAL | 1 000 | 13.83 / 15.05 ms | 7.41 / 7.84 ms |
| NORMAL | 10 000 | 13.68 / 15.42 ms | 7.55 / 7.87 ms |
| FULL | 10 000 | 14.14 / 16.31 ms | 7.66 / 8.18 ms |

The CPU cost is under a millisecond and the same for 0, 1 000 and 10 000 rows.
What remains at `synchronous=FULL` or `NORMAL` is the device: a checkout
makes the pointer write durable (one root record) and, when the branch you left
has local writes that no commit made durable, records its new state (a second
one). A transaction made with [`{ commit }`](#commit-at-the-end-of-a-transaction)
leaves nothing to record. On a device with a fast flush the checkout is about
a millisecond.

## Local tables

Some tables belong to an object and not to its history: an outbox of messages
to send, receipts of what was delivered, a cache. They must change in the same
transaction as the versioned facts they come from, so they live in the same
store, but they must not be rewound by `reset`, copied by a fork, or carried
into a merge. Declare them as **local tables** by name pattern.

For a Dolt class, in the deployment config's `celld` block:

```jsonc
"celld": { "durable_objects": { "Channel": { "doltLocal": ["outbox_%", "receipt_%"] } } }
```

For a Dolt facet: a facet is not a deployment class (it can come from a Worker
Loader, which has no manifest), so it declares them where it declares its
engine, in the startup options. The class block does not reach facets.

```js
ctx.facets.get("ws:a", () => ({
  class: WorkspaceClass,
  celld: { storage: "dolt", doltLocal: ["outbox_%"] },
}));
```

A pattern is up to 64 ASCII letters, digits and `_`, starting with a letter,
with `%` standing for any run of characters; it is case-sensitive. A pattern
that could cover a table celld or Dolt owns (`%`, `d%`, `_cf_%`, `dolt_%`,
`sqlite_%`) is refused, at `celld deploy` for a class and when the facet starts
for a facet. `doltLocal` on a class that is not a Dolt class is refused. A
deployment that uses it requires the `dolt-local-v1` feature, so a node that
does not know it refuses the deployment instead of versioning the tables.

A table whose name matches is an ordinary table to `ctx.storage.sql` (create
it, index it, read and write it in the same transactions as any other), and:

| operation | local tables |
| --- | --- |
| `commit`, `status`, `diff`, `log` | never staged, never shown, never in a commit hash |
| `reset({ hard })` | left as they are, not rewound |
| `merge` | left as they are |
| `checkout` | move with the object to the target branch, as one set, [without copying rows](#checkout) |
| `env.NS.fork`, `restore`, `ctx.facets.fork` | start **empty** in the new object (their schema stays, so the class's `CREATE TABLE IF NOT EXISTS` finds it) |
| `deleteAll()` | dropped, like any application table |

There is one live copy: the checked-out branch holds it, and `checkout` moves
it. They are replicated and restored with the rest of the store.

How it works: a pattern is a row of `dolt_ignore`, like celld's own `_cf_%`, so
Dolt never stages the tables, and `checkout` carries untracked ignored tables
between branches (see [Checkout](#checkout)).

Things to know:

- The rules are rows of a versioned table. celld puts them back wherever they
  decide something (at every open, on `commit` and on `checkout`), but a rule
  added by a redeploy, or put back on a branch that lacks it, is an uncommitted
  change to `dolt_ignore` until that branch commits (`status` shows it, and a
  `merge` into that branch asks you to commit first). Removing a pattern from
  the declaration leaves its rule in place.
- Declare a pattern before the class creates its first table: a table that was
  committed before it matched a rule stays tracked (Dolt's own rule for ignored
  tables).
- Do not use `AUTOINCREMENT` in a local table (`INTEGER PRIMARY KEY` is fine).
  DoltLite keeps `sqlite_sequence` as one tracked table, so a counter for an
  ignored table leaks into commits and makes `merge` refuse with "uncommitted
  changes". A fork empties the rows of a local table but a rowid may continue
  from where the source's table was.
- A facet's declaration is read when the facet starts, so each
  `ctx.facets.get` that creates the facet must name it. The rules persist in the
  store: a start that names fewer patterns does not remove any, and one that
  names more adds them.

## Semantics

**Output gate.** Each method that can change the store (commit, branch create
and delete, checkout, reset, merge, gc) counts as a write: the next response,
or any other output, waits until a durability proof covers it, exactly as
after a SQL `INSERT`. Read methods behave like a SQL `SELECT`.

**Transactions.** No method runs inside `transaction()` or
`transactionSync()`; each one throws there. SQL writes made before a
`commit()` in the same handler are committed SQLite transactions already, so
`commit()` sees them, but they are a separate durable step from the commit.
To make writes and commit one step, give the transaction
[`{ commit }`](#commit-at-the-end-of-a-transaction).

**Merge conflicts never commit.** `merge(name)` runs in a transaction. When it
conflicts, the transaction rolls back, the branch is unchanged, and the result
names the conflicting tables. Resolve on a branch (for example, update the
rows so they agree) and merge again, or merge with a
[resolver](#merge-with-a-resolver), which resolves inside the merge's own
transaction.

**Checkout persists.** The checked-out branch survives a restart, an eviction,
a takeover, and a restore. An object starts on `main`.

**celld's own state is not versioned.** The key-value API, alarms, and celld's
bookkeeping live in `_cf_*` tables inside the store, so a handler's KV writes
and SQL writes commit and replicate together. They are excluded from every
commit (a `dolt_ignore` rule celld installs), survive `reset` and `merge`, and
move with `checkout` without being copied (see [Checkout](#checkout)). The
`dolt_ignore` rule itself is part of the first commit. celld's alarm
bookkeeping triggers are connection-local on a Dolt cell, so they never enter
`dolt_schemas` or a commit.

## Storage and replication

A Dolt cell's file is a DoltLite chunk store: a header, then an append-only
log of chunks and root records. Every commit (a SQLite transaction or a Dolt
commit) appends chunks and one sealed root. celld captures the bytes appended
since its previous capture, cut at the end of the newest sealed root, and
ships them as the page images that cover them in its ordinary replication
unit. Followers, node bundles, the bucket upload, compaction, epochs and
fencing are the same code and the same guarantees as a SQLite cell
([what celld guarantees](../guarantees.md)): an acknowledged commit is never
lost. A restore rebuilds the chunk file, and DoltLite verifies every chunk
hash and root as it opens it.

`gc()` rewrites the whole file. celld then captures the new file whole, which
the bucket compacts like any snapshot. Each activation of a cell uploads one
whole image as its epoch's base, as for a SQLite cell, so the bucket cost of a
Dolt cell grows with its compacted size.

## Forks

`fork()` asks the source object to publish an image of itself. The source
reads its file at the newest commit in its own turn, removes the state that
belongs to its identity (its alarm and its id name), moves its key-value data
to the new id, resets the checked-out branch to `at`, and uploads the image
as the new object's first epoch, with an image of each of its facets. The
upload waits behind the source's output gate, so a fork never starts from
state the source could still lose, and the new id is claimed with a
conditional create, so a fork never overwrites an object.
[Object fork, restore and import](object-fork.md) has the whole contract.

The cost is one copy of the source's file on the source's node and one whole
upload; a fork shares no storage with its source. Copy-on-write forks over
shared chunks are planned.

## Dolt facets

A facet of any object can be a Dolt store too:
`ctx.facets.get(name, () => ({ class, celld: { storage: "dolt" } }))`.
It has the whole API above, including `execute`, and `ctx.facets.fork()`
copies one into a new facet. See
[Durable Object Facets](durable-object-facets.md#dolt-facets) and the
[dolt-facets example](../../examples/dolt-facets).

## Limits

- The replication format never carries SQLite's lock page (the 4 KiB page
  at 1 GiB), so a chunk file's pages are numbered past it, and a restore
  removes the hole it leaves. A chunk file can grow past 1 GiB. Two costs
  remain: a capture after `gc()` and a restore read the whole file into
  memory, and DoltLite's own GC buffer limits live data to about 2 GiB.
- A Dolt cell never restores paged; its chain downloads whole.
- Managed (control-plane) deployments do not accept Dolt classes yet.
