// Copyright 2026 the celld fork authors. Apache-2.0 license.

//! Dolt remotes between cells (F7; docs/fork/design.md section 3b.13,
//! docs/services/dolt-remotes.md).
//!
//! A cell names another as a **remote**, fetches its branches into
//! remote-tracking branches `remote/branch`, and pushes its own branches to it,
//! with nothing in between that could write another cell's file: the
//! receiver of a pack installs it through its own connection in its own turn,
//! so what lands is captured and replicated like any commit. The transfer
//! itself is DoltLite's own fetch over a pack (celld patch 0004,
//! `dolt_pack_build` / `dolt_pack_fetch`).
//!
//! Everything here runs on a cell's connection with the application
//! authorizer lifted (`storage::dolt_blob_op`), never from application SQL.
//!
//! # State, all in `_cf_*` tables (unversioned, replicated, identity-bound)
//!
//! - `_cf_DOLT_REMOTES(name, ref, token)`: the remotes this cell holds. `ref`
//!   is the routable id of the source, `Class:<id>`; `token` is the grant the
//!   source gave. A fork, restore or import clears it ([`strip`]): a new
//!   identity does not inherit its source's authority.
//! - `_cf_DOLT_GRANTS(id, label, token_hash, fetch, push, branches, peer,
//!   expires, created)`: what this cell lets others do. The token is shown
//!   once; only its SHA-256 is kept.
//!
//! # The wire
//!
//! Three requests (`refs`, `fetch`, `push`), one framing, no transport:
//!
//! ```text
//! request  [u8 version=1][u8 op][u32 n][json: n bytes][bytes...]
//! response [u8 status][u32 n][json: n bytes][bytes...]      status 0 = ok
//! ```
//!
//! [`serve`] answers one request for a cell. The runtime carries it to the
//! source's Durable Object as an internal request, which is how a fetch reaches
//! a cell on another node; an application can carry it any other way
//! (`remote.serve`, the P2P binding).

use anyhow::{anyhow, bail};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub(crate) const REMOTES_TABLE: &str = "_cf_DOLT_REMOTES";
pub(crate) const GRANTS_TABLE: &str = "_cf_DOLT_GRANTS";

const OP_REFS: u8 = 1;
const OP_FETCH: u8 = 2;
const OP_PUSH: u8 = 3;
const VERSION: u8 = 1;

/// Packs and requests are one buffer each; the engine's own limit is 2 GiB.
const MAX_MESSAGE: usize = 1 << 30;
const MAX_HAVES: usize = 200_000;

/// What an operation answers: JSON for the caller's script, or bytes.
#[derive(Debug)]
pub(crate) enum Output {
    Json(Value),
    Bytes(Vec<u8>),
}

/// A refusal the wire carries to the holder: a stable code and a message.
#[derive(Debug)]
pub(crate) struct Refusal {
    pub code: &'static str,
    pub message: String,
    pub extra: Value,
}

impl Refusal {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), extra: Value::Null }
    }

    fn with(mut self, extra: Value) -> Self {
        self.extra = extra;
        self
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self::new("ERR_DOLT_REMOTE_FORBIDDEN", message)
    }

    fn bad(message: impl Into<String>) -> Self {
        Self::new("ERR_DOLT_REMOTE_BAD_REQUEST", message)
    }
}

impl From<anyhow::Error> for Refusal {
    fn from(error: anyhow::Error) -> Self {
        Self::new("ERR_DOLT_REMOTE_FAILED", format!("{error:#}"))
    }
}

impl From<rusqlite::Error> for Refusal {
    fn from(error: rusqlite::Error) -> Self {
        Self::new("ERR_DOLT_REMOTE_FAILED", crate::dolt_cell::sqlite_message(&error))
    }
}

/// The error text a script sees for a refusal: `[CODE] message`, which the
/// harness turns into `error.code`.
fn tagged(refusal: &Refusal) -> String {
    format!("[{}] {}", refusal.code, refusal.message)
}

// ---- which operations are here, and what they do to the store ------------

pub(crate) fn handles(op: &str) -> bool {
    matches!(
        op,
        "remoteAdd"
            | "remoteRemove"
            | "remoteList"
            | "remoteBranches"
            | "remoteTarget"
            | "remoteRequest"
            | "remoteApply"
            | "remoteServe"
            | "grant"
            | "grants"
            | "revoke"
    )
}

fn arg_op(args: &Value) -> &str {
    args.get("op").and_then(Value::as_str).unwrap_or("")
}

/// Whether the operation can change the store, so that the next output waits
/// for a durability proof.
pub(crate) fn mutates(op: &str, args: &Value, input: &[u8]) -> bool {
    match op {
        "remoteAdd" | "remoteRemove" | "grant" | "revoke" => true,
        "remoteApply" => arg_op(args) == "fetch",
        "remoteServe" => request_op(input) == Some(OP_PUSH),
        _ => false,
    }
}

/// Whether the operation can change the schema this connection sees (a push
/// to the checked-out branch resets it), so cached statements must go.
pub(crate) fn changes_schema(op: &str, input: &[u8]) -> bool {
    op == "remoteServe" && request_op(input) == Some(OP_PUSH)
}

pub(crate) fn run(
    connection: &Connection,
    op: &str,
    args: &Value,
    input: &[u8],
) -> anyhow::Result<Output> {
    match op {
        "remoteAdd" => remote_add(connection, args).map(Output::Json),
        "remoteRemove" => remote_remove(connection, args).map(Output::Json),
        "remoteList" => remote_list(connection).map(Output::Json),
        "remoteBranches" => remote_branches(connection).map(Output::Json),
        "remoteTarget" => remote_target(connection, args).map(Output::Json),
        "remoteRequest" => request(connection, args, input).map(Output::Bytes),
        "remoteApply" => apply(connection, args, input).map(Output::Json),
        "remoteServe" => Ok(Output::Bytes(serve(connection, input, args.get("peer").and_then(Value::as_str)))),
        "grant" => grant(connection, args).map(Output::Json),
        "grants" => grants(connection).map(Output::Json),
        "revoke" => revoke(connection, args).map(Output::Json),
        other => bail!("unknown dolt remote operation {other:?}"),
    }
}

// ---- tables ----------------------------------------------------------------

fn ensure_tables(connection: &Connection) -> anyhow::Result<()> {
    connection.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {REMOTES_TABLE} \
           (name TEXT PRIMARY KEY, ref TEXT NOT NULL, token TEXT NOT NULL) WITHOUT ROWID; \
         CREATE TABLE IF NOT EXISTS {GRANTS_TABLE} \
           (id TEXT PRIMARY KEY, label TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE, \
            fetch INTEGER NOT NULL, push TEXT NOT NULL, branches TEXT NOT NULL, \
            peer TEXT, expires INTEGER, created INTEGER NOT NULL) WITHOUT ROWID;"
    ))?;
    Ok(())
}

