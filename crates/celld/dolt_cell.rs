// Copyright 2026 the celld fork authors. Apache-2.0 license.

//! Dolt cells: the storage side of `ctx.storage.dolt`.
//!
//! A Dolt cell is a cell whose file is a DoltLite prolly store (created with
//! `?doltlite_engine=prolly`). Application SQL runs on it unchanged through
//! `ctx.storage.sql`; versioning runs only through the operations here, which
//! the storage layer calls with the application authorizer lifted
//! (`storage::dolt_op`). The capture half is `celld_ltx::DoltStore`.
//!
//! # celld's runtime tables
//!
//! celld keeps its own state in `_cf_*` tables (`_cf_KV`, `_cf_ALARM`,
//! `_cf_METADATA`, `_cf_FACETS`, `_cf_WAKE`, and `_cf_DOLT` here). They live
//! in the prolly store, so one root record commits a handler's application
//! rows and celld's rows together and one capture replicates both. They are
//! never versioned:
//!
//! - `dolt_ignore` holds `_cf_%`, so `commit` (which stages everything)
//!   never stages them, `reset --hard` and `merge` leave them alone, and
//!   commit hashes depend only on application tables.
//! - A Dolt working set is per branch, and DoltLite's checkout is per
//!   connection, so the runtime tables travel with the active branch:
//!   [`checkout`] copies them into the target branch's working set in one
//!   transaction, then flips the pointer `_cf_DOLT.branch` on `main`, which
//!   is the one place an open reads first. A crash before the flip reopens
//!   on the old branch, whose copy is still current; a crash after it opens
//!   the new branch, whose copy is complete. Copies left on other branches
//!   are stale and never read. The cost of a checkout is the size of the
//!   runtime tables.
//!
//! Application SQL cannot reach any of this: the authorizer reserves `_cf_*`,
//! denies writes to `dolt_*` system tables, and denies every `dolt_*`
//! function except read-only ones (`storage.rs`).

use anyhow::{anyhow, bail, Context};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Value};

/// The branch every Dolt cell is created on, and where the active-branch
/// pointer lives.
pub const HOME_BRANCH: &str = "main";
const RUNTIME_PATTERN: &str = "_cf_%";

