//! `execute`: run a replicated SQL program deterministically (Chronolog Spec
//! 10 section 4, version 1: SQL steps only, no platform operations).
//!
//! The contract, in the order it happens:
//!
//! 1. `BEGIN IMMEDIATE` (or, for [`execute_in_transaction`], the caller's
//!    transaction) and a `SAVEPOINT`.
//! 2. The engine inputs that do not come from SQL or the database are pinned
//!    to the program: the clock (`ctx.now_ms`), the PRNG (`ctx.seed`), UTC as
//!    the local zone, `last_insert_rowid()` and `changes()` at 0, and
//!    `chronolog_actor()` (`ctx.actor`). All of it lives on this connection
//!    only (DoltLite patch 0002); other connections and threads are untouched.
//! 3. Each statement is prepared and stepped to completion under the consensus
//!    authorizer and a VM-step budget, rows discarded.
//! 4. Accepted: release the savepoint. Rejected: `ROLLBACK TO SAVEPOINT`,
//!    never a full `ROLLBACK` (after a Dolt branch checkout a full rollback can
//!    restore the previous branch's working set; see chronolog
//!    `materializer.ts`).
//!
//! A deterministic failure is an [`Outcome::Rejected`]; an environmental one
//! (I/O, corruption, out of memory, busy, interrupt from outside) is an
//! [`ExecError`] and stops replay on that replica.

use crate::authorizer::{AuthMode, AuthOptions, Authorizer, OwnerTablePolicy};
use crate::ffi;
use rusqlite::ffi as sys;
use rusqlite::Connection;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::sync::Mutex;

// ───────────────────────────── program model ─────────────────────────────

/// A SQLite storage-class value (the only value type programs carry).
#[derive(Clone, Debug, PartialEq)]
pub enum SqlValue {
    Null,
    Integer(i64),
    /// Must be finite. Bound as REAL even when integral (`1.0`).
    Real(f64),
    Text(String),
    /// An empty blob is bound as an empty BLOB, not NULL.
    Blob(Vec<u8>),
}

/// Which parameter a binding fills.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parameter {
    /// SQLite's 1-based parameter position.
    Index(usize),
    /// `:name`, `@name`, `$name` or `?NNN`. A bare `name` matches `:name`,
    /// then `@name`, then `$name`.
    Name(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub parameter: Parameter,
    pub value: SqlValue,
}

/// One step: exactly one SQL statement (trailing comments, whitespace and
/// semicolons are allowed) and its bindings. Every parameter must be bound
/// exactly once.
#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub sql: String,
    pub bindings: Vec<Binding>,
}

impl Statement {
    pub fn new(sql: impl Into<String>) -> Self {
        Self { sql: sql.into(), bindings: Vec::new() }
    }

    pub fn bind(mut self, parameter: Parameter, value: SqlValue) -> Self {
        self.bindings.push(Binding { parameter, value });
        self
    }
}

/// An ordered list of steps, 1 to [`Limits::max_steps`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    pub steps: Vec<Statement>,
}

impl Program {
    pub fn new(steps: Vec<Statement>) -> Self {
        Self { steps }
    }
}

/// Resource limits (Chronolog `SqlRuntimeLimits` plus the step-count cap).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Maximum SQL source size in UTF-8 bytes, per statement.
    pub max_sql_bytes: usize,
    /// VM operations allowed per statement.
    pub max_vm_steps: u64,
    /// VM operations between progress callbacks.
    pub progress_granularity: u32,
    /// Maximum bytes of one string or BLOB value (SQLITE_LIMIT_LENGTH).
    pub max_value_bytes: u32,
    /// Maximum steps in a program.
    pub max_steps: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sql_bytes: 1_000_000,
            max_vm_steps: 1_000_000,
            progress_granularity: 1_000,
            max_value_bytes: 16 * 1024 * 1024,
            max_steps: 256,
        }
    }
}

impl Limits {
    fn validate(&self) -> Result<(), ExecError> {
        let ok = self.max_sql_bytes >= 1
            && self.max_vm_steps >= 1
            // SQLITE_MAX_VDBE_OP: the budget is also applied as SQLITE_LIMIT_VDBE_OP, as in Chronolog.
            && self.max_vm_steps <= 250_000_000
            && self.progress_granularity >= 1
            && self.max_value_bytes >= 1
            && self.max_steps >= 1
            && u64::from(self.progress_granularity) <= self.max_vm_steps
            && self.progress_granularity <= i32::MAX as u32
            && self.max_value_bytes <= i32::MAX as u32
            && self.max_sql_bytes <= i32::MAX as usize;
        if ok {
            Ok(())
        } else {
            Err(ExecError::InvalidInput("execution limits are out of range".into()))
        }
    }
}

/// The authorizer profile a program runs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Profile {
    /// Chronolog's consensus block-list. `schema_admin` lets the signer run DDL.
    /// A future `Platform` variant (owner-rows) will add trusted platform steps.
    Consensus { schema_admin: bool },
}

impl Default for Profile {
    fn default() -> Self {
        Profile::Consensus { schema_admin: false }
    }
}

/// The engine inputs fixed from the signed transaction.
#[derive(Clone, Debug)]
pub struct ExecContext {
    /// `'now'`, `CURRENT_TIMESTAMP`, ... and every Dolt timestamp: the
    /// transaction's signed author timestamp, in Unix milliseconds
    /// (0..=253402300799999).
    pub now_ms: i64,
    /// Seeds `random()`, `randomblob()` and random rowids. Chronolog uses the
    /// first four bytes of the transaction id ([`prng_seed_from_tx_id`]).
    pub seed: u64,
    /// The signing key: `chronolog_actor()` (1 to 64 bytes). `None` makes it NULL.
    pub actor: Option<Vec<u8>>,
    /// The signer's resolved principal id within the application.
    pub actor_id: Option<i64>,
    /// The exact transaction identity available to deterministic SQL.
    pub transaction_id: Option<Vec<u8>>,
    pub profile: Profile,
    pub limits: Limits,
}

impl ExecContext {
    pub fn new(now_ms: i64, seed: u64) -> Self {
        Self {
            now_ms,
            seed,
            actor: None,
            actor_id: None,
            transaction_id: None,
            profile: Profile::default(),
            limits: Limits::default(),
        }
    }
}

/// A nonzero seed from a transaction id, exactly as Chronolog's `prngSeed`:
/// the first four bytes, big-endian; zero becomes one.
pub fn prng_seed_from_tx_id(tx_id: &[u8]) -> u64 {
    let mut seed: u32 = 0;
    for b in tx_id.iter().take(4) {
        seed = (seed << 8) | u32::from(*b);
    }
    u64::from(if seed == 0 { 1 } else { seed })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Rejected {
        /// `assert:<code>`, `constraint`, `step_limit`, `denied`, `prepare`,
        /// `binding`, `too_large`, `mismatch`, `error`, or `policy:<t>:<cmd>`.
        code: String,
        /// Index of the failing step.
        statement_index: Option<usize>,
    },
}

impl Outcome {
    pub fn is_accepted(&self) -> bool {
        matches!(self, Outcome::Accepted)
    }
}

/// A failure that is not a deterministic rejection.
#[derive(Debug)]
pub enum ExecError {
    /// I/O, corruption, out of memory, busy, an outside interrupt, ...
    /// Carries the SQLite result code.
    Operational { code: i32, message: String },
    /// The caller passed a malformed program, context or limits.
    InvalidInput(String),
    /// `execute` needs no open transaction; `execute_in_transaction` needs one.
    TransactionState(&'static str),
    /// A connection setting the profile requires could not be applied.
    Profile(String),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecError::Operational { code, message } => write!(f, "operational SQLite error {code}: {message}"),
            ExecError::InvalidInput(m) => write!(f, "invalid input: {m}"),
            ExecError::TransactionState(m) => write!(f, "transaction state: {m}"),
            ExecError::Profile(m) => write!(f, "execution profile: {m}"),
        }
    }
}

impl std::error::Error for ExecError {}

/// A statement-level failure: a deterministic rejection or a fatal error.
enum Fail {
    Reject(String),
    Fatal(ExecError),
}

fn reject(code: &str) -> Fail {
    Fail::Reject(code.to_string())
}

// ─────────────────────────────── entry points ────────────────────────────

