// Copyright 2026 the celld fork authors. Apache-2.0 license.

//! Dolt cells: the storage side of `ctx.storage.dolt`.
//!
//! A Dolt cell is a cell whose file is a DoltLite prolly store (created with
//! `?doltlite_engine=prolly`). Application SQL runs on it unchanged through
//! `ctx.storage.sql`; versioning runs only through the operations here, which
//! the storage layer calls with the application authorizer lifted
//! (`storage::dolt_op`). The capture half is `celld_ltx::DoltStore`.
//!
//! # celld's runtime tables and a class's local tables
//!
//! celld keeps its own state in `_cf_*` tables (`_cf_KV`, `_cf_ALARM`,
//! `_cf_METADATA`, `_cf_FACETS`, `_cf_WAKE`, and `_cf_DOLT` here). They live
//! in the prolly store, so one root record commits a handler's application
//! rows and celld's rows together and one capture replicates both. A class
//! can declare **local tables** of its own (`celld.durable_objects.<Class>.
//! doltLocal`, or `FacetStartupOptions.celld.doltLocal`): name patterns such
//! as `outbox_%` for tables that belong to this object's identity and not to
//! its history (outboxes, receipts, caches). Both kinds are *local tables* and
//! are never versioned:
//!
//! - `dolt_ignore` holds `_cf_%` and each declared pattern, so `commit` (which
//!   stages everything) never stages them, `reset --hard` and `merge` leave
//!   them alone, and commit hashes depend only on application tables. The
//!   rules are rows of a versioned table, so [`ensure_runtime`] restores them
//!   at every open and `commit` and `checkout` restore them on the branch they
//!   work on (a branch made from a commit older than a rule lacks it).
//! - A Dolt working set is per branch, and DoltLite's checkout is per
//!   connection, so the local tables travel with the active branch. The
//!   checkout is `dolt_checkout('--carry-ignored', branch)` (celld patch 0003):
//!   the engine grafts the session's untracked ignored tables onto the target
//!   branch's working catalog by catalog entry, replacing the target's stale
//!   copy of the same name, **without reading or writing a row**. The cost of a
//!   checkout does not depend on how much local state there is. A branch's
//!   own copy is left behind and is stale from then on; it is never read,
//!   because the next checkout to that branch carries the live one over it.
//! - DoltLite opens every connection on `main`, so the active branch is a
//!   pointer, `_cf_DOLT.branch` in `main`'s working set, which every open reads
//!   first (crash-safe: [`checkout`] flips it only after the target holds the
//!   live local tables, so a crash before the flip reopens on the old branch,
//!   whose copy is still current, and a crash after it opens the new one).
//! - A fork starts with empty local tables ([`fork_image`]): the declared
//!   patterns are read back from `dolt_ignore`, and the identity-bound
//!   `_cf_ALARM`, `_cf_WAKE` and `_cf_METADATA` are emptied too.
//!
//! Application SQL cannot reach any `_cf_` table: the authorizer reserves
//! `_cf_*`, denies writes to `dolt_*` system tables, and denies every `dolt_*`
//! function except read-only ones (`storage.rs`). A class's own local tables
//! are ordinary tables to its SQL.

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

pub(crate) fn active_branch(connection: &Connection) -> anyhow::Result<String> {
    Ok(connection.query_row("SELECT active_branch()", [], |row| row.get(0))?)
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
/// table and the ignore rules of the runtime tables and of the class's
/// `local` patterns. Idempotent; runs after `storage::schema`.
pub(crate) fn ensure_runtime(connection: &Connection, local: &[String]) -> anyhow::Result<()> {
    connection.execute(
        "CREATE TABLE IF NOT EXISTS _cf_DOLT (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
        [],
    )?;
    ensure_ignore_rules(connection, local)
}

/// A table name celld or Dolt owns: no local pattern may match one.
const RESERVED_TABLE_SAMPLES: &[&str] = &[
    "_cf_KV",
    "_cf_ALARM",
    "_cf_DOLT",
    "dolt_ignore",
    "dolt_log",
    "doltlite_engine",
    "sqlite_sequence",
    "sqlite_schema",
    "_litestream_seq",
    "_litestream_lock",
];

/// Whether `name` is covered by a `dolt_ignore` pattern. DoltLite's matcher:
/// `%` and `*` match any run of characters, everything else matches itself
/// (case-sensitively). Declared patterns hold only `%` as a wildcard.
fn pattern_matches(pattern: &str, name: &str) -> bool {
    let (p, n) = (pattern.as_bytes(), name.as_bytes());
    let (mut pi, mut ni) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == b'%' || p[pi] == b'*') {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if pi < p.len() && p[pi] == n[ni] {
            pi += 1;
            ni += 1;
        } else if let Some(at) = star {
            pi = at + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && (p[pi] == b'%' || p[pi] == b'*') {
        pi += 1;
    }
    pi == p.len()
}

/// The longest a local table pattern may be.
const MAX_LOCAL_PATTERN: usize = 64;

/// Check the table-name patterns a class declares as local (`doltLocal`):
/// ASCII letters, digits, `_` and the wildcard `%`, starting with a letter,
/// and none that could cover a table celld or Dolt owns (`_cf_*`, `dolt_*`,
/// `sqlite_*`), so `%` and `d%` are refused and `outbox_%` is fine.
pub fn validate_local_patterns(patterns: &[String]) -> anyhow::Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for pattern in patterns {
        anyhow::ensure!(
            !pattern.is_empty()
                && pattern.len() <= MAX_LOCAL_PATTERN
                && pattern.starts_with(|c: char| c.is_ascii_alphabetic())
                && pattern.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'%'),
            "local table pattern {pattern:?} is not valid: use up to {MAX_LOCAL_PATTERN} ASCII letters, \
             digits and `_`, starting with a letter, with `%` for any run of characters"
        );
        if let Some(reserved) = RESERVED_TABLE_SAMPLES.iter().find(|name| pattern_matches(pattern, name)) {
            bail!(
                "local table pattern {pattern:?} would cover {reserved:?}, a table celld or Dolt owns; \
                 give the pattern a prefix of your own, such as `outbox_%`"
            );
        }
        anyhow::ensure!(seen.insert(pattern.as_str()), "local table pattern {pattern:?} is listed twice");
    }
    Ok(())
}

