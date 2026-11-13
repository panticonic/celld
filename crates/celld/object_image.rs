// Copyright 2026 the celld fork authors. Apache-2.0 license.

//! Whole-object images: fork, restore and import of a Durable Object with its
//! facets, through one mechanism (docs/services/object-fork.md).
//!
//! An object is a root cell plus a tree of facets, each with a database and a
//! replication stream of its own, each SQLite or Dolt. An [`ObjectImage`] is
//! that tree as whole-file images, one [`Component`] per database. There are
//! three producers and two consumers:
//!
//! - `env.NS.fork` exports the source from its own turn
//!   (`storage_ops::op_object_export`, `CellRuntime::export_facets`);
//! - `celld cell import` reads plain files ([`import`]);
//! - `env.NS.restore` reads a staging object back from the bucket
//!   ([`read_published`]).
//!
//! - [`publish_new`] writes an image as a new cell: a conditional create of
//!   its ownership record, one TXID-1 snapshot per component at epoch 1, and
//!   a compare-and-swap that opens the record for activation. Fork, import
//!   and the staging copy of a restore all use it.
//! - The restore installs an image into the live streams of a quiesced
//!   target, on the target's owner (`CellRuntime::adopt_streams`,
//!   `storage::adopt_root`).
//!
//! # The facet index
//!
//! A facet's stream is named by a hash of its name, and its engine is known
//! from its file only. So each parent (the root, or a facet with facets of
//! its own) records its direct children in its own database, in
//! `_cf_FACET_INDEX(name, engine)`. The parent writes the row in its own
//! turn when it starts the facet, before the facet can write anything, and
//! the row is a write like any other: the parent's output gate covers it,
//! and a fork or a restore reads it from the same image as the rest of the
//! parent. `ctx.facets.delete` removes the row.

// Images are whole files: read, rewritten and installed through SQLite and
// DoltLite connections on scratch paths beside a cell's database, inside
// blocking sections, as `dolt_cell`'s fork images are; an import reads an
// operator's files. SQLite opens those paths itself, so the injected
// filesystem could not stand in for the plain reads beside it.
#![allow(clippy::disallowed_methods)]

use anyhow::{anyhow, bail, ensure, Context};
use celld_ltx::Engine;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

/// Each parent's record of its direct facets.
pub(crate) const FACET_INDEX: &str = "_cf_FACET_INDEX";
/// A restore's roll-forward intent in the target root (see
/// `storage::restore_begin`).
pub(crate) const RESTORE_TABLE: &str = "_cf_RESTORE";

pub(crate) const FACET_INDEX_SQL: &str = "CREATE TABLE IF NOT EXISTS _cf_FACET_INDEX \
     (name TEXT PRIMARY KEY, engine TEXT NOT NULL) WITHOUT ROWID";

/// One database of an object: the root (an empty `path`) or the facet at
/// `path` below it, as a whole-file image.
#[derive(Clone, Debug)]
pub struct Component {
    pub path: Vec<String>,
    pub engine: Engine,
    pub image: Vec<u8>,
}

/// An object as its components, the root first and every parent before its
/// children.
pub type ObjectImage = Vec<Component>;

pub(crate) fn engine_name(engine: Engine) -> &'static str {
    match engine {
        Engine::Sqlite => "sqlite",
        Engine::Dolt => "dolt",
    }
}

pub(crate) fn parse_engine(name: &str) -> anyhow::Result<Engine> {
    match name {
        "sqlite" => Ok(Engine::Sqlite),
        "dolt" => Ok(Engine::Dolt),
        other => bail!("unknown storage engine {other:?} (expected \"sqlite\" or \"dolt\")"),
    }
}

pub(crate) fn other_engine(engine: Engine) -> Engine {
    match engine {
        Engine::Sqlite => Engine::Dolt,
        Engine::Dolt => Engine::Sqlite,
    }
}

/// The engine a file's first bytes record: a DoltLite chunk store or a stock
/// SQLite database. `None` for anything else, an empty file included.
pub(crate) fn engine_of(bytes: &[u8]) -> Option<Engine> {
    if celld_ltx::dolt::is_chunk_store(bytes) {
        Some(Engine::Dolt)
    } else if bytes.len() >= 16 && &bytes[..16] == b"SQLite format 3\0" {
        Some(Engine::Sqlite)
    } else {
        None
    }
}

// ---- reading images ---------------------------------------------------------

/// A scratch file beside `beside`, unique in this process: two forks of one
/// object read its components at once.
pub(crate) fn scratch_beside(beside: &std::path::Path, what: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    beside.with_file_name(format!(".object-{what}-{n}.db"))
}