/// Execute `program` as its own transaction: `BEGIN IMMEDIATE`, a savepoint,
/// the steps, then release (accepted) or roll back to the savepoint
/// (rejected), and `COMMIT`.
///
/// The connection must not be in a transaction. If a write statement exhausts
/// the step budget SQLite rolls the whole transaction back itself (documented
/// `sqlite3_interrupt` behaviour); the result is still a rejection and the
/// state is the one before `BEGIN`.
pub fn execute(conn: &Connection, program: &Program, ctx: &ExecContext) -> Result<Outcome, ExecError> {
    run(conn, program, ctx, true)
}

/// Like [`execute`], but inside a transaction the caller opened (and will
/// commit), for callers that write their own log row atomically with the
/// program's effects, as Chronolog's materializer does. Only the savepoint is
/// managed here. The connection must have `foreign_keys` enabled.
///
/// After a rejection caused by a write statement exhausting the step budget
/// SQLite has already rolled the whole transaction back; check
/// `conn.is_autocommit()` before writing more.
pub fn execute_in_transaction(
    conn: &Connection,
    program: &Program,
    ctx: &ExecContext,
) -> Result<Outcome, ExecError> {
    run(conn, program, ctx, false)
}

const SAVEPOINT_NAME: &str = "celld_exec_program";

fn run(conn: &Connection, program: &Program, ctx: &ExecContext, own: bool) -> Result<Outcome, ExecError> {
    validate(program, ctx)?;
    // SAFETY: the handle stays valid for the borrow of `conn`; everything
    // below runs on this thread and the connection's mutex serialises it.
    let db = unsafe { conn.handle() };
    // SAFETY: `db` outlives this call, and so does the state hung off it.
    let state = unsafe { register_state(db) }?;
    let in_txn = unsafe { sys::sqlite3_get_autocommit(db) } == 0;
    if own && in_txn {
        return Err(ExecError::TransactionState("execute() needs a connection with no open transaction"));
    }
    if !own && !in_txn {
        return Err(ExecError::TransactionState("execute_in_transaction() needs an open transaction"));
    }
    if det_context_active(conn) {
        return Err(ExecError::TransactionState("the connection already has a pinned context"));
    }
    let _profile = ProfileScope::enter(db, &ctx.limits, !own)?;
    if own {
        exec_sql(db, "BEGIN IMMEDIATE")?;
    }
    match run_inner(conn, program, ctx, own, state) {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            if own && unsafe { sys::sqlite3_get_autocommit(db) } == 0 {
                // Environmental failure: nothing of this program may persist.
                let _ = exec_sql(db, "ROLLBACK");
            }
            Err(error)
        }
    }
}

fn run_inner(
    conn: &Connection,
    program: &Program,
    ctx: &ExecContext,
    own: bool,
    state: &ExecState,
) -> Result<Outcome, ExecError> {
    let db = unsafe { conn.handle() };
    exec_sql(db, &format!("SAVEPOINT {SAVEPOINT_NAME}"))?;
    let rejection = {
        install_program_context(db, state, ctx)?;
        let _pin = ProgramPin { db, state };
        run_steps(conn, program, ctx)?
    };
    let transaction_gone = unsafe { sys::sqlite3_get_autocommit(db) } != 0;
    match rejection {
        None => {
            if transaction_gone {
                // An accepted program cannot lose its transaction.
                return Err(ExecError::Operational {
                    code: sys::SQLITE_ABORT,
                    message: "transaction ended while the program was running".into(),
                });
            }
            exec_sql(db, &format!("RELEASE SAVEPOINT {SAVEPOINT_NAME}"))?;
            if own {
                exec_sql(db, "COMMIT")?;
            }
            Ok(Outcome::Accepted)
        }
        Some((code, index)) => {
            // If SQLite rolled the transaction back itself (a write statement
            // hit the step budget), the state is already the pre-BEGIN state.
            if !transaction_gone {
                exec_sql(db, &format!("ROLLBACK TO SAVEPOINT {SAVEPOINT_NAME}"))?;
                exec_sql(db, &format!("RELEASE SAVEPOINT {SAVEPOINT_NAME}"))?;
                if own {
                    exec_sql(db, "COMMIT")?;
                }
            }
            Ok(Outcome::Rejected { code, statement_index: Some(index) })
        }
    }
}

fn run_steps(conn: &Connection, program: &Program, ctx: &ExecContext) -> Result<Option<(String, usize)>, ExecError> {
    let Profile::Consensus { schema_admin } = ctx.profile;
    for (index, statement) in program.steps.iter().enumerate() {
        match run_statement(conn, statement, &ctx.limits, AuthMode::Consensus, schema_admin) {
            Ok(()) => {}
            Err(Fail::Reject(code)) => return Ok(Some((code, index))),
            Err(Fail::Fatal(error)) => return Err(error),
        }
    }
    Ok(None)
}

fn validate(program: &Program, ctx: &ExecContext) -> Result<(), ExecError> {
    ctx.limits.validate()?;
    if program.steps.is_empty() || program.steps.len() > ctx.limits.max_steps {
        return Err(ExecError::InvalidInput(format!(
            "a program has 1 to {} steps, got {}",
            ctx.limits.max_steps,
            program.steps.len()
        )));
    }
    validate_context(ctx)
}

fn validate_context(ctx: &ExecContext) -> Result<(), ExecError> {
    if !(0..=253_402_300_799_999).contains(&ctx.now_ms) {
        return Err(ExecError::InvalidInput("now_ms is out of range".into()));
    }
    if let Some(actor) = &ctx.actor {
        if actor.is_empty() || actor.len() > 64 {
            return Err(ExecError::InvalidInput("actor must be 1 to 64 bytes".into()));
        }
    }
    if ctx.actor_id.is_some_and(|id| id < 1) {
        return Err(ExecError::InvalidInput("actor_id must be positive".into()));
    }
    if ctx.transaction_id.as_ref().is_some_and(|id| id.len() != 32) {
        return Err(ExecError::InvalidInput("transaction_id must be 32 bytes".into()));
    }
    Ok(())
}

// ───────────────────────────── one statement ─────────────────────────────

struct Budget {
    callbacks: Cell<u64>,
    max_callbacks: u64,
    exceeded: Cell<bool>,
}

/// Installs the authorizer and the step budget for one statement; clears both
/// on drop (also when unwinding).
struct Controls<'a> {
    db: *mut sys::sqlite3,
    _auth: &'a RefCell<Authorizer>,
    _budget: &'a Budget,
}

impl<'a> Controls<'a> {
    fn install(db: *mut sys::sqlite3, auth: &'a RefCell<Authorizer>, budget: &'a Budget, granularity: u32) -> Self {
        unsafe {
            sys::sqlite3_set_authorizer(db, Some(authorizer_trampoline), auth as *const _ as *mut c_void);
            sys::sqlite3_progress_handler(
                db,
                granularity as c_int,
                Some(progress_trampoline),
                budget as *const _ as *mut c_void,
            );
        }
        Self { db, _auth: auth, _budget: budget }
    }
}

impl Drop for Controls<'_> {
    fn drop(&mut self) {
        unsafe {
            sys::sqlite3_set_authorizer(self.db, None, ptr::null_mut());
            sys::sqlite3_progress_handler(self.db, 0, None, ptr::null_mut());
        }
    }
}

unsafe extern "C" fn authorizer_trampoline(
    user: *mut c_void,
    action: c_int,
    arg1: *const c_char,
    arg2: *const c_char,
    arg3: *const c_char,
    arg4: *const c_char,
) -> c_int {
    let cell = &*(user as *const RefCell<Authorizer>);
    let verdict = catch_unwind(AssertUnwindSafe(|| {
        let text = |p: *const c_char| -> Option<String> {
            if p.is_null() {
                None
            } else {
                Some(CStr::from_ptr(p).to_string_lossy().into_owned())
            }
        };
        let (a1, a2, a3, a4) = (text(arg1), text(arg2), text(arg3), text(arg4));
        match cell.try_borrow_mut() {
            Ok(mut auth) => auth.allow(action, a1.as_deref(), a2.as_deref(), a3.as_deref(), a4.as_deref()),
            Err(_) => false,
        }
    }));
    let allow = verdict.unwrap_or(false);
    if allow {
        sys::SQLITE_OK
    } else {
        sys::SQLITE_DENY
    }
}

unsafe extern "C" fn progress_trampoline(user: *mut c_void) -> c_int {
    let budget = &*(user as *const Budget);
    let calls = budget.callbacks.get() + 1;
    budget.callbacks.set(calls);
    let exceeded = calls >= budget.max_callbacks;
    budget.exceeded.set(exceeded);
    c_int::from(exceeded)
}

struct Stmt(*mut sys::sqlite3_stmt);

impl Drop for Stmt {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { sys::sqlite3_finalize(self.0) };
        }
    }
}