/// Put `_cf_%` and each of `local` in `dolt_ignore` of the active branch,
/// where absent. The rules are rows of a versioned table: a branch made from
/// a commit that predates a rule lacks it, and a reset to such a commit
/// removes it, so this runs wherever the rules decide something.
fn ensure_ignore_rules(connection: &Connection, local: &[String]) -> anyhow::Result<()> {
    for pattern in std::iter::once(RUNTIME_PATTERN).chain(local.iter().map(String::as_str)) {
        let present = connection
            .query_row(
                "SELECT 1 FROM dolt_ignore WHERE pattern=?1 AND ignored=1",
                [pattern],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !present {
            connection.execute(
                "INSERT OR REPLACE INTO dolt_ignore(pattern, ignored) VALUES (?1, 1)",
                [pattern],
            )?;
        }
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
pub(crate) fn branch_name(value: &Value, what: &str) -> anyhow::Result<String> {
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
pub(crate) fn call(connection: &Connection, function: &str, args: &[String]) -> anyhow::Result<Value> {
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

pub(crate) fn sqlite_message(error: &rusqlite::Error) -> String {
    match error {
        rusqlite::Error::SqliteFailure(_, Some(message)) => message.clone(),
        other => other.to_string(),
    }
}

pub(crate) fn rows(connection: &Connection, sql: &str, params: &[String]) -> anyhow::Result<Vec<Value>> {
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
    matches!(op, "txCommit" | "mergeBegin" | "mergeTake" | "mergeFinish")
}

/// Whether an operation that runs inside the caller's open transaction can
/// change the schema the connection sees.
pub(crate) fn changes_schema_in_transaction(op: &str) -> bool {
    matches!(op, "mergeBegin")
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
    local: &[String],
) -> anyhow::Result<Value> {
    match op {
        "txCommit" => commit(connection, args, local),
        "mergeBegin" => merge_begin(connection, args),
        "mergeTake" => merge_take(connection, args),
        "mergeFinish" => merge_finish(connection, args),
        other => bail!("dolt operation {other:?} cannot run inside a storage transaction"),
    }
}

/// A SQLite value as JSON that keeps its storage class exact for the
/// harness: an INTEGER beyond 2^53, a non-finite REAL and a BLOB cannot be
/// plain JSON numbers.
fn exact_json(value: rusqlite::types::Value) -> Value {
    use base64::Engine as _;
    match value {
        rusqlite::types::Value::Null => Value::Null,
        rusqlite::types::Value::Integer(n) if n.unsigned_abs() <= 1 << 53 => json!(n),
        rusqlite::types::Value::Integer(n) => json!({ "__celld_int": n.to_string() }),
        rusqlite::types::Value::Real(f) if f.is_finite() => json!(f),
        rusqlite::types::Value::Real(f) => json!({ "__celld_real": f.to_string() }),
        rusqlite::types::Value::Text(text) => Value::String(text),
        rusqlite::types::Value::Blob(bytes) => json!({
            "__celld_blob": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes),
        }),
    }
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The most rows of conflicts and violations one merge hands a resolver.
const MAX_MERGE_ITEMS: usize = 100_000;

fn table_names(connection: &Connection, sql: &str) -> anyhow::Result<Vec<String>> {
    let mut statement = connection.prepare(sql)?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names)
}

fn primary_key(connection: &Connection, table: &str) -> anyhow::Result<Vec<String>> {
    let mut statement = connection
        .prepare("SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk")?;
    let names = statement
        .query_map([table], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names)
}

/// Which side of a conflict a `dolt_conflicts_<t>` column belongs to
/// (0 base, 1 ours, 2 theirs) and the table column it holds.
fn conflict_side(name: &str) -> Option<(usize, &str)> {
    if name == "our_diff_type" || name == "their_diff_type" {
        return None;
    }
    ["base_", "our_", "their_"]
        .iter()
        .enumerate()
        .find_map(|(side, prefix)| name.strip_prefix(prefix).map(|column| (side, column)))
}

/// The conflicting rows of `table`, from `dolt_conflicts_<table>`: its
/// `base_*`, `our_*` and `their_*` columns folded back into one row each.
fn conflict_rows(connection: &Connection, table: &str, budget: &mut usize) -> anyhow::Result<Vec<Value>> {
    let key_columns = primary_key(connection, table)?;
    let mut statement =
        connection.prepare(&format!("SELECT * FROM {}", quote_ident(&format!("dolt_conflicts_{table}"))))?;
    let names: Vec<String> = statement.column_names().iter().map(|name| name.to_string()).collect();
    let index_of = |wanted: &str| names.iter().position(|name| name == wanted);
    let (our_change, their_change, id) =
        (index_of("our_diff_type"), index_of("their_diff_type"), index_of("dolt_conflict_id"));
    let mut out = Vec::new();
    let mut cursor = statement.query([])?;
    while let Some(row) = cursor.next()? {
        anyhow::ensure!(
            *budget > 0,
            "merge has more than {MAX_MERGE_ITEMS} conflicting rows and violations; \
             it cannot be resolved in one transaction"
        );
        *budget -= 1;
        let text = |index: Option<usize>| -> anyhow::Result<Option<String>> {
            match index {
                Some(index) => Ok(row.get::<_, Option<String>>(index)?),
                None => Ok(None),
            }
        };
        let our = text(our_change)?;
        let their = text(their_change)?;
        let mut sides: [serde_json::Map<String, Value>; 3] = Default::default();
        for (index, name) in names.iter().enumerate() {
            if let Some((side, column)) = conflict_side(name) {
                sides[side].insert(column.to_string(), exact_json(row.get(index)?));
            }
        }
        let absent = [
            our.as_deref() == Some("added") && their.as_deref() == Some("added"),
            our.as_deref() == Some("removed"),
            their.as_deref() == Some("removed"),
        ];
        let [base, ours, theirs] = sides;
        let present = |map: serde_json::Map<String, Value>, absent: bool| {
            if absent {
                Value::Null
            } else {
                Value::Object(map)
            }
        };
        let (base, ours, theirs) =
            (present(base, absent[0]), present(ours, absent[1]), present(theirs, absent[2]));
        let from = [&ours, &theirs, &base].into_iter().find(|side| !side.is_null());
        let mut key = serde_json::Map::new();
        if let Some(Value::Object(from)) = from {
            for column in &key_columns {
                if let Some(value) = from.get(column) {
                    key.insert(column.clone(), value.clone());
                }
            }
        }
        out.push(json!({
            "table": table,
            "id": text(id)?,
            "key": key,
            "base": base,
            "ours": ours,
            "theirs": theirs,
            "ourChange": our,
            "theirChange": their,
        }));
    }
    Ok(out)
}

/// The constraint violations of `table`, from
/// `dolt_constraint_violations_<table>`.
fn violation_rows(connection: &Connection, table: &str, budget: &mut usize) -> anyhow::Result<Vec<Value>> {
    let mut statement = connection.prepare(&format!(
        "SELECT * FROM {}",
        quote_ident(&format!("dolt_constraint_violations_{table}"))
    ))?;
    let names: Vec<String> = statement.column_names().iter().map(|name| name.to_string()).collect();
    let mut out = Vec::new();
    let mut cursor = statement.query([])?;
    while let Some(row) = cursor.next()? {
        anyhow::ensure!(
            *budget > 0,
            "merge has more than {MAX_MERGE_ITEMS} conflicting rows and violations; \
             it cannot be resolved in one transaction"
        );
        *budget -= 1;
        let mut data = serde_json::Map::new();
        let mut kind = Value::Null;
        let mut info = Value::Null;
        for (index, name) in names.iter().enumerate() {
            let value = exact_json(row.get(index)?);
            match name.as_str() {
                "violation_type" => kind = value,
                "violation_info" => {
                    // Dolt writes the details as a JSON document.
                    info = match &value {
                        Value::String(text) => serde_json::from_str(text).unwrap_or(value.clone()),
                        _ => value,
                    }
                }
                _ => {
                    data.insert(name.clone(), value);
                }
            }
        }
        out.push(json!({ "table": table, "type": kind, "info": info, "row": data }));
    }
    Ok(out)
}

/// `dolt.merge(branch, { resolve })`, step one, inside the merge's own
/// transaction: `dolt_merge --no-commit`. A merge that cannot be finished
/// without the resolver (conflicts, constraint violations) is not an error
/// here: its state stays in the open transaction, where DoltLite keeps it
/// (`dolt_conflicts_<t>`, `dolt_constraint_violations_<t>`), and is
/// reported. Answers
///
/// ```json
/// { "state": "upToDate" | "fastForward" | "merging",
///   "hash": "<HEAD, for the first two>", "ours": "<hash>", "theirs": "<hash>",
///   "base": "<merge base or null>",
///   "conflicts": [{ "table", "id", "key", "base", "ours", "theirs", "ourChange", "theirChange" }],
///   "violations": [{ "table", "type", "info", "row" }],
///   "schemaConflicts": ["<table>"] }
/// ```
fn merge_begin(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    let branch = revision(&args["branch"], "branch")?;
    let ours = hash_of(connection, "HEAD")?;
    let theirs = hash_of(connection, &branch)?;
    let mut call_args = vec![branch.clone(), "--no-commit".to_string()];
    if args.get("fastForward").and_then(Value::as_bool) == Some(false) {
        call_args.push("--no-ff".to_string());
    }
    let merged = call(connection, "dolt_merge", &call_args);
    let mut conflict_tables = Vec::new();
    let mut violation_tables = Vec::new();
    let mut schema_conflicts = Vec::new();
    let state = match merged {
        Ok(Value::String(text)) if text == "Already up to date" => "upToDate",
        Ok(Value::String(hash)) if hash.len() == 40 => "fastForward",
        Ok(_) => "merging",
        Err(error) => {
            let message = format!("{error:#}");
            if !(message.contains("conflict") || message.contains("constraint violations")) {
                return Err(error);
            }
            "merging"
        }
    };
    if state == "merging" {
        conflict_tables = table_names(connection, "SELECT \"table\" FROM dolt_conflicts ORDER BY 1")?;
        violation_tables =
            table_names(connection, "SELECT \"table\" FROM dolt_constraint_violations ORDER BY 1")?;
        schema_conflicts = table_names(connection, "SELECT \"table\" FROM dolt_schema_conflicts ORDER BY 1")
            .unwrap_or_default();
    }
    let mut budget = MAX_MERGE_ITEMS;
    let mut conflicts = Vec::new();
    for table in &conflict_tables {
        conflicts.extend(conflict_rows(connection, table, &mut budget)?);
    }
    let mut violations = Vec::new();
    for table in &violation_tables {
        violations.extend(violation_rows(connection, table, &mut budget)?);
    }
    let base = match call(connection, "dolt_merge_base", &["HEAD".to_string(), theirs.clone()]) {
        Ok(Value::String(hash)) if !hash.is_empty() => Value::String(hash),
        _ => Value::Null,
    };
    Ok(json!({
        "state": state,
        "hash": if state == "merging" { Value::Null } else { Value::String(hash_of(connection, "HEAD")?) },
        "ours": ours,
        "theirs": theirs,
        "base": base,
        "conflicts": conflicts,
        "violations": violations,
        "schemaConflicts": schema_conflicts,
    }))
}

/// `conflict.takeTheirs()`: write their side of one conflicting row into
/// the table (their row, or the deletion of ours when they deleted it),
/// through SQL so every value keeps its storage class. The conflict itself
/// is cleared with the others by `mergeFinish`.
fn merge_take(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    let table = args["table"].as_str().ok_or_else(|| anyhow!("table must be a string"))?;
    let id = args["id"].as_str().ok_or_else(|| anyhow!("id must be a string"))?;
    let conflicts = quote_ident(&format!("dolt_conflicts_{table}"));
    let their_change: Option<String> = connection
        .query_row(
            &format!("SELECT their_diff_type FROM {conflicts} WHERE dolt_conflict_id = ?1"),
            [id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("{table} has no conflict {id:?}"))?;
    let columns = table_names(
        connection,
        &format!("SELECT name FROM pragma_table_info({}) ORDER BY cid", sql_string(table)),
    )?;
    let key = primary_key(connection, table)?;
    anyhow::ensure!(!columns.is_empty(), "{table} has no columns");
    let target = quote_ident(table);
    if their_change.as_deref() == Some("removed") {
        anyhow::ensure!(
            !key.is_empty(),
            "{table} has no primary key: delete the row with SQL and mark the conflict resolved"
        );
        // Their side deleted it: delete the row the working set holds, which
        // is ours (or what is left of it).
        let matches = key
            .iter()
            .map(|column| {
                let (ours, base) =
                    (quote_ident(&format!("our_{column}")), quote_ident(&format!("base_{column}")));
                format!(
                    "{target}.{column} IS (SELECT coalesce({ours}, {base}) FROM {conflicts} \
                     WHERE dolt_conflict_id = ?1)",
                    column = quote_ident(column),
                )
            })
            .collect::<Vec<_>>()
            .join(" AND ");
        connection.execute(&format!("DELETE FROM {target} WHERE {matches}"), [id])?;
    } else {
        let names = columns.iter().map(|column| quote_ident(column)).collect::<Vec<_>>().join(", ");
        let theirs = columns
            .iter()
            .map(|column| quote_ident(&format!("their_{column}")))
            .collect::<Vec<_>>()
            .join(", ");
        connection.execute(
            &format!(
                "INSERT OR REPLACE INTO {target}({names}) \
                 SELECT {theirs} FROM {conflicts} WHERE dolt_conflict_id = ?1"
            ),
            [id],
        )?;
    }
    Ok(Value::Null)
}

fn sql_string(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// Step two: the resolver is done. Every conflict and violation row it was
/// handed was answered (the harness checks that, row by row), so the
/// conflict and violation tables are cleared here, with whatever rows the
/// resolver wrote left in place, and what remains must be empty before the
/// merge commits: a merge cannot commit with either one outstanding.
fn merge_finish(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    let names = |key: &str| -> anyhow::Result<Vec<String>> {
        args[key]
            .as_array()
            .into_iter()
            .flatten()
            .map(|name| {
                name.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("{key} must be table names"))
            })
            .collect()
    };
    for table in names("conflictTables")? {
        connection.execute_batch(&format!(
            "DELETE FROM {}",
            quote_ident(&format!("dolt_conflicts_{table}"))
        ))?;
    }
    for table in names("violationTables")? {
        connection.execute_batch(&format!(
            "DELETE FROM {}",
            quote_ident(&format!("dolt_constraint_violations_{table}"))
        ))?;
    }
    let left: i64 = connection.query_row(
        "SELECT (SELECT coalesce(sum(num_conflicts), 0) FROM dolt_conflicts) \
         + (SELECT coalesce(sum(num_violations), 0) FROM dolt_constraint_violations)",
        [],
        |row| row.get(0),
    )?;
    anyhow::ensure!(left == 0, "the merge still has {left} unresolved conflicts or violations");
    Ok(Value::Null)
}

/// [`run_with`] for a cell that declares no local tables.
#[cfg(test)]
pub(crate) fn run(connection: &Connection, op: &str, args: &Value) -> anyhow::Result<Value> {
    run_with(connection, op, args, &[])
}

/// Run one `ctx.storage.dolt` operation. The caller holds the cell's
/// connection outside any transaction, with the application authorizer
/// lifted. `local` is the cell's declared local table patterns.
pub(crate) fn run_with(
    connection: &Connection,
    op: &str,
    args: &Value,
    local: &[String],
) -> anyhow::Result<Value> {
    match op {
        "commit" => commit(connection, args, local),
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
            checkout(connection, &target, local)?;
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

pub(crate) fn hash_of(connection: &Connection, rev: &str) -> anyhow::Result<String> {
    match call(connection, "dolt_hashof", &[rev.to_string()])? {
        Value::String(hash) => Ok(hash),
        other => bail!("dolt_hashof returned {other}"),
    }
}

fn commit(connection: &Connection, args: &Value, local: &[String]) -> anyhow::Result<Value> {
    let message = optional_string(args, "message")?
        .filter(|message| !message.is_empty())
        .ok_or_else(|| anyhow!("commit needs a non-empty message"))?;
    // A branch created from a commit made before a rule existed lacks it;
    // restore the rules so the local tables are never staged.
    ensure_ignore_rules(connection, local)?;
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
    // The patch is asked for table by table: `dolt_patch(from, to)` fails
    // over a range that changes `dolt_ignore` (a class that adds a local table
    // pattern does that), and a system table has no patch of use to an
    // application anyway.
    let mut patch = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for row in &summary {
        let name = row
            .get("toTable")
            .and_then(Value::as_str)
            .or_else(|| row.get("fromTable").and_then(Value::as_str))
            .unwrap_or("");
        if name.is_empty()
            || name.starts_with("_cf_")
            || name.starts_with("dolt_")
            || table.as_deref().is_some_and(|table| table != name)
            || !seen.insert(name.to_string())
        {
            continue;
        }
        patch.extend(rows(
            connection,
            "SELECT table_name AS \"table\", diff_type AS diffType, statement \
             FROM dolt_patch(?1, ?2, ?3) ORDER BY statement_order",
            &[from.clone(), to.clone(), name.to_string()],
        )?);
    }
    Ok(json!({ "summary": summary, "patch": patch }))
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

/// Check out `target`, carrying the local tables (celld's `_cf_*` and the
/// class's declared patterns) along. See the module documentation for the
/// carry and the crash argument.
///
/// 1. `--carry-ignored` moves the live local tables onto `target` by catalog
///    entry; the cost does not depend on their size.
/// 2. The active-branch pointer flips on `main`: a plain switch to `main`
///    (whose own local tables are stale and never read), the pointer write, a
///    plain switch back. Landing on `main` carries the pointer with the
///    tables, so it is one write.
pub(crate) fn checkout(connection: &Connection, target: &str, local: &[String]) -> anyhow::Result<()> {
    let source = active_branch(connection)?;
    if source == target {
        return Ok(());
    }
    call(connection, "dolt_checkout", &["--carry-ignored".to_string(), target.to_string()])
        .with_context(|| format!("check out {target:?}"))?;
    // The rules are rows of a versioned table the target may lack.
    ensure_ignore_rules(connection, local)?;
    if target == HOME_BRANCH {
        return set_pointer(connection, HOME_BRANCH);
    }
    // Flip the pointer on `main`: the commit point of the checkout.
    switch(connection, HOME_BRANCH)?;
    let flipped = ensure_ignore_rules(connection, local).and_then(|()| set_pointer(connection, target));
    switch(connection, target)?;
    flipped
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
            // The class's local tables are read back from the rules, which a
            // reset to an older commit can drop, so they are emptied first.
            empty_local_tables(&connection)?;
            crate::dolt_remote::strip(&connection)?;
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

/// The tables of the active branch that a class declared local: those
/// covered by a `dolt_ignore` rule other than celld's own `_cf_%`.
fn declared_local_tables(connection: &Connection) -> anyhow::Result<Vec<String>> {
    let patterns = table_names(
        connection,
        &format!("SELECT pattern FROM dolt_ignore WHERE ignored = 1 AND pattern <> '{RUNTIME_PATTERN}'"),
    )?;
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    let tables = table_names(
        connection,
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
         AND name NOT LIKE '\\_cf\\_%' ESCAPE '\\' AND name NOT LIKE 'dolt\\_%' ESCAPE '\\' ORDER BY name",
    )?;
    Ok(tables
        .into_iter()
        .filter(|table| patterns.iter().any(|pattern| pattern_matches(pattern, table)))
        .collect())
}

/// A fork, a restore or an import is a new identity: the tables a class
/// declared local start empty (their schema stays, so the class's migrations
/// find what they made). Their names are read back from the `dolt_ignore`
/// rules of the active branch; call this before a reset to an older commit
/// can drop the rules.
pub(crate) fn empty_local_tables(connection: &Connection) -> anyhow::Result<()> {
    let local = declared_local_tables(connection)?;
    for table in &local {
        connection.execute(&format!("DELETE FROM {}", quote_ident(table)), [])?;
    }
    if !local.is_empty() && table_exists(connection, "sqlite_sequence")? {
        for table in &local {
            connection.execute("DELETE FROM sqlite_sequence WHERE name = ?1", [table])?;
        }
    }
    Ok(())
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

    /// [`super::run_in_transaction`] for a cell that declares no local tables.
    fn run_in_transaction(connection: &Connection, op: &str, args: &Value) -> anyhow::Result<Value> {
        super::run_in_transaction(connection, op, args, &[])
    }

    fn cell(dir: &std::path::Path) -> (String, Connection) {
        let path = dir.join("cell.db").to_string_lossy().into_owned();
        let connection = open_connection(&path, celld_ltx::Engine::Dolt, None).unwrap();
        attach_active_branch(&connection).unwrap();
        crate::storage::schema(&connection).unwrap();
        ensure_runtime(&connection, &[]).unwrap();
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
    #[allow(clippy::disallowed_methods)]
    fn roots_after(path: &str, from: celld_ltx::dolt::Cut) -> usize {
        let bytes = std::fs::read(path).unwrap();
        let mut ends = std::collections::BTreeSet::new();
        for end in from.resume as usize..=bytes.len() {
            ends.insert(celld_ltx::dolt::scan_wal(&bytes[..end], 0, from.resume, from).end);
        }
        ends.len() - 1
    }

    #[allow(clippy::disallowed_methods)]
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

    /// A cell with `t(a PRIMARY KEY, b, c UNIQUE)` whose `main` and `other`
    /// both changed row 1 (a conflict), added a row with the same `c` (a
    /// unique violation), and where `other` deleted row 2 that `main` edited
    /// (a modify/delete conflict).
    fn diverged(dir: &std::path::Path) -> (String, Connection) {
        let (path, c) = cell(dir);
        c.execute_batch(
            "CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT, c TEXT UNIQUE); \
             INSERT INTO t VALUES (1,'x','u1'),(2,'y','u2'),(5,'z','u5');",
        )
        .unwrap();
        run(&c, "commit", &json!({"message": "base"})).unwrap();
        run(&c, "branchCreate", &json!({"name": "other"})).unwrap();
        c.execute_batch(
            "UPDATE t SET b='main' WHERE a=1; UPDATE t SET b='main2' WHERE a=2; \
             INSERT INTO t VALUES (3,'m3','dup'); UPDATE t SET b='only-main' WHERE a=5;",
        )
        .unwrap();
        run(&c, "commit", &json!({"message": "main side", "date": 1_700_000_000_000_i64})).unwrap();
        run(&c, "checkout", &json!({"branch": "other"})).unwrap();
        c.execute_batch(
            "UPDATE t SET b='other' WHERE a=1; DELETE FROM t WHERE a=2; \
             INSERT INTO t VALUES (4,'o4','dup');",
        )
        .unwrap();
        run(&c, "commit", &json!({"message": "other side", "date": 1_700_000_001_000_i64})).unwrap();
        run(&c, "checkout", &json!({"branch": "main"})).unwrap();
        (path, c)
    }

    /// F1: the steps `dolt.merge(branch, { resolve })` is made of.
    #[test]
    fn merge_with_a_resolver_commits_one_two_parent_commit() {
        let dir = tempfile::tempdir().unwrap();
        let (path, c) = diverged(dir.path());
        let main_head = run(&c, "hashOf", &json!({"ref": "HEAD"})).unwrap();
        let other_head = run(&c, "hashOf", &json!({"ref": "other"})).unwrap();

        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        let begin = run_in_transaction(&c, "mergeBegin", &json!({"branch": "other"})).unwrap();
        assert_eq!(begin["state"], "merging", "{begin}");
        assert_eq!(begin["ours"], main_head);
        assert_eq!(begin["theirs"], other_head);
        assert!(begin["base"].is_string());
        let conflicts = begin["conflicts"].as_array().unwrap();
        let by_key = |key: i64| {
            conflicts.iter().find(|c| c["key"] == json!({ "a": key })).unwrap_or_else(|| panic!("{begin}"))
        };
        let both_edited = by_key(1);
        assert_eq!(both_edited["table"], "t");
        assert_eq!(both_edited["base"]["b"], "x");
        assert_eq!(both_edited["ours"]["b"], "main");
        assert_eq!(both_edited["theirs"]["b"], "other");
        assert_eq!(both_edited["ourChange"], "modified");
        // Their side deleted the row our side edited.
        let deleted = by_key(2);
        assert_eq!(deleted["ours"]["b"], "main2");
        assert!(deleted["theirs"].is_null(), "{deleted}");
        assert_eq!(deleted["theirChange"], "removed");
        // Row 5 changed on one side only: merged without a conflict.
        assert_eq!(conflicts.len(), 2, "{begin}");
        let violations = begin["violations"].as_array().unwrap();
        assert_eq!(violations.len(), 2, "{begin}");
        assert_eq!(violations[0]["table"], "t");
        assert_eq!(violations[0]["type"], "unique index");
        assert_eq!(violations[0]["info"]["Columns"], json!(["c"]));
        assert!(violations.iter().any(|v| v["row"]["a"] == 3 && v["row"]["b"] == "m3"));
        assert!(violations.iter().any(|v| v["row"]["a"] == 4 && v["row"]["b"] == "o4"));
        assert_eq!(begin["schemaConflicts"], json!([]));

        // The resolver reads the system tables under the application authorizer
        // in the cell; here it writes rows, as the callback would.
        let finish = run_in_transaction(
            &c,
            "mergeFinish",
            &json!({"conflictTables": ["t"], "violationTables": []}),
        );
        assert!(
            format!("{:#}", finish.unwrap_err()).contains("unresolved"),
            "violations left must stop the merge"
        );
        c.execute_batch(
            "UPDATE t SET b='resolved' WHERE a=1; DELETE FROM t WHERE a=2; UPDATE t SET c='fixed' WHERE a=4;",
        )
        .unwrap();
        run_in_transaction(
            &c,
            "mergeFinish",
            &json!({"conflictTables": ["t"], "violationTables": ["t"]}),
        )
        .unwrap();
        let before = cut_of(&path);
        let hash = run_in_transaction(
            &c,
            "txCommit",
            &json!({"message": "Merge other", "author": "Ann <ann@example.com>", "date": 1_700_000_002_000_i64}),
        )
        .unwrap();
        c.execute_batch("COMMIT").unwrap();
        assert_eq!(roots_after(&path, before), 1, "the resolver's writes and the merge commit are one root");

        // One merge commit with both parents, the resolver's rows in it, and
        // nothing of the merge left open.
        let parents = rows(
            &c,
            "SELECT parent_hash FROM dolt_commit_ancestors WHERE commit_hash=dolt_hashof('HEAD') ORDER BY parent_index",
            &[],
        )
        .unwrap();
        assert_eq!(
            parents.iter().map(|p| p["parent_hash"].clone()).collect::<Vec<_>>(),
            vec![main_head, other_head]
        );
        assert_eq!(hash, run(&c, "hashOf", &json!({"ref": "HEAD"})).unwrap());
        let got = rows(&c, "SELECT a, b, c FROM t ORDER BY a", &[]).unwrap();
        let got: Vec<(i64, &str, &str)> = got
            .iter()
            .map(|r| (r["a"].as_i64().unwrap(), r["b"].as_str().unwrap(), r["c"].as_str().unwrap()))
            .collect();
        assert_eq!(
            got,
            vec![(1, "resolved", "u1"), (3, "m3", "dup"), (4, "o4", "fixed"), (5, "only-main", "u5")]
        );
        assert_eq!(run(&c, "status", &json!({})).unwrap(), json!([]));
        assert_eq!(rows(&c, "SELECT * FROM dolt_conflicts", &[]).unwrap().len(), 0);
        assert_eq!(rows(&c, "SELECT * FROM dolt_constraint_violations", &[]).unwrap().len(), 0);
    }

    /// F1: a merge that is abandoned (the resolver threw, or left conflicts)
    /// leaves no merge state: the branch, its rows and its status are as they
    /// were, and the same merge can be run again.
    #[test]
    fn an_abandoned_merge_leaves_no_merge_state() {
        let dir = tempfile::tempdir().unwrap();
        let (path, c) = diverged(dir.path());
        let head = run(&c, "hashOf", &json!({"ref": "HEAD"})).unwrap();
        let before = cut_of(&path);
        let rows_before = rows(&c, "SELECT * FROM t ORDER BY a", &[]).unwrap();
        for _ in 0..2 {
            c.execute_batch("BEGIN IMMEDIATE").unwrap();
            let begin = run_in_transaction(&c, "mergeBegin", &json!({"branch": "other"})).unwrap();
            assert_eq!(begin["state"], "merging");
            c.execute("UPDATE t SET b='half resolved' WHERE a=1", []).unwrap();
            c.execute_batch("ROLLBACK").unwrap();
            assert_eq!(run(&c, "hashOf", &json!({"ref": "HEAD"})).unwrap(), head);
            assert_eq!(rows(&c, "SELECT * FROM t ORDER BY a", &[]).unwrap(), rows_before);
            assert_eq!(rows(&c, "SELECT * FROM dolt_conflicts", &[]).unwrap().len(), 0);
            assert_eq!(rows(&c, "SELECT * FROM dolt_constraint_violations", &[]).unwrap().len(), 0);
            assert_eq!(run(&c, "status", &json!({})).unwrap(), json!([]));
        }
        assert_eq!(roots_after(&path, before), 0, "an abandoned merge writes nothing");
        // And the ordinary merge still sees the conflicts.
        let merged = run(&c, "merge", &json!({"branch": "other"})).unwrap();
        assert!(merged["hash"].is_null());
    }

    /// F1: a merge with nothing to resolve still goes through the same steps:
    /// fast-forward and up-to-date merges commit nothing; a non-conflicting
    /// divergence merges its rows, commits once, and `--no-ff` forces the
    /// merge commit.
    #[test]
    fn merge_with_a_resolver_handles_the_clean_cases() {
        let dir = tempfile::tempdir().unwrap();
        let (_path, c) = cell(dir.path());
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT); INSERT INTO t VALUES (1,'x');").unwrap();
        run(&c, "commit", &json!({"message": "base"})).unwrap();
        run(&c, "branchCreate", &json!({"name": "ahead"})).unwrap();
        run(&c, "checkout", &json!({"branch": "ahead"})).unwrap();
        c.execute("INSERT INTO t VALUES (2,'ahead')", []).unwrap();
        run(&c, "commit", &json!({"message": "ahead"})).unwrap();
        let ahead = run(&c, "hashOf", &json!({"ref": "HEAD"})).unwrap();
        run(&c, "checkout", &json!({"branch": "main"})).unwrap();

        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        let begin = run_in_transaction(&c, "mergeBegin", &json!({"branch": "ahead"})).unwrap();
        assert_eq!(begin["state"], "fastForward");
        assert_eq!(begin["hash"], ahead);
        c.execute_batch("COMMIT").unwrap();
        assert_eq!(run(&c, "hashOf", &json!({"ref": "HEAD"})).unwrap(), ahead);

        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        let begin = run_in_transaction(&c, "mergeBegin", &json!({"branch": "ahead"})).unwrap();
        assert_eq!(begin["state"], "upToDate");
        c.execute_batch("COMMIT").unwrap();

        // Diverge without a conflict.
        c.execute("INSERT INTO t VALUES (3,'main')", []).unwrap();
        let main = run(&c, "commit", &json!({"message": "main"})).unwrap();
        run(&c, "checkout", &json!({"branch": "ahead"})).unwrap();
        c.execute("INSERT INTO t VALUES (4,'ahead2')", []).unwrap();
        run(&c, "commit", &json!({"message": "ahead2"})).unwrap();
        run(&c, "checkout", &json!({"branch": "main"})).unwrap();
        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        let begin = run_in_transaction(&c, "mergeBegin", &json!({"branch": "ahead"})).unwrap();
        assert_eq!(begin["state"], "merging");
        assert_eq!(begin["conflicts"], json!([]));
        assert_eq!(begin["violations"], json!([]));
        run_in_transaction(&c, "mergeFinish", &json!({})).unwrap();
        run_in_transaction(&c, "txCommit", &json!({"message": "merged"})).unwrap();
        c.execute_batch("COMMIT").unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 4);
        let first_parent = rows(
            &c,
            "SELECT parent_hash FROM dolt_commit_ancestors WHERE commit_hash=dolt_hashof('HEAD') AND parent_index=0",
            &[],
        )
        .unwrap();
        assert_eq!(first_parent[0]["parent_hash"], main);

        // --no-ff on a branch that is ahead makes the merge commit.
        run(&c, "checkout", &json!({"branch": "ahead"})).unwrap();
        run(&c, "branchCreate", &json!({"name": "behind", "from": "HEAD~1"})).unwrap();
        run(&c, "checkout", &json!({"branch": "behind"})).unwrap();
        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        let begin =
            run_in_transaction(&c, "mergeBegin", &json!({"branch": "ahead", "fastForward": false})).unwrap();
        assert_eq!(begin["state"], "merging", "{begin}");
        run_in_transaction(&c, "mergeFinish", &json!({})).unwrap();
        run_in_transaction(&c, "txCommit", &json!({"message": "no-ff"})).unwrap();
        c.execute_batch("COMMIT").unwrap();
        let parents: i64 = c
            .query_row(
                "SELECT count(*) FROM dolt_commit_ancestors WHERE commit_hash=dolt_hashof('HEAD')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(parents, 2);
    }

    /// `conflict.takeTheirs()`: their row replaces ours, and their deletion
    /// deletes ours, with the values written through SQL.
    #[test]
    fn taking_their_side_of_a_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let (_path, c) = diverged(dir.path());
        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        let begin = run_in_transaction(&c, "mergeBegin", &json!({"branch": "other"})).unwrap();
        for conflict in begin["conflicts"].as_array().unwrap() {
            run_in_transaction(
                &c,
                "mergeTake",
                &json!({"table": conflict["table"], "id": conflict["id"]}),
            )
            .unwrap();
        }
        c.execute("UPDATE t SET c='fixed' WHERE a=4", []).unwrap();
        run_in_transaction(&c, "mergeFinish", &json!({"conflictTables": ["t"], "violationTables": ["t"]}))
            .unwrap();
        run_in_transaction(&c, "txCommit", &json!({"message": "theirs"})).unwrap();
        c.execute_batch("COMMIT").unwrap();
        let got = rows(&c, "SELECT a, b FROM t ORDER BY a", &[]).unwrap();
        let got: Vec<(i64, &str)> =
            got.iter().map(|r| (r["a"].as_i64().unwrap(), r["b"].as_str().unwrap())).collect();
        // Row 1: theirs. Row 2: they deleted it. Rows 3-5 merged by the engine.
        assert_eq!(got, vec![(1, "other"), (3, "m3"), (4, "o4"), (5, "only-main")]);
        // A conflict that is not there cannot be taken.
        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        let error = run_in_transaction(&c, "mergeTake", &json!({"table": "t", "id": "nope"}));
        c.execute_batch("ROLLBACK").unwrap();
        assert!(error.is_err());
    }

    /// A cell that declares `outbox_%` local, and the calls it makes.
    fn local_cell(dir: &std::path::Path) -> (String, Connection) {
        let path = dir.join("cell.db").to_string_lossy().into_owned();
        let connection = open_connection(&path, celld_ltx::Engine::Dolt, None).unwrap();
        attach_active_branch(&connection).unwrap();
        crate::storage::schema(&connection).unwrap();
        ensure_runtime(&connection, &LOCAL).unwrap();
        (path, connection)
    }

    static LOCAL: std::sync::LazyLock<Vec<String>> =
        std::sync::LazyLock::new(|| vec!["outbox_%".to_string()]);

    fn dolt(c: &Connection, op: &str, args: Value) -> Value {
        run_with(c, op, &args, &LOCAL).unwrap_or_else(|error| panic!("{op} {args}: {error:#}"))
    }

    fn outbox(c: &Connection) -> Vec<String> {
        let mut statement = c.prepare("SELECT body FROM outbox_msgs ORDER BY id").unwrap();
        statement.query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    }

    /// F4: tables the class declared local are ignored by commits, survive
    /// reset and merge, move with checkout, and never enter a diff.
    #[test]
    fn declared_local_tables_follow_the_branch_and_stay_out_of_history() {
        let dir = tempfile::tempdir().unwrap();
        let (_path, c) = local_cell(dir.path());
        c.execute_batch(
            "CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT); \
             CREATE TABLE outbox_msgs(id INTEGER PRIMARY KEY, body TEXT); \
             CREATE INDEX outbox_by_body ON outbox_msgs(body); \
             INSERT INTO t VALUES (1,'x'); INSERT INTO outbox_msgs(body) VALUES ('o1');",
        )
        .unwrap();
        let base = dolt(&c, "commit", json!({"message": "base"}));
        // Nothing local was staged, and no later diff shows it.
        assert_eq!(dolt(&c, "status", json!({})), json!([]));
        c.execute("INSERT INTO outbox_msgs(body) VALUES ('o2')", []).unwrap();
        assert_eq!(dolt(&c, "status", json!({})), json!([]));
        let diff = dolt(&c, "diff", json!({"from": base}));
        assert!(
            diff["summary"].as_array().unwrap().iter().all(|row| row["toTable"] != "outbox_msgs"),
            "{diff}"
        );

        // Moves with checkout; the branch's stale copy is replaced.
        dolt(&c, "branchCreate", json!({"name": "b"}));
        dolt(&c, "checkout", json!({"branch": "b"}));
        assert_eq!(outbox(&c), ["o1", "o2"]);
        c.execute("INSERT INTO outbox_msgs(body) VALUES ('o3')", []).unwrap();
        c.execute("UPDATE t SET b='on b' WHERE a=1", []).unwrap();
        dolt(&c, "commit", json!({"message": "on b"}));
        dolt(&c, "checkout", json!({"branch": "main"}));
        assert_eq!(outbox(&c), ["o1", "o2", "o3"], "main's copy from before is replaced by the live one");
        c.execute("DELETE FROM outbox_msgs WHERE body='o1'", []).unwrap();
        dolt(&c, "checkout", json!({"branch": "b"}));
        assert_eq!(outbox(&c), ["o2", "o3"]);
        // The index came along.
        c.execute("INSERT INTO outbox_msgs(body) VALUES ('o4')", []).unwrap();
        let indexes: i64 = c
            .query_row("SELECT count(*) FROM sqlite_schema WHERE name='outbox_by_body'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(indexes, 1);
        dolt(&c, "checkout", json!({"branch": "main"}));

        // Survives merge and reset --hard.
        let merged = dolt(&c, "merge", json!({"branch": "b"}));
        assert_eq!(merged["conflicts"], json!([]));
        assert_eq!(outbox(&c), ["o2", "o3", "o4"]);
        dolt(&c, "reset", json!({"hard": true, "to": base}));
        assert_eq!(outbox(&c), ["o2", "o3", "o4"]);
        let b: String = c.query_row("SELECT b FROM t WHERE a=1", [], |r| r.get(0)).unwrap();
        assert_eq!(b, "x");
        // Local rows do not change a commit's identity.
        dolt(&c, "commit", json!({"message": "empty", "allowEmpty": true, "date": 1}));
        let first = dolt(&c, "hashOf", json!({"ref": "HEAD"}));
        dolt(&c, "reset", json!({"hard": true, "to": base}));
        c.execute("DELETE FROM outbox_msgs", []).unwrap();
        dolt(&c, "commit", json!({"message": "empty", "allowEmpty": true, "date": 1}));
        assert_eq!(dolt(&c, "hashOf", json!({"ref": "HEAD"})), first);
    }

    /// A branch made from a commit that predates a rule, or reset to one,
    /// gets the rule back where it matters: commit and checkout.
    #[test]
    fn the_local_rules_come_back_on_a_branch_that_lacks_them() {
        let dir = tempfile::tempdir().unwrap();
        let (_path, c) = cell(dir.path());
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY);").unwrap();
        let before_rule = run(&c, "commit", &json!({"message": "before the class declared a local table"})).unwrap();
        // The class is redeployed with `outbox_%` local.
        ensure_runtime(&c, &LOCAL).unwrap();
        c.execute_batch("CREATE TABLE outbox_msgs(id INTEGER PRIMARY KEY, body TEXT); INSERT INTO outbox_msgs(body) VALUES ('o1');")
            .unwrap();
        dolt(&c, "commit", json!({"message": "after"}));
        assert_eq!(dolt(&c, "status", json!({})), json!([]));
        // A branch from the old commit has no rule, yet the table follows it
        // there. The rule is a row of `dolt_ignore`, a versioned table, so
        // putting it back is a change the branch commits; the table itself
        // is never one.
        dolt(&c, "branchCreate", json!({"name": "old", "from": before_rule}));
        dolt(&c, "checkout", json!({"branch": "old"}));
        assert_eq!(outbox(&c), ["o1"]);
        let status = dolt(&c, "status", json!({}));
        assert_eq!(status.as_array().unwrap().len(), 1, "{status}");
        assert_eq!(status[0]["table"], "dolt_ignore");
        dolt(&c, "commit", json!({"message": "on old"}));
        assert_eq!(dolt(&c, "status", json!({})), json!([]));
        let diff = dolt(&c, "diff", json!({"from": "HEAD~1", "to": "HEAD"}));
        let changed: Vec<&str> = diff["summary"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|row| row["toTable"].as_str().or(row["fromTable"].as_str()))
            .collect();
        assert_eq!(changed, ["dolt_ignore"], "{diff}");
    }

    /// F4: a fork starts with empty local tables and keeps their schema; the
    /// identity-bound runtime state goes too.
    #[test]
    #[allow(clippy::disallowed_methods)]
    fn a_fork_image_empties_the_declared_local_tables() {
        let dir = tempfile::tempdir().unwrap();
        let (path, c) = local_cell(dir.path());
        c.execute_batch(
            "CREATE TABLE t(a INTEGER PRIMARY KEY); INSERT INTO t VALUES (1); \
             CREATE TABLE outbox_msgs(id INTEGER PRIMARY KEY, body TEXT); \
             INSERT INTO outbox_msgs(body) VALUES ('o1'),('o2'); \
             CREATE TABLE ordinary(a INTEGER PRIMARY KEY); INSERT INTO ordinary VALUES (7);",
        )
        .unwrap();
        let first = dolt(&c, "commit", json!({"message": "one"}));
        // A fork at an older commit: the rules the reset would drop are read first.
        c.execute("INSERT INTO t VALUES (2)", []).unwrap();
        dolt(&c, "commit", json!({"message": "two"}));
        let source = std::fs::read(&path).unwrap();
        for at in [None, Some(first.as_str().unwrap())] {
            let image = fork_image(&source, &dir.path().join("scratch.db"), at).unwrap();
            let fork = dir.path().join("fork.db");
            std::fs::write(&fork, celld_ltx::ltx::decode_database_image(&fork_snapshot(&image).unwrap()).unwrap())
                .unwrap();
            celld_ltx::DoltStore::open(&fork, celld_ltx::LtxHost::default()).unwrap();
            let f = Connection::open(&fork).unwrap();
            attach_active_branch(&f).unwrap();
            assert_eq!(outbox(&f), Vec::<String>::new(), "at {at:?}");
            let ordinary: i64 = f.query_row("SELECT count(*) FROM ordinary", [], |r| r.get(0)).unwrap();
            assert_eq!(ordinary, 1, "an ordinary table is a fork's data");
            // The table works as a new one does.
            f.execute("INSERT INTO outbox_msgs(body) VALUES ('fresh')", []).unwrap();
            assert_eq!(outbox(&f), ["fresh"]);
            drop(f);
            std::fs::remove_file(&fork).unwrap();
        }
        assert_eq!(outbox(&c), ["o1", "o2"], "the source is untouched");
    }

    /// F5: a checkout moves local tables by catalog entry and does not copy
    /// rows, so the store grows by a few chunks however much local state
    /// there is (copying 5000 rows of 100 bytes would append over 500 KB).
    #[test]
    #[allow(clippy::disallowed_methods)]
    fn checkout_does_not_copy_local_rows() {
        let dir = tempfile::tempdir().unwrap();
        let (path, c) = local_cell(dir.path());
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY); CREATE TABLE outbox_msgs(id INTEGER PRIMARY KEY, body TEXT);")
            .unwrap();
        c.execute_batch("BEGIN").unwrap();
        for i in 0..5000 {
            c.execute("INSERT INTO outbox_msgs(body) VALUES (?1)", [format!("{i:0>100}")]).unwrap();
        }
        c.execute_batch("COMMIT").unwrap();
        dolt(&c, "commit", json!({"message": "base"}));
        dolt(&c, "branchCreate", json!({"name": "b"}));
        dolt(&c, "checkout", json!({"branch": "b"}));
        let before = std::fs::metadata(&path).unwrap().len();
        for _ in 0..5 {
            dolt(&c, "checkout", json!({"branch": "main"}));
            dolt(&c, "checkout", json!({"branch": "b"}));
        }
        let grown = std::fs::metadata(&path).unwrap().len() - before;
        assert!(grown < 100_000, "ten checkouts appended {grown} bytes");
        assert_eq!(outbox(&c).len(), 5000);
    }

    /// Release benchmark for F5 (`cargo test -p celld --release --lib
    /// checkout_cost -- --ignored --nocapture`): checkout latency with
    /// local rows in the table, at each `synchronous` level (celld runs cells
    /// at NORMAL).
    #[test]
    #[ignore]
    #[allow(clippy::disallowed_methods)]
    fn checkout_cost_by_local_rows() {
        fn percentile(mut v: Vec<f64>, p: f64) -> f64 {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[((v.len() as f64 * p) as usize).min(v.len() - 1)]
        }
        for sync in ["FULL", "NORMAL", "OFF"] {
            for rows in [0usize, 1_000, 10_000] {
                let dir = tempfile::tempdir().unwrap();
                let (_path, c) = local_cell(dir.path());
                c.execute_batch(&format!("PRAGMA synchronous={sync}")).unwrap();
                c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY); CREATE TABLE outbox_msgs(id INTEGER PRIMARY KEY, body TEXT);")
                    .unwrap();
                c.execute_batch("BEGIN").unwrap();
                for i in 0..rows {
                    c.execute("INSERT INTO outbox_msgs(body) VALUES (?1)", [format!("{i:0>64}")]).unwrap();
                }
                c.execute_batch("COMMIT").unwrap();
                dolt(&c, "commit", json!({"message": "base"}));
                dolt(&c, "branchCreate", json!({"name": "b"}));
                let mut quiet = Vec::new();
                let mut written = Vec::new();
                for i in 0..200 {
                    let target = if i % 2 == 0 { "b" } else { "main" };
                    // A local write before each checkout, as an outbox sees.
                    c.execute("INSERT INTO outbox_msgs(body) VALUES ('x')", []).unwrap();
                    let started = std::time::Instant::now();
                    dolt(&c, "checkout", json!({"branch": target}));
                    written.push(started.elapsed().as_secs_f64() * 1000.0);
                }
                for i in 0..200 {
                    let target = if i % 2 == 0 { "b" } else { "main" };
                    let started = std::time::Instant::now();
                    dolt(&c, "checkout", json!({"branch": target}));
                    quiet.push(started.elapsed().as_secs_f64() * 1000.0);
                }
                eprintln!(
                    "synchronous={sync:<4} local rows {rows:>6}: checkout p50/p95 ms: \
                     after a local write {:.2}/{:.2}, no write {:.2}/{:.2}",
                    percentile(written.clone(), 0.5),
                    percentile(written, 0.95),
                    percentile(quiet.clone(), 0.5),
                    percentile(quiet, 0.95),
                );
            }
        }
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

    /// DoltLite re-renders the `sql` of every table and index in
    /// `sqlite_master` when the transaction that created it commits (its
    /// catalog stores a canonical form, which is also what its schema hashes
    /// cover, so that `ADD COLUMN` and a typed-final `CREATE TABLE` compare
    /// equal). Neither a Dolt commit, a checkout nor a reopen changes the text
    /// again, and rendering a rendered text gives the same text. A class that
    /// fingerprints its schema from `sqlite_master.sql` must fingerprint this
    /// form (docs/services/dolt-cells.md, "Schema text"). The test pins the
    /// rules so that an engine upgrade that changes them is noticed.
    #[test]
    fn schema_text_is_rendered_when_the_creating_transaction_commits() {
        let dir = tempfile::tempdir().unwrap();
        let (path, c) = cell(dir.path());
        let master = |c: &Connection| -> Vec<(String, Option<String>)> {
            let mut statement = c
                .prepare(
                    "SELECT name, sql FROM sqlite_master \
                     WHERE name NOT LIKE '\\_cf\\_%' ESCAPE '\\' AND name NOT LIKE 'dolt%' \
                       AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' ORDER BY name",
                )
                .unwrap();
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        let ddl = [
            "CREATE TABLE \"q\" ( a INTEGER , b TEXT )",
            "CREATE TABLE [br] (x)",
            "CREATE TABLE t3 (\n  a INTEGER, -- comment\n  b /* c */ TEXT\n)",
            "CREATE UNIQUE INDEX u ON t3 ( a )",
            "CREATE TABLE t4 (a TEXT DEFAULT 'it''s  spaced  ', \"Mixed Case\" INT)",
            "CREATE TABLE \"my table\" (a)",
            "CREATE VIEW v1 AS SELECT  a ,  b FROM t3",
            "CREATE TRIGGER tr1 AFTER INSERT ON t3 BEGIN  SELECT 1 ;  END",
            "CREATE TABLE t6 (a)",
        ];
        c.execute_batch("BEGIN").unwrap();
        for sql in ddl {
            c.execute_batch(sql).unwrap();
        }
        c.execute_batch("ALTER TABLE t6 ADD COLUMN  tag  TEXT").unwrap();
        // Inside the creating transaction the text is what SQLite stored.
        let written = master(&c);
        assert!(written.contains(&(
            "t3".to_string(),
            Some("CREATE TABLE t3 (\n  a INTEGER, -- comment\n  b /* c */ TEXT\n)".to_string())
        )), "{written:?}");
        c.execute_batch("COMMIT").unwrap();

        let rendered = master(&c);
        let text = |name: &str| {
            rendered.iter().find(|(n, _)| n == name).and_then(|(_, sql)| sql.clone()).unwrap()
        };
        // Whitespace runs are one space, none before `(`, `)` and `,`;
        // comments go; quoting is kept only where the name needs it; string
        // literals and quoted identifiers are untouched.
        assert_eq!(text("q"), "CREATE TABLE q( a INTEGER, b TEXT)");
        assert_eq!(text("br"), "CREATE TABLE br(x)");
        assert_eq!(text("t3"), "CREATE TABLE t3( a INTEGER, b TEXT)");
        assert_eq!(text("u"), "CREATE UNIQUE INDEX u ON t3( a)");
        assert_eq!(
            text("t4"),
            "CREATE TABLE t4(a TEXT DEFAULT 'it''s  spaced  ', \"Mixed Case\" INT)"
        );
        assert_eq!(text("my table"), "CREATE TABLE \"my table\"(a)");
        assert_eq!(text("t6"), "CREATE TABLE t6(a, tag TEXT)");
        // Views and triggers are kept as written.
        assert_eq!(text("v1"), "CREATE VIEW v1 AS SELECT  a ,  b FROM t3");
        assert_eq!(text("tr1"), "CREATE TRIGGER tr1 AFTER INSERT ON t3 BEGIN  SELECT 1 ;  END");

        // Nothing later changes it again.
        run(&c, "commit", &json!({"message": "schema"})).unwrap();
        run(&c, "checkout", &json!({"branch": "other", "create": true})).unwrap();
        run(&c, "checkout", &json!({"branch": "main"})).unwrap();
        assert_eq!(master(&c), rendered);
        drop(c);
        let c = open_connection(&path, celld_ltx::Engine::Dolt, None).unwrap();
        attach_active_branch(&c).unwrap();
        assert_eq!(master(&c), rendered);
        // And a rendered text renders to itself.
        c.execute_batch("CREATE TABLE again( a INTEGER, b TEXT)").unwrap();
        assert_eq!(
            master(&c).iter().find(|(n, _)| n == "again").and_then(|(_, sql)| sql.clone()).unwrap(),
            "CREATE TABLE again( a INTEGER, b TEXT)"
        );
    }

    /// What a reader of the store sees, for comparing the live cell with the
    /// store a capture would restore.
    fn observed(c: &Connection) -> Value {
        json!({
            "branch": active_branch(c).unwrap(),
            "head": hash_of(c, "HEAD").unwrap(),
            "branches": rows(c, "SELECT name, hash FROM dolt_branches ORDER BY name", &[]).unwrap(),
            "log": rows(c, "SELECT commit_hash, message FROM dolt_log", &[]).unwrap(),
            "rows": rows(c, "SELECT a, b FROM t ORDER BY a", &[]).unwrap(),
            "status": rows(c, "SELECT table_name, staged, status FROM dolt_status \
                               WHERE table_name NOT LIKE '\\_cf\\_%' ESCAPE '\\'", &[]).unwrap(),
            "outbox": rows(c, "SELECT body FROM outbox_msgs ORDER BY id", &[]).unwrap(),
        })
    }

    /// What a node restores after a kill: the capture's L0 files applied in
    /// order (the page images the bucket holds), opened as a restore opens
    /// them. Captures first, so only what a capture shipped counts.
    #[allow(clippy::disallowed_methods)]
    fn restored_from_captures(
        store: &mut celld_ltx::DoltStore,
        into: &std::path::Path,
    ) -> Connection {
        use celld_ltx::CellStore as _;
        store.sync().unwrap();
        let restored = into.join("restored.db");
        let _ = std::fs::remove_file(&restored);
        let mut image: Vec<u8> = Vec::new();
        for txid in 1..=store.pos().unwrap().txid.0 {
            let id = celld_ltx::TXID(txid);
            let bytes = std::fs::read(store.ltx_path(0, id, id)).unwrap();
            let decoded = celld_ltx::ltx::decode_file(&bytes).unwrap();
            for (pgno, page) in celld_ltx::ltx::decode_file_pages(&bytes).unwrap() {
                let at = (pgno as usize - 1) * 4096;
                if image.len() < at + 4096 {
                    image.resize(at + 4096, 0);
                }
                image[at..at + 4096].copy_from_slice(&page);
            }
            image.resize(decoded.header.commit as usize * 4096, 0);
        }
        std::fs::write(&restored, image).unwrap();
        celld_ltx::DoltStore::open(&restored, celld_ltx::LtxHost::default()).unwrap();
        let c = open_connection(&restored.to_string_lossy(), celld_ltx::Engine::Dolt, None).unwrap();
        attach_active_branch(&c).unwrap();
        c
    }

    /// Two commits on `main` of `t`, with a local row, a second branch that
    /// has a commit of its own, and an uncommitted row.
    fn two_commits(
        dir: &std::path::Path,
        in_transaction: bool,
    ) -> (celld_ltx::DoltStore, Connection) {
        // The capture store opens before the application opens the file.
        let store =
            celld_ltx::DoltStore::open(dir.join("cell.db"), celld_ltx::LtxHost::default()).unwrap();
        let (_path, c) = local_cell(dir);
        c.execute_batch(
            "CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT); \
             CREATE TABLE outbox_msgs(id INTEGER PRIMARY KEY, body TEXT);",
        )
        .unwrap();
        let commit = |row: &str, message: &str| {
            if in_transaction {
                // `transactionSync(fn, { commit })`: the write and the commit are one step.
                c.execute_batch("BEGIN IMMEDIATE").unwrap();
                c.execute(&format!("INSERT INTO t VALUES {row}"), []).unwrap();
                super::run_in_transaction(&c, "txCommit", &json!({"message": message}), &LOCAL).unwrap();
                c.execute_batch("COMMIT").unwrap();
            } else {
                c.execute(&format!("INSERT INTO t VALUES {row}"), []).unwrap();
                dolt(&c, "commit", json!({"message": message}));
            }
        };
        commit("(1, 'one')", "one");
        dolt(&c, "branchCreate", json!({"name": "side"}));
        commit("(2, 'two')", "two");
        c.execute("INSERT INTO t VALUES (3, 'pending')", []).unwrap();
        c.execute("INSERT INTO outbox_msgs(body) VALUES ('local')", []).unwrap();
        (store, c)
    }

    /// The durability contract of the output gate: an operation that
    /// `mutates` has appended its effects to the chunk file behind a sealed
    /// root before it returns, so the capture that follows ships them. Every
    /// operation, on a store with history, a second branch, a dirty working
    /// set and a local table, must leave the live cell and the store a kill
    /// would restore reading the same. (A hard reset once left its branch
    /// move in the connection until the next write.)
    #[test]
    fn an_acknowledged_operation_is_in_the_committed_prefix() {
        type Op = (&'static str, &'static str, Value);
        let ops: Vec<Op> = vec![
            ("commit", "commit", json!({"message": "three"})),
            ("branchCreate", "branchCreate", json!({"name": "new", "from": "HEAD~1"})),
            ("branchDelete", "branchDelete", json!({"name": "side"})),
            ("branchDelete force", "branchDelete", json!({"name": "side", "force": true})),
            ("checkout", "checkout", json!({"branch": "side"})),
            ("checkout create", "checkout", json!({"branch": "fresh", "create": true})),
            ("reset hard to a commit", "reset", json!({"hard": true, "to": "HEAD~1"})),
            ("reset hard", "reset", json!({"hard": true})),
            ("reset soft to a commit", "reset", json!({"to": "HEAD~1"})),
            ("reset soft", "reset", json!({})),
            ("merge fast-forward", "merge", json!({"branch": "side"})),
            ("gc", "gc", json!({})),
        ];
        for (name, op, args, in_transaction) in ops.into_iter().flat_map(|(name, op, args)| {
            [(name, op, args.clone(), false), (name, op, args, true)]
        }) {
            let name = &format!("{name} (commits made {})", if in_transaction { "in a transaction" } else { "outside one" });
            let dir = tempfile::tempdir().unwrap();
            let (mut store, c) = two_commits(dir.path(), in_transaction);
            // Everything before the operation is shipped, as it is by the
            // outputs that answered those requests.
            use celld_ltx::CellStore as _;
            store.sync().unwrap();
            if name.starts_with("merge fast-forward") {
                dolt(&c, "reset", json!({"hard": true}));
                dolt(&c, "branchCreate", json!({"name": "ahead", "from": "HEAD"}));
                dolt(&c, "checkout", json!({"branch": "ahead"}));
                c.execute("INSERT INTO t VALUES (9, 'nine')", []).unwrap();
                dolt(&c, "commit", json!({"message": "ahead"}));
                dolt(&c, "checkout", json!({"branch": "main"}));
            }
            let args = if name.starts_with("merge fast-forward") { json!({"branch": "ahead"}) } else { args };
            assert!(mutates(op), "{name}: {op} must count as a mutation");
            dolt(&c, op, args);
            let live = observed(&c);
            let kept = restored_from_captures(&mut store, dir.path());
            assert_eq!(observed(&kept), live, "{name}: what a kill right after it restores");
        }
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