/// Remove a scratch database and the sidecars SQLite or DoltLite leave
/// beside it.
pub(crate) fn remove_scratch(scratch: &std::path::Path) {
    let (Some(dir), Some(name)) = (scratch.parent(), scratch.file_name()) else {
        return;
    };
    let name = name.to_string_lossy().into_owned();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with(&name) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Copy the committed state `source` sees into a new rollback-journal file
/// at `scratch`, with SQLite's online backup. The source may be live: the
/// copy is one read transaction's snapshot, and a source in WAL mode keeps
/// accepting writes meanwhile.
fn backup_into_scratch(source: &Connection, scratch: &std::path::Path) -> anyhow::Result<()> {
    remove_scratch(scratch);
    let mut target = Connection::open(scratch)?;
    target.pragma_update(None, "journal_mode", "DELETE")?;
    rusqlite::backup::Backup::new(source, &mut target)?
        .run_to_completion(1024, std::time::Duration::ZERO, None)
        .context("copy the database")?;
    target.pragma_update(None, "journal_mode", "DELETE")?;
    Ok(())
}

/// A SQLite image of what `source` has committed (see
/// [`backup_into_scratch`]), as the bytes of a rollback-journal file.
pub(crate) fn sqlite_image_from_connection(
    source: &Connection,
    scratch: &std::path::Path,
) -> anyhow::Result<Vec<u8>> {
    let result = (|| {
        backup_into_scratch(source, scratch)?;
        Ok(std::fs::read(scratch)?)
    })();
    remove_scratch(scratch);
    result
}

/// A SQLite image of the database file at `path`, which another connection
/// (a running facet) may be writing. A WAL beside it is read too.
pub(crate) fn sqlite_image_from_file(
    path: &std::path::Path,
    scratch: &std::path::Path,
) -> anyhow::Result<Vec<u8>> {
    let source = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open {}", path.display()))?;
    source.busy_timeout(std::time::Duration::from_secs(5))?;
    let image = sqlite_image_from_connection(&source, scratch)?;
    ensure!(
        engine_of(&image) == Some(Engine::Sqlite),
        "{} is not a SQLite database",
        path.display()
    );
    Ok(image)
}

/// The committed prefix of a Dolt chunk file: every byte up to the end of
/// its newest sealed root.
pub(crate) fn dolt_committed(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let cut = celld_ltx::dolt::scan_file(bytes)
        .ok_or_else(|| anyhow!("the Dolt store holds no committed state"))?;
    Ok(bytes[..cut.end as usize].to_vec())
}

/// A component's image read from its file: the committed prefix of a chunk
/// file, or a backup of a SQLite database. `None` when the file is absent or
/// empty (a facet that never wrote).
pub(crate) fn image_from_file(
    path: &std::path::Path,
    scratch: &std::path::Path,
) -> anyhow::Result<Option<(Engine, Vec<u8>)>> {
    let head = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    match engine_of(&head) {
        Some(Engine::Dolt) => match celld_ltx::dolt::scan_file(&head) {
            Some(cut) => Ok(Some((Engine::Dolt, head[..cut.end as usize].to_vec()))),
            None => Ok(None),
        },
        Some(Engine::Sqlite) => {
            drop(head);
            let image = sqlite_image_from_file(path, scratch)?;
            Ok(Some((Engine::Sqlite, image)))
        }
        // An empty file: a SQLite stream whose database was created and
        // never written.
        None if head.iter().all(|byte| *byte == 0) => Ok(None),
        None => bail!(
            "{} is neither a SQLite database nor a Dolt store",
            path.display()
        ),
    }
}

/// Write a SQLite image into the database at `dest`, a facet stream's file
/// that replication has just activated (and may have created empty), with
/// SQLite's backup: one write transaction in `dest`'s WAL, which the stream
/// captures like any other.
pub(crate) fn install_sqlite(
    dest: &std::path::Path,
    image: &[u8],
    scratch: &std::path::Path,
) -> anyhow::Result<()> {
    remove_scratch(scratch);
    std::fs::write(scratch, image)?;
    let result = (|| -> anyhow::Result<()> {
        let source = Connection::open(scratch)?;
        source.pragma_update(None, "journal_mode", "DELETE")?;
        let mut target = Connection::open(dest)?;
        target.busy_timeout(std::time::Duration::from_secs(5))?;
        target.pragma_update(None, "journal_mode", "WAL")?;
        // The capture checkpoints; a checkpoint from here could move frames
        // into the file before the capture reads them.
        target.pragma_update(None, "wal_autocheckpoint", 0)?;
        // A WAL database takes a backup only from a source of its own page
        // size.
        let wanted: i64 = target.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        let ours: i64 = source.query_row("PRAGMA page_size", [], |row| row.get(0))?;
        if wanted != ours {
            source.execute_batch(&format!("PRAGMA page_size={wanted}; VACUUM;"))?;
        }
        rusqlite::backup::Backup::new(&source, &mut target)?
            .run_to_completion(1024, std::time::Duration::ZERO, None)
            .context("install the facet database")?;
        Ok(())
    })();
    remove_scratch(scratch);
    result
}

// ---- rewriting images -------------------------------------------------------

/// Open `image` as a database at `scratch`, let `edit` change it, and answer
/// the image it leaves together with what `edit` answered.
///
/// A Dolt image is opened on its active branch (celld's `_cf_*` tables live
/// there, unversioned) and cut at its newest sealed root afterwards. A SQLite
/// image is rewritten in rollback-journal mode, then marked as a WAL database
/// in its header, as every cell file is, so a restore of it opens without a
/// journal-mode change outside the replicated log.
pub(crate) fn rewrite<T>(
    engine: Engine,
    image: &[u8],
    scratch: &std::path::Path,
    edit: impl FnOnce(&Connection) -> anyhow::Result<T>,
) -> anyhow::Result<(Vec<u8>, T)> {
    remove_scratch(scratch);
    ensure!(
        engine_of(image) == Some(engine),
        "the image is not a {} database",
        engine_name(engine)
    );
    std::fs::write(scratch, image)?;
    let result = (|| -> anyhow::Result<(Vec<u8>, T)> {
        let value = {
            let connection = Connection::open(scratch)?;
            match engine {
                Engine::Dolt => {
                    ensure!(
                        crate::dolt_cell::is_prolly(&connection),
                        "the image did not open as a Dolt store"
                    );
                    crate::dolt_cell::attach_active_branch(&connection)?;
                }
                Engine::Sqlite => {
                    connection.pragma_update(None, "journal_mode", "DELETE")?;
                }
            }
            edit(&connection)?
        };
        let bytes = std::fs::read(scratch)?;
        let bytes = match engine {
            Engine::Dolt => dolt_committed(&bytes)?,
            Engine::Sqlite => mark_wal(bytes),
        };
        Ok((bytes, value))
    })();
    remove_scratch(scratch);
    result
}

/// Set the file-format bytes of a SQLite header to WAL (2, 2).
fn mark_wal(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() >= 20 {
        bytes[18] = 2;
        bytes[19] = 2;
    }
    bytes
}

pub(crate) fn table_exists(connection: &Connection, name: &str) -> anyhow::Result<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1",
            [name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn columns(connection: &Connection, table: &str) -> anyhow::Result<Vec<String>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    Ok(names.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The facets `connection`'s database records as its direct children.
pub(crate) fn read_index(connection: &Connection) -> anyhow::Result<Vec<(String, Engine)>> {
    if !table_exists(connection, FACET_INDEX)? {
        return Ok(Vec::new());
    }
    let mut statement =
        connection.prepare("SELECT name, engine FROM _cf_FACET_INDEX ORDER BY name")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (name, engine) = row?;
        out.push((name, parse_engine(&engine)?));
    }
    Ok(out)
}

/// Replace the facet index with `entries`.
pub(crate) fn write_index(
    connection: &Connection,
    entries: &[(String, Engine)],
) -> anyhow::Result<()> {
    connection.execute_batch(FACET_INDEX_SQL)?;
    connection.execute("DELETE FROM _cf_FACET_INDEX", [])?;
    for (name, engine) in entries {
        connection.execute(
            "INSERT INTO _cf_FACET_INDEX(name, engine) VALUES (?1, ?2)",
            rusqlite::params![name, engine_name(*engine)],
        )?;
    }
    Ok(())
}

/// Drop the state that belongs to the identity of the object an image came
/// from rather than to its data: its alarm and that alarm's wake record, its
/// id name, and a restore intent. `_cf_KV` is application state; it moves to
/// `scope`, the identity the image is for (its rows are keyed by scope).
pub(crate) fn strip_identity(connection: &Connection, scope: &str) -> anyhow::Result<()> {
    for table in ["_cf_ALARM", "_cf_WAKE", "_cf_METADATA"] {
        if table_exists(connection, table)? {
            connection.execute(&format!("DELETE FROM {table}"), [])?;
        }
    }
    if table_exists(connection, RESTORE_TABLE)? {
        connection.execute_batch("DROP TABLE _cf_RESTORE")?;
    }
    if table_exists(connection, "_cf_KV")?
        && columns(connection, "_cf_KV")?.iter().any(|c| c == "scope")
    {
        connection.execute("UPDATE _cf_KV SET scope=?1", [scope])?;
    }
    Ok(())
}

/// Bring a database written by workerd to celld's runtime tables: workerd's
/// `_cf_KV(key, value)` becomes celld's `_cf_KV(scope, k, v)` (values keep
/// their V8 serialization), and workerd's `_cf_METADATA(key, value)` (its
/// alarm) is dropped. A database without them is unchanged.
pub(crate) fn normalize_workerd(connection: &Connection, scope: &str) -> anyhow::Result<bool> {
    let mut changed = false;
    if table_exists(connection, "_cf_KV")? {
        let names = columns(connection, "_cf_KV")?;
        if !names.iter().any(|c| c == "scope") {
            ensure!(
                names.iter().any(|c| c == "key") && names.iter().any(|c| c == "value"),
                "_cf_KV has columns {names:?}, neither celld's nor workerd's"
            );
            connection.execute_batch(
                "ALTER TABLE _cf_KV RENAME TO _cf_KV_workerd;
                 CREATE TABLE _cf_KV (scope TEXT, k TEXT, v TEXT, PRIMARY KEY(scope,k));",
            )?;
            connection.execute(
                "INSERT INTO _cf_KV(scope, k, v) SELECT ?1, key, value FROM _cf_KV_workerd",
                [scope],
            )?;
            connection.execute_batch("DROP TABLE _cf_KV_workerd")?;
            changed = true;
        }
    }
    if table_exists(connection, "_cf_METADATA")?
        && !columns(connection, "_cf_METADATA")?
            .iter()
            .any(|c| c == "actor_name")
    {
        connection.execute_batch("DROP TABLE _cf_METADATA")?;
        changed = true;
    }
    Ok(changed)
}

// ---- snapshots and publication ---------------------------------------------

/// The TXID-1 LTX snapshot of a whole SQLite image: every page but the lock
/// page, with the rolling checksum a restore verifies.
pub(crate) fn sqlite_snapshot(image: &[u8]) -> anyhow::Result<Vec<u8>> {
    ensure!(
        engine_of(image) == Some(Engine::Sqlite),
        "not a SQLite image"
    );
    let raw = u16::from_be_bytes([image[16], image[17]]);
    let page_size: u32 = if raw == 1 { 65536 } else { raw as u32 };
    ensure!(
        page_size >= 512 && page_size.is_power_of_two(),
        "the SQLite image has an invalid page size {page_size}"
    );
    let page = page_size as usize;
    ensure!(
        !image.is_empty() && image.len().is_multiple_of(page),
        "the SQLite image is not a whole number of pages"
    );
    let commit = (image.len() / page) as u32;
    let lock = celld_ltx::ltx::lock_pgno(page_size);
    let mut pages = Vec::with_capacity(commit as usize);
    let mut rolling: celld_ltx::Checksum = celld_ltx::CHECKSUM_FLAG;
    for index in 0..commit {
        let pgno = index + 1;
        if pgno == lock {
            continue;
        }
        let mut data = image[index as usize * page..(index as usize + 1) * page].to_vec();
        if pgno == 1 {
            data = mark_wal(data);
        }
        rolling = celld_ltx::CHECKSUM_FLAG | (rolling ^ celld_ltx::ltx::checksum_page(pgno, &data));
        pages.push((pgno, data));
    }
    let header = celld_ltx::ltx::Header {
        version: celld_ltx::ltx::VERSION,
        flags: 0,
        page_size,
        commit,
        min_txid: celld_ltx::TXID(1),
        max_txid: celld_ltx::TXID(1),
        timestamp: chrono::Utc::now().timestamp_millis(),
        pre_apply_checksum: 0,
        wal_offset: 0,
        wal_size: 0,
        wal_salt1: 0,
        wal_salt2: 0,
        node_id: 0,
    };
    Ok(celld_ltx::ltx::encode_file(&header, &pages, rolling)?)
}

/// A component's first-epoch snapshot.
pub(crate) fn snapshot(component: &Component) -> anyhow::Result<Vec<u8>> {
    match component.engine {
        Engine::Sqlite => sqlite_snapshot(&component.image),
        Engine::Dolt => crate::dolt_cell::fork_snapshot(&component.image),
    }
}

/// The ownership record of a cell being published. A node refuses to
/// acquire a record that carries `publishing` (`ownership_store::read_owner`),
/// so nothing activates the cell before every component is uploaded.
fn publishing_record(nonce: &str) -> anyhow::Result<Vec<u8>> {
    Ok(serde_json::to_vec(
        &serde_json::json!({ "node": "", "epoch": 1, "publishing": nonce }),
    )?)
}

/// What [`publish_new`] does with a target whose record already exists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Existing {
    /// Refuse: a publication never overwrites an object.
    #[default]
    Refuse,
    /// Take over a publication that did not finish (its record still says
    /// `publishing`); refuse any other record.
    ReplaceUnfinished,
}

/// Publish `image` as the new cell `target`:
///
/// 1. create `cells/<target>/own.json` conditionally, as a released record
///    at epoch 1 that still carries `publishing` (so a node refuses to
///    acquire it); an existing record refuses the publication;
/// 2. upload each component's TXID-1 snapshot at epoch 1, under the root's
///    prefix and each facet stream's;
/// 3. swap the record, conditionally on step 1's version, to a plain
///    released record at epoch 1.
///
/// The object's first activation then acquires epoch 2 and restores every
/// stream from epoch 1, as for an operator-released cell
/// (`docs/guarantees.md`). A publication interrupted before step 3 leaves a
/// cell that no node will activate; only [`Existing::ReplaceUnfinished`]
/// takes it over.
pub(crate) async fn publish_new(
    bucket: &crate::bucket::Bucket,
    target: &str,
    image: &[Component],
    existing: Existing,
    staging_for: Option<&str>,
) -> anyhow::Result<u64> {
    ensure!(
        image.first().is_some_and(|root| root.path.is_empty()),
        "an object image starts with its root"
    );
    let snapshots = image
        .iter()
        .map(|component| Ok((component, snapshot(component)?)))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|error| anyhow!("random nonce: {error}"))?;
    let nonce: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
    let key = format!("cells/{target}/own.json");
    let token = match bucket
        .put_cas(&key, publishing_record(&nonce)?, None)
        .await?
    {
        Some(token) => token,
        None => {
            let current = bucket.get(&key).await?;
            let unfinished = current.as_ref().and_then(|(body, token)| {
                let record: serde_json::Value = serde_json::from_slice(body).ok()?;
                record.get("publishing")?;
                Some(token.clone())
            });
            match (existing, unfinished) {
                (Existing::ReplaceUnfinished, Some(token)) => {
                    let token = bucket
                        .put_cas(&key, publishing_record(&nonce)?, Some(&token))
                        .await?
                        .ok_or_else(|| {
                            anyhow!(
                                "{target} changed while its unfinished publication was taken over"
                            )
                        })?;
                    // Leftovers of the unfinished publication: its record
                    // stays, everything else goes.
                    let stale: Vec<String> = bucket
                        .list(&format!("cells/{target}"))
                        .await?
                        .into_iter()
                        .map(|meta| meta.location.to_string())
                        .filter(|location| location != &key)
                        .collect();
                    let gone = bucket.delete_many(&stale).await;
                    ensure!(
                        gone.len() == stale.len(),
                        "could not clear the unfinished publication of {target}"
                    );
                    token
                }
                (_, Some(_)) => bail!(
                    "{target} has an unfinished publication (a fork, restore or import that \
                     stopped before it completed); it is never activated, and an import with \
                     --replace-unfinished can take it over"
                ),
                (_, None) => bail!("the target object {target} already exists"),
            }
        }
    };
    let name = celld_ltx::ltx::format_filename(celld_ltx::TXID(1), celld_ltx::TXID(1));
    let mut bytes = 0u64;
    for (component, snapshot) in snapshots {
        let stream = crate::engine_api::facet_cell(target, &component.path);
        bytes += component.image.len() as u64;
        bucket
            .put(&format!("cells/{stream}/ltx/e1/0000/{name}"), snapshot)
            .await
            .with_context(|| format!("upload {stream}"))?;
    }
    let mut released = serde_json::json!({ "node": "", "epoch": 1 });
    if let Some(restored) = staging_for {
        released["staging_for"] = serde_json::Value::from(restored);
    }
    let released = serde_json::to_vec(&released)?;
    bucket
        .put_cas(&key, released, Some(&token))
        .await?
        .ok_or_else(|| anyhow!("the publication of {target} was taken over before it completed"))?;
    tracing::info!(
        target,
        components = image.len(),
        bytes,
        "published object image"
    );
    Ok(bytes)
}