fn run_statement(
    conn: &Connection,
    statement: &Statement,
    limits: &Limits,
    mode: AuthMode,
    schema_admin: bool,
) -> Result<(), Fail> {
    let sql = statement.sql.as_bytes();
    if sql.contains(&0) {
        return Err(reject("prepare")); // SQL_INVALID_SOURCE
    }
    if sql.len() > limits.max_sql_bytes {
        return Err(reject("too_large"));
    }
    let db = unsafe { conn.handle() };
    let auth = RefCell::new(profile_authorizer(conn, mode, AuthOptions { schema_admin })
        .map_err(Fail::Fatal)?);
    let budget = Budget {
        callbacks: Cell::new(0),
        max_callbacks: limits.max_vm_steps.div_ceil(u64::from(limits.progress_granularity)).max(1),
        exceeded: Cell::new(false),
    };
    let _controls = Controls::install(db, &auth, &budget, limits.progress_granularity);

    let mut raw: *mut sys::sqlite3_stmt = ptr::null_mut();
    let mut tail: *const c_char = ptr::null();
    let rc = unsafe {
        sys::sqlite3_prepare_v2(db, sql.as_ptr() as *const c_char, sql.len() as c_int, &mut raw, &mut tail)
    };
    let stmt = Stmt(raw);
    if rc != sys::SQLITE_OK {
        return Err(map_error(db, &budget, &auth, Phase::Prepare));
    }
    if stmt.0.is_null() {
        return Err(reject("prepare")); // "SQL contains no statement"
    }
    // Exactly one statement: whatever follows must be whitespace, ';' or comments.
    let consumed = (tail as usize).wrapping_sub(sql.as_ptr() as usize);
    match sql.get(consumed..).and_then(|rest| std::str::from_utf8(rest).ok()) {
        Some(rest) if is_trivia_only(rest) => {}
        _ => return Err(reject("prepare")), // SQL_MULTIPLE_STATEMENTS / tail invalid
    }
    bind_all(stmt.0, statement)?;
    auth.borrow_mut().in_step = true;
    loop {
        match unsafe { sys::sqlite3_step(stmt.0) } {
            sys::SQLITE_ROW => continue, // rows are not recorded or compared
            sys::SQLITE_DONE => return Ok(()),
            _ => return Err(map_error(db, &budget, &auth, Phase::Step)),
        }
    }
}

/// Prepare `sql` under the authorizer of `mode` without running it, and
/// check it is exactly one statement (chronolog `prepareProfiledStatement`).
/// `Err` carries the rejection code: `denied`, `prepare` or `too_large`.
/// Meant for tests, tooling and local reads; [`execute`] does this itself.
pub fn check_statement(conn: &Connection, sql: &str, mode: AuthMode, options: AuthOptions) -> Result<(), String> {
    let limits = Limits::default();
    let db = unsafe { conn.handle() };
    // Same function set as `execute`, so a statement using chronolog_* checks like it runs.
    unsafe { register_state(db) }.map_err(|e| format!("operational: {e}"))?;
    let bytes = sql.as_bytes();
    if bytes.contains(&0) {
        return Err("prepare".into());
    }
    if bytes.len() > limits.max_sql_bytes {
        return Err("too_large".into());
    }
    let auth = RefCell::new(profile_authorizer(conn, mode, options).map_err(|e| format!("operational: {e}"))?);
    let budget = Budget {
        callbacks: Cell::new(0),
        max_callbacks: limits.max_vm_steps.div_ceil(u64::from(limits.progress_granularity)).max(1),
        exceeded: Cell::new(false),
    };
    let _controls = Controls::install(db, &auth, &budget, limits.progress_granularity);
    let mut raw: *mut sys::sqlite3_stmt = ptr::null_mut();
    let mut tail: *const c_char = ptr::null();
    let rc = unsafe {
        sys::sqlite3_prepare_v2(db, bytes.as_ptr() as *const c_char, bytes.len() as c_int, &mut raw, &mut tail)
    };
    let stmt = Stmt(raw);
    if rc != sys::SQLITE_OK {
        return match map_error(db, &budget, &auth, Phase::Prepare) {
            Fail::Reject(code) => Err(code),
            Fail::Fatal(e) => Err(format!("operational: {e}")),
        };
    }
    if stmt.0.is_null() {
        return Err("prepare".into());
    }
    let consumed = (tail as usize).wrapping_sub(bytes.as_ptr() as usize);
    match bytes.get(consumed..).and_then(|rest| std::str::from_utf8(rest).ok()) {
        Some(rest) if is_trivia_only(rest) => Ok(()),
        _ => Err("prepare".into()),
    }
}

fn profile_authorizer(conn: &Connection, mode: AuthMode, options: AuthOptions) -> Result<Authorizer, ExecError> {
    let mut auth = Authorizer::new(mode, options);
    let mut catalogue = || -> rusqlite::Result<()> {
        let has_registry = conn.prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM main.sqlite_schema WHERE type = 'table' AND name = 'chronolog_tables')"
        )?.query_row([], |row| row.get::<_, bool>(0))?;
        if !has_registry {
            return Ok(());
        }
        if mode == AuthMode::LocalRead {
            auth.set_owner_tables(OwnerTablePolicy { shadow: shadow_tables(conn)?, ..Default::default() });
        } else {
            auth.set_lifecycle_triggers(lifecycle_triggers(conn)?);
            auth.set_owner_tables(owner_table_policy(conn)?);
        }
        Ok(())
    };
    catalogue().map_err(|error| {
        let code = match &error {
            rusqlite::Error::SqliteFailure(cause, _) => cause.extended_code,
            _ => sys::SQLITE_ERROR,
        };
        ExecError::Operational { code, message: format!("cannot read Chronolog table registry: {error}") }
    })?;
    Ok(auth)
}

/// Only a trusted platform operation can register a table or create these
/// reserved triggers. Check the live registry and attached schema, rather
/// than using a schema cookie that can repeat after rollback.
fn lifecycle_triggers(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT lower(s.name)
           FROM main.sqlite_schema s JOIN main.chronolog_tables t
             ON s.tbl_name = t.name COLLATE NOCASE
          WHERE s.type = 'trigger' AND lower(s.name) IN (
                'chronolog_k_' || lower(s.tbl_name) || '_upd',
                'chronolog_k_' || lower(s.tbl_name) || '_del',
                'chronolog_l_' || lower(s.tbl_name) || '_ins')"
    )?;
    let names = stmt.query_map([], |row| row.get::<_, String>(0))?;
    names.collect()
}

fn shadow_tables(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT lower(name) FROM pragma_table_list WHERE schema = 'main' AND type = 'shadow'"
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect()
}

/// The current Chronolog policy also covers tables which do not have owner
/// columns. Their direct writes cannot be treated as owner-checked SQL.
fn owner_table_policy(conn: &Connection) -> rusqlite::Result<OwnerTablePolicy> {
    use std::collections::HashMap;
    use std::sync::OnceLock;

    static VIRTUAL: OnceLock<regex::Regex> = OnceLock::new();
    static VEC: OnceLock<regex::Regex> = OnceLock::new();
    static VECTOR_COLUMN: OnceLock<regex::Regex> = OnceLock::new();
    let virtual_table = VIRTUAL.get_or_init(|| regex::Regex::new(r"(?iu)^\s*create\s+virtual\s+table\b").unwrap());
    let vec_definition = VEC.get_or_init(|| regex::Regex::new(
        r"(?isu)^\s*create\s+virtual\s+table\s+.*?\busing\s+vec0\s*\((.*)\)\s*$"
    ).unwrap());
    let vector_column = VECTOR_COLUMN.get_or_init(|| regex::Regex::new(r"(?iu)\b(?:float|f32|int8|i8|bit)\s*\[").unwrap());

    let mut policy = OwnerTablePolicy::default();
    let mut tables = conn.prepare_cached(
        "SELECT name, sql FROM main.sqlite_schema WHERE type = 'table' AND sql IS NOT NULL"
    )?;
    let catalogue = tables.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut vector_tables = HashMap::new();
    for (name, declaration) in &catalogue {
        if !virtual_table.is_match(declaration) {
            continue;
        }
        policy.protected.insert(name.to_ascii_lowercase());
        if let Some(captures) = vec_definition.captures(declaration) {
            vector_tables.insert(name.to_ascii_lowercase(), vector_column.find_iter(&captures[1]).count());
        }
    }
    let mut columns = conn.prepare_cached(
        r#"SELECT name, upper(type), "notnull", pk, hidden FROM pragma_table_xinfo(?1, 'main') ORDER BY cid"#
    )?;
    for (name, _) in catalogue {
        let normalized = name.to_ascii_lowercase();
        let Some((owner, ordinal)) = normalized.rsplit_once("_vector_chunks") else { continue };
        let Some(count) = vector_tables.get(owner) else { continue };
        if ordinal.len() != 2 || !ordinal.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let index = usize::from(ordinal.as_bytes()[0] - b'0') * 10 + usize::from(ordinal.as_bytes()[1] - b'0');
        if index >= *count { continue; }
        let shape = columns.query_map([name], |row| Ok((
            row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?, row.get::<_, i64>(4)?,
        )))?.collect::<rusqlite::Result<Vec<_>>>()?;
        // DoltLite can store vec0's implicit rowid type as an empty
        // declaration. The exact primary-key and payload shape is unchanged.
        if matches!(shape.as_slice(),
            [(rowid, rowid_type, 0, 1, 0), (vectors, payload_type, 1, 0, 0)]
                if rowid == "rowid" && matches!(rowid_type.as_str(), "" | "INTEGER")
                    && vectors == "vectors" && payload_type == "BLOB")
        {
            policy.protected.insert(normalized);
        }
    }
    let mut triggers = conn.prepare_cached(
        "SELECT lower(s.name) FROM main.sqlite_schema s
          JOIN main.chronolog_tables t ON s.tbl_name = t.name COLLATE NOCASE
          WHERE s.type = 'trigger' AND substr(lower(s.name), 1, 10) <> 'chronolog_'"
    )?;
    policy.application_triggers = triggers.query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(policy)
}