/// Open a cell's database for its engine. A Dolt cell's new file is created
/// in prolly mode; an existing file must already be of the cell's engine.
pub(crate) fn open_connection(
    path: &str,
    engine: celld_ltx::Engine,
    vfs: Option<&str>,
) -> anyhow::Result<Connection> {
    let magic = std::fs::File::open(path)
        .ok()
        .and_then(|mut file| {
            use std::io::Read;
            let mut head = [0u8; 16];
            file.read(&mut head).ok().map(|n| head[..n].to_vec())
        })
        .unwrap_or_default();
    let stock = magic.len() >= 16 && &magic[..16] == b"SQLite format 3\0";
    let prolly = celld_ltx::dolt::is_chunk_store(&magic);
    match engine {
        celld_ltx::Engine::Sqlite => {
            anyhow::ensure!(
                !prolly,
                "{path} is a Dolt cell's file but its class is a SQLite class; \
                 a class's storage engine is fixed when it is created"
            );
            Ok(match vfs {
                Some(vfs) => Connection::open_with_flags_and_vfs(path, OpenFlags::default(), vfs)?,
                None => Connection::open(path)?,
            })
        }
        celld_ltx::Engine::Dolt => {
            anyhow::ensure!(
                !stock,
                "{path} is a SQLite cell's file but its class is a Dolt class; \
                 a class's storage engine is fixed when it is created"
            );
            anyhow::ensure!(vfs.is_none(), "a Dolt cell never opens through a paged VFS");
            if prolly {
                return Ok(Connection::open(path)?);
            }
            // A new store's genesis ("Initialize data repository") commit is
            // pinned, so every Dolt cell starts from the same root hash and
            // two cells fed the same deterministic programs (with pinned
            // commit dates) reach the same commit hashes.
            Ok(Connection::open_with_flags(
                crate::dolt_exec::prolly_uri(
                    std::path::Path::new(path),
                    Some(crate::dolt_exec::Genesis { now_ms: 0, seed: 1 }),
                ),
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?)
        }
    }
}

/// Whether a connection's main database is a prolly store.
pub(crate) fn is_prolly(connection: &Connection) -> bool {
    connection
        .query_row("SELECT doltlite_engine()", [], |row| row.get::<_, String>(0))
        .map(|engine| engine.eq_ignore_ascii_case("prolly"))
        .unwrap_or(false)
}

fn active_branch(connection: &Connection) -> anyhow::Result<String> {
    Ok(connection.query_row("SELECT active_branch()", [], |row| row.get(0))?)
}

fn table_exists(connection: &Connection, name: &str) -> anyhow::Result<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1",
            [name],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// The branch the pointer on `main` names; `main` when there is none.
fn pointed_branch(connection: &Connection) -> anyhow::Result<String> {
    if !table_exists(connection, "_cf_DOLT")? {
        return Ok(HOME_BRANCH.to_string());
    }
    Ok(connection
        .query_row("SELECT v FROM _cf_DOLT WHERE k='branch'", [], |row| row.get::<_, String>(0))
        .optional()?
        .unwrap_or_else(|| HOME_BRANCH.to_string()))
}

/// Move a just-opened connection to the cell's active branch. Runs before
/// any runtime table is read: DoltLite opens every connection on `main`.
///
/// Idempotent: the pointer is read on `main`, where every connection opens,
/// so a connection already on another branch was attached before (a second
/// read there would find no pointer and move it back to `main`).
pub(crate) fn attach_active_branch(connection: &Connection) -> anyhow::Result<()> {
    if active_branch(connection)? != HOME_BRANCH {
        return Ok(());
    }
    let target = pointed_branch(connection)?;
    if target != active_branch(connection)? {
        let _: i64 = connection
            .query_row("SELECT dolt_checkout(?1)", [&target], |row| row.get(0))
            .with_context(|| format!("reopen the Dolt cell on its active branch {target:?}"))?;
    }
    Ok(())
}

/// Create the Dolt runtime bookkeeping on the active branch: the pointer
/// table and the ignore rule. Idempotent; runs after `storage::schema`.
pub(crate) fn ensure_runtime(connection: &Connection) -> anyhow::Result<()> {
    connection.execute(
        "CREATE TABLE IF NOT EXISTS _cf_DOLT (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    )?;
    ensure_ignore_rule(connection)
}

fn ensure_ignore_rule(connection: &Connection) -> anyhow::Result<()> {
    let present = connection
        .query_row(
            "SELECT 1 FROM dolt_ignore WHERE pattern=?1 AND ignored=1",
            [RUNTIME_PATTERN],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !present {
        connection.execute(
            "INSERT OR REPLACE INTO dolt_ignore(pattern, ignored) VALUES (?1, 1)",
            [RUNTIME_PATTERN],
        )?;
    }
    Ok(())
}

/// Recreate the TEMP wake triggers (see `storage::wake_record::trigger_sql`)
/// when the active branch has the wake table.
pub(crate) fn ensure_wake_triggers(connection: &Connection) -> anyhow::Result<()> {
    if table_exists(connection, "_cf_WAKE")? && table_exists(connection, "_cf_ALARM")? {
        connection.execute_batch(&crate::storage::wake_record_trigger_sql())?;
    }
    Ok(())
}

/// A branch name the API accepts: no leading `-` (DoltLite would read it as
/// a flag), and a conservative character set.
fn branch_name(value: &Value, what: &str) -> anyhow::Result<String> {
    let name = value
        .as_str()
        .ok_or_else(|| anyhow!("{what} must be a string"))?;
    anyhow::ensure!(
        !name.is_empty()
            && name.len() <= 200
            && !name.starts_with('-')
            && !name.starts_with('/')
            && !name.contains("..")
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/')),
        "{what} {name:?} is not a valid branch name"
    );
    Ok(name.to_string())
}

/// A revision: a branch, tag, commit hash, or `HEAD`-relative spec.
fn revision(value: &Value, what: &str) -> anyhow::Result<String> {
    let rev = value
        .as_str()
        .ok_or_else(|| anyhow!("{what} must be a string"))?;
    anyhow::ensure!(
        !rev.is_empty()
            && rev.len() <= 200
            && !rev.starts_with('-')
            && rev
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/' | b'~' | b'^')),
        "{what} {rev:?} is not a valid revision"
    );
    Ok(rev.to_string())
}

fn optional_string(args: &Value, key: &str) -> anyhow::Result<Option<String>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => bail!("{key} must be a string"),
    }
}

/// Run a dolt function whose argument list is built at run time.
fn call(connection: &Connection, function: &str, args: &[String]) -> anyhow::Result<Value> {
    let placeholders = (1..=args.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT {function}({placeholders})");
    let value: rusqlite::types::Value = connection
        .query_row(&sql, rusqlite::params_from_iter(args.iter()), |row| row.get(0))
        .map_err(|error| anyhow!("{function}: {}", sqlite_message(&error)))?;
    Ok(match value {
        rusqlite::types::Value::Text(text) => Value::String(text),
        rusqlite::types::Value::Integer(n) => json!(n),
        rusqlite::types::Value::Null => Value::Null,
        rusqlite::types::Value::Real(f) => json!(f),
        rusqlite::types::Value::Blob(_) => Value::Null,
    })
}

fn sqlite_message(error: &rusqlite::Error) -> String {
    match error {
        rusqlite::Error::SqliteFailure(_, Some(message)) => message.clone(),
        other => other.to_string(),
    }
}

fn rows(connection: &Connection, sql: &str, params: &[String]) -> anyhow::Result<Vec<Value>> {
    let mut statement = connection.prepare(sql)?;
    let names: Vec<String> = statement
        .column_names()
        .iter()
        .map(|name| name.to_string())
        .collect();
    let mut out = Vec::new();
    let mut cursor = statement.query(rusqlite::params_from_iter(params.iter()))?;
    while let Some(row) = cursor.next()? {
        let mut object = serde_json::Map::new();
        for (index, name) in names.iter().enumerate() {
            let value: rusqlite::types::Value = row.get(index)?;
            object.insert(
                name.clone(),
                match value {
                    rusqlite::types::Value::Null => Value::Null,
                    rusqlite::types::Value::Integer(n) => json!(n),
                    rusqlite::types::Value::Real(f) => json!(f),
                    rusqlite::types::Value::Text(text) => Value::String(text),
                    rusqlite::types::Value::Blob(bytes) => json!({ "__celld_bytes": bytes }),
                },
            );
        }
        out.push(Value::Object(object));
    }
    Ok(out)
}

/// Whether an operation can change the store (and so must be proved durable
/// before an output that follows it).
pub(crate) fn mutates(op: &str) -> bool {
    matches!(
        op,
        "commit"
            | "branchCreate"
            | "branchDelete"
            | "checkout"
            | "reset"
            | "merge"
            | "gc"
            | "execute"
    )
}

/// Whether an operation can change the schema the connection sees, so
/// cached statements must be dropped.
pub(crate) fn changes_schema(op: &str) -> bool {
    matches!(op, "checkout" | "reset" | "merge" | "gc" | "execute")
}

/// Whether an operation that runs inside the caller's open transaction
/// ([`run_in_transaction`]) can change the store.
pub(crate) fn mutates_in_transaction(op: &str) -> bool {
    matches!(op, "txCommit")
}

/// Whether an operation that runs inside the caller's open transaction can
/// change the schema the connection sees.
pub(crate) fn changes_schema_in_transaction(_op: &str) -> bool {
    false
}

/// Run one operation inside the caller's open storage transaction, with the
/// application authorizer lifted. Only the operations whose engine commands
/// are written for that (DoltLite's scoped commands) are here; the others
/// refuse a transaction (`storage::dolt_op`).
///
/// `txCommit` is `commit` inside the transaction: DoltLite's `dolt_commit`
/// commits the transaction's SQL writes and records the Dolt commit in one
/// root record, then reopens the transaction, so the caller's COMMIT that
/// follows has nothing left to write. The commit and the writes are one
/// durable step, and one replicated unit.
pub(crate) fn run_in_transaction(
    connection: &Connection,
    op: &str,
    args: &Value,
) -> anyhow::Result<Value> {
    match op {
        "txCommit" => commit(connection, args),
        other => bail!("dolt operation {other:?} cannot run inside a storage transaction"),
    }
}

/// Run one `ctx.storage.dolt` operation. The caller holds the cell's
/// connection outside any transaction, with the application authorizer
/// lifted.
pub(crate) fn run(connection: &Connection, op: &str, args: &Value) -> anyhow::Result<Value> {
    match op {
        "commit" => commit(connection, args),
        "branchCreate" => {
            let name = branch_name(&args["name"], "branch name")?;
            let mut call_args = vec![name];
            if let Some(from) = args.get("from").filter(|value| !value.is_null()) {
                call_args.push(revision(from, "from")?);
            }
            call(connection, "dolt_branch", &call_args)?;
            Ok(Value::Null)
        }
        "branchDelete" => {
            let name = branch_name(&args["name"], "branch name")?;
            anyhow::ensure!(name != HOME_BRANCH, "the {HOME_BRANCH} branch cannot be deleted");
            anyhow::ensure!(
                name != active_branch(connection)?,
                "cannot delete the checked-out branch {name:?}"
            );
            let flag = if args.get("force").and_then(Value::as_bool).unwrap_or(false) {
                "-D"
            } else {
                "-d"
            };
            call(connection, "dolt_branch", &[flag.to_string(), name])?;
            Ok(Value::Null)
        }
        "branchList" => Ok(Value::Array(rows(
            connection,
            "SELECT name, hash, latest_committer AS committer, \
             latest_commit_date AS date, latest_commit_message AS message \
             FROM dolt_branches ORDER BY name",
            &[],
        )?)),
        "currentBranch" => Ok(Value::String(active_branch(connection)?)),
        "checkout" => {
            let target = branch_name(&args["branch"], "branch")?;
            if args.get("create").and_then(Value::as_bool).unwrap_or(false) {
                call(connection, "dolt_branch", &[target.clone()])?;
            }
            checkout(connection, &target)?;
            Ok(Value::String(target))
        }
        "reset" => {
            let mode = if args.get("hard").and_then(Value::as_bool).unwrap_or(false) {
                "--hard"
            } else {
                "--soft"
            };
            let mut call_args = vec![mode.to_string()];
            if let Some(to) = args.get("to").filter(|value| !value.is_null()) {
                call_args.push(revision(to, "to")?);
            }
            call(connection, "dolt_reset", &call_args)?;
            Ok(Value::String(hash_of(connection, "HEAD")?))
        }
        "merge" => merge(connection, args),
        "log" => {
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(100)
                .min(10_000);
            Ok(Value::Array(rows(
                connection,
                &format!(
                    "SELECT commit_hash AS hash, committer, email, date, message \
                     FROM dolt_log LIMIT {limit}"
                ),
                &[],
            )?))
        }
        "diff" => diff(connection, args),
        "hashOf" => {
            let rev = revision(&args["ref"], "ref")?;
            Ok(Value::String(hash_of(connection, &rev)?))
        }
        "status" => Ok(Value::Array(rows(
            connection,
            "SELECT table_name AS \"table\", staged, status FROM dolt_status \
             WHERE table_name NOT LIKE '\\_cf\\_%' ESCAPE '\\'",
            &[],
        )?)),
        "gc" => {
            let summary = call(connection, "dolt_gc", &[])?;
            Ok(summary)
        }
        "execute" => execute(connection, args),
        "query" => query(connection, args),
        "engineDigest" => Ok(Value::String(crate::dolt_exec::engine_digest_hex())),
        other => bail!("unknown dolt operation {other:?}"),
    }
}

fn hash_of(connection: &Connection, rev: &str) -> anyhow::Result<String> {
    match call(connection, "dolt_hashof", &[rev.to_string()])? {
        Value::String(hash) => Ok(hash),
        other => bail!("dolt_hashof returned {other}"),
    }
}

fn commit(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    let message = optional_string(args, "message")?
        .filter(|message| !message.is_empty())
        .ok_or_else(|| anyhow!("commit needs a non-empty message"))?;
    // A branch created from a commit made before the rule existed lacks
    // it; restore it so the runtime tables are never staged.
    ensure_ignore_rule(connection)?;
    let mut call_args = vec!["-A".to_string(), "-m".to_string(), message];
    if let Some(author) = optional_string(args, "author")? {
        anyhow::ensure!(
            author.contains('<') && author.ends_with('>'),
            "author must look like \"Name <email>\""
        );
        call_args.push("--author".to_string());
        call_args.push(author);
    }
    if let Some(date) = args.get("date").filter(|value| !value.is_null()) {
        let date = match date {
            Value::String(date) => date.clone(),
            Value::Number(ms) => {
                let ms = ms
                    .as_i64()
                    .ok_or_else(|| anyhow!("date must be a string or epoch milliseconds"))?;
                let time = chrono::DateTime::from_timestamp_millis(ms)
                    .ok_or_else(|| anyhow!("date is out of range"))?;
                time.format("%Y-%m-%dT%H:%M:%S").to_string()
            }
            _ => bail!("date must be a string or epoch milliseconds"),
        };
        call_args.push("--date".to_string());
        call_args.push(date);
    }
    if args.get("allowEmpty").and_then(Value::as_bool).unwrap_or(false) {
        call_args.push("--allow-empty".to_string());
    }
    match call(connection, "dolt_commit", &call_args)? {
        Value::String(hash) => Ok(Value::String(hash)),
        other => bail!("dolt_commit returned {other}"),
    }
}

/// Merge `branch` into the active branch. Conflicts never commit: the
/// merge runs in a transaction, and on conflict the conflicting tables are
/// reported and the transaction rolls back, leaving the branch unchanged.
fn merge(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    let branch = revision(&args["branch"], "branch")?;
    let mut call_args = vec![branch];
    if let Some(message) = optional_string(args, "message")? {
        call_args.push("-m".to_string());
        call_args.push(message);
    }
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let merged = call(connection, "dolt_merge", &call_args);
    let conflicts = rows(
        connection,
        "SELECT \"table\", num_conflicts AS conflicts FROM dolt_conflicts",
        &[],
    )
    .unwrap_or_default();
    match merged {
        Ok(result) if conflicts.is_empty() => {
            connection.execute_batch("COMMIT")?;
            let hash = match result {
                Value::String(hash) if !hash.is_empty() => hash,
                _ => hash_of(connection, "HEAD")?,
            };
            Ok(json!({ "hash": hash, "conflicts": [] }))
        }
        Ok(_) => {
            connection.execute_batch("ROLLBACK")?;
            Ok(json!({ "hash": null, "conflicts": conflicts }))
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            if !conflicts.is_empty() {
                return Ok(json!({ "hash": null, "conflicts": conflicts }));
            }
            let message = error.to_string();
            if message.contains("conflict") {
                return Ok(json!({
                    "hash": null,
                    "conflicts": [{ "table": null, "conflicts": null, "message": message }],
                }));
            }
            Err(error)
        }
    }
}

fn diff(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    let from = revision(&args["from"], "from")?;
    let to = match args.get("to").filter(|value| !value.is_null()) {
        Some(to) => revision(to, "to")?,
        None => "WORKING".to_string(),
    };
    let table = optional_string(args, "table")?;
    let mut params = vec![from.clone(), to.clone()];
    let summary_sql = match &table {
        Some(table) => {
            params.push(table.clone());
            "SELECT from_table_name AS fromTable, to_table_name AS toTable, \
             diff_type AS diffType, data_change AS dataChange, \
             schema_change AS schemaChange FROM dolt_diff_summary(?1, ?2, ?3)"
        }
        None => {
            "SELECT from_table_name AS fromTable, to_table_name AS toTable, \
             diff_type AS diffType, data_change AS dataChange, \
             schema_change AS schemaChange FROM dolt_diff_summary(?1, ?2) \
             WHERE coalesce(to_table_name, from_table_name) NOT LIKE '\\_cf\\_%' ESCAPE '\\'"
        }
    };
    let summary = rows(connection, summary_sql, &params)?;
    let patch = rows(
        connection,
        "SELECT table_name AS \"table\", diff_type AS diffType, statement \
         FROM dolt_patch(?1, ?2) ORDER BY statement_order",
        &[from, to],
    )?
    .into_iter()
    .filter(|row| {
        let name = row.get("table").and_then(Value::as_str).unwrap_or("");
        !name.starts_with("_cf_")
            && table.as_deref().is_none_or(|table| table == name)
    })
    .collect::<Vec<_>>();
    Ok(json!({ "summary": summary, "patch": patch }))
}

/// One runtime table's definition and rows.
struct RuntimeTable {
    name: String,
    sql: String,
    indexes: Vec<String>,
    rows: Vec<Vec<rusqlite::types::Value>>,
}

fn dump_runtime(connection: &Connection) -> anyhow::Result<Vec<RuntimeTable>> {
    let mut statement = connection.prepare(
        "SELECT name, sql FROM sqlite_schema WHERE type='table' \
         AND name LIKE '\\_cf\\_%' ESCAPE '\\' AND sql IS NOT NULL ORDER BY name",
    )?;
    let tables = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let mut out = Vec::new();
    for (name, sql) in tables {
        let mut statement = connection.prepare(
            "SELECT sql FROM sqlite_schema WHERE type='index' AND tbl_name=?1 AND sql IS NOT NULL",
        )?;
        let indexes = statement
            .query_map([&name], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        let quoted = name.replace('"', "\"\"");
        let mut statement = connection.prepare(&format!("SELECT * FROM \"{quoted}\""))?;
        let columns = statement.column_count();
        let rows = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|index| row.get::<_, rusqlite::types::Value>(index))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        out.push(RuntimeTable {
            name,
            sql,
            indexes,
            rows,
        });
    }
    Ok(out)
}

fn load_runtime(connection: &Connection, tables: &[RuntimeTable]) -> anyhow::Result<()> {
    for table in tables {
        let quoted = table.name.replace('"', "\"\"");
        // Reuse a table the branch already has, so the branch's catalog
        // keeps its shape: DoltLite compares catalogs to decide whether a
        // working set is clean, and a dropped and recreated table is not.
        let existing: Option<String> = connection
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1",
                [&table.name],
                |row| row.get(0),
            )
            .optional()?;
        if existing.as_deref() == Some(table.sql.as_str()) {
            connection.execute_batch(&format!("DELETE FROM \"{quoted}\""))?;
        } else {
            connection.execute_batch(&format!("DROP TABLE IF EXISTS \"{quoted}\""))?;
            connection.execute_batch(&table.sql)?;
            for index in &table.indexes {
                connection.execute_batch(index)?;
            }
        }
        if let Some(first) = table.rows.first() {
            let placeholders = (1..=first.len())
                .map(|index| format!("?{index}"))
                .collect::<Vec<_>>()
                .join(",");
            let mut insert =
                connection.prepare(&format!("INSERT INTO \"{quoted}\" VALUES ({placeholders})"))?;
            for row in &table.rows {
                insert.execute(rusqlite::params_from_iter(row.iter()))?;
            }
        }
    }
    ensure_ignore_rule(connection)
}

fn set_pointer(connection: &Connection, branch: &str) -> anyhow::Result<()> {
    connection.execute(
        "CREATE TABLE IF NOT EXISTS _cf_DOLT (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    )?;
    connection.execute(
        "INSERT INTO _cf_DOLT(k, v) VALUES ('branch', ?1) \
         ON CONFLICT(k) DO UPDATE SET v=excluded.v",
        [branch],
    )?;
    Ok(())
}

fn switch(connection: &Connection, branch: &str) -> anyhow::Result<()> {
    let _ = call(connection, "dolt_checkout", &[branch.to_string()])?;
    Ok(())
}

/// Check out `target`, carrying celld's runtime tables along. See the module
/// documentation for the crash argument.
pub(crate) fn checkout(connection: &Connection, target: &str) -> anyhow::Result<()> {
    let source = active_branch(connection)?;
    if source == target {
        return Ok(());
    }
    let tables = dump_runtime(connection)?;
    switch(connection, target)?;
    let copied = (|| -> anyhow::Result<()> {
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> anyhow::Result<()> {
            load_runtime(connection, &tables)?;
            if target == HOME_BRANCH {
                // On `main` the copy and the pointer commit together.
                set_pointer(connection, HOME_BRANCH)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(connection.execute_batch("COMMIT")?),
            Err(error) => {
                let _ = connection.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    })();
    if let Err(error) = copied {
        let _ = switch(connection, &source);
        return Err(error.context(format!("carry runtime state to branch {target:?}")));
    }
    if target != HOME_BRANCH {
        // Flip the pointer on `main`: the commit point of the checkout.
        switch(connection, HOME_BRANCH)?;
        let flipped = set_pointer(connection, target);
        switch(connection, target)?;
        flipped?;
    }
    Ok(())
}

/// The image a fork of a Dolt cell starts from: the source's chunk file at
/// its committed cut, with the fork's active branch reset to `at` (when
/// given) and the source's identity-bound runtime state removed: alarms and
/// their wake records, and the id name. Everything else is copied, every
/// branch and all history included; `_cf_KV` is application state and stays.
///
/// `source_bytes` is the source file read in the source's own turn, outside
/// any transaction, so it holds only committed roots.
pub(crate) fn fork_image(
    source_bytes: &[u8],
    scratch: &std::path::Path,
    at: Option<&str>,
) -> anyhow::Result<Vec<u8>> {
    let cut = celld_ltx::dolt::scan_file(source_bytes)
        .ok_or_else(|| anyhow!("the source Dolt cell holds no committed state"))?;
    std::fs::write(scratch, &source_bytes[..cut.end as usize])?;
    let built = (|| -> anyhow::Result<Vec<u8>> {
        {
            let connection = Connection::open(scratch)?;
            anyhow::ensure!(is_prolly(&connection), "fork source is not a prolly store");
            attach_active_branch(&connection)?;
            if let Some(at) = at {
                reset_hard(&connection, at)?;
            }
            for table in ["_cf_ALARM", "_cf_WAKE", "_cf_METADATA"] {
                if table_exists(&connection, table)? {
                    connection.execute(&format!("DELETE FROM {table}"), [])?;
                }
            }
        }
        let bytes = std::fs::read(scratch)?;
        let cut = celld_ltx::dolt::scan_file(&bytes)
            .ok_or_else(|| anyhow!("the fork image holds no committed state"))?;
        Ok(bytes[..cut.end as usize].to_vec())
    })();
    // DoltLite may leave sidecars (a lock file) beside the scratch copy.
    if let (Some(dir), Some(name)) = (scratch.parent(), scratch.file_name()) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            let name = name.to_string_lossy().into_owned();
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with(&name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    built
}

/// Reset the active branch of a fork's image to the revision `at`.
pub(crate) fn reset_hard(connection: &Connection, at: &str) -> anyhow::Result<()> {
    let at = revision(&Value::String(at.to_string()), "at")?;
    call(connection, "dolt_reset", &["--hard".to_string(), at])?;
    Ok(())
}

/// Refuse a facet fork onto a facet that exists: one whose stream restored
/// state from the bucket, or whose file is already present locally.
pub(crate) fn ensure_fork_target(target: &std::path::Path, restored: bool) -> anyhow::Result<()> {
    let present = std::fs::metadata(target).is_ok_and(|metadata| metadata.len() > 0);
    anyhow::ensure!(
        !restored && !present,
        "the fork target facet already exists; delete it first"
    );
    Ok(())
}

/// The image a fork of the Dolt facet whose file is `source` starts from
/// (see [`fork_image`]). The scratch copy sits beside `target`.
pub(crate) fn fork_facet_image(
    source: &std::path::Path,
    target: &std::path::Path,
    at: Option<&str>,
) -> anyhow::Result<Vec<u8>> {
    let bytes = match std::fs::read(source) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!("the source facet has no storage")
        }
        Err(error) => return Err(error).context("read the source facet"),
    };
    anyhow::ensure!(
        celld_ltx::dolt::is_chunk_store(&bytes),
        "only a Dolt facet can be forked (FacetStartupOptions.celld.storage)"
    );
    let scratch = target.with_file_name(".facet-fork-scratch.db");
    fork_image(&bytes, &scratch, at)
}

/// Write a fork's image as the target facet's file, durably. The target's
/// stream is resident and saw no file, so its next capture takes the whole
/// image as a new generation.
pub(crate) fn install_fork_image(target: &std::path::Path, image: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    let parent = target.parent().context("facet file has no parent")?;
    std::fs::create_dir_all(parent)?;
    let temp = target.with_file_name(".facet-fork.tmp");
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(image)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, target)?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

/// The whole-image LTX snapshot that opens a fork's first epoch.
pub(crate) fn fork_snapshot(image: &[u8]) -> anyhow::Result<Vec<u8>> {
    let page = celld_ltx::dolt::DOLT_PAGE_SIZE as usize;
    // Numbered past the LTX lock page, as every Dolt capture is.
    let (commit, pages) = celld_ltx::dolt::image_pages(image)?;
    let mut rolling: celld_ltx::Checksum = celld_ltx::CHECKSUM_FLAG;
    for (pgno, data) in &pages {
        rolling = celld_ltx::CHECKSUM_FLAG | (rolling ^ celld_ltx::ltx::checksum_page(*pgno, data));
    }
    let header = celld_ltx::ltx::Header {
        version: celld_ltx::ltx::VERSION,
        flags: 0,
        page_size: page as u32,
        commit,
        min_txid: celld_ltx::TXID(1),
        max_txid: celld_ltx::TXID(1),
        timestamp: chrono::Utc::now().timestamp_millis(),
        pre_apply_checksum: 0,
        wal_offset: image.len() as i64,
        wal_size: image.len() as i64,
        wal_salt1: celld_ltx::dolt::DOLT_LTX_TAG,
        wal_salt2: 0,
        node_id: 0,
    };
    Ok(celld_ltx::ltx::encode_file(&header, &pages, rolling)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(dir: &std::path::Path) -> (String, Connection) {
        let path = dir.join("cell.db").to_string_lossy().into_owned();
        let connection = open_connection(&path, celld_ltx::Engine::Dolt, None).unwrap();
        attach_active_branch(&connection).unwrap();
        crate::storage::schema(&connection).unwrap();
        ensure_runtime(&connection).unwrap();
        (path, connection)
    }

    #[test]
    fn runtime_tables_follow_the_active_branch_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let (path, c) = cell(dir.path());
        assert!(is_prolly(&c));
        c.execute("INSERT INTO _cf_KV(scope,k,v) VALUES ('s','k','v1')", []).unwrap();
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT); INSERT INTO t(b) VALUES ('x');")
            .unwrap();
        let first = run(&c, "commit", &json!({"message": "first"})).unwrap();
        assert_eq!(first.as_str().unwrap().len(), 40);
        // Runtime tables are not part of the commit.
        let status = run(&c, "status", &json!({})).unwrap();
        assert_eq!(status, json!([]));
        run(&c, "checkout", &json!({"branch": "feature", "create": true})).unwrap();
        let v: String = c.query_row("SELECT v FROM _cf_KV", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "v1");
        c.execute("UPDATE _cf_KV SET v='v2'", []).unwrap();
        c.execute("INSERT INTO t(b) VALUES ('on-feature')", []).unwrap();
        run(&c, "commit", &json!({"message": "feature work", "author": "Ann <ann@example.com>"}))
            .unwrap();
        drop(c);
        // Reopen: DoltLite opens on main; the pointer brings us back.
        let c = open_connection(&path, celld_ltx::Engine::Dolt, None).unwrap();
        attach_active_branch(&c).unwrap();
        assert_eq!(run(&c, "currentBranch", &json!({})).unwrap(), json!("feature"));
        let v: String = c.query_row("SELECT v FROM _cf_KV", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "v2");
        // Back to main and merge.
        run(&c, "checkout", &json!({"branch": "main"})).unwrap();
        let v: String = c.query_row("SELECT v FROM _cf_KV", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "v2", "runtime state moved with the checkout");
        let n: i64 = c.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let merged = run(&c, "merge", &json!({"branch": "feature"})).unwrap();
        assert_eq!(merged["conflicts"], json!([]));
        let n: i64 = c.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        let log = run(&c, "log", &json!({"limit": 10})).unwrap();
        assert!(log.as_array().unwrap().len() >= 3);
        let head = run(&c, "hashOf", &json!({"ref": "HEAD"})).unwrap();
        assert_eq!(head, merged["hash"]);
        // Reset hard back to the first commit keeps runtime state.
        run(&c, "reset", &json!({"hard": true, "to": first})).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let v: String = c.query_row("SELECT v FROM _cf_KV", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "v2");
        let diff = run(&c, "diff", &json!({"from": first, "to": head})).unwrap();
        assert_eq!(diff["patch"].as_array().unwrap().len(), 1);
        run(&c, "branchDelete", &json!({"name": "feature", "force": true})).unwrap();
        let branches = run(&c, "branchList", &json!({})).unwrap();
        assert_eq!(branches.as_array().unwrap().len(), 1);
        assert!(run(&c, "branchDelete", &json!({"name": "main"})).is_err());
        assert!(run(&c, "checkout", &json!({"branch": "-D"})).is_err());
    }

    /// The number of sealed root records appended to the chunk file at
    /// `path` after the cut `from` (a commit appends chunks and one root).
    fn roots_after(path: &str, from: celld_ltx::dolt::Cut) -> usize {
        let bytes = std::fs::read(path).unwrap();
        let mut ends = std::collections::BTreeSet::new();
        for end in from.resume as usize..=bytes.len() {
            ends.insert(celld_ltx::dolt::scan_wal(&bytes[..end], 0, from.resume, from).end);
        }
        ends.len() - 1
    }

    fn cut_of(path: &str) -> celld_ltx::dolt::Cut {
        celld_ltx::dolt::scan_file(&std::fs::read(path).unwrap()).unwrap()
    }

    /// F2: the SQL writes of a transaction and the Dolt commit made inside it
    /// are one root record. Written the old way (a write, then a commit),
    /// they are two, and a crash between them leaves uncommitted rows.
    #[test]
    fn commit_in_a_transaction_is_one_durable_step() {
        let dir = tempfile::tempdir().unwrap();
        let (path, c) = cell(dir.path());
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT);").unwrap();
        run(&c, "commit", &json!({"message": "base"})).unwrap();

        let before = cut_of(&path);
        c.execute("INSERT INTO t(b) VALUES ('two steps')", []).unwrap();
        run(&c, "commit", &json!({"message": "two steps"})).unwrap();
        assert_eq!(roots_after(&path, before), 2);

        let before = cut_of(&path);
        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        c.execute("INSERT INTO t(b) VALUES ('one step')", []).unwrap();
        let hash = run_in_transaction(&c, "txCommit", &json!({"message": "one step"})).unwrap();
        c.execute_batch("COMMIT").unwrap();
        assert_eq!(roots_after(&path, before), 1);
        assert_eq!(hash, run(&c, "hashOf", &json!({"ref": "HEAD"})).unwrap());
        assert_eq!(run(&c, "status", &json!({})).unwrap(), json!([]));
        let diff = run(&c, "diff", &json!({"from": "HEAD~1", "to": "HEAD"})).unwrap();
        assert_eq!(diff["patch"].as_array().unwrap().len(), 1, "{diff}");

        // A commit that cannot be made (nothing to commit, a bad author)
        // fails with the transaction still open, and its rollback drops the
        // writes: nothing of it is left behind.
        let before = cut_of(&path);
        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        c.execute("INSERT INTO t(b) VALUES ('doomed')", []).unwrap();
        let error = run_in_transaction(
            &c,
            "txCommit",
            &json!({"message": "doomed", "author": "nobody"}),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("author must look like"), "{error:#}");
        assert!(!c.is_autocommit());
        c.execute_batch("ROLLBACK").unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM t WHERE b='doomed'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        assert_eq!(roots_after(&path, before), 0);

        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        let error = run_in_transaction(&c, "txCommit", &json!({"message": "empty"})).unwrap_err();
        assert!(format!("{error:#}").contains("nothing to commit"), "{error:#}");
        c.execute_batch("ROLLBACK").unwrap();
    }

    #[test]
    fn merge_conflicts_are_reported_and_not_committed() {
        let dir = tempfile::tempdir().unwrap();
        let (_path, c) = cell(dir.path());
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT); INSERT INTO t VALUES (1,'x');")
            .unwrap();
        run(&c, "commit", &json!({"message": "base"})).unwrap();
        run(&c, "branchCreate", &json!({"name": "other"})).unwrap();
        c.execute("UPDATE t SET b='main'", []).unwrap();
        run(&c, "commit", &json!({"message": "main side"})).unwrap();
        run(&c, "checkout", &json!({"branch": "other"})).unwrap();
        c.execute("UPDATE t SET b='other'", []).unwrap();
        run(&c, "commit", &json!({"message": "other side"})).unwrap();
        run(&c, "checkout", &json!({"branch": "main"})).unwrap();
        let before = run(&c, "hashOf", &json!({"ref": "HEAD"})).unwrap();
        let merged = run(&c, "merge", &json!({"branch": "other"})).unwrap();
        assert!(merged["hash"].is_null());
        assert!(!merged["conflicts"].as_array().unwrap().is_empty());
        assert_eq!(run(&c, "hashOf", &json!({"ref": "HEAD"})).unwrap(), before);
        let b: String = c.query_row("SELECT b FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(b, "main");
    }

    #[test]
    fn fork_image_resets_and_drops_identity_state() {
        let dir = tempfile::tempdir().unwrap();
        let (path, c) = cell(dir.path());
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY);").unwrap();
        c.execute("INSERT INTO t VALUES (1)", []).unwrap();
        let first = run(&c, "commit", &json!({"message": "one"})).unwrap();
        c.execute("INSERT INTO t VALUES (2)", []).unwrap();
        run(&c, "commit", &json!({"message": "two"})).unwrap();
        c.execute("INSERT INTO _cf_ALARM(scope, at_ms) VALUES ('s', 5)", []).unwrap();
        c.execute("INSERT INTO _cf_KV(scope,k,v) VALUES ('s','k','v')", []).unwrap();
        let source = std::fs::read(&path).unwrap();
        let image =
            fork_image(&source, &dir.path().join("scratch.db"), Some(first.as_str().unwrap())).unwrap();
        let snapshot = fork_snapshot(&image).unwrap();
        let restored = celld_ltx::ltx::decode_database_image(&snapshot).unwrap();
        let fork = dir.path().join("fork.db");
        std::fs::write(&fork, restored).unwrap();
        celld_ltx::DoltStore::open(&fork, celld_ltx::LtxHost::default()).unwrap();
        let f = Connection::open(&fork).unwrap();
        attach_active_branch(&f).unwrap();
        let n: i64 = f.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let alarms: i64 = f.query_row("SELECT count(*) FROM _cf_ALARM", [], |r| r.get(0)).unwrap();
        assert_eq!(alarms, 0);
        let kv: i64 = f.query_row("SELECT count(*) FROM _cf_KV", [], |r| r.get(0)).unwrap();
        assert_eq!(kv, 1);
        // The source is untouched.
        let n: i64 = c.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn engine_is_fixed_at_creation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stock.db").to_string_lossy().into_owned();
        let c = Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE t(a)").unwrap();
        drop(c);
        assert!(open_connection(&path, celld_ltx::Engine::Dolt, None).is_err());
        let (dolt, c) = cell(dir.path());
        drop(c);
        assert!(open_connection(&dolt, celld_ltx::Engine::Sqlite, None).is_err());
    }

    /// A Dolt facet opens in two steps (`storage::open_embedded`, then
    /// `finish_open`), each attaching to the active branch: the second must
    /// not move a connection already on a branch back to `main`.
    #[test]
    fn attach_is_idempotent_off_main() {
        let dir = tempfile::tempdir().unwrap();
        let (path, c) = cell(dir.path());
        run(&c, "checkout", &json!({"branch": "work", "create": true})).unwrap();
        drop(c);
        let c = open_connection(&path, celld_ltx::Engine::Dolt, None).unwrap();
        attach_active_branch(&c).unwrap();
        attach_active_branch(&c).unwrap();
        assert_eq!(active_branch(&c).unwrap(), "work");
    }

    /// `ctx.facets.fork`: the target must not exist, the source must be a
    /// Dolt facet, and the installed image is a complete store that a fresh
    /// capture store over the target picks up whole.
    #[test]
    fn facet_fork_installs_a_capturable_image() {
        let dir = tempfile::tempdir().unwrap();
        let (source, c) = cell(dir.path());
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY);").unwrap();
        c.execute("INSERT INTO t VALUES (1)", []).unwrap();
        let first = run(&c, "commit", &json!({"message": "one"})).unwrap();
        c.execute("INSERT INTO t VALUES (2)", []).unwrap();
        run(&c, "commit", &json!({"message": "two"})).unwrap();
        run(&c, "checkout", &json!({"branch": "side", "create": true})).unwrap();
        let target_dir = dir.path().join("target");
        std::fs::create_dir_all(&target_dir).unwrap();
        let target = target_dir.join("db.sqlite");
        // The target's stream is resident with no file before the install.
        let mut store =
            celld_ltx::DoltStore::open(&target, celld_ltx::LtxHost::default()).unwrap();
        use celld_ltx::CellStore as _;
        store.sync().unwrap();
        assert_eq!(store.pos().unwrap().txid, celld_ltx::TXID(0));

        ensure_fork_target(&target, false).unwrap();
        assert!(ensure_fork_target(&target, true).is_err(), "a restored target exists");
        let image = fork_facet_image(
            std::path::Path::new(&source),
            &target,
            Some(first.as_str().unwrap()),
        )
        .unwrap();
        install_fork_image(&target, &image).unwrap();
        assert!(ensure_fork_target(&target, false).is_err(), "a local target exists");
        // No scratch file is left beside the target.
        let leftovers: Vec<_> = std::fs::read_dir(&target_dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name != "db.sqlite")
            .collect();
        assert!(
            leftovers.iter().all(|name| !name.starts_with(".facet-fork")),
            "{leftovers:?}"
        );

        store.sync().unwrap();
        assert_eq!(store.pos().unwrap().txid, celld_ltx::TXID(1));
        let mut snapshot = Vec::new();
        store.snapshot_to_writer(&mut snapshot).unwrap();
        let restored = dir.path().join("restored.db");
        std::fs::write(&restored, celld_ltx::ltx::decode_database_image(&snapshot).unwrap())
            .unwrap();
        celld_ltx::DoltStore::open(&restored, celld_ltx::LtxHost::default()).unwrap();
        let f = Connection::open(&restored).unwrap();
        attach_active_branch(&f).unwrap();
        // The fork starts on the source's checked-out branch, reset to `at`.
        assert_eq!(active_branch(&f).unwrap(), "side");
        let n: i64 = f.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        let branches: i64 = f
            .query_row("SELECT count(*) FROM dolt_branches", [], |r| r.get(0))
            .unwrap();
        assert_eq!(branches, 2);

        // A SQLite facet's file cannot be forked as a Dolt facet.
        let stock = dir.path().join("stock.db");
        Connection::open(&stock).unwrap().execute_batch("CREATE TABLE t(a)").unwrap();
        let error = fork_facet_image(&stock, &target_dir.join("other.db"), None).unwrap_err();
        assert!(format!("{error:#}").contains("only a Dolt facet"), "{error:#}");
        // A missing source has no storage.
        let error =
            fork_facet_image(&dir.path().join("absent.db"), &target_dir.join("x.db"), None)
                .unwrap_err();
        assert!(format!("{error:#}").contains("no storage"), "{error:#}");
    }
}