/// Check that `staging` is a restore's staging copy for `target`: published
/// by an export for that restore (its record names `target` in
/// `staging_for`) and never activated (still released at epoch 1). Nothing
/// else may be read into a restore or discarded by one, so a restore can
/// never consume an ordinary fork, a backup for instance.
pub(crate) async fn check_staging(
    bucket: &crate::bucket::Bucket,
    staging: &str,
    target: &str,
) -> anyhow::Result<()> {
    let (body, _) = bucket
        .get(&format!("cells/{staging}/own.json"))
        .await?
        .ok_or_else(|| anyhow!("the restore's staging object {staging} does not exist"))?;
    let record: serde_json::Value =
        serde_json::from_slice(&body).context("decode the staging record")?;
    ensure!(
        record.get("staging_for").and_then(|v| v.as_str()) == Some(target)
            && record.get("epoch").and_then(|v| v.as_u64()) == Some(1)
            && record.get("node").and_then(|v| v.as_str()) == Some("")
            && record.get("publishing").is_none(),
        "{staging} is not a restore staging object for {target}"
    );
    Ok(())
}

/// Delete every object of a restore's staging copy, which was published
/// and never activated, its record included.
pub(crate) async fn discard(bucket: &crate::bucket::Bucket, cell: &str) -> anyhow::Result<()> {
    let keys: Vec<String> = bucket
        .list(&format!("cells/{cell}"))
        .await?
        .into_iter()
        .map(|meta| meta.location.to_string())
        .collect();
    let gone = bucket.delete_many(&keys).await;
    ensure!(
        gone.len() == keys.len(),
        "could not delete every object of {cell}"
    );
    Ok(())
}