/// The result of [`local_query`].
#[derive(Clone, Debug, PartialEq)]
pub struct LocalRows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<SqlValue>>,
}

/// Run one read-only statement under the [`AuthMode::LocalRead`] authorizer
/// and the step budget of `limits` (per statement), returning every row with
/// its exact storage class (Chronolog Spec 10 section 7, local reads). The
/// clock is not pinned: local reads may use the real one.
///
/// `Err` carries a Chronolog local-read code: `SQL_PROFILE_VIOLATION`
/// (authorizer), `SQL_PREPARE_FAILED`, `SQL_MULTIPLE_STATEMENTS`,
/// `SQL_STATEMENT_TOO_LARGE`, `SQL_INVALID_SOURCE`, `SQL_BINDING_INVALID`,
/// `SQL_STEP_LIMIT`, `SQL_RESULT_LIMIT` (more than `max_rows` rows), or
/// `SQL_EXECUTION_FAILED:<message>` for any other step error.
pub fn local_query(
    conn: &Connection,
    statement: &Statement,
    limits: &Limits,
    max_rows: usize,
) -> Result<LocalRows, String> {
    let auth = profile_authorizer(conn, AuthMode::LocalRead, AuthOptions::default())
        .map_err(|error| format!("operational: {error}"))?;
    query_with_authorizer(conn, statement, limits, max_rows, auth, None)
}

/// Read solely through prepared, caller-owned CTEs. CTE names must be
/// allocated by the trusted compiler, never reused or derived from user SQL.
pub fn scoped_query(
    conn: &Connection,
    statement: &Statement,
    limits: &Limits,
    max_rows: usize,
    read_sources: std::collections::HashSet<String>,
) -> Result<LocalRows, String> {
    limits.validate().map_err(|error| error.to_string())?;
    let db = unsafe { conn.handle() };
    let _profile = ScopedReadLimits::new(db, limits);
    query_with_authorizer(conn, statement, limits, max_rows,
        Authorizer::scoped_read(read_sources), Some(8 * 1024 * 1024))
}

struct ScopedReadLimits {
    db: *mut sys::sqlite3,
    saved: [(c_int, c_int); 4],
}

impl ScopedReadLimits {
    fn new(db: *mut sys::sqlite3, limits: &Limits) -> Self {
        let wanted = [
            (ffi::LIMIT_LENGTH, limits.max_value_bytes as c_int),
            (ffi::LIMIT_SQL_LENGTH, limits.max_sql_bytes as c_int),
            (ffi::LIMIT_COLUMN, 256),
            (ffi::LIMIT_ATTACHED, 0),
        ];
        let saved = wanted.map(|(category, amount)| {
            (category, unsafe { sys::sqlite3_limit(db, category, amount) })
        });
        Self { db, saved }
    }
}

impl Drop for ScopedReadLimits {
    fn drop(&mut self) {
        for (category, amount) in self.saved {
            unsafe { sys::sqlite3_limit(self.db, category, amount) };
        }
    }
}