/// A fork, restore or import is a new identity: it holds no remote and has
/// granted nothing, and the tracking branches it copied name remotes it never
/// had. Called on the image before it is published.
pub(crate) fn strip(connection: &Connection) -> anyhow::Result<()> {
    let remotes = crate::dolt_cell::rows(connection, "SELECT name FROM dolt_remotes", &[])
        .unwrap_or_default();
    for remote in remotes {
        if let Some(name) = remote["name"].as_str() {
            crate::dolt_cell::call(connection, "dolt_remote", &["remove".to_string(), name.to_string()])?;
        }
    }
    for table in [REMOTES_TABLE, GRANTS_TABLE] {
        if crate::dolt_cell::table_exists(connection, table)? {
            connection.execute(&format!("DELETE FROM {table}"), [])?;
        }
    }
    Ok(())
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn hex_text(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha256_hex(text: &str) -> String {
    hex_text(&Sha256::digest(text.as_bytes()))
}

// ---- validation ------------------------------------------------------------

/// A remote's name: the engine's own rule allows more, ours is what a script
/// can type without surprises.
fn remote_name(value: &Value) -> anyhow::Result<String> {
    let name = value.as_str().ok_or_else(|| anyhow!("remote name must be a string"))?;
    anyhow::ensure!(
        !name.is_empty()
            && name.len() <= 64
            && name.starts_with(|c: char| c.is_ascii_alphabetic())
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "remote name {name:?} must be a letter followed by up to 63 letters, digits, `_` or `-`"
    );
    Ok(name.to_string())
}

/// The routable id of a source: `Class:<id>` (a `DurableObjectId`'s scope).
fn cell_ref(value: &Value) -> anyhow::Result<String> {
    let text = value.as_str().ok_or_else(|| anyhow!("remote ref must be a string"))?;
    let (class, id) = text
        .split_once(':')
        .ok_or_else(|| anyhow!("remote ref {text:?} must look like \"Class:<id>\""))?;
    anyhow::ensure!(
        !class.is_empty()
            && class.len() <= 128
            && class.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$'))
            && !id.is_empty()
            && id.len() <= 512
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "remote ref {text:?} must look like \"Class:<id>\""
    );
    Ok(text.to_string())
}

/// A branch pattern: a branch name where `*` stands for any run of
/// characters.
fn valid_pattern(pattern: &str) -> bool {
    !pattern.is_empty()
        && pattern.len() <= 200
        && !pattern.starts_with('-')
        && !pattern.contains("..")
        && pattern
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/' | b'*'))
}

fn patterns(value: Option<&Value>, what: &str) -> anyhow::Result<Vec<String>> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };
    let list = value.as_array().ok_or_else(|| anyhow!("{what} must be an array of branch patterns"))?;
    let mut out = Vec::new();
    for item in list {
        let pattern = item.as_str().ok_or_else(|| anyhow!("{what} must be an array of branch patterns"))?;
        anyhow::ensure!(valid_pattern(pattern), "{what}: {pattern:?} is not a valid branch pattern");
        out.push(pattern.to_string());
    }
    anyhow::ensure!(out.len() <= 256, "{what} lists too many patterns");
    Ok(out)
}

/// `*` matches any run of characters; nothing else is special.
pub(crate) fn pattern_matches(pattern: &str, name: &str) -> bool {
    fn go(pattern: &[u8], name: &[u8]) -> bool {
        match pattern.split_first() {
            None => name.is_empty(),
            Some((b'*', rest)) => (0..=name.len()).any(|skip| go(rest, &name[skip..])),
            Some((byte, rest)) => name.first() == Some(byte) && go(rest, &name[1..]),
        }
    }
    go(pattern.as_bytes(), name.as_bytes())
}

fn any_matches(patterns: &[String], name: &str) -> bool {
    patterns.iter().any(|pattern| pattern_matches(pattern, name))
}