/// `ctx.storage.dolt.execute(program, context)`: one deterministic program
/// (Chronolog Spec 10) as its own transaction, through
/// [`crate::dolt_exec::execute`]. The arguments are the harness's tagged
/// encoding, which keeps INTEGER, REAL and BLOB exact across JSON:
///
/// ```json
/// { "program": [{ "sql": "...", "bindings": [{ "index": 1, "value": { "t": "int", "v": "9007199254740993" } }] }],
///   "context": { "nowMs": "1700000000000", "seed": "1", "actor": "<base64url>" | null,
///                "schemaAdmin": false, "limits": { "maxVmSteps": 1000000, ... } },
///   "log": null | { "sql": "INSERT ...", "bindings": [...] } }
/// ```
///
/// With `log`, the program runs through
/// [`crate::dolt_exec::execute_in_transaction`] inside a transaction this
/// function opens, and the log statement runs after it in the same
/// transaction, so the program's effects and its log row commit together or
/// not at all. The log statement runs as application SQL (the cell's
/// authorizer, no pinned clock) and may name the parameters `:outcome`
/// (`'accepted'`/`'rejected'`), `:rejection_code` and
/// `:failing_statement_index`, which are bound from the outcome.
///
/// The executor installs its own authorizer and progress handler for the
/// program and clears them; `storage::dolt_op` reinstalls the cell's
/// application authorizer afterwards.
fn execute(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    use crate::dolt_exec::{
        execute as run_program, execute_in_transaction, ExecContext, Limits, Outcome, Profile, Program,
    };
    use base64::Engine as _;

    let steps = args["program"]
        .as_array()
        .ok_or_else(|| anyhow!("program must be an array of statements"))?
        .iter()
        .map(statement)
        .collect::<anyhow::Result<Vec<_>>>()?;

    let context = &args["context"];
    let mut ctx = ExecContext::new(
        integer(&context["nowMs"], "context.nowMs")?,
        u64::try_from(integer(&context["seed"], "context.seed")?)
            .map_err(|_| anyhow!("context.seed must not be negative"))?,
    );
    if let Some(actor) = context["actor"].as_str() {
        ctx.actor = Some(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(actor)
                .context("context.actor must be base64url")?,
        );
    }
    ctx.profile = Profile::Consensus { schema_admin: context["schemaAdmin"].as_bool().unwrap_or(false) };
    ctx.limits = limits(&context["limits"], Limits::default())?;
    let program = Program::new(steps);

    let log = match args.get("log").filter(|value| !value.is_null()) {
        None => None,
        Some(log) => Some(statement(log).context("log")?),
    };
    let Some(log) = log else {
        let outcome = run_program(connection, &program, &ctx).map_err(|error| anyhow!("{error}"))?;
        return Ok(outcome_json(&outcome));
    };

    // execute_in_transaction needs foreign_keys ON before the transaction
    // begins (it cannot change inside one).
    let foreign_keys: i64 = connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    if foreign_keys != 1 {
        connection.execute_batch("PRAGMA foreign_keys = ON")?;
    }
    let result = (|| -> anyhow::Result<Outcome> {
        connection.execute_batch("BEGIN IMMEDIATE")?;
        let outcome = match execute_in_transaction(connection, &program, &ctx) {
            Ok(outcome) => outcome,
            // Not a deterministic outcome: nothing of this program, and no
            // log row, may persist.
            Err(error) => bail!("{error}"),
        };
        // A write statement that exhausted the step budget makes SQLite roll
        // the whole transaction back; the state is the one before BEGIN, so a
        // fresh transaction for the log row is equivalent.
        if connection.is_autocommit() {
            connection.execute_batch("BEGIN IMMEDIATE")?;
        }
        crate::storage::with_user_sql_authorizer(connection, || run_log_statement(connection, &log, &outcome))?;
        connection.execute_batch("COMMIT")?;
        Ok(outcome)
    })();
    if result.is_err() && !connection.is_autocommit() {
        let _ = connection.execute_batch("ROLLBACK");
    }
    if foreign_keys != 1 && connection.is_autocommit() {
        let _ = connection.execute_batch("PRAGMA foreign_keys = OFF");
    }
    Ok(outcome_json(&result?))
}