fn query_with_authorizer(
    conn: &Connection,
    statement: &Statement,
    limits: &Limits,
    max_rows: usize,
    authorizer: Authorizer,
    max_result_bytes: Option<usize>,
) -> Result<LocalRows, String> {
    limits.validate().map_err(|e| e.to_string())?;
    let db = unsafe { conn.handle() };
    unsafe { register_state(db) }.map_err(|e| format!("operational: {e}"))?;
    let sql = statement.sql.as_bytes();
    if sql.contains(&0) {
        return Err("SQL_INVALID_SOURCE".into());
    }
    if sql.len() > limits.max_sql_bytes {
        return Err("SQL_STATEMENT_TOO_LARGE".into());
    }
    let auth = RefCell::new(authorizer);
    let budget = Budget {
        callbacks: Cell::new(0),
        max_callbacks: limits.max_vm_steps.div_ceil(u64::from(limits.progress_granularity)).max(1),
        exceeded: Cell::new(false),
    };
    let _controls = Controls::install(db, &auth, &budget, limits.progress_granularity);
    let code_of = |fail: Fail| -> String {
        match fail {
            Fail::Reject(code) => match code.as_str() {
                "denied" => "SQL_PROFILE_VIOLATION".into(),
                "prepare" => "SQL_PREPARE_FAILED".into(),
                "step_limit" => "SQL_STEP_LIMIT".into(),
                "too_large" => "SQL_RESULT_LIMIT".into(),
                "binding" => "SQL_BINDING_INVALID".into(),
                other => format!("SQL_EXECUTION_FAILED:{other}"),
            },
            Fail::Fatal(e) => format!("operational: {e}"),
        }
    };
    let mut raw: *mut sys::sqlite3_stmt = ptr::null_mut();
    let mut tail: *const c_char = ptr::null();
    let rc = unsafe {
        sys::sqlite3_prepare_v2(db, sql.as_ptr() as *const c_char, sql.len() as c_int, &mut raw, &mut tail)
    };
    let stmt = Stmt(raw);
    if rc != sys::SQLITE_OK {
        return Err(code_of(map_error(db, &budget, &auth, Phase::Prepare)));
    }
    if stmt.0.is_null() {
        return Err("SQL_PREPARE_FAILED".into());
    }
    let consumed = (tail as usize).wrapping_sub(sql.as_ptr() as usize);
    match sql.get(consumed..).and_then(|rest| std::str::from_utf8(rest).ok()) {
        Some(rest) if is_trivia_only(rest) => {}
        _ => return Err("SQL_MULTIPLE_STATEMENTS".into()),
    }
    // Belt and braces: the LocalRead authorizer denies every write, and a
    // statement SQLite itself knows to write is refused before it steps.
    if unsafe { sys::sqlite3_stmt_readonly(stmt.0) } == 0 {
        return Err("SQL_PROFILE_VIOLATION".into());
    }
    bind_all(stmt.0, statement).map_err(code_of)?;
    auth.borrow_mut().in_step = true;
    let count = unsafe { sys::sqlite3_column_count(stmt.0) };
    let columns = (0..count)
        .map(|i| unsafe {
            let p = sys::sqlite3_column_name(stmt.0, i);
            if p.is_null() {
                format!("column_{i}")
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        })
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    let mut result_bytes = 0_usize;
    loop {
        match unsafe { sys::sqlite3_step(stmt.0) } {
            sys::SQLITE_ROW => {
                if rows.len() >= max_rows {
                    return Err("SQL_RESULT_LIMIT".into());
                }
                let mut row = Vec::with_capacity(count as usize);
                for i in 0..count {
                    let value = unsafe { column_value(stmt.0, i) };
                    if let Some(max_bytes) = max_result_bytes {
                        result_bytes = result_bytes.saturating_add(match &value {
                            SqlValue::Blob(bytes) => bytes.len(),
                            SqlValue::Text(text) => text.len(),
                            _ => 0,
                        }).saturating_add(std::mem::size_of::<SqlValue>());
                        if result_bytes > max_bytes {
                            return Err("SQL_RESULT_LIMIT".into());
                        }
                    }
                    row.push(value);
                }
                rows.push(row);
            }
            sys::SQLITE_DONE => return Ok(LocalRows { columns, rows }),
            _ => return Err(code_of(map_error(db, &budget, &auth, Phase::Step))),
        }
    }
}

/// One result column with its exact storage class.
unsafe fn column_value(stmt: *mut sys::sqlite3_stmt, i: c_int) -> SqlValue {
    match sys::sqlite3_column_type(stmt, i) {
        sys::SQLITE_INTEGER => SqlValue::Integer(sys::sqlite3_column_int64(stmt, i)),
        sys::SQLITE_FLOAT => SqlValue::Real(sys::sqlite3_column_double(stmt, i)),
        sys::SQLITE_TEXT => {
            let p = sys::sqlite3_column_text(stmt, i);
            let n = sys::sqlite3_column_bytes(stmt, i);
            if p.is_null() {
                SqlValue::Text(String::new())
            } else {
                SqlValue::Text(String::from_utf8_lossy(std::slice::from_raw_parts(p, n as usize)).into_owned())
            }
        }
        sys::SQLITE_BLOB => {
            let p = sys::sqlite3_column_blob(stmt, i);
            let n = sys::sqlite3_column_bytes(stmt, i);
            if p.is_null() || n == 0 {
                SqlValue::Blob(Vec::new())
            } else {
                SqlValue::Blob(std::slice::from_raw_parts(p as *const u8, n as usize).to_vec())
            }
        }
        _ => SqlValue::Null,
    }
}

fn bind_all(stmt: *mut sys::sqlite3_stmt, statement: &Statement) -> Result<(), Fail> {
    let count = unsafe { sys::sqlite3_bind_parameter_count(stmt) } as usize;
    let mut bound = vec![false; count];
    for binding in &statement.bindings {
        let position = match &binding.parameter {
            Parameter::Index(i) => *i,
            Parameter::Name(name) => parameter_index(stmt, name),
        };
        if position < 1 || position > count || bound[position - 1] {
            return Err(reject("binding"));
        }
        bound[position - 1] = true;
        bind_value(stmt, position as c_int, &binding.value)?;
    }
    if bound.iter().any(|b| !b) {
        return Err(reject("binding"));
    }
    Ok(())
}

fn parameter_index(stmt: *mut sys::sqlite3_stmt, name: &str) -> usize {
    let lookup = |n: &str| -> usize {
        match CString::new(n) {
            Ok(c) => unsafe { sys::sqlite3_bind_parameter_index(stmt, c.as_ptr()) as usize },
            Err(_) => 0,
        }
    };
    if name.starts_with([':', '@', '$', '?']) {
        return lookup(name);
    }
    for prefix in [':', '@', '$'] {
        let found = lookup(&format!("{prefix}{name}"));
        if found != 0 {
            return found;
        }
    }
    0
}

fn bind_value(stmt: *mut sys::sqlite3_stmt, i: c_int, value: &SqlValue) -> Result<(), Fail> {
    let rc = unsafe {
        match value {
            SqlValue::Null => sys::sqlite3_bind_null(stmt, i),
            SqlValue::Integer(v) => sys::sqlite3_bind_int64(stmt, i, *v),
            SqlValue::Real(v) => {
                if !v.is_finite() {
                    return Err(reject("binding"));
                }
                sys::sqlite3_bind_double(stmt, i, *v)
            }
            SqlValue::Text(s) => sys::sqlite3_bind_text64(
                stmt,
                i,
                s.as_ptr() as *const c_char,
                s.len() as u64,
                sys::SQLITE_TRANSIENT(),
                sys::SQLITE_UTF8 as u8,
            ),
            SqlValue::Blob(b) if b.is_empty() => sys::sqlite3_bind_zeroblob(stmt, i, 0),
            SqlValue::Blob(b) => sys::sqlite3_bind_blob64(
                stmt,
                i,
                b.as_ptr() as *const c_void,
                b.len() as u64,
                sys::SQLITE_TRANSIENT(),
            ),
        }
    };
    match rc & 0xff {
        sys::SQLITE_OK => Ok(()),
        sys::SQLITE_TOOBIG => Err(reject("too_large")),
        sys::SQLITE_NOMEM => Err(Fail::Fatal(ExecError::Operational { code: rc, message: "out of memory".into() })),
        _ => Err(reject("binding")),
    }
}

/// SQLite result codes (primary) that mean the environment failed, not that
/// the program is wrong. Mirrors chronolog `OPERATIONAL_SQLITE_CODES`.
const OPERATIONAL_CODES: &[c_int] = &[
    2,  // SQLITE_INTERNAL
    3,  // SQLITE_PERM
    4,  // SQLITE_ABORT
    5,  // SQLITE_BUSY
    6,  // SQLITE_LOCKED
    7,  // SQLITE_NOMEM
    8,  // SQLITE_READONLY
    9,  // SQLITE_INTERRUPT (budget interrupts are recognised first)
    10, // SQLITE_IOERR
    11, // SQLITE_CORRUPT
    12, // SQLITE_NOTFOUND
    13, // SQLITE_FULL
    14, // SQLITE_CANTOPEN
    15, // SQLITE_PROTOCOL
    17, // SQLITE_SCHEMA after automatic reprepare failed
    21, // SQLITE_MISUSE
    24, // SQLITE_FORMAT
    26, // SQLITE_NOTADB
];

const ASSERT_PREFIX: &str = "chronolog_assert:";
const POLICY_PREFIX: &str = "chronolog_policy:";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prepare,
    Step,
}

/// Map the connection's last error to a rejection code or a fatal error
/// (chronolog `rejectionFor`).
fn map_error(db: *mut sys::sqlite3, budget: &Budget, auth: &RefCell<Authorizer>, phase: Phase) -> Fail {
    let extended = unsafe { sys::sqlite3_extended_errcode(db) };
    let primary = extended & 0xff;
    let message = unsafe {
        let p = sys::sqlite3_errmsg(db);
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    };
    if budget.exceeded.get() && primary == sys::SQLITE_INTERRUPT {
        return reject("step_limit");
    }
    // The authorizer denied something. That is SQLITE_AUTH for a table, column
    // or action, but SQLITE_ERROR "not authorized to use function" for a
    // function, and a virtual table may wrap the message again. Chronolog
    // matches /authoriz/ in the message of any error, which also catches an
    // assertion code such as `unauthorized`; asking the authorizer is exact.
    if primary == sys::SQLITE_AUTH || auth.borrow().denied {
        return reject("denied");
    }
    if OPERATIONAL_CODES.contains(&primary) {
        return Fail::Fatal(ExecError::Operational { code: extended, message });
    }
    if phase == Phase::Prepare {
        return reject("prepare");
    }
    if extended == ffi::SQLITE_CONSTRAINT_FUNCTION {
        if let Some(at) = message.find(ASSERT_PREFIX) {
            return Fail::Reject(format!("assert:{}", &message[at + ASSERT_PREFIX.len()..]));
        }
        // Only platform triggers can call chronolog_policy_violation (the
        // authorizer denies it elsewhere).
        if let Some(at) = message.find(POLICY_PREFIX) {
            return Fail::Reject(format!("policy:{}", &message[at + POLICY_PREFIX.len()..]));
        }
    }
    match primary {
        sys::SQLITE_CONSTRAINT => reject("constraint"),
        sys::SQLITE_TOOBIG => reject("too_large"),
        sys::SQLITE_MISMATCH => reject("mismatch"),
        sys::SQLITE_RANGE => reject("binding"),
        _ => reject("error"),
    }
}

/// True when `text` is only whitespace, `;` and SQL comments. Whitespace is
/// JavaScript's `\s` (not Rust's), to stay identical to Chronolog.
pub(crate) fn is_trivia_only(text: &str) -> bool {
    fn is_js_space(c: char) -> bool {
        matches!(
            c,
            '\u{9}'..='\u{d}'
                | ' '
                | '\u{a0}'
                | '\u{1680}'
                | '\u{2000}'..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
        )
    }
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if is_js_space(c) || c == ';' {
            rest = &rest[c.len_utf8()..];
        } else if rest.starts_with("--") {
            match rest.find(['\n', '\r']) {
                Some(end) => rest = &rest[end..],
                None => return true,
            }
        } else if rest.starts_with("/*") {
            match rest[2..].find("*/") {
                Some(end) => rest = &rest[2 + end + 2..],
                None => return false,
            }
        } else {
            return false;
        }
    }
    true
}

