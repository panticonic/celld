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
| `log({ limit? })` | `[{ hash, committer, email, date, message }]`, newest first |
| `diff({ from, to?, table? })` | `{ summary, patch }`: the changed tables and the SQL statements that turn `from` into `to` (`to` defaults to the working set) |
| `hashOf(ref)` | the commit hash of a branch, tag, hash, or `HEAD~n` |
| `status()` | the uncommitted changes to application tables |
| `gc()` | compacts the store (see [Storage and replication](#storage-and-replication)) |

`env.REPO.fork(sourceId, { at? })` returns a new id whose object starts with a
copy of the source: every branch and its history, with the checked-out branch
reset to `at` when it is given.

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

## Semantics

**Output gate.** Each method that can change the store (commit, branch create
and delete, checkout, reset, merge, gc) counts as a write: the next response,
or any other output, waits until a durability proof covers it, exactly as
after a SQL `INSERT`. Read methods behave like a SQL `SELECT`.

**Transactions.** No method runs inside `transaction()` or
`transactionSync()`; each one throws there. SQL writes made before a
`commit()` in the same handler are committed SQLite transactions already, so
`commit()` sees them.

**Merge conflicts never commit.** A merge runs in a transaction. When it
conflicts, the transaction rolls back, the branch is unchanged, and the result
names the conflicting tables. Resolve on a branch (for example, update the
rows so they agree) and merge again.

**Checkout persists.** The checked-out branch survives a restart, an eviction,
a takeover, and a restore. An object starts on `main`.

**celld's own state is not versioned.** The key-value API, alarms, and celld's
bookkeeping live in `_cf_*` tables inside the store, so a handler's KV writes
and SQL writes commit and replicate together. They are excluded from every
commit (a `dolt_ignore` rule celld installs), survive `reset` and `merge`, and
move with `checkout`. Because Dolt keeps a working set per branch, a checkout
copies these tables to the target branch; its cost grows with the size of the
key-value data, so keep large data in SQL tables on a Dolt cell. The
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
belongs to its identity (its alarm and its id name), resets the checked-out
branch to `at`, and uploads the image as the new object's first epoch. The
upload waits behind the source's output gate, so a fork never starts from
state the source could still lose, and the new id is claimed with a
conditional create, so a fork never overwrites an object.

The cost is one copy of the source's file on the source's node and one whole
upload; a fork shares no storage with its source. Copy-on-write forks over
shared chunks are planned.

## Limits

- A chunk file must stay below 1 GiB to be captured (the replication format
  reserves the page at 1 GiB). A capture past it fails and the output gate
  refuses to acknowledge; run `gc()` to compact.
- A Dolt cell never restores paged; its chain downloads whole.
- `ctx.storage.dolt.execute()` (deterministic programs) is reserved and
  throws in this build.
- Managed (control-plane) deployments do not accept Dolt classes yet.
- Facets of a Dolt object are ordinary SQLite facets.