fn outcome_json(outcome: &crate::dolt_exec::Outcome) -> Value {
    match outcome {
        crate::dolt_exec::Outcome::Accepted => json!({ "outcome": "accepted" }),
        crate::dolt_exec::Outcome::Rejected { code, statement_index } => json!({
            "outcome": "rejected",
            "code": code,
            "statementIndex": statement_index,
        }),
    }
}

/// The log statement of `execute`: the caller's bindings plus the outcome
/// parameters it names. Stepped to completion; rows are discarded.
fn run_log_statement(
    connection: &Connection,
    log: &crate::dolt_exec::Statement,
    outcome: &crate::dolt_exec::Outcome,
) -> anyhow::Result<()> {
    use crate::dolt_exec::{Outcome, Parameter, SqlValue};
    use rusqlite::types::Value as Sql;

    let mut prepared = connection.prepare(&log.sql).context("prepare the log statement")?;
    let to_sql = |value: &SqlValue| match value {
        SqlValue::Null => Sql::Null,
        SqlValue::Integer(v) => Sql::Integer(*v),
        SqlValue::Real(v) => Sql::Real(*v),
        SqlValue::Text(v) => Sql::Text(v.clone()),
        SqlValue::Blob(v) => Sql::Blob(v.clone()),
    };
    let index_of = |prepared: &rusqlite::Statement<'_>, name: &str| -> anyhow::Result<Option<usize>> {
        if name.starts_with([':', '@', '$', '?']) {
            return Ok(prepared.parameter_index(name)?);
        }
        for prefix in [':', '@', '$'] {
            if let Some(index) = prepared.parameter_index(&format!("{prefix}{name}"))? {
                return Ok(Some(index));
            }
        }
        Ok(None)
    };
    let count = prepared.parameter_count();
    let mut bound = vec![false; count];
    let mut bind = |prepared: &mut rusqlite::Statement<'_>, index: usize, value: Sql| -> anyhow::Result<()> {
        anyhow::ensure!(index >= 1 && index <= count && !bound[index - 1], "log binding {index} is invalid or repeated");
        bound[index - 1] = true;
        prepared.raw_bind_parameter(index, value)?;
        Ok(())
    };
    for binding in &log.bindings {
        let index = match &binding.parameter {
            Parameter::Index(index) => *index,
            Parameter::Name(name) => {
                index_of(&prepared, name)?.ok_or_else(|| anyhow!("the log statement has no parameter {name:?}"))?
            }
        };
        bind(&mut prepared, index, to_sql(&binding.value))?;
    }
    let (kind, code, statement_index) = match outcome {
        Outcome::Accepted => ("accepted", Sql::Null, Sql::Null),
        Outcome::Rejected { code, statement_index } => (
            "rejected",
            Sql::Text(code.clone()),
            statement_index.map_or(Sql::Null, |index| Sql::Integer(index as i64)),
        ),
    };
    for (name, value) in [
        ("outcome", Sql::Text(kind.into())),
        ("rejection_code", code),
        ("failing_statement_index", statement_index),
    ] {
        if let Some(index) = index_of(&prepared, name)? {
            bind(&mut prepared, index, value)?;
        }
    }
    anyhow::ensure!(bound.iter().all(|b| *b), "every parameter of the log statement must be bound");
    let mut rows = prepared.raw_query();
    while rows.next()?.is_some() {}
    Ok(())
}