// ───────────────────────── connection-level scopes ───────────────────────

fn exec_sql(db: *mut sys::sqlite3, sql: &str) -> Result<(), ExecError> {
    let c = CString::new(sql).expect("internal SQL has no NUL");
    let mut err: *mut c_char = ptr::null_mut();
    let rc = unsafe { sys::sqlite3_exec(db, c.as_ptr(), None, ptr::null_mut(), &mut err) };
    if rc == sys::SQLITE_OK {
        return Ok(());
    }
    let message = if err.is_null() {
        String::new()
    } else {
        let m = unsafe { CStr::from_ptr(err) }.to_string_lossy().into_owned();
        unsafe { sys::sqlite3_free(err as *mut c_void) };
        m
    };
    Err(ExecError::Operational { code: rc, message })
}

fn pragma_int(db: *mut sys::sqlite3, name: &str) -> Result<i64, ExecError> {
    let c = CString::new(format!("PRAGMA {name}")).unwrap();
    let mut raw = ptr::null_mut();
    let rc = unsafe { sys::sqlite3_prepare_v2(db, c.as_ptr(), -1, &mut raw, ptr::null_mut()) };
    let stmt = Stmt(raw);
    if rc != sys::SQLITE_OK || stmt.0.is_null() {
        return Err(ExecError::Operational { code: rc, message: format!("PRAGMA {name} failed") });
    }
    let rc = unsafe { sys::sqlite3_step(stmt.0) };
    if rc != sys::SQLITE_ROW {
        return Err(ExecError::Operational { code: rc, message: format!("PRAGMA {name} returned no row") });
    }
    Ok(unsafe { sys::sqlite3_column_int64(stmt.0, 0) })
}

/// Applies the execution profile to the connection (limits, security
/// `db_config`, `foreign_keys`, `recursive_triggers`) and restores what was
/// there on drop, so a connection that also serves ordinary SQL is not left
/// locked down.
struct ProfileScope {
    db: *mut sys::sqlite3,
    limits: Vec<(c_int, c_int)>,
    config: Vec<(c_int, c_int)>,
    recursive_triggers: Option<i64>,
    foreign_keys_restore: Option<c_int>,
}

impl ProfileScope {
    fn enter(db: *mut sys::sqlite3, l: &Limits, in_transaction: bool) -> Result<Self, ExecError> {
        let mut scope = Self { db, limits: vec![], config: vec![], recursive_triggers: None, foreign_keys_restore: None };
        // Limits (chronolog configureSqliteLimits). Applied, read back, verified.
        let wanted: [(c_int, c_int); 12] = [
            (ffi::LIMIT_LENGTH, l.max_value_bytes as c_int),
            (ffi::LIMIT_SQL_LENGTH, l.max_sql_bytes as c_int),
            (ffi::LIMIT_COLUMN, 256),
            (ffi::LIMIT_EXPR_DEPTH, 100),
            (ffi::LIMIT_COMPOUND_SELECT, 32),
            (ffi::LIMIT_VDBE_OP, l.max_vm_steps.min(i32::MAX as u64) as c_int),
            (ffi::LIMIT_FUNCTION_ARG, 64),
            (ffi::LIMIT_ATTACHED, 0),
            (ffi::LIMIT_LIKE_PATTERN_LENGTH, 10_000),
            (ffi::LIMIT_VARIABLE_NUMBER, 1_000),
            (ffi::LIMIT_TRIGGER_DEPTH, 16),
            (ffi::LIMIT_WORKER_THREADS, 0),
        ];
        for (id, value) in wanted {
            let old = unsafe { sys::sqlite3_limit(db, id, value) };
            scope.limits.push((id, old));
            let actual = unsafe { sys::sqlite3_limit(db, id, -1) };
            if actual != value {
                return Err(ExecError::Profile(format!("SQLite limit {id} did not apply (wanted {value}, got {actual})")));
            }
        }
        // Security configuration (chronolog configureSecurity). FP_DIGITS is
        // an integer; every other entry is a boolean.
        let config: [(c_int, c_int); 12] = [
            (ffi::DBCONFIG_DEFENSIVE, 1),
            (ffi::DBCONFIG_TRUSTED_SCHEMA, 0),
            (ffi::DBCONFIG_ENABLE_LOAD_EXTENSION, 0),
            (ffi::DBCONFIG_DQS_DML, 0),
            (ffi::DBCONFIG_DQS_DDL, 0),
            (ffi::DBCONFIG_ENABLE_QPSG, 1),
            (ffi::DBCONFIG_ENABLE_FTS3_TOKENIZER, 0),
            (ffi::DBCONFIG_WRITABLE_SCHEMA, 0),
            (ffi::DBCONFIG_ENABLE_ATTACH_CREATE, 0),
            (ffi::DBCONFIG_ENABLE_ATTACH_WRITE, 0),
            (ffi::DBCONFIG_REVERSE_SCANORDER, 0),
            (ffi::DBCONFIG_FP_DIGITS, 17),
        ];
        for (op, value) in config {
            let mut old: c_int = -1;
            if unsafe { sys::sqlite3_db_config(db, op, -1 as c_int, &mut old as *mut c_int) } != sys::SQLITE_OK {
                return Err(ExecError::Profile(format!("sqlite3_db_config({op}) is unavailable")));
            }
            scope.config.push((op, old));
            let mut applied: c_int = -1;
            let rc = unsafe { sys::sqlite3_db_config(db, op, value, &mut applied as *mut c_int) };
            if rc != sys::SQLITE_OK || applied != value {
                return Err(ExecError::Profile(format!("sqlite3_db_config({op}) did not apply")));
            }
        }
        // Pragmas of the execution profile.
        let mut fk: c_int = -1;
        unsafe { sys::sqlite3_db_config(db, ffi::DBCONFIG_ENABLE_FKEY, -1 as c_int, &mut fk as *mut c_int) };
        if fk != 1 {
            if in_transaction {
                return Err(ExecError::Profile(
                    "foreign_keys must be ON before the caller's transaction begins".into(),
                ));
            }
            exec_sql(db, "PRAGMA foreign_keys = ON")?;
            scope.foreign_keys_restore = Some(fk);
        }
        let recursive = pragma_int(db, "recursive_triggers")?;
        if recursive != 1 {
            scope.recursive_triggers = Some(recursive);
            exec_sql(db, "PRAGMA recursive_triggers = ON")?;
        }
        Ok(scope)
    }
}

impl Drop for ProfileScope {
    fn drop(&mut self) {
        let db = self.db;
        if self.recursive_triggers == Some(0) {
            let _ = exec_sql(db, "PRAGMA recursive_triggers = OFF");
        }
        if let Some(old) = self.foreign_keys_restore {
            // Skipped when a transaction is still open: foreign_keys cannot change inside one.
            if unsafe { sys::sqlite3_get_autocommit(db) } != 0 && old == 0 {
                let _ = exec_sql(db, "PRAGMA foreign_keys = OFF");
            }
        }
        for &(op, old) in self.config.iter().rev() {
            let mut out: c_int = 0;
            unsafe { sys::sqlite3_db_config(db, op, old, &mut out as *mut c_int) };
        }
        for &(id, old) in self.limits.iter().rev() {
            unsafe { sys::sqlite3_limit(db, id, old) };
        }
    }
}

/// Holds the per-connection deterministic context (patch 0002) and clears it
/// on drop.
pub struct DetScope {
    db: *mut sys::sqlite3,
}

impl DetScope {
    fn set_raw(db: *mut sys::sqlite3, now_ms: i64, seed: u64, flags: u32) -> Result<Self, ExecError> {
        let rc = unsafe { ffi::sqlite3_doltlite_set_det_context(db, now_ms, seed, flags) };
        if rc != sys::SQLITE_OK {
            return Err(ExecError::Operational { code: rc, message: "sqlite3_doltlite_set_det_context failed".into() });
        }
        Ok(Self { db })
    }
}

impl Drop for DetScope {
    fn drop(&mut self) {
        unsafe { ffi::sqlite3_doltlite_clear_det_context(self.db) };
    }
}

/// Pin the clock (and PRNG, UTC) of `conn` until the returned guard drops.
///
/// Use it around Dolt calls that are not programs, for example a pinned
/// `dolt_commit` or a genesis commit; [`execute`] sets its own.
pub fn pin_clock(conn: &Connection, now_ms: i64, seed: u64) -> Result<DetScope, ExecError> {
    DetScope::set_raw(
        unsafe { conn.handle() },
        now_ms,
        seed,
        ffi::SQLITE_DOLTLITE_DET_UTC_LOCALTIME | ffi::SQLITE_DOLTLITE_DET_RESET_COUNTERS,
    )
}

