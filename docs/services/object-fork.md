# Object fork, restore and import

A Durable Object on celld is a root database plus any number of
[facets](durable-object-facets.md), each with a database of its own, each
SQLite or [Dolt](dolt-cells.md). Three operations act on the whole object,
every component with its engine:

| Operation | API | Use |
| --- | --- | --- |
| fork | `env.NS.fork(sourceId, options?)` | clone, backup (fork to a backup id) |
| restore | `env.NS.restore(targetId, fromId, options?)` | restore a backup, reset (restore from a pristine object) |
| import | `celld cell import SCRIPT CLASS NAME --file ...` | create an object from database files (a one-time migration) |

These are features of the celld fork; Cloudflare has no equivalent API.

## Example

The [object-fork example](../../examples/object-fork) keeps a workspace: a
SQLite object with a SQLite facet `notes` (which has a facet of its own,
`attachments`) and a Dolt facet `ledger`. `examples/object-fork/demo.sh
[celld]` runs it under `celld dev`: fork, a fork at a ledger commit, a fork
while the source takes writes, restore into an inactive target, refusal on
an active one and `stop`, reset from a pristine object, a restore interrupted
by a crash and completed after the restart, an import from files written by
stock `sqlite3` and from a DoltLite store, and all of it again after a
restart and after a restore from the bucket with the node's files deleted.

## Fork

```js
const copy = await env.WORKSPACE.fork(sourceId);                     // a new unique id
const backup = await env.WORKSPACE.fork(sourceId, { name: "backup/1" }); // idFromName
await env.WORKSPACE.fork(sourceId, { to: someId });                  // an id of this namespace
await env.WORKSPACE.fork(sourceId, { facets: { ledger: { at: hash } } });
```

`fork()` resolves to the new object's id once every component of the copy is
in the bucket. The copy holds:

- the root database, and every facet, recursively, each with its engine;
- for a Dolt component, every branch and its history, with the checked-out
  branch reset to `at` when the options give one: `at` for a Dolt root,
  `facets[name].at` for a Dolt facet (and `facets[name].facets[child].at`
  below it);
- the key-value data (`ctx.storage.put`), moved to the new object's id.

It does not hold what belongs to the source's identity: its alarm and its id
name. The new object has no alarm.

**`at` applies to Dolt components only.** A SQLite database has no history,
so a SQLite component is always copied at its newest committed state. A fork
that gives `at` for a SQLite root, or `facets[name].at` for a SQLite facet,
is refused. A Dolt root's `at` rewinds the root only: which facets exist, and
their contents, are not versioned with the root's commits.

**A fork never overwrites an object.** The target is claimed with a
conditional create of its ownership record. A target that exists, including
one a fork, restore or import is still writing, refuses the fork.

**Consistency.** The source exports itself, as an event of its own:

1. The root is read in the source's turn, outside any storage transaction,
   so the image holds committed state. A SQLite root is copied with
   SQLite's online backup through the object's own connection; a Dolt root
   is cut at its newest sealed root.
2. The facets are read after the root, each parent before its children.
   Each facet's stream is proved durable past its read before the fork goes
   on.
3. The copy is published only after the root's output gate releases, which
   covers the root read.

So no component of a fork holds a write the source could still lose. And
because a parent learns of a child's writes only through the child's replies
and outbound effects, which leave only once the child's stream proved them,
every child write that a parent's copied state depends on is in the child's
copy. The components are read one after another, not at one instant: a write
made during the fork can be in a later-read component and not in an earlier
one, as a crash of the source at that moment could also leave it.

The source does not need to be quiesced, and its instance is not
constructed by the export. A fork can be taken while the source serves.

## Restore

```js
await env.WORKSPACE.restore(targetId, backupId);                  // target quiesced
await env.WORKSPACE.restore(targetId, pristineId, { stop: true }); // stop it first
```

`restore()` replaces the target's root and all of its facets with a copy of
`fromId`'s, made as a fork makes one (`at` and `facets` are the same
options). Facets of the target that the source does not have are deleted.
The target keeps its identity: its id name, and its alarm record's
sequence. It has no alarm afterwards. `restore()` resolves to
`{ restored: true }` once every component is durable.

**Quiesced.** The target must be quiesced: in its current activation

- no instance of its class exists (it was never constructed, or it was
  reset with `ctx.abort()`),
- no facet of it is running or being started,
- no WebSocket is attached to it,
- it holds no [residency hold](residency.md),
- no other event of it is in flight, and no fork of it is being exported.