/// `ctx.storage.dolt.query(sql, bindings, { limits, maxRows })`: one
/// read-only statement under the local-read authorizer, with exact values
/// (`{ t, v }` tagged like `execute`'s bindings). A refusal answers
/// `{ "error": "<code>" }` rather than throwing, so the caller sees the code.
fn query(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    use crate::dolt_exec::{local_query, Limits, SqlValue};
    use base64::Engine as _;

    let statement = statement(args)?;
    let limits = limits(&args["limits"], Limits::default())?;
    let max_rows = args["maxRows"].as_u64().unwrap_or(10_000) as usize;
    match local_query(connection, &statement, &limits, max_rows) {
        Ok(result) => {
            let rows = result
                .rows
                .iter()
                .map(|row| {
                    Value::Array(
                        row.iter()
                            .map(|value| match value {
                                SqlValue::Null => json!({ "t": "null" }),
                                SqlValue::Integer(v) => json!({ "t": "int", "v": v.to_string() }),
                                // A non-finite REAL (9e999) has no JSON form.
                                SqlValue::Real(v) if v.is_finite() => json!({ "t": "real", "v": v }),
                                SqlValue::Real(v) => json!({ "t": "real", "v": v.to_string() }),
                                SqlValue::Text(v) => json!({ "t": "text", "v": v }),
                                SqlValue::Blob(v) => json!({
                                    "t": "blob",
                                    "v": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v),
                                }),
                            })
                            .collect(),
                    )
                })
                .collect::<Vec<_>>();
            Ok(json!({ "columns": result.columns, "rows": rows }))
        }
        Err(code) if code.starts_with("operational:") => Err(anyhow!("{code}")),
        Err(code) => Ok(json!({ "error": code })),
    }
}