/// True while a deterministic context is set on `conn`.
pub fn det_context_active(conn: &Connection) -> bool {
    unsafe { ffi::sqlite3_doltlite_det_context_active(conn.handle()) != 0 }
}

// ─────────────────────── chronolog_* native functions ────────────────────

#[derive(Default)]
struct ExecutionIdentity {
    actor: Option<Vec<u8>>,
    actor_id: Option<i64>,
    transaction_id: Option<Vec<u8>>,
}

/// Per-connection identity, freed by SQLite when the connection closes.
pub struct ExecState {
    identity: Mutex<ExecutionIdentity>,
    program_pinned: Cell<bool>,
    program_serial: Cell<u64>,
    program_limits: Cell<Limits>,
    program_steps: Cell<usize>,
    program_foreign_keys: Cell<Option<bool>>,
}

struct ProgramPin<'a> {
    db: *mut sys::sqlite3,
    state: &'a ExecState,
}

impl Drop for ProgramPin<'_> {
    fn drop(&mut self) {
        clear_program_context(self.db, self.state);
    }
}

fn install_program_context(db: *mut sys::sqlite3, state: &ExecState, ctx: &ExecContext) -> Result<(), ExecError> {
    let rc = unsafe {
        ffi::sqlite3_doltlite_set_det_context(db, ctx.now_ms, ctx.seed,
            ffi::SQLITE_DOLTLITE_DET_UTC_LOCALTIME | ffi::SQLITE_DOLTLITE_DET_RESET_COUNTERS)
    };
    if rc != sys::SQLITE_OK {
        return Err(ExecError::Operational { code: rc, message: "cannot pin SQL program context".into() });
    }
    *state.identity.lock().unwrap() = ExecutionIdentity {
        actor: ctx.actor.clone(),
        actor_id: ctx.actor_id,
        transaction_id: ctx.transaction_id.clone(),
    };
    state.program_limits.set(ctx.limits);
    state.program_steps.set(0);
    state.program_pinned.set(true);
    Ok(())
}

fn clear_program_context(db: *mut sys::sqlite3, state: &ExecState) {
    if state.program_pinned.replace(false) {
        *state.identity.lock().unwrap() = ExecutionIdentity::default();
        unsafe { ffi::sqlite3_doltlite_clear_det_context(db) };
    }
}

/// Pin the deterministic inputs once for a caller-owned SQL transaction.
/// The owning runtime must clear the scope on commit, rollback and unwind.
pub fn pin_transaction_program(conn: &Connection, context: &ExecContext) -> Result<u64, ExecError> {
    context.limits.validate()?;
    validate_context(context)?;
    if conn.is_autocommit() {
        return Err(ExecError::TransactionState("program pin requires an open storage transaction"));
    }
    if det_context_active(conn) {
        return Err(ExecError::TransactionState("the connection already has a pinned context"));
    }
    if !foreign_keys(conn)? {
        return Err(ExecError::Profile("foreign_keys must be ON before the caller's transaction begins".into()));
    }
    let db = unsafe { conn.handle() };
    let state = unsafe { register_state(db) }?;
    let serial = state.program_serial.get().checked_add(1)
        .ok_or_else(|| ExecError::InvalidInput("program scope sequence exhausted".into()))?;
    install_program_context(db, state, context)?;
    state.program_serial.set(serial);
    Ok(serial)
}

fn existing_program_state(conn: &Connection) -> Option<&ExecState> {
    let db = unsafe { conn.handle() };
    let known = unsafe { ffi::sqlite3_get_clientdata(db, CLIENT_DATA_KEY.as_ptr()) };
    if known.is_null() {
        None
    } else {
        Some(unsafe { &*(known as *const ExecState) })
    }
}

const FOREIGN_KEYS: rusqlite::config::DbConfig = rusqlite::config::DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY;

fn foreign_keys(conn: &Connection) -> Result<bool, ExecError> {
    conn.db_config(FOREIGN_KEYS)
        .map_err(|error| ExecError::Profile(format!("read foreign_keys: {error}")))
}

fn set_foreign_keys(conn: &Connection, enabled: bool) -> Result<(), ExecError> {
    let actual = conn.set_db_config(FOREIGN_KEYS, enabled)
        .map_err(|error| ExecError::Profile(format!("set foreign_keys: {error}")))?;
    if actual != enabled {
        return Err(ExecError::Profile("foreign_keys did not apply".into()));
    }
    Ok(())
}

/// Set up the connection before the runtime begins a program transaction.
pub fn prepare_program_transaction(conn: &Connection) -> Result<(), ExecError> {
    if !conn.is_autocommit() {
        return Err(ExecError::TransactionState("a program transaction must open at the outermost boundary"));
    }
    let state = unsafe { register_state(conn.handle()) }?;
    if state.program_foreign_keys.get().is_some() {
        return Err(ExecError::TransactionState("the previous program transaction has not ended"));
    }
    let previous = foreign_keys(conn)?;
    if !previous {
        set_foreign_keys(conn, true)?;
    }
    state.program_foreign_keys.set(Some(previous));
    Ok(())
}

pub fn program_transaction_prepared(conn: &Connection) -> bool {
    existing_program_state(conn).is_some_and(|state| state.program_foreign_keys.get().is_some())
}

/// Restore the configuration only after the native transaction has ended.
pub fn finish_program_transaction(conn: &Connection) -> Result<(), ExecError> {
    if let Some(state) = existing_program_state(conn) {
        if let Some(previous) = state.program_foreign_keys.get() {
            if conn.is_autocommit() {
                clear_transaction_program(conn);
                if !previous {
                    set_foreign_keys(conn, false)?;
                }
                state.program_foreign_keys.set(None);
            }
        }
    }
    Ok(())
}

pub fn transaction_program_id(conn: &Connection) -> Option<u64> {
    let state = existing_program_state(conn)?;
    state.program_pinned.get().then(|| state.program_serial.get())
}

/// Clear only a program pin. This is safe after SQLite has already canceled
/// the transaction itself and after an earlier successful clear.
pub fn clear_transaction_program(conn: &Connection) {
    if let Some(state) = existing_program_state(conn) {
        clear_program_context(unsafe { conn.handle() }, state);
    }
}

/// A statement in the same native transaction and deterministic stream.
pub fn run_pinned_transaction_step(
    conn: &Connection, statement: &Statement, mode: AuthMode, schema_admin: bool,
) -> Result<Outcome, ExecError> {
    if conn.is_autocommit() {
        return Err(ExecError::TransactionState("program step requires an open pinned transaction"));
    }
    let state = existing_program_state(conn).filter(|state| state.program_pinned.get())
        .ok_or(ExecError::TransactionState("program step requires an open pinned transaction"))?;
    if !matches!(mode, AuthMode::Consensus | AuthMode::Platform) {
        return Err(ExecError::InvalidInput("program step requires a writable profile".into()));
    }
    let limits = state.program_limits.get();
    if mode == AuthMode::Consensus {
        let steps = state.program_steps.get();
        if steps >= limits.max_steps {
            return Err(ExecError::InvalidInput(format!("a program can have at most {} steps", limits.max_steps)));
        }
        state.program_steps.set(steps + 1);
    }
    let _profile = ProfileScope::enter(unsafe { conn.handle() }, &limits, true)?;
    match run_statement(conn, statement, &limits, mode, schema_admin) {
        Ok(()) => Ok(Outcome::Accepted),
        Err(Fail::Reject(code)) => Ok(Outcome::Rejected { code, statement_index: None }),
        Err(Fail::Fatal(error)) => Err(error),
    }
}

const CLIENT_DATA_KEY: &CStr = c"celld.dolt_exec.v1";

unsafe extern "C" fn drop_state(p: *mut c_void) {
    drop(Box::from_raw(p as *mut ExecState));
}

/// Register the Chronolog SQL functions on this connection. Idempotent.
/// [`execute`] calls it, and ordinary SQL can register them to use the same
/// schema defaults. Actor and transaction identities are NULL outside execution.
pub fn register_functions(conn: &Connection) -> Result<(), ExecError> {
    // SAFETY: the handle is valid for the borrow of `conn`.
    unsafe { register_state(conn.handle()) }.map(|_| ())
}