fn hash_text(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

// ---- remotes: the holder's side ---------------------------------------------

fn remote_add(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    let name = remote_name(&args["name"])?;
    let target = cell_ref(&args["ref"])?;
    let token = args["token"]
        .as_str()
        .filter(|token| !token.is_empty() && token.len() <= 512)
        .ok_or_else(|| anyhow!("remote needs the token of a grant its source gave"))?;
    ensure_tables(connection)?;
    let exists: bool = connection
        .query_row(&format!("SELECT 1 FROM {REMOTES_TABLE} WHERE name = ?1"), [&name], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    anyhow::ensure!(!exists, "remote {name:?} already exists");
    crate::dolt_cell::call(
        connection,
        "dolt_remote",
        &["add".to_string(), name.clone(), format!("celld:{target}")],
    )?;
    if let Err(error) = connection.execute(
        &format!("INSERT INTO {REMOTES_TABLE}(name, ref, token) VALUES (?1, ?2, ?3)"),
        params![name, target, token],
    ) {
        let _ = crate::dolt_cell::call(connection, "dolt_remote", &["remove".to_string(), name]);
        return Err(error.into());
    }
    Ok(json!({ "name": name, "ref": target }))
}

fn remote_remove(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    let name = remote_name(&args["name"])?;
    ensure_tables(connection)?;
    let removed = connection.execute(&format!("DELETE FROM {REMOTES_TABLE} WHERE name = ?1"), [&name])?;
    anyhow::ensure!(removed == 1, "no remote named {name:?}");
    // The engine drops the tracking branches with the remote.
    crate::dolt_cell::call(connection, "dolt_remote", &["remove".to_string(), name.clone()])?;
    Ok(json!({ "name": name }))
}

fn remote_list(connection: &Connection) -> anyhow::Result<Value> {
    ensure_tables(connection)?;
    let list = crate::dolt_cell::rows(
        connection,
        &format!("SELECT name, ref FROM {REMOTES_TABLE} ORDER BY name"),
        &[],
    )?;
    Ok(Value::Array(list))
}

/// The remote-tracking branches, `remote/branch` and the commit each is at:
/// what the last fetches learned. Read them with `merge`, `diff` and
/// `dolt_at_<table>('remote/branch')`.
fn remote_branches(connection: &Connection) -> anyhow::Result<Value> {
    let list = crate::dolt_cell::rows(
        connection,
        "SELECT name, hash FROM dolt_remote_branches ORDER BY name",
        &[],
    )?;
    Ok(Value::Array(
        list.into_iter()
            .map(|row| {
                let name = row["name"].as_str().unwrap_or("");
                json!({ "name": name.strip_prefix("remotes/").unwrap_or(name), "hash": row["hash"] })
            })
            .collect(),
    ))
}

struct Remote {
    name: String,
    target: String,
    token: String,
}

fn find_remote(connection: &Connection, name: &Value) -> anyhow::Result<Remote> {
    let name = remote_name(name)?;
    ensure_tables(connection)?;
    connection
        .query_row(
            &format!("SELECT ref, token FROM {REMOTES_TABLE} WHERE name = ?1"),
            [&name],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .map(|(target, token)| Remote { name: name.clone(), target, token })
        .ok_or_else(|| anyhow!("no remote named {name:?}; add one with dolt.remote.add"))
}

/// Where a remote's source is: the ref the harness routes to. The token never
/// leaves the store except inside a request.
fn remote_target(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    let remote = find_remote(connection, &args["name"])?;
    Ok(json!({ "name": remote.name, "ref": remote.target }))
}

// ---- grants: the source's side ------------------------------------------------

struct Grant {
    id: String,
    fetch: bool,
    push: Vec<String>,
    branches: Vec<String>,
}

fn grant(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    ensure_tables(connection)?;
    let label = match args.get("label").filter(|value| !value.is_null()) {
        None => String::new(),
        Some(Value::String(label)) if label.len() <= 128 => label.clone(),
        Some(_) => bail!("grant label must be a string of at most 128 characters"),
    };
    let fetch = args.get("fetch").and_then(Value::as_bool).unwrap_or(false);
    let push = patterns(args.get("push"), "grant push")?;
    let mut branches = patterns(args.get("branches"), "grant branches")?;
    anyhow::ensure!(fetch || !push.is_empty(), "a grant must allow fetch, push, or both");
    if branches.is_empty() {
        branches.push("*".to_string());
    }
    let peer = match args.get("peer").filter(|value| !value.is_null()) {
        None => None,
        Some(Value::String(peer)) if !peer.is_empty() && peer.len() <= 256 => Some(peer.clone()),
        Some(_) => bail!("grant peer must be a non-empty string"),
    };
    let expires = match args.get("expiresAt").filter(|value| !value.is_null()) {
        None => None,
        Some(value) => Some(value.as_i64().ok_or_else(|| anyhow!("grant expiresAt must be epoch milliseconds"))?),
    };
    let mut raw_id = [0u8; 8];
    getrandom::fill(&mut raw_id).map_err(|error| anyhow!("random grant id: {error}"))?;
    let mut raw_token = [0u8; 32];
    getrandom::fill(&mut raw_token).map_err(|error| anyhow!("random grant token: {error}"))?;
    let id = hex_text(&raw_id);
    use base64::Engine as _;
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw_token);
    connection.execute(
        &format!(
            "INSERT INTO {GRANTS_TABLE}(id, label, token_hash, fetch, push, branches, peer, expires, created) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        ),
        params![
            id,
            label,
            sha256_hex(&token),
            fetch as i64,
            serde_json::to_string(&push)?,
            serde_json::to_string(&branches)?,
            peer,
            expires,
            now_ms()
        ],
    )?;
    Ok(json!({
        "id": id, "token": token, "label": label, "fetch": fetch,
        "push": push, "branches": branches, "peer": peer, "expiresAt": expires,
    }))
}

fn grants(connection: &Connection) -> anyhow::Result<Value> {
    ensure_tables(connection)?;
    let mut statement = connection.prepare(&format!(
        "SELECT id, label, fetch, push, branches, peer, expires, created FROM {GRANTS_TABLE} ORDER BY created, id"
    ))?;
    let list = statement
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "label": row.get::<_, String>(1)?,
                "fetch": row.get::<_, i64>(2)? != 0,
                "push": serde_json::from_str::<Value>(&row.get::<_, String>(3)?).unwrap_or(Value::Null),
                "branches": serde_json::from_str::<Value>(&row.get::<_, String>(4)?).unwrap_or(Value::Null),
                "peer": row.get::<_, Option<String>>(5)?,
                "expiresAt": row.get::<_, Option<i64>>(6)?,
                "created": row.get::<_, i64>(7)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(list))
}

fn revoke(connection: &Connection, args: &Value) -> anyhow::Result<Value> {
    ensure_tables(connection)?;
    let id = args["id"].as_str().ok_or_else(|| anyhow!("revoke needs a grant id"))?;
    let removed = connection.execute(&format!("DELETE FROM {GRANTS_TABLE} WHERE id = ?1"), [id])?;
    anyhow::ensure!(removed == 1, "no grant {id:?}");
    // What the holder pushed stays in the history it pushed; its tracking
    // ref is only bookkeeping, and goes with its remote entry.
    let remote = pushed_remote(id);
    let known = crate::dolt_cell::rows(connection, "SELECT name FROM dolt_remotes", &[])?
        .iter()
        .any(|row| row["name"].as_str() == Some(remote.as_str()));
    if known {
        crate::dolt_cell::call(connection, "dolt_remote", &["remove".to_string(), remote])?;
    }
    Ok(json!({ "id": id }))
}

fn pushed_remote(grant_id: &str) -> String {
    format!("pushed-{grant_id}")
}

// ---- framing -----------------------------------------------------------------

fn frame_request(op: u8, json: &Value, bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let json = serde_json::to_vec(json)?;
    anyhow::ensure!(json.len() + bytes.len() < MAX_MESSAGE, "the request is too large");
    let mut out = Vec::with_capacity(6 + json.len() + bytes.len());
    out.push(VERSION);
    out.push(op);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(bytes);
    Ok(out)
}

fn parse_request(request: &[u8]) -> Result<(u8, Value, &[u8]), Refusal> {
    if request.len() < 6 || request[0] != VERSION {
        return Err(Refusal::bad("not a Dolt remote request (version 1)"));
    }
    let length = u32::from_le_bytes([request[2], request[3], request[4], request[5]]) as usize;
    let body = &request[6..];
    if length > body.len() {
        return Err(Refusal::bad("the request is truncated"));
    }
    let json: Value = serde_json::from_slice(&body[..length]).map_err(|error| Refusal::bad(format!("the request: {error}")))?;
    Ok((request[1], json, &body[length..]))
}

fn request_op(request: &[u8]) -> Option<u8> {
    (request.len() >= 6 && request[0] == VERSION).then(|| request[1])
}

fn frame_response(status: u8, json: &Value, bytes: &[u8]) -> Vec<u8> {
    let json = serde_json::to_vec(json).unwrap_or_else(|_| b"null".to_vec());
    let mut out = Vec::with_capacity(5 + json.len() + bytes.len());
    out.push(status);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(bytes);
    out
}

/// A response as its status, JSON and bytes.
fn parse_response(response: &[u8]) -> Result<(u8, Value, &[u8]), Refusal> {
    if response.len() < 5 {
        return Err(Refusal::new("ERR_DOLT_REMOTE_FAILED", "the response is not a Dolt remote response"));
    }
    let length = u32::from_le_bytes([response[1], response[2], response[3], response[4]]) as usize;
    let body = &response[5..];
    if length > body.len() {
        return Err(Refusal::new("ERR_DOLT_REMOTE_FAILED", "the response is truncated"));
    }
    let json: Value = serde_json::from_slice(&body[..length])
        .map_err(|error| Refusal::new("ERR_DOLT_REMOTE_FAILED", format!("the response: {error}")))?;
    Ok((response[0], json, &body[length..]))
}

/// A response that carries a refusal back to the holder.
fn response_error(json: &Value) -> Refusal {
    let code = match json["code"].as_str().unwrap_or("") {
        "ERR_DOLT_REMOTE_FORBIDDEN" => "ERR_DOLT_REMOTE_FORBIDDEN",
        "ERR_DOLT_REMOTE_BAD_REQUEST" => "ERR_DOLT_REMOTE_BAD_REQUEST",
        "ERR_DOLT_REMOTE_NOT_FOUND" => "ERR_DOLT_REMOTE_NOT_FOUND",
        "ERR_DOLT_NON_FAST_FORWARD" => "ERR_DOLT_NON_FAST_FORWARD",
        "ERR_DOLT_TARGET_DIRTY" => "ERR_DOLT_TARGET_DIRTY",
        _ => "ERR_DOLT_REMOTE_FAILED",
    };
    Refusal::new(code, json["message"].as_str().unwrap_or("the remote refused the request"))
        .with(json["extra"].clone())
}

// ---- the holder's half of a request -------------------------------------------

fn lines(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect()
}

fn local_haves(connection: &Connection) -> anyhow::Result<Vec<String>> {
    let text: String = connection.query_row("SELECT dolt_pack_haves()", [], |row| row.get(0))?;
    Ok(lines(&text))
}

/// Build the request for `args.op` (`refs`, `fetch` or `push`) to the remote
/// `args.name`. A push also takes, as `input`, the remote's answer to `refs`:
/// the pack is built against the tips it names.
fn request(connection: &Connection, args: &Value, input: &[u8]) -> anyhow::Result<Vec<u8>> {
    let remote = find_remote(connection, &args["name"])?;
    match arg_op(args) {
        "refs" => frame_request(OP_REFS, &json!({ "token": remote.token }), &[]),
        "fetch" => {
            let requested = patterns(args.get("branches"), "fetch branches")?;
            frame_request(
                OP_FETCH,
                &json!({ "token": remote.token, "haves": local_haves(connection)?, "branches": requested }),
                &[],
            )
        }
        "push" => {
            let from = crate::dolt_cell::branch_name(&args["branch"], "branch")?;
            let to = match args.get("to").filter(|value| !value.is_null()) {
                Some(to) => crate::dolt_cell::branch_name(to, "to")?,
                None => from.clone(),
            };
            let (status, refs, _) = parse_response(input).map_err(|error| anyhow!(tagged(&error)))?;
            if status != 0 {
                bail!("{}", tagged(&response_error(&refs)));
            }
            let mut haves: Vec<String> = Vec::new();
            let mut expect = Value::Null;
            if let Some(tips) = refs["branches"].as_object() {
                for (name, hash) in tips {
                    if let Some(hash) = hash.as_str().filter(|hash| hash_text(hash)) {
                        haves.push(hash.to_string());
                        if *name == to {
                            expect = Value::String(hash.to_string());
                        }
                    }
                }
            }
            // What this cell already learned of the remote is on the remote.
            let tracked = crate::dolt_cell::rows(
                connection,
                "SELECT hash FROM dolt_remote_branches WHERE name LIKE ?1 ESCAPE '\\'",
                &[format!("remotes/{}/%", remote.name.replace('_', "\\_"))],
            )?;
            haves.extend(tracked.iter().filter_map(|row| row["hash"].as_str().map(str::to_string)));
            let tip: String = connection
                .query_row("SELECT hash FROM dolt_branches WHERE name = ?1", [&from], |row| row.get(0))
                .optional()?
                .ok_or_else(|| anyhow!("no branch {from:?} to push"))?;
            let spec = if from == to { from.clone() } else { format!("{from}={to}") };
            let pack: Vec<u8> = connection
                .query_row("SELECT dolt_pack_build(?1, ?2)", params![spec, haves.join("\n")], |row| row.get(0))
                .map_err(|error| anyhow!("{}", crate::dolt_cell::sqlite_message(&error)))?;
            frame_request(
                OP_PUSH,
                &json!({ "token": remote.token, "branch": to, "tip": tip, "expect": expect }),
                &pack,
            )
        }
        other => bail!("unknown remote request {other:?} (expected refs, fetch or push)"),
    }
}

/// Apply the remote's `input` response to the request `args.op` made for
/// `args.name`: install a fetched pack as tracking branches, or report what a
/// refs or push answered. A refusal is an error carrying its code.
fn apply(connection: &Connection, args: &Value, input: &[u8]) -> anyhow::Result<Value> {
    let remote = find_remote(connection, &args["name"])?;
    let (status, json, bytes) = parse_response(input).map_err(|error| anyhow!(tagged(&error)))?;
    if status != 0 {
        bail!("{}", tagged(&response_error(&json)));
    }
    match arg_op(args) {
        "refs" => Ok(json),
        "push" => {
            // What the target tells its own class is not for the pusher.
            let mut json = json;
            if let Some(object) = json.as_object_mut() {
                object.remove("accepted");
            }
            Ok(json)
        }
        "fetch" => {
            let text: String = connection
                .query_row("SELECT dolt_pack_fetch(?1, ?2)", params![remote.name, bytes], |row| row.get(0))
                .map_err(|error| {
                    anyhow!(
                        "{}",
                        tagged(&Refusal::new("ERR_DOLT_REMOTE_FAILED", crate::dolt_cell::sqlite_message(&error)))
                    )
                })?;
            let mut branches = serde_json::Map::new();
            for line in lines(&text) {
                if let Some((name, hash)) = line.split_once(' ') {
                    branches.insert(name.to_string(), Value::String(hash.to_string()));
                }
            }
            Ok(json!({ "remote": remote.name, "branches": branches, "bytes": bytes.len() }))
        }
        other => bail!("unknown remote request {other:?}"),
    }
}

// ---- the source's half: one request in, one response out -----------------------

/// Answer one request for this cell. Never fails: a refusal is a response.
pub(crate) fn serve(connection: &Connection, request: &[u8], peer: Option<&str>) -> Vec<u8> {
    match serve_inner(connection, request, peer) {
        Ok((json, bytes)) => frame_response(0, &json, &bytes),
        Err(refusal) => frame_response(
            1,
            &json!({ "code": refusal.code, "message": refusal.message, "extra": refusal.extra }),
            &[],
        ),
    }
}

fn serve_inner(
    connection: &Connection,
    request: &[u8],
    peer: Option<&str>,
) -> Result<(Value, Vec<u8>), Refusal> {
    let (op, json, bytes) = parse_request(request)?;
    let grant = authenticate(connection, &json, peer)?;
    match op {
        OP_REFS => serve_refs(connection, &grant),
        OP_FETCH => serve_fetch(connection, &grant, &json),
        OP_PUSH => serve_push(connection, &grant, &json, bytes),
        other => Err(Refusal::bad(format!("unknown request {other}"))),
    }
}

fn authenticate(connection: &Connection, json: &Value, peer: Option<&str>) -> Result<Grant, Refusal> {
    let token = json["token"].as_str().filter(|token| !token.is_empty() && token.len() <= 512);
    let Some(token) = token else {
        return Err(Refusal::forbidden("the request carries no grant token"));
    };
    if !crate::dolt_cell::table_exists(connection, GRANTS_TABLE)? {
        return Err(Refusal::forbidden("this cell has granted nothing"));
    }
    let row = connection
        .query_row(
            &format!("SELECT id, fetch, push, branches, peer, expires FROM {GRANTS_TABLE} WHERE token_hash = ?1"),
            [sha256_hex(token)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((id, fetch, push, branches, grant_peer, expires)) = row else {
        return Err(Refusal::forbidden("the grant token is not (or no longer) valid for this cell"));
    };
    if expires.is_some_and(|expires| expires <= now_ms()) {
        return Err(Refusal::forbidden("the grant has expired"));
    }
    if let Some(required) = grant_peer {
        if peer != Some(required.as_str()) {
            return Err(Refusal::forbidden("the grant is bound to a peer this request did not come from"));
        }
    }
    Ok(Grant {
        id,
        fetch: fetch != 0,
        push: serde_json::from_str(&push).unwrap_or_default(),
        branches: serde_json::from_str(&branches).unwrap_or_default(),
    })
}

/// The committed branches of this cell: name and tip.
fn branch_tips(connection: &Connection) -> Result<Vec<(String, String)>, Refusal> {
    let mut statement = connection.prepare("SELECT name, hash FROM dolt_branches ORDER BY name")?;
    let tips = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tips)
}

fn serve_refs(connection: &Connection, grant: &Grant) -> Result<(Value, Vec<u8>), Refusal> {
    let mut visible = serde_json::Map::new();
    for (name, hash) in branch_tips(connection)? {
        let fetchable = grant.fetch && any_matches(&grant.branches, &name);
        if fetchable || any_matches(&grant.push, &name) {
            visible.insert(name, Value::String(hash));
        }
    }
    Ok((json!({ "branches": visible, "fetch": grant.fetch, "push": grant.push }), Vec::new()))
}

fn serve_fetch(connection: &Connection, grant: &Grant, request: &Value) -> Result<(Value, Vec<u8>), Refusal> {
    if !grant.fetch {
        return Err(Refusal::forbidden("the grant does not allow fetch"));
    }
    let requested = patterns(request.get("branches"), "fetch branches").map_err(|error| Refusal::bad(error.to_string()))?;
    let mut haves = Vec::new();
    if let Some(list) = request["haves"].as_array() {
        if list.len() > MAX_HAVES {
            return Err(Refusal::bad("too many haves"));
        }
        for have in list {
            let have = have.as_str().filter(|have| hash_text(have)).ok_or_else(|| Refusal::bad("a have is not a commit hash"))?;
            haves.push(have.to_string());
        }
    }
    let mut chosen = Vec::new();
    let mut revealed = serde_json::Map::new();
    for (name, hash) in branch_tips(connection)? {
        if any_matches(&grant.branches, &name) && (requested.is_empty() || any_matches(&requested, &name)) {
            revealed.insert(name.clone(), Value::String(hash));
            chosen.push(name);
        }
    }
    // A branch asked for by name that this grant does not show is not found,
    // however it came to be hidden.
    for pattern in requested.iter().filter(|pattern| !pattern.contains('*')) {
        if !chosen.iter().any(|name| name == pattern) {
            return Err(Refusal::new(
                "ERR_DOLT_REMOTE_NOT_FOUND",
                format!("no branch {pattern:?} on the remote (or this grant does not show it)"),
            ));
        }
    }
    let pack: Vec<u8> = connection.query_row(
        "SELECT dolt_pack_build(?1, ?2)",
        params![chosen.join("\n"), haves.join("\n")],
        |row| row.get(0),
    )?;
    if pack.len() > MAX_MESSAGE {
        return Err(Refusal::new("ERR_DOLT_REMOTE_FAILED", "the pack is too large for one message"));
    }
    Ok((json!({ "branches": revealed, "grant": grant.id }), pack))
}

fn serve_push(
    connection: &Connection,
    grant: &Grant,
    request: &Value,
    pack: &[u8],
) -> Result<(Value, Vec<u8>), Refusal> {
    let branch = request["branch"].as_str().ok_or_else(|| Refusal::bad("a push names its branch"))?.to_string();
    crate::dolt_cell::branch_name(&Value::String(branch.clone()), "branch").map_err(|error| Refusal::bad(error.to_string()))?;
    let tip = request["tip"].as_str().filter(|tip| hash_text(tip)).ok_or_else(|| Refusal::bad("a push names its tip"))?.to_string();
    if !any_matches(&grant.push, &branch) {
        return Err(Refusal::forbidden(format!("the grant does not allow pushing to {branch:?}")));
    }
    // The fetch lands the pack as `pushed-<grant>/<branch>`, the engine's own
    // checks included: every chunk hashed, the whole graph present.
    let remote = pushed_remote(&grant.id);
    let known = crate::dolt_cell::rows(connection, "SELECT name FROM dolt_remotes", &[])?
        .iter()
        .any(|row| row["name"].as_str() == Some(remote.as_str()));
    if !known {
        crate::dolt_cell::call(
            connection,
            "dolt_remote",
            &["add".to_string(), remote.clone(), "celld:pushed".to_string()],
        )?;
    }
    let fetched: String = connection
        .query_row("SELECT dolt_pack_fetch(?1, ?2)", params![remote, pack], |row| row.get(0))
        .map_err(|error| Refusal::new("ERR_DOLT_REMOTE_FAILED", crate::dolt_cell::sqlite_message(&error)))?;
    let landed = lines(&fetched)
        .into_iter()
        .filter_map(|line| line.split_once(' ').map(|(name, hash)| (name.to_string(), hash.to_string())))
        .find(|(name, _)| *name == branch)
        .map(|(_, hash)| hash);
    if landed.as_deref() != Some(tip.as_str()) {
        return Err(Refusal::bad("the pack does not carry the pushed branch at the tip it names"));
    }
    let current = branch_tips(connection)?.into_iter().find(|(name, _)| *name == branch).map(|(_, hash)| hash);
    if current.as_deref() == Some(tip.as_str()) {
        return Ok((json!({ "branch": branch, "hash": tip, "created": false, "unchanged": true }), Vec::new()));
    }
    if let Some(current) = &current {
        let base: String = connection.query_row(
            "SELECT dolt_merge_base(?1, ?2)",
            params![current, tip],
            |row| row.get(0),
        )?;
        if base != *current {
            return Err(Refusal::new(
                "ERR_DOLT_NON_FAST_FORWARD",
                format!("{branch:?} is at {current}, which {tip} does not contain: fetch, merge and push again"),
            )
            .with(json!({ "current": current })));
        }
    }
    let active = crate::dolt_cell::active_branch(connection)?;
    if branch == active {
        // The checked-out branch moves under the application only when it has
        // nothing uncommitted to lose.
        let dirty = crate::dolt_cell::rows(
            connection,
            "SELECT table_name AS \"table\" FROM dolt_status WHERE table_name NOT LIKE '\\_cf\\_%' ESCAPE '\\'",
            &[],
        )?;
        if !dirty.is_empty() {
            return Err(Refusal::new(
                "ERR_DOLT_TARGET_DIRTY",
                format!("{branch:?} is checked out and has uncommitted changes; commit or reset them first"),
            ));
        }
        crate::dolt_cell::call(connection, "dolt_reset", &["--hard".to_string(), tip.clone()])?;
    } else if current.is_some() {
        crate::dolt_cell::call(connection, "dolt_branch", &["-f".to_string(), branch.clone(), tip.clone()])?;
    } else {
        crate::dolt_cell::call(connection, "dolt_branch", &[branch.clone(), tip.clone()])?;
    }
    // `accepted` is for the target's own runtime, which tells the class
    // (`ondoltpush`); the pusher's `apply` drops it.
    Ok((
        json!({
            "branch": branch, "hash": tip, "created": current.is_none(), "unchanged": false,
            "accepted": {
                "remote": remote, "grant": grant.id, "branch": branch,
                "from": current, "to": tip, "created": current.is_none(),
            },
        }),
        Vec::new(),
    ))
}

#[cfg(test)]
// The tests copy a cell's file to make a fork of it.
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    fn cell(dir: &std::path::Path, name: &str) -> Connection {
        let path = dir.join(name).to_string_lossy().into_owned();
        let connection = crate::dolt_cell::open_connection(&path, celld_ltx::Engine::Dolt, None).unwrap();
        crate::dolt_cell::attach_active_branch(&connection).unwrap();
        crate::storage::schema(&connection).unwrap();
        crate::dolt_cell::ensure_runtime(&connection, &[]).unwrap();
        connection
    }

    fn commit(connection: &Connection, message: &str) -> String {
        let hash = crate::dolt_cell::run(
            connection,
            "commit",
            &json!({ "message": message, "author": "T <t@example.com>", "date": 1_700_000_000_000_i64 }),
        )
        .unwrap();
        hash.as_str().unwrap().to_string()
    }

    /// A source with one committed note, and a fork of it (a copy of its file).
    fn pair() -> (tempfile::TempDir, Connection, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let source = cell(dir.path(), "source.db");
        source
            .execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT); INSERT INTO notes VALUES (1, 'one');")
            .unwrap();
        commit(&source, "base");
        drop(source);
        std::fs::copy(dir.path().join("source.db"), dir.path().join("holder.db")).unwrap();
        let source = cell(dir.path(), "source.db");
        let holder = cell(dir.path(), "holder.db");
        (dir, source, holder)
    }

    fn json_of(output: Output) -> Value {
        match output {
            Output::Json(value) => value,
            Output::Bytes(_) => panic!("expected JSON"),
        }
    }

    fn bytes_of(output: Output) -> Vec<u8> {
        match output {
            Output::Bytes(bytes) => bytes,
            Output::Json(value) => panic!("expected bytes, got {value}"),
        }
    }

    fn grant(source: &Connection, args: Value) -> Value {
        json_of(run(source, "grant", &args, &[]).unwrap())
    }

    fn add_remote(holder: &Connection, token: &str) {
        run(holder, "remoteAdd", &json!({ "name": "origin", "ref": "Graph:abc123", "token": token }), &[]).unwrap();
    }

    /// One request from the holder, answered by the source: the response.
    fn exchange(holder: &Connection, source: &Connection, args: Value, input: &[u8]) -> Vec<u8> {
        let request = bytes_of(run(holder, "remoteRequest", &args, input).unwrap());
        bytes_of(run(source, "remoteServe", &json!({}), &request).unwrap())
    }

    fn fetch(holder: &Connection, source: &Connection) -> anyhow::Result<Value> {
        let response = exchange(holder, source, json!({ "name": "origin", "op": "fetch" }), &[]);
        run(holder, "remoteApply", &json!({ "name": "origin", "op": "fetch" }), &response).map(json_of)
    }

    fn push(holder: &Connection, source: &Connection, branch: &str, to: Option<&str>) -> anyhow::Result<Value> {
        let refs = exchange(holder, source, json!({ "name": "origin", "op": "refs" }), &[]);
        let response = exchange(
            holder,
            source,
            json!({ "name": "origin", "op": "push", "branch": branch, "to": to }),
            &refs,
        );
        run(holder, "remoteApply", &json!({ "name": "origin", "op": "push" }), &response).map(json_of)
    }

    fn head(connection: &Connection, rev: &str) -> String {
        crate::dolt_cell::hash_of(connection, rev).unwrap()
    }

    #[test]
    fn patterns_match_whole_names_with_star_as_the_only_wildcard() {
        for (pattern, name, expected) in [
            ("main", "main", true),
            ("main", "main2", false),
            ("*", "anything/at/all", true),
            ("ctx/*", "ctx/a/b", true),
            ("ctx/*", "ctx", false),
            ("a*c", "abbbc", true),
            ("a*c", "abbbd", false),
            ("*x*", "ax", true),
            ("m.in", "main", false),
        ] {
            assert_eq!(pattern_matches(pattern, name), expected, "{pattern} vs {name}");
        }
    }

    #[test]
    fn fetch_brings_tracking_branches_and_changes_nothing_else() {
        let (_dir, source, holder) = pair();
        let token = grant(&source, json!({ "fetch": true }))["token"].as_str().unwrap().to_string();
        add_remote(&holder, &token);
        source.execute("INSERT INTO notes VALUES (2, 'two')", []).unwrap();
        let tip = commit(&source, "two");
        let before = head(&holder, "HEAD");

        let fetched = fetch(&holder, &source).unwrap();
        assert_eq!(fetched["branches"]["main"], json!(tip));
        assert_eq!(head(&holder, "origin/main"), tip);
        assert_eq!(head(&holder, "HEAD"), before, "a fetch moves no branch");
        let rows: i64 = holder.query_row("SELECT count(*) FROM notes", [], |row| row.get(0)).unwrap();
        assert_eq!(rows, 1, "a fetch changes no working data");
        let tracked = json_of(run(&holder, "remoteBranches", &json!({}), &[]).unwrap());
        assert_eq!(tracked, json!([{ "name": "origin/main", "hash": tip }]));
        // Fetching again finds nothing new.
        assert_eq!(fetch(&holder, &source).unwrap()["branches"]["main"], json!(tip));
    }

    #[test]
    fn a_token_is_the_only_authority_and_expires_and_revokes() {
        let (_dir, source, holder) = pair();
        add_remote(&holder, "made-up");
        let error = format!("{:#}", fetch(&holder, &source).unwrap_err());
        assert!(error.contains("[ERR_DOLT_REMOTE_FORBIDDEN]"), "{error}");

        // A grant that never allowed fetch.
        let push_only = grant(&source, json!({ "push": ["main"] }));
        run(&holder, "remoteRemove", &json!({ "name": "origin" }), &[]).unwrap();
        add_remote(&holder, push_only["token"].as_str().unwrap());
        let error = format!("{:#}", fetch(&holder, &source).unwrap_err());
        assert!(error.contains("does not allow fetch"), "{error}");

        // Expired, then revoked.
        let expired = grant(&source, json!({ "fetch": true, "expiresAt": 1 }));
        run(&holder, "remoteRemove", &json!({ "name": "origin" }), &[]).unwrap();
        add_remote(&holder, expired["token"].as_str().unwrap());
        assert!(format!("{:#}", fetch(&holder, &source).unwrap_err()).contains("expired"));
        let live = grant(&source, json!({ "fetch": true }));
        run(&holder, "remoteRemove", &json!({ "name": "origin" }), &[]).unwrap();
        add_remote(&holder, live["token"].as_str().unwrap());
        fetch(&holder, &source).unwrap();
        run(&source, "revoke", &json!({ "id": live["id"] }), &[]).unwrap();
        assert!(format!("{:#}", fetch(&holder, &source).unwrap_err()).contains("[ERR_DOLT_REMOTE_FORBIDDEN]"));

        // Only the hash of a token is stored, and a listing shows no token.
        let stored: Vec<String> = source
            .prepare(&format!("SELECT token_hash FROM {GRANTS_TABLE}"))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        for grant in [&push_only, &expired, &live] {
            let token = grant["token"].as_str().unwrap();
            assert!(!stored.iter().any(|stored| stored == token));
        }
        let listed = json_of(run(&source, "grants", &json!({}), &[]).unwrap()).to_string();
        for grant in [&push_only, &expired] {
            assert!(!listed.contains(grant["token"].as_str().unwrap()), "{listed}");
        }
    }

    #[test]
    fn a_grant_bound_to_a_peer_refuses_everyone_else() {
        let (_dir, source, holder) = pair();
        let bound = grant(&source, json!({ "fetch": true, "peer": "node-1" }));
        add_remote(&holder, bound["token"].as_str().unwrap());
        let request = bytes_of(run(&holder, "remoteRequest", &json!({ "name": "origin", "op": "refs" }), &[]).unwrap());
        for peer in [None, Some("node-2")] {
            let response = serve(&source, &request, peer);
            assert_eq!(response[0], 1, "peer {peer:?}");
        }
        let response = serve(&source, &request, Some("node-1"));
        assert_eq!(response[0], 0);
    }

    #[test]
    fn only_the_granted_branches_are_revealed_and_nothing_uncommitted() {
        let (_dir, source, holder) = pair();
        crate::dolt_cell::run(&source, "branchCreate", &json!({ "name": "private" })).unwrap();
        crate::dolt_cell::run(&source, "checkout", &json!({ "branch": "private" })).unwrap();
        source.execute("INSERT INTO notes VALUES (9, 'SECRET-COMMITTED')", []).unwrap();
        commit(&source, "private work");
        crate::dolt_cell::run(&source, "checkout", &json!({ "branch": "main" })).unwrap();
        source.execute("INSERT INTO notes VALUES (8, 'SECRET-UNCOMMITTED')", []).unwrap();
        let token = grant(&source, json!({ "fetch": true, "branches": ["main"] }))["token"].as_str().unwrap().to_string();
        add_remote(&holder, &token);

        let response = exchange(&holder, &source, json!({ "name": "origin", "op": "fetch" }), &[]);
        for secret in ["SECRET-COMMITTED", "SECRET-UNCOMMITTED", "private work"] {
            assert!(!response.windows(secret.len()).any(|window| window == secret.as_bytes()), "{secret} leaked");
        }
        let fetched = run(&holder, "remoteApply", &json!({ "name": "origin", "op": "fetch" }), &response).map(json_of).unwrap();
        assert_eq!(fetched["branches"].as_object().unwrap().keys().collect::<Vec<_>>(), vec!["main"]);
        // Asked for by name: not found, not "empty".
        let response = exchange(&holder, &source, json!({ "name": "origin", "op": "fetch", "branches": ["private"] }), &[]);
        let error = run(&holder, "remoteApply", &json!({ "name": "origin", "op": "fetch" }), &response).unwrap_err();
        assert!(format!("{error:#}").contains("[ERR_DOLT_REMOTE_NOT_FOUND]"), "{error:#}");
    }

    #[test]
    fn a_push_fast_forwards_and_never_forces() {
        let (_dir, source, holder) = pair();
        let token = grant(&source, json!({ "fetch": true, "push": ["main", "work/*"] }))["token"].as_str().unwrap().to_string();
        add_remote(&holder, &token);
        holder.execute("INSERT INTO notes VALUES (2, 'holder')", []).unwrap();
        let tip = commit(&holder, "holder work");

        // The source's checked-out branch moves, since it has nothing uncommitted.
        let pushed = push(&holder, &source, "main", None).unwrap();
        assert_eq!(pushed, json!({ "branch": "main", "hash": tip, "created": false, "unchanged": false }));
        assert_eq!(head(&source, "HEAD"), tip);
        let rows: i64 = source.query_row("SELECT count(*) FROM notes", [], |row| row.get(0)).unwrap();
        assert_eq!(rows, 2, "the working set follows the checked-out branch");
        assert_eq!(push(&holder, &source, "main", None).unwrap()["unchanged"], json!(true));

        // A new branch, under another name.
        let created = push(&holder, &source, "main", Some("work/one")).unwrap();
        assert_eq!(created["created"], json!(true));
        assert_eq!(head(&source, "work/one"), tip);

        // Not allowed by the grant.
        let error = format!("{:#}", push(&holder, &source, "main", Some("other")).unwrap_err());
        assert!(error.contains("[ERR_DOLT_REMOTE_FORBIDDEN]"), "{error}");

        // The source moved on: the holder's commit is no longer a fast-forward.
        source.execute("INSERT INTO notes VALUES (3, 'source')", []).unwrap();
        let moved = commit(&source, "source work");
        holder.execute("INSERT INTO notes VALUES (4, 'holder again')", []).unwrap();
        commit(&holder, "holder again");
        let error = format!("{:#}", push(&holder, &source, "main", None).unwrap_err());
        assert!(error.contains("[ERR_DOLT_NON_FAST_FORWARD]"), "{error}");
        assert_eq!(head(&source, "HEAD"), moved, "a refused push changes nothing");

        // Fetch, merge, push again: now it fast-forwards.
        fetch(&holder, &source).unwrap();
        let merged = crate::dolt_cell::run(&holder, "merge", &json!({ "branch": "origin/main", "message": "merge" })).unwrap();
        assert_eq!(merged["conflicts"], json!([]));
        let merge_hash = merged["hash"].as_str().unwrap().to_string();
        let pushed = push(&holder, &source, "main", None).unwrap();
        assert_eq!(pushed["hash"], json!(merge_hash));
        assert_eq!(head(&source, "main"), head(&holder, "main"), "one hash on both cells");

        // A checked-out branch with uncommitted changes is not moved under the application.
        source.execute("INSERT INTO notes VALUES (50, 'uncommitted')", []).unwrap();
        holder.execute("INSERT INTO notes VALUES (5, 'holder third')", []).unwrap();
        commit(&holder, "holder third");
        fetch(&holder, &source).unwrap();
        let error = format!("{:#}", push(&holder, &source, "main", None).unwrap_err());
        assert!(error.contains("[ERR_DOLT_TARGET_DIRTY]"), "{error}");
        let rows: i64 = source.query_row("SELECT count(*) FROM notes WHERE id = 50", [], |row| row.get(0)).unwrap();
        assert_eq!(rows, 1, "the uncommitted row is untouched");
    }

    /// What the target's own runtime is told of a push (`ondoltpush`): the
    /// response `serve` frames carries `accepted` for a push that moved a
    /// branch, and for nothing else; the pusher's `apply` drops it.
    #[test]
    fn an_accepted_push_carries_what_the_target_is_told() {
        let (_dir, source, holder) = pair();
        let granted = grant(&source, json!({ "fetch": true, "push": ["main", "work/*"] }));
        let (id, token) = (granted["id"].as_str().unwrap(), granted["token"].as_str().unwrap().to_string());
        add_remote(&holder, &token);
        let base = head(&source, "main");
        holder.execute("INSERT INTO notes VALUES (2, 'holder')", []).unwrap();
        let tip = commit(&holder, "holder work");

        let raw_push = |to: Option<&str>| {
            let refs = exchange(&holder, &source, json!({ "name": "origin", "op": "refs" }), &[]);
            exchange(&holder, &source, json!({ "name": "origin", "op": "push", "branch": "main", "to": to }), &refs)
        };
        let accepted = |response: &[u8]| parse_response(response).unwrap().1.get("accepted").cloned();

        assert_eq!(
            accepted(&raw_push(None)),
            Some(json!({
                "remote": format!("pushed-{id}"), "grant": id, "branch": "main",
                "from": base, "to": tip, "created": false,
            }))
        );
        assert_eq!(head(&source, "main"), tip);
        // A new branch has no previous tip.
        let created = accepted(&raw_push(Some("work/one"))).unwrap();
        assert_eq!((created["from"].clone(), created["created"].clone()), (Value::Null, json!(true)));
        // Nothing changed, nothing refused: nothing to tell.
        assert_eq!(accepted(&raw_push(None)), None);
        let refused = raw_push(Some("other"));
        assert_eq!(parse_response(&refused).unwrap().0, 1);
        assert_eq!(accepted(&refused), None);
        // A fetch is not a push.
        let fetched = exchange(&holder, &source, json!({ "name": "origin", "op": "fetch" }), &[]);
        assert_eq!(accepted(&fetched), None);
    }

    #[test]
    fn a_pushed_pack_that_lies_about_its_tip_is_refused() {
        let (_dir, source, holder) = pair();
        let token = grant(&source, json!({ "push": ["main"] }))["token"].as_str().unwrap().to_string();
        add_remote(&holder, &token);
        holder.execute("INSERT INTO notes VALUES (2, 'holder')", []).unwrap();
        commit(&holder, "holder work");
        let refs = exchange(&holder, &source, json!({ "name": "origin", "op": "refs" }), &[]);
        let request = bytes_of(
            run(&holder, "remoteRequest", &json!({ "name": "origin", "op": "push", "branch": "main" }), &refs).unwrap(),
        );
        // A damaged pack installs nothing.
        // The first chunk of the pack is the pushed commit, which the receiver
        // must read: one flipped byte makes it not hash to its address.
        let mut damaged = request.clone();
        let json_length = u32::from_le_bytes(request[2..6].try_into().unwrap()) as usize;
        let pack_at = 6 + json_length;
        let refs_length = u32::from_le_bytes(request[pack_at + 4..pack_at + 8].try_into().unwrap()) as usize;
        let first_chunk = pack_at + 8 + refs_length + 4 + 24;
        damaged[first_chunk + 2] ^= 0xff;
        let (status, _, _) = parse_response(&serve(&source, &damaged, None)).unwrap();
        assert_eq!(status, 1);

        // Rewrite the claimed tip to the source's own commit.
        let (op, mut body, pack) = parse_request(&request).unwrap();
        body["tip"] = json!(head(&source, "main"));
        let forged = frame_request(op, &body, pack).unwrap();
        let (status, answer, _) = parse_response(&serve(&source, &forged, None)).unwrap();
        assert_eq!(status, 1, "{answer}");
        assert_eq!(answer["code"], json!("ERR_DOLT_REMOTE_BAD_REQUEST"));
        assert_ne!(head(&source, "main"), head(&holder, "main"));
    }

    #[test]
    fn malformed_requests_are_refusals_not_panics() {
        let (_dir, source, _holder) = pair();
        for request in [
            Vec::new(),
            vec![1],
            vec![9, 1, 0, 0, 0, 0],
            vec![1, 1, 255, 255, 255, 255, b'{'],
            vec![1, 1, 2, 0, 0, 0, b'{', b'}'],
            vec![1, 7, 2, 0, 0, 0, b'{', b'}'],
            frame_request(OP_FETCH, &json!({ "token": "x" }), &[]).unwrap(),
        ] {
            let (status, answer, _) = parse_response(&serve(&source, &request, None)).unwrap();
            assert_eq!(status, 1, "{request:?}");
            assert!(answer["code"].as_str().unwrap().starts_with("ERR_DOLT_REMOTE_"), "{answer}");
        }
    }

    #[test]
    fn names_refs_and_patterns_are_validated() {
        let (_dir, source, holder) = pair();
        for name in ["", "1x", "a b", "a/b", "a.b", &"x".repeat(65)] {
            assert!(run(&holder, "remoteAdd", &json!({ "name": name, "ref": "Graph:1", "token": "t" }), &[]).is_err(), "{name:?}");
        }
        for target in ["", "Graph", "Graph:", ":1", "Gra ph:1", "Graph:1 2"] {
            assert!(run(&holder, "remoteAdd", &json!({ "name": "r", "ref": target, "token": "t" }), &[]).is_err(), "{target:?}");
        }
        assert!(run(&holder, "remoteAdd", &json!({ "name": "r", "ref": "Graph:1" }), &[]).is_err());
        for bad in [json!(["../x"]), json!(["-x"]), json!([""]), json!("main"), json!([1])] {
            assert!(run(&source, "grant", &json!({ "fetch": true, "branches": bad }), &[]).is_err(), "{bad}");
        }
        assert!(run(&source, "grant", &json!({}), &[]).is_err(), "a grant must allow something");
        add_remote(&holder, "t");
        assert!(run(&holder, "remoteAdd", &json!({ "name": "origin", "ref": "Graph:1", "token": "t" }), &[]).is_err());
        run(&holder, "remoteRemove", &json!({ "name": "origin" }), &[]).unwrap();
        assert!(run(&holder, "remoteRemove", &json!({ "name": "origin" }), &[]).is_err());
    }

    #[test]
    fn strip_gives_a_fork_no_authority_and_no_tracking_branch() {
        let (_dir, source, holder) = pair();
        let token = grant(&source, json!({ "fetch": true, "push": ["main"] }))["token"].as_str().unwrap().to_string();
        add_remote(&holder, &token);
        fetch(&holder, &source).unwrap();
        // The holder also issued a grant.
        let own = grant(&holder, json!({ "push": ["main"] }));
        assert!(own["token"].is_string());
        strip(&holder).unwrap();
        assert_eq!(json_of(run(&holder, "remoteList", &json!({}), &[]).unwrap()), json!([]));
        assert_eq!(json_of(run(&holder, "grants", &json!({}), &[]).unwrap()), json!([]));
        assert_eq!(json_of(run(&holder, "remoteBranches", &json!({}), &[]).unwrap()), json!([]));
        // The old token opens nothing on the holder any more.
        let request = frame_request(OP_REFS, &json!({ "token": own["token"] }), &[]).unwrap();
        assert_eq!(serve(&holder, &request, None)[0], 1);
    }

    #[test]
    fn what_changes_the_store_is_counted_as_a_write() {
        let fetch_args = json!({ "op": "fetch" });
        let refs_args = json!({ "op": "refs" });
        assert!(mutates("remoteAdd", &json!({}), &[]));
        assert!(mutates("grant", &json!({}), &[]));
        assert!(mutates("remoteApply", &fetch_args, &[]));
        assert!(!mutates("remoteApply", &refs_args, &[]));
        assert!(!mutates("remoteList", &json!({}), &[]));
        let push = frame_request(OP_PUSH, &json!({}), &[]).unwrap();
        let refs = frame_request(OP_REFS, &json!({}), &[]).unwrap();
        assert!(mutates("remoteServe", &json!({}), &push));
        assert!(!mutates("remoteServe", &json!({}), &refs));
        assert!(changes_schema("remoteServe", &push));
        assert!(!changes_schema("remoteServe", &refs));
    }
}
