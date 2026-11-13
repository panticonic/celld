# vibestore: celld on the Vibestudio blobstore

Status: implemented on `feature/vibestore` (crate `crates/vibestore`, adapter
`crates/celld/vibestore.rs`). Part of the unified fork
([design §5](design.md#5-vibestudio-requirements), "Local store on the
Vibestudio blobstore"). Facts about Vibestudio are from `32e992c1d`:
`src/server/storage/blobCas.ts`, `blobRetentions.ts`,
`src/server/services/blobstoreService.ts`.

## What it is

A bucket backend for a single-node Vibestudio workspace. celld runs with no
cloud bucket and keeps everything it would put in S3 in a directory:

```sh
celld deploy . --bucket 'vibestore:///work/ws/state/blobs?index=/work/ws/state/celld/index.sqlite'
celld --bucket    'vibestore:///work/ws/state/blobs?index=/work/ws/state/celld/index.sqlite' \
      --no-control-plane --listen 127.0.0.1:8080 --internal-listen 127.0.0.1:0
```

The URL is accepted wherever celld accepts a bucket (`--bucket`, `CELLD_BUCKET`,
`deploy`, `cell`, `kv`, `r2`, `queue`, `d1`, `diagnose`), because it is handled
in the one place that opens a bucket, `Bucket::open`. A `vibestore://` bucket
refuses `--endpoint`, static credentials and `--control-plane`.

| parameter | default | meaning |
| --- | --- | --- |
| path | required, absolute | the blob root, laid out as Vibestudio's `blobsDir` |
| `index` | `<root>/celld/index.sqlite` | the key index |
| `shared` | `1` | `1`: the root is Vibestudio's. Keep GC retentions in `<root>/retentions.db`, never unlink a blob. `0`: the root is celld's alone: no retentions, celld sweeps orphans |
| `namespace` | hash of the index path | retention namespace id, `["celld", id]` |
| `inline` | `16384` | objects of at most this many bytes live in the index. `0` disables |
| `grace_secs` | `3600` | age an unreferenced blob needs before an exclusive-mode sweep unlinks it |

## Design, and what changed from the first sketch

The design doc proposed: immutable objects in the CAS, mutable keys in a small
SQLite table that is also the GC root set. The implementation keeps that shape
and changes four things, each for a reason found in the source.

1. **Every key is in the index, not only the "mutable" ones.** `ObjectStore`
   has no notion of immutability. An LTX segment is a key like any other, and
   `list` has to enumerate it. So the index maps every key; what differs is
   where the bytes are.
2. **Small objects live in the index row, not the CAS.** An ownership lease is
   rewritten about once a second. In the CAS that is a new file, three fsyncs
   (file, directory, retention row), a retention row and a garbage blob per
   renewal, for 86,000 files a day per lease, all of which Vibestudio's GC then
   has to find. As an index row it is one SQLite commit. Objects of at most
   16 KiB (`inline`) are therefore index rows; larger ones, the LTX segments,
   snapshots, bundles, R2 and KV blobs, go to the CAS. Retention rows then
   exist only for CAS objects.
3. **The retention owner is `<key>@<digest>`, not `<key>`.** In
   `blobRetentions.ts` an owner names one digest and `retainBlob` throws if you
   try to retarget it ("release it explicitly first"). Releasing then retaining
   leaves the key unprotected between the two statements, and a crash there
   lets Vibestudio's GC delete a live object. With the digest in the owner, an
   overwrite holds the old and new rows at once, and the old row is released
   only after the index names the new digest.
4. **The version tag is a store-wide counter, not a content hash.** An etag
   made from content lets a conditional update succeed against an object that
   was deleted and re-created with the same bytes (ABA). The counter is never
   reused, so `If-Match` means "this exact write".

It is a separate crate (`celld-vibestore`) so it builds and tests without
compiling V8; `crates/celld/vibestore.rs` only chooses how a blocking job runs
on celld's execution domain.

## On-disk format

```text
<root>/
  sha256/aa/bb/<rest-of-digest>   immutable object, mode 0600     (Vibestudio's layout)
  tmp/                            Vibestudio's; created if missing, never used by celld
  tmp-celld/                      celld's private staging directory
  retentions.db                   Vibestudio's GC root registry; celld writes its namespace
<index>                           stock-format SQLite, WAL, synchronous=FULL
```

**Blobs** follow `blobCas.ts` exactly: the address is the lowercase hex SHA-256
of the bytes; a blob is written to a private temp file (`wx`, 0600), fsynced,
published with `link(2)` (so an existing address means identical bytes, which
is re-hashed and checked, as `existingBlob` does), then the directories created
on the way are fsynced, then the temp name is unlinked. The one deliberate
difference is the staging directory: Vibestudio's blobstore empties `tmp/` when
it starts, which would kill an in-flight celld write, so celld stages in
`tmp-celld/`.

**`retentions.db`** is opened, never altered: celld creates it with the exact
schema `blobRetentions.ts` creates (verbatim `CREATE TABLE`, `user_version = 1`)
when the file is absent, validates it with a port of
`openCanonicalSqliteDatabase` when it exists, and refuses any other shape. It
does not change the journal mode. It checks that the file has the stock SQLite
header, because DoltLite can create prolly-format files and `node:sqlite` could
not read one. Namespace is `["celld", id]`, byte-identical to
`blobRetentionNamespace`.

**The index** has `objects(key, etag, size, modified_ms, digest | body,
attributes)` and a one-row `sequence`. `key` is the percent-encoded
`object_store::Path`, compared as UTF-8 bytes (S3's order). `digest` or `body`
is set, never both. `user_version = 1`; an index with another version, or any
other file, is refused rather than upgraded in place.

## Writes and crash safety

Every write is one `BEGIN IMMEDIATE` transaction that checks the precondition
and installs the row. SQLite's lock is the arbiter: racers in one process, or in
the `celld dev` supervisor and its node, serialize on it, and exactly one of any
set of racers on one version wins. `PutMode::Create` fails with `AlreadyExists`,
`Update(etag)` with `Precondition`, and neither changes anything.

For a CAS-backed object the order is the contract:

1. bytes durable at their address (file fsync, link, directory fsyncs);
2. retention row `<key>@<digest>` committed to `retentions.db`;
3. index transaction commits. **This is the commit point.**
4. the retention row of the digest this key held before is released.

A death after 1 or 2 leaves an unreferenced blob and at most a retention row
no key points at. A death after 3 leaves a stale row for the old digest.
At no instant does a key point at a missing blob, and at no instant is a blob a
key points at unretained. An inline object skips 1, 2 and 4; its single
transaction is the whole write.

**Recovery is orphan cleanup, nothing else.** On open, `Core::recover`:

- removes temp files older than a day (they have no authority);
- shared mode: makes the namespace in `retentions.db` equal exactly the set
  `{<key>@<digest>}` read from the index, adding missing rows (so a deleted or
  recreated `retentions.db` heals) and releasing stale ones;
- exclusive mode: unlinks blobs no key references and that are older than
  `grace_secs`.

An unreferenced blob in shared mode is simply left for Vibestudio's GC, which
collects anything unretained and older than its `minAgeMs` (24 h by default).
There is no log to replay and no journal of intent. These points are exercised
by `crates/vibestore/tests/crash.rs`, which stops a write at each step without
running any cleanup and then reopens the store.

### The GC race this cannot close from outside

Vibestudio's `withBlobContentLock` is an in-process mutex, so celld, a separate
process, cannot take it. A GC could in principle read the retention set just
before celld retains a blob it just wrote, and unlink it just after. Two
things close this: the GC only unlinks blobs quiet for `minAgeMs` (measured
from the newer of mtime and ctime), and celld refreshes the mtime of any
existing blob it deduplicates against. A deployment that sets `minAgeMs` to 0
for this root gives that protection up; the tests assert that a GC with a
non-zero age never takes a freshly written, not-yet-retained blob.

## What celld asks of the store, and what is refused

The `ObjectStore` surface was audited against every call site (`bucket.rs`, the
LTX client in `crates/ltx/src/client/object_store.rs`, `ltx_repl.rs`,
`r2_store.rs`, `kv_blob.rs`, `js/r2_ops.rs`, `dead_node_gc.rs`, `cell_cli.rs`).

| call | support |
| --- | --- |
| `put_opts` `Overwrite`, `Create`, `Update(e_tag)` | yes, one transaction |
| `put_opts` `Update` carrying only `version` (GCS style) | `NotSupported` |
| `put_opts` / multipart with object tags | `NotSupported` |
| attributes (`content-type`, `metadata`, the standard set) | stored and returned |
| `get_opts` range (`Bounded`, `Offset`, `Suffix`), `head`, `if_match`, `if_none_match`, `if_modified_since`, `if_unmodified_since` | yes; a range past the end is clamped, a range that starts past the end is an error, as on S3 |
| `get_opts` with `version` | `NotSupported` |
| `get_range`, `get_ranges` | through `get_opts` |
| `list`, `list_with_offset`, `list_with_delimiter` | yes, keyset-paged in SQL |
| `PaginatedListStore` (`objects_page`, `common_prefixes_page`) | yes; the delimiter and token rules are `LocalStore`'s |
| `delete`, `delete_stream` | yes; absent key is `Ok`, as on S3 |
| `copy`, `copy_if_not_exists`, `rename` | yes, by reference (no bytes move) |
| `put_multipart_opts` | yes; parts are written at their final offsets into one temp file, in any completion order |

A blob that the index names but that is missing from disk is an error
(`Error::Generic`), never `NotFound`: `NotFound` would tell celld the key was
never written.

`Bucket::validate` and `celld diagnose` run celld's own conditional-write and
ranged-read conformance probes against it, like any other bucket.

## Operating it

- **Backup** is a copy of the blob root plus the index (call `close()`, or copy
  after a clean stop, so the WAL is folded in). The index is the only mutable
  state.
- **Loss of the index** loses every key; the blobs alone do not say what they
  are. Treat the index like Vibestudio treats its own databases.
- **Loss of `retentions.db`** is repaired on the next open from the index.
- **Cross-process use** is supported (the supervisor and the node). Sharing one
  index across machines over a network filesystem is not.
- **Hard links** are required in the blob root, as for Vibestudio's own CAS.
- **Windows**: directory fsync is skipped (as in `blobCas.ts`); file writes use
  `seek_write`. Not exercised yet.

## Verification

Unit and integration tests (`cargo test -p celld-vibestore`): conditional-write
races across threads, separate store instances and four OS processes; ranges on
inline and 3 MiB blobs; 5 MiB streaming; multipart; listing compared with
`object_store`'s in-memory reference and, for pagination, with a transcription
of `LocalStore`'s rules page by page; the crash points; the GC contract against
a re-implementation of `sweepUnreachableBlobs`; every refusal in the table above.

End-to-end: see the section below.