/// # Safety
/// `db` must be a live connection handle, and the returned reference must not
/// outlive it (it is freed when the connection closes).
unsafe fn register_state<'a>(db: *mut sys::sqlite3) -> Result<&'a ExecState, ExecError> {
    {
        let existing = ffi::sqlite3_get_clientdata(db, CLIENT_DATA_KEY.as_ptr());
        if !existing.is_null() {
            return Ok(&*(existing as *const ExecState));
        }
        let state = Box::into_raw(Box::new(ExecState {
            identity: Mutex::default(),
            program_pinned: Cell::new(false),
            program_serial: Cell::new(0),
            program_limits: Cell::new(Limits::default()),
            program_steps: Cell::new(0),
            program_foreign_keys: Cell::new(None),
        }));
        let flags = sys::SQLITE_UTF8 | sys::SQLITE_INNOCUOUS;
        let defs: [(&CStr, c_int, c_int, unsafe extern "C" fn(*mut sys::sqlite3_context, c_int, *mut *mut sys::sqlite3_value)); 7] = [
            (c"chronolog_assert", 2, flags | sys::SQLITE_DETERMINISTIC, chronolog_assert),
            (c"chronolog_actor", 0, flags, chronolog_actor),
            (c"chronolog_actor_id", 0, flags, chronolog_actor_id),
            (c"chronolog_transaction_id", 0, flags, chronolog_transaction_id),
            (c"chronolog_policy_violation", 2, flags, chronolog_policy_violation),
            (c"chronolog_row_key", -1, flags | sys::SQLITE_DETERMINISTIC, crate::owner_values::row_key),
            (c"chronolog_value_equal", 2, flags | sys::SQLITE_DETERMINISTIC, crate::owner_values::value_equal),
        ];
        for (name, n_args, f, func) in defs {
            let rc = sys::sqlite3_create_function_v2(db, name.as_ptr(), n_args, f, ptr::null_mut(), Some(func), None, None, None);
            if rc != sys::SQLITE_OK {
                drop(Box::from_raw(state));
                return Err(ExecError::Operational { code: rc, message: format!("cannot register {name:?}") });
            }
        }
        let rc = ffi::sqlite3_set_clientdata(db, CLIENT_DATA_KEY.as_ptr(), state as *mut c_void, Some(drop_state));
        if rc != sys::SQLITE_OK {
            return Err(ExecError::Operational { code: rc, message: "cannot attach execution state".into() });
        }
        // Owned by the connection from here, until it closes.
        Ok(&*state)
    }
}

unsafe fn text_arg(value: *mut sys::sqlite3_value, max: c_int) -> Option<Vec<u8>> {
    // Check the type before asking for text: sqlite3_value_text converts in place.
    if sys::sqlite3_value_type(value) != sys::SQLITE_TEXT {
        return None;
    }
    let n = sys::sqlite3_value_bytes(value);
    let p = sys::sqlite3_value_text(value);
    if p.is_null() || n < 1 || n > max {
        return None;
    }
    Some(std::slice::from_raw_parts(p, n as usize).to_vec())
}

unsafe fn result_constraint_function(ctx: *mut sys::sqlite3_context, message: &[u8]) {
    sys::sqlite3_result_error(ctx, message.as_ptr() as *const c_char, message.len() as c_int);
    sys::sqlite3_result_error_code(ctx, ffi::SQLITE_CONSTRAINT_FUNCTION);
}

/// `chronolog_assert(condition, code)`: 1 when `condition` is true (non-zero
/// and not NULL), otherwise a SQLITE_CONSTRAINT_FUNCTION error whose message is
/// `chronolog_assert:<code>`. `code` is text of 1 to 128 bytes.
unsafe extern "C" fn chronolog_assert(ctx: *mut sys::sqlite3_context, _argc: c_int, argv: *mut *mut sys::sqlite3_value) {
    let args = std::slice::from_raw_parts(argv, 2);
    let Some(code) = text_arg(args[1], 128) else {
        let m = c"chronolog_assert: code must be text of 1 to 128 bytes";
        sys::sqlite3_result_error(ctx, m.as_ptr(), -1);
        return;
    };
    let truthy = match sys::sqlite3_value_numeric_type(args[0]) {
        sys::SQLITE_NULL => false,
        sys::SQLITE_INTEGER => sys::sqlite3_value_int64(args[0]) != 0,
        _ => sys::sqlite3_value_double(args[0]) != 0.0,
    };
    if truthy {
        sys::sqlite3_result_int(ctx, 1);
        return;
    }
    let mut message = ASSERT_PREFIX.as_bytes().to_vec();
    message.extend_from_slice(&code);
    result_constraint_function(ctx, &message);
}

/// The function context keeps its owning connection live for this call.
unsafe fn with_execution_identity<R>(
    ctx: *mut sys::sqlite3_context,
    action: impl FnOnce(Option<&ExecutionIdentity>) -> R,
) -> R {
    let db = sys::sqlite3_context_db_handle(ctx);
    let state = ffi::sqlite3_get_clientdata(db, CLIENT_DATA_KEY.as_ptr()) as *const ExecState;
    let identity = state.as_ref().map(|state| state.identity.lock().unwrap());
    action(identity.as_deref())
}

unsafe fn result_identity_blob(ctx: *mut sys::sqlite3_context, value: Option<&[u8]>) {
    match value {
        Some(bytes) => sys::sqlite3_result_blob(
            ctx, bytes.as_ptr() as *const c_void, bytes.len() as c_int, sys::SQLITE_TRANSIENT(),
        ),
        None => sys::sqlite3_result_null(ctx),
    }
}

/// `chronolog_actor()`: the signing key of the current program.
unsafe extern "C" fn chronolog_actor(ctx: *mut sys::sqlite3_context, _argc: c_int, _argv: *mut *mut sys::sqlite3_value) {
    with_execution_identity(ctx, |identity| {
        result_identity_blob(ctx, identity.and_then(|value| value.actor.as_deref()));
    });
}

/// `chronolog_actor_id()`: the signer's principal row for the current program.
unsafe extern "C" fn chronolog_actor_id(ctx: *mut sys::sqlite3_context, _argc: c_int, _argv: *mut *mut sys::sqlite3_value) {
    with_execution_identity(ctx, |identity| match identity.and_then(|value| value.actor_id) {
        Some(id) => sys::sqlite3_result_int64(ctx, id),
        None => sys::sqlite3_result_null(ctx),
    });
}

/// `chronolog_transaction_id()`: the current signed transaction identity.
unsafe extern "C" fn chronolog_transaction_id(ctx: *mut sys::sqlite3_context, _argc: c_int, _argv: *mut *mut sys::sqlite3_value) {
    with_execution_identity(ctx, |identity| {
        result_identity_blob(ctx, identity.and_then(|value| value.transaction_id.as_deref()));
    });
}

/// `chronolog_policy_violation(table, command)`: always a
/// SQLITE_CONSTRAINT_FUNCTION error `chronolog_policy:<table>:<command>`. The
/// authorizer allows it only inside `chronolog_*` triggers and views.
unsafe extern "C" fn chronolog_policy_violation(ctx: *mut sys::sqlite3_context, _argc: c_int, argv: *mut *mut sys::sqlite3_value) {
    let args = std::slice::from_raw_parts(argv, 2);
    let (Some(table), Some(command)) = (text_arg(args[0], 128), text_arg(args[1], 128)) else {
        let m = c"chronolog_policy_violation: arguments must be text of 1 to 128 bytes";
        sys::sqlite3_result_error(ctx, m.as_ptr(), -1);
        return;
    };
    let mut message = POLICY_PREFIX.as_bytes().to_vec();
    message.extend_from_slice(&table);
    message.push(b':');
    message.extend_from_slice(&command);
    result_constraint_function(ctx, &message);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trivia_matches_chronolog() {
        for ok in ["", " ", ";", " ; ;\n", "-- c", "-- c\n;", "/* x */", "/* a */ ; -- b\n", "\u{a0}\u{feff}"] {
            assert!(is_trivia_only(ok), "{ok:?}");
        }
        for bad in ["SELECT 1", "/* open", "- x", "/", "; DELETE FROM t", "\u{85}"] {
            assert!(!is_trivia_only(bad), "{bad:?}");
        }
    }

    #[test]
    fn seed_from_tx_id() {
        assert_eq!(prng_seed_from_tx_id(&[0, 0, 0, 0, 9]), 1);
        assert_eq!(prng_seed_from_tx_id(&[1, 2, 3, 4, 5]), 0x0102_0304);
        assert_eq!(prng_seed_from_tx_id(&[0xff, 0xff, 0xff, 0xff]), 0xffff_ffff);
        assert_eq!(prng_seed_from_tx_id(&[7]), 7);
    }
}