fn integer(value: &Value, what: &str) -> anyhow::Result<i64> {
    match value {
        Value::String(text) => text
            .parse::<i64>()
            .map_err(|_| anyhow!("{what} must be a 64-bit integer")),
        Value::Number(number) => number
            .as_i64()
            .ok_or_else(|| anyhow!("{what} must be a 64-bit integer")),
        _ => bail!("{what} must be an integer"),
    }
}

fn sql_value(value: &Value) -> anyhow::Result<crate::dolt_exec::SqlValue> {
    use crate::dolt_exec::SqlValue;
    use base64::Engine as _;
    let tag = value["t"].as_str().ok_or_else(|| anyhow!("binding value has no type tag"))?;
    Ok(match tag {
        "null" => SqlValue::Null,
        "int" => SqlValue::Integer(integer(&value["v"], "an INTEGER binding")?),
        "real" => {
            let real = value["v"]
                .as_f64()
                .ok_or_else(|| anyhow!("a REAL binding must be a finite number"))?;
            anyhow::ensure!(real.is_finite(), "a REAL binding must be finite");
            SqlValue::Real(real)
        }
        "text" => SqlValue::Text(
            value["v"].as_str().ok_or_else(|| anyhow!("a TEXT binding must be a string"))?.to_string(),
        ),
        "blob" => SqlValue::Blob(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(value["v"].as_str().unwrap_or_default())
                .context("a BLOB binding must be base64url")?,
        ),
        other => bail!("unknown binding type {other:?}"),
    })
}