/// Read back an object that [`publish_new`] wrote and nothing activated:
/// each stream restored from the bucket without a claim, the root first,
/// then each facet its parent's index names.
pub(crate) async fn read_published(
    replication: &crate::ltx_replication::Replication,
    cell: &str,
    scratch: &std::path::Path,
) -> anyhow::Result<ObjectImage> {
    let mut out: ObjectImage = Vec::new();
    let mut queue: std::collections::VecDeque<Vec<String>> = std::collections::VecDeque::new();
    queue.push_back(Vec::new());
    while let Some(path) = queue.pop_front() {
        let stream = crate::engine_api::facet_cell(cell, &path);
        let restored = replication
            .restore_snapshot(&stream)
            .await
            .with_context(|| format!("read {stream}"))?;
        let Some(restored) = restored else {
            ensure!(
                !path.is_empty(),
                "{cell} has no root database in the bucket"
            );
            // A facet its parent indexes but that never wrote.
            continue;
        };
        let file = restored.path().to_path_buf();
        let scratch = scratch.to_path_buf();
        let (engine, image, children) = crate::asyncrt::blocking(move || -> anyhow::Result<_> {
            let head = std::fs::read(&file)?;
            if celld_ltx::dolt::is_chunk_store(&head) {
                celld_ltx::dolt::unmap_restored_image(&file)?;
            }
            let bytes = std::fs::read(&file)?;
            let engine =
                engine_of(&bytes).ok_or_else(|| anyhow!("{} is not a database", file.display()))?;
            let image = match engine {
                Engine::Dolt => dolt_committed(&bytes)?,
                Engine::Sqlite => bytes,
            };
            let (image, children) = rewrite(engine, &image, &scratch, read_index)?;
            Ok((engine, image, children))
        })
        .await??;
        drop(restored);
        for (name, _) in children {
            let mut child = path.clone();
            child.push(name);
            queue.push_back(child);
        }
        out.push(Component {
            path,
            engine,
            image,
        });
    }
    Ok(out)
}