An inactive target, resident on no node, is quiesced: the restore activates
it without constructing it. A target that is not quiesced refuses the
restore with an error whose `code` is `ERR_RESTORE_TARGET_ACTIVE`, and the
message names the reason. Nothing waits for a target to go idle.

**`stop`.** With `{ stop: true }` the restore stops a running target itself,
as one event, the way a reset does: its instance goes (a continuation of an
old event finds its storage closed), its residency holds end, its
WebSockets close with 1012, and its facets close. Then the restore checks
again. A facet of a class from the root's own script (`ctx.exports`) is not
closed by `stop`; abort it with `ctx.facets.abort()` first.

**Nothing observes a half-restored object.** Every event of the target waits
until the restore finishes before it constructs the object. The restore
first records its intent in the target root and proves it durable, then
replaces the facets, then replaces the root, which removes the intent in the
same commit. If the node stops in between, the target's next event finds
the intent and completes the restore before the object runs, from the same
staging copy. The staging copy is a hidden object that the source's export
publishes for this restore only; it is deleted once the restore is durable.

## Import

```sh
celld cell import "$SCRIPT" UniversalDO "$KEY" \
  --file "$HASH.sqlite" --facet do="$HASH.1.sqlite" \
  --bucket "$CELLD_BUCKET"
celld cell import my-app Workspace imported --file root.sqlite \
  --facet notes=notes.sqlite --facet ledger=ledger.db:dolt --dev .
```

`celld cell import SCRIPT CLASS NAME` creates the object that SCRIPT's
namespace for CLASS gives `idFromName(NAME)` (with `--id`, NAME is the
object's hex id), and prints its scope. `--file` is the root database, each
`--facet NAME=FILE[:ENGINE]` the database of the facet NAME, which the object
then reaches with `ctx.facets.get(NAME, ...)`. ENGINE is `sqlite` (the
default) or `dolt`, a DoltLite store; start a Dolt facet with
`celld: { storage: "dolt" }`. `--bucket` names a fleet bucket, `--dev
PROJECT` the local bucket of `celld dev` in PROJECT.

The import refuses:

- an object that exists;
- a facet file that is not of its declared engine;
- a root file whose engine is not the class's in SCRIPT's current deployment
  (`new_sqlite_classes` or `new_dolt_classes`).

A SQLite file is read through SQLite, so a WAL file beside it counts. A file
that workerd wrote has its runtime tables converted: workerd's `_cf_KV`
becomes celld's (values keep their V8 serialization), and workerd's
`_cf_METADATA`, its alarm, is dropped. The object is created as a fork is:
claimed conditionally, with every database uploaded before any node can
activate it. An import that stopped before it completed leaves an object
that no node activates; `--replace-unfinished` takes it over.

The import writes the bucket directly, so the object must not be addressed
until the import returns.

## How it is built

- **The facet index.** Each parent (the root, or a facet with facets of its
  own) records its direct facets, by name and engine, in its own database
  (`_cf_FACET_INDEX`). The parent writes the row in its own turn when it
  starts the facet, before the facet can write, so the parent's output gate
  covers it. `ctx.facets.delete()` removes the row; `deleteAll()` keeps it.
  The engine recorded is the most recent start's; a facet's file is what
  decides its engine, so a fork opens a facet the index names wrongly with
  the other engine.
- **One image format.** Each component is a whole-file image. A new object
  (fork, import, a restore's staging copy) is published as one TXID-1
  snapshot per component at epoch 1, under the root's prefix and each facet
  stream's, behind an ownership record that carries `publishing` until the
  last upload, and that no node acquires. Its first activation acquires
  epoch 2 and restores every stream from epoch 1, as for an
  operator-released cell.
- **Restore installs into live streams.** A SQLite root takes the image
  through SQLite's backup into its own connection, one write transaction that
  replication captures and the output gate proves like any other. A Dolt
  root's file is replaced and reopened, and its capture ships the new
  generation whole, as after `gc()`. The target's facet streams are deleted
  and recreated from the images.

## Limits

- Every image is held in memory whole, on the source's node and on the
  target's, and a fork shares no storage with its source.
- A facet created before the facet index existed is not in its parent's
  index until the parent starts it again, and a fork does not carry it until
  then. A restore deletes it anyway.
- `stop` does not close a facet of a class from the root's own script.
- `restore()` takes the target's class's namespace only: source and target
  are of one class.
- An import creates direct facets of the root only.