/// `{ sql, bindings: [{ index | name, value }] }` in the harness's encoding.
fn statement(step: &Value) -> anyhow::Result<crate::dolt_exec::Statement> {
    use crate::dolt_exec::{Parameter, Statement};
    let sql = step["sql"].as_str().ok_or_else(|| anyhow!("a statement needs `sql`"))?;
    let mut statement = Statement::new(sql);
    for binding in step["bindings"].as_array().into_iter().flatten() {
        let parameter = match (&binding["index"], &binding["name"]) {
            (Value::Number(index), Value::Null) => Parameter::Index(
                index
                    .as_u64()
                    .filter(|index| *index >= 1)
                    .ok_or_else(|| anyhow!("a binding index is 1-based"))? as usize,
            ),
            (Value::Null, Value::String(name)) => Parameter::Name(name.clone()),
            _ => bail!("a binding names exactly one of `index` or `name`"),
        };
        statement = statement.bind(parameter, sql_value(&binding["value"])?);
    }
    Ok(statement)
}

fn limits(limits: &Value, mut chosen: crate::dolt_exec::Limits) -> anyhow::Result<crate::dolt_exec::Limits> {
    if let Some(value) = limits["maxSqlBytes"].as_u64() { chosen.max_sql_bytes = value as usize; }
    if let Some(value) = limits["maxVmSteps"].as_u64() { chosen.max_vm_steps = value; }
    if let Some(value) = limits["progressGranularity"].as_u64() {
        chosen.progress_granularity = u32::try_from(value).context("limits.progressGranularity")?;
    }
    if let Some(value) = limits["maxValueBytes"].as_u64() {
        chosen.max_value_bytes = u32::try_from(value).context("limits.maxValueBytes")?;
    }
    if let Some(value) = limits["maxSteps"].as_u64() { chosen.max_steps = value as usize; }
    Ok(chosen)
}