// ---- import -------------------------------------------------------------------

/// One facet file of an import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportFacet {
    pub name: String,
    pub file: std::path::PathBuf,
    pub engine: Engine,
}

/// Build the image of a new object from plain files: a root database and
/// its direct facets. Each file's engine must be the one declared for it
/// (the class's for the root, `facet.engine` for a facet). A SQLite file is
/// read through SQLite (a WAL beside it included); a Dolt file is cut at its
/// newest sealed root. A file workerd wrote has its runtime tables brought to
/// celld's ([`normalize_workerd`]).
pub(crate) fn import_image(
    scope: &str,
    root_engine: Engine,
    root_file: &std::path::Path,
    facets: &[ImportFacet],
    scratch_dir: &std::path::Path,
) -> anyhow::Result<ObjectImage> {
    let read = |file: &std::path::Path, engine: Engine, what: &str| -> anyhow::Result<Vec<u8>> {
        let head = std::fs::read(file).with_context(|| format!("read {}", file.display()))?;
        let found = engine_of(&head).ok_or_else(|| {
            anyhow!(
                "{} ({what}) is neither a SQLite database nor a Dolt store",
                file.display()
            )
        })?;
        ensure!(
            found == engine,
            "{} ({what}) is a {} database, but {what} must be a {} database",
            file.display(),
            engine_name(found),
            engine_name(engine)
        );
        match engine {
            Engine::Dolt => dolt_committed(&head),
            Engine::Sqlite => sqlite_image_from_file(file, &scratch_dir.join("import-read.db")),
        }
    };
    let mut seen = std::collections::BTreeSet::new();
    for facet in facets {
        ensure!(
            !facet.name.is_empty() && facet.name.len() <= 256,
            "a facet name is 1 to 256 bytes"
        );
        ensure!(
            seen.insert(facet.name.clone()),
            "facet {:?} is named twice",
            facet.name
        );
    }
    let scratch = scratch_dir.join("import-rewrite.db");
    let root = read(
        root_file,
        root_engine,
        &format!("the root, of a {} class", engine_name(root_engine)),
    )?;
    let index: Vec<(String, Engine)> = facets.iter().map(|f| (f.name.clone(), f.engine)).collect();
    let (root, ()) = rewrite(root_engine, &root, &scratch, |c| {
        normalize_workerd(c, scope)?;
        strip_identity(c, scope)?;
        write_index(c, &index)
    })?;
    let mut out = vec![Component {
        path: Vec::new(),
        engine: root_engine,
        image: root,
    }];
    for facet in facets {
        let what = format!("facet {:?}", facet.name);
        let image = read(&facet.file, facet.engine, &what)?;
        let (image, ()) = rewrite(facet.engine, &image, &scratch, |c| {
            normalize_workerd(c, scope)?;
            strip_identity(c, scope)?;
            // A facet file brings no facets of its own.
            if table_exists(c, FACET_INDEX)? {
                c.execute("DELETE FROM _cf_FACET_INDEX", [])?;
            }
            Ok(())
        })?;
        out.push(Component {
            path: vec![facet.name.clone()],
            engine: facet.engine,
            image,
        });
    }
    Ok(out)
}

/// The engine `script`'s current deployment declares for `class`.
pub(crate) async fn deployed_class_engine(
    bucket: &crate::bucket::Bucket,
    script: &str,
    class: &str,
) -> anyhow::Result<Engine> {
    let pointer = bucket
        .get(&format!("deploy/{script}/current.json"))
        .await?
        .ok_or_else(|| anyhow!("script {script:?} has no deployment in this bucket"))?;
    let pointer: serde_json::Value =
        serde_json::from_slice(&pointer.0).context("decode the deploy pointer")?;
    let prefix = pointer
        .get("prefix")
        .and_then(|prefix| prefix.as_str())
        .ok_or_else(|| anyhow!("the deploy pointer of {script:?} names no prefix"))?;
    let manifest = bucket
        .get(&format!("{prefix}/manifest.json"))
        .await?
        .ok_or_else(|| anyhow!("the deployment of {script:?} has no manifest"))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest.0).context("decode the manifest")?;
    let listed = |key: &str| {
        manifest
            .get(key)
            .and_then(|value| value.as_array())
            .is_some_and(|classes| classes.iter().any(|c| c.as_str() == Some(class)))
    };
    if listed("dolt_classes") {
        Ok(Engine::Dolt)
    } else if listed("sqlite_classes") {
        Ok(Engine::Sqlite)
    } else {
        bail!("class {class:?} is not a SQLite-backed Durable Object class of {script:?}'s deployment")
    }
}

/// `celld cell import`: create the object `scope` of `script`'s class from
/// plain files, through [`publish_new`]. Refuses an existing object, and a
/// file whose engine is not the class's (root) or the declared one (facet).
pub async fn import(
    bucket: &crate::bucket::Bucket,
    script: &str,
    scope: &str,
    root_file: &std::path::Path,
    facets: &[ImportFacet],
    replace_unfinished: bool,
) -> anyhow::Result<u64> {
    let (class, _) = scope
        .split_once(':')
        .ok_or_else(|| anyhow!("{scope} is not a Durable Object scope"))?;
    let engine = deployed_class_engine(bucket, script, class).await?;
    let scratch = tempfile::tempdir().context("create a scratch directory")?;
    let image = import_image(scope, engine, root_file, facets, scratch.path())?;
    publish_new(
        bucket,
        scope,
        &image,
        if replace_unfinished {
            Existing::ReplaceUnfinished
        } else {
            Existing::Refuse
        },
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock(path: &std::path::Path, sql: &str) {
        let c = Connection::open(path).unwrap();
        c.execute_batch(sql).unwrap();
    }

    fn open_image(engine: Engine, image: &[u8], dir: &std::path::Path, name: &str) -> Connection {
        let path = dir.join(name);
        std::fs::write(&path, image).unwrap();
        let c = Connection::open(&path).unwrap();
        if engine == Engine::Dolt {
            crate::dolt_cell::attach_active_branch(&c).unwrap();
        }
        c
    }

    #[test]
    fn sqlite_snapshot_round_trips_and_marks_wal() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("a.db");
        stock(&db, "CREATE TABLE t(a); INSERT INTO t VALUES (1), (2);");
        let image = sqlite_image_from_file(&db, &dir.path().join("s.db")).unwrap();
        let snapshot = sqlite_snapshot(&image).unwrap();
        let restored = celld_ltx::ltx::decode_database_image(&snapshot).unwrap();
        assert_eq!(restored[18], 2);
        assert_eq!(restored[19], 2);
        let c = open_image(Engine::Sqlite, &restored, dir.path(), "r.db");
        let n: i64 = c
            .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn a_live_wal_database_is_read_through_its_wal() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("live.db");
        let writer = Connection::open(&db).unwrap();
        writer.pragma_update(None, "journal_mode", "WAL").unwrap();
        writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        writer
            .execute_batch("CREATE TABLE t(a); INSERT INTO t VALUES (1);")
            .unwrap();
        // Uncheckpointed: the row is only in the WAL.
        let image = image_from_file(&db, &dir.path().join("s.db"))
            .unwrap()
            .unwrap();
        assert_eq!(image.0, Engine::Sqlite);
        let c = open_image(Engine::Sqlite, &image.1, dir.path(), "r.db");
        let n: i64 = c
            .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        drop(writer);
    }

    #[test]
    fn identity_is_stripped_and_kv_moves_to_the_new_scope() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("a.db");
        let c = Connection::open(&db).unwrap();
        crate::storage::schema(&c).unwrap();
        c.execute("INSERT INTO _cf_KV(scope,k,v) VALUES ('A:1','k','v')", [])
            .unwrap();
        c.execute("INSERT INTO _cf_ALARM(scope,at_ms) VALUES ('A:1',5)", [])
            .unwrap();
        c.execute(
            "INSERT INTO _cf_METADATA(scope,actor_name) VALUES ('A:1','n')",
            [],
        )
        .unwrap();
        write_index(&c, &[("x".into(), Engine::Dolt)]).unwrap();
        drop(c);
        let image = sqlite_image_from_file(&db, &dir.path().join("s.db")).unwrap();
        let (image, index) = rewrite(Engine::Sqlite, &image, &dir.path().join("w.db"), |c| {
            strip_identity(c, "A:2")?;
            read_index(c)
        })
        .unwrap();
        assert_eq!(index, vec![("x".to_string(), Engine::Dolt)]);
        let c = open_image(Engine::Sqlite, &image, dir.path(), "r.db");
        let scope: String = c
            .query_row("SELECT scope FROM _cf_KV", [], |r| r.get(0))
            .unwrap();
        assert_eq!(scope, "A:2");
        for table in ["_cf_ALARM", "_cf_METADATA"] {
            let n: i64 = c
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table}");
        }
    }

    #[test]
    fn workerd_runtime_tables_are_normalized() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("w.db");
        stock(
            &db,
            "CREATE TABLE _cf_KV (key TEXT PRIMARY KEY, value BLOB) WITHOUT ROWID;
             INSERT INTO _cf_KV VALUES ('k', x'ff0d');
             CREATE TABLE _cf_METADATA (key INTEGER PRIMARY KEY, value BLOB);
             INSERT INTO _cf_METADATA VALUES (1, x'00');
             CREATE TABLE notes(id INTEGER PRIMARY KEY, body TEXT);
             INSERT INTO notes(body) VALUES ('hello');",
        );
        let image = import_image("C:abc", Engine::Sqlite, &db, &[], dir.path()).unwrap();
        assert_eq!(image.len(), 1);
        let c = open_image(Engine::Sqlite, &image[0].image, dir.path(), "r.db");
        let (scope, v): (String, Vec<u8>) = c
            .query_row("SELECT scope, v FROM _cf_KV WHERE k='k'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(scope, "C:abc");
        assert_eq!(v, vec![0xff, 0x0d]);
        assert!(!table_exists(&c, "_cf_METADATA").unwrap());
        // celld's schema opens over it.
        crate::storage::schema(&c).unwrap();
        let body: String = c
            .query_row("SELECT body FROM notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(body, "hello");
    }

    #[test]
    fn import_checks_each_files_engine_and_indexes_the_facets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root.db");
        stock(&root, "CREATE TABLE t(a); INSERT INTO t VALUES (1);");
        let facet = dir.path().join("facet.db");
        stock(&facet, "CREATE TABLE f(a); INSERT INTO f VALUES (2);");
        // A Dolt store, as DoltLite writes one.
        let dolt = dir.path().join("dolt.db");
        {
            let c = crate::dolt_cell::open_connection(&dolt.to_string_lossy(), Engine::Dolt, None)
                .unwrap();
            c.execute_batch("CREATE TABLE d(a); INSERT INTO d VALUES (3);")
                .unwrap();
            let _: String = c
                .query_row("SELECT dolt_commit('-Am', 'one')", [], |r| r.get(0))
                .unwrap();
        }
        let facets = vec![
            ImportFacet {
                name: "do".into(),
                file: facet.clone(),
                engine: Engine::Sqlite,
            },
            ImportFacet {
                name: "ws".into(),
                file: dolt.clone(),
                engine: Engine::Dolt,
            },
        ];
        let image = import_image("C:abc", Engine::Sqlite, &root, &facets, dir.path()).unwrap();
        assert_eq!(image.len(), 3);
        let c = open_image(Engine::Sqlite, &image[0].image, dir.path(), "r.db");
        assert_eq!(
            read_index(&c).unwrap(),
            vec![
                ("do".to_string(), Engine::Sqlite),
                ("ws".to_string(), Engine::Dolt)
            ]
        );
        let d = open_image(Engine::Dolt, &image[2].image, dir.path(), "d.db");
        let n: i64 = d
            .query_row("SELECT count(*) FROM d", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        // Every component has a snapshot a restore accepts.
        for component in &image {
            let restored =
                celld_ltx::ltx::decode_database_image(&snapshot(component).unwrap()).unwrap();
            assert_eq!(engine_of(&restored), Some(component.engine));
        }
        // Wrong engines are refused: a Dolt file declared SQLite, a SQLite
        // root for a Dolt class.
        let wrong = vec![ImportFacet {
            name: "ws".into(),
            file: dolt.clone(),
            engine: Engine::Sqlite,
        }];
        let error = import_image("C:abc", Engine::Sqlite, &root, &wrong, dir.path()).unwrap_err();
        assert!(
            format!("{error:#}").contains("is a dolt database"),
            "{error:#}"
        );
        let error = import_image("C:abc", Engine::Dolt, &root, &[], dir.path()).unwrap_err();
        assert!(
            format!("{error:#}").contains("must be a dolt database"),
            "{error:#}"
        );
        let twice = vec![facets[0].clone(), facets[0].clone()];
        assert!(import_image("C:abc", Engine::Sqlite, &root, &twice, dir.path()).is_err());
    }

    /// A restore replaces a live root's database (SQLite through a backup
    /// into its connection, Dolt by replacing its file) and keeps what
    /// belongs to the target: its wake record moves on, its name stays, its
    /// alarm goes, its key-value rows are the source's under the target's
    /// scope, and the intent goes in the same step.
    fn restore_root(engine: Engine) {
        use crate::storage;
        let dir = tempfile::tempdir().unwrap();
        let cells = storage::Cells::default();
        let _installed = cells.install();
        let open = |scope: &str, file: &str| {
            let path = dir.path().join(file).to_string_lossy().into_owned();
            storage::open_at_epoch(scope, &path, 3, true, None, false, engine).unwrap();
        };
        open("C:t", "t.db");
        open("C:s", "s.db");
        for (scope, rows) in [("C:t", "(9)"), ("C:s", "(1),(2)")] {
            storage::sql_exec(scope, "CREATE TABLE t(a INTEGER)", &[]).unwrap();
            storage::sql_exec(scope, &format!("INSERT INTO t VALUES {rows}"), &[]).unwrap();
            storage::put_serialized(scope, "k", scope.as_bytes()).unwrap();
        }
        storage::set_alarm("C:t", 1_000).unwrap();
        storage::set_alarm("C:s", 2_000).unwrap();
        storage::set_actor_name("C:t", "target").unwrap();
        storage::register_facet("C:s", "ws", Engine::Dolt, true).unwrap();
        let sequence = |scope: &str| -> i64 {
            let snapshot = storage::alarm_snapshot(scope).unwrap();
            snapshot
                .source()
                .map(|source| source.id.sequence as i64)
                .unwrap_or(-1)
        };
        let before = sequence("C:t");
        let (exported, image) = storage::export_root("C:s").unwrap();
        assert_eq!(exported, engine);
        storage::restore_begin("C:t", "C:staging").unwrap();
        assert_eq!(
            storage::restore_pending("C:t").as_deref(),
            Some("C:staging")
        );
        let position = storage::write_position("C:t").unwrap().unwrap();
        storage::adopt_root("C:t", image, engine).unwrap();
        assert!(storage::write_position("C:t").unwrap().unwrap() > position);
        assert_eq!(storage::restore_pending("C:t"), None);
        let (_, rows, _) = storage::sql_exec("C:t", "SELECT a FROM t ORDER BY a", &[]).unwrap();
        assert_eq!(
            rows,
            vec![vec![serde_json::json!(1)], vec![serde_json::json!(2)]]
        );
        match storage::get_stored("C:t", "k").unwrap() {
            Some(storage::StoredValue::V8(bytes)) => assert_eq!(bytes, b"C:s"),
            _ => panic!("the source's key-value row did not move to the target"),
        }
        assert_eq!(storage::get_alarm("C:t"), None);
        assert_eq!(
            storage::get_actor_name("C:t").unwrap().as_deref(),
            Some("target")
        );
        assert!(
            sequence("C:t") > before,
            "the wake sequence never goes back"
        );
        // The source's facet index came with its image; it is untouched.
        assert_eq!(storage::get_alarm("C:s"), Some(2_000));
        storage::close("C:t");
        storage::close("C:s");
    }

    #[test]
    fn restore_replaces_a_sqlite_root() {
        restore_root(Engine::Sqlite);
    }

    #[test]
    fn restore_replaces_a_dolt_root() {
        restore_root(Engine::Dolt);
    }

    /// The local bucket runs its work on celld's host runtime, which a
    /// process installs once.
    fn host_runtime() -> &'static tokio::runtime::Runtime {
        static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
        RUNTIME.get_or_init(|| {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            crate::asyncrt::set_host_handle(runtime.handle().clone());
            runtime
        })
    }

    #[test]
    fn publish_claims_conditionally_and_never_overwrites() {
        host_runtime().block_on(publish_claims_conditionally());
    }

    async fn publish_claims_conditionally() {
        let dir = tempfile::tempdir().unwrap();
        let bucket = crate::bucket::Bucket::open_dev(&dir.path().join("objects.sqlite3")).unwrap();
        let root = dir.path().join("root.db");
        stock(&root, "CREATE TABLE t(a); INSERT INTO t VALUES (1);");
        let image = import_image("C:abc", Engine::Sqlite, &root, &[], dir.path()).unwrap();
        publish_new(&bucket, "C:abc", &image, Existing::Refuse, None)
            .await
            .unwrap();
        let (record, _) = bucket.get("cells/C:abc/own.json").await.unwrap().unwrap();
        let record: serde_json::Value = serde_json::from_slice(&record).unwrap();
        assert_eq!(record, serde_json::json!({ "node": "", "epoch": 1 }));
        let error = publish_new(&bucket, "C:abc", &image, Existing::Refuse, None)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("already exists"), "{error:#}");
        let error = publish_new(&bucket, "C:abc", &image, Existing::ReplaceUnfinished, None)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("already exists"), "{error:#}");
        // An unfinished publication is never activated, and only an explicit
        // takeover replaces it.
        bucket
            .put_cas(
                "cells/C:def/own.json",
                publishing_record("x").unwrap(),
                None,
            )
            .await
            .unwrap()
            .unwrap();
        bucket
            .put("cells/C:def/ltx/e1/0000/stale.ltx", vec![1u8])
            .await
            .unwrap();
        let error = publish_new(&bucket, "C:def", &image, Existing::Refuse, None)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("unfinished"), "{error:#}");
        publish_new(
            &bucket,
            "C:def",
            &image,
            Existing::ReplaceUnfinished,
            Some("C:abc"),
        )
        .await
        .unwrap();
        check_staging(&bucket, "C:def", "C:abc").await.unwrap();
        assert!(check_staging(&bucket, "C:def", "C:xyz").await.is_err());
        assert!(check_staging(&bucket, "C:abc", "C:abc").await.is_err());
        let keys: Vec<String> = bucket
            .list("cells/C:def")
            .await
            .unwrap()
            .into_iter()
            .map(|meta| meta.location.to_string())
            .collect();
        assert!(
            !keys.iter().any(|key| key.ends_with("stale.ltx")),
            "{keys:?}"
        );
        discard(&bucket, "C:def").await.unwrap();
        assert!(bucket.list("cells/C:def").await.unwrap().is_empty());
    }
}
