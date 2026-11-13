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

use crate::authorizer::{AuthMode, AuthOptions, Authorizer};
use crate::ffi;
use rusqlite::ffi as sys;
use rusqlite::Connection;
use std::cell::{Cell, RefCell};
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
    pub profile: Profile,
    pub limits: Limits,
}

impl ExecContext {
    pub fn new(now_ms: i64, seed: u64) -> Self {
        Self { now_ms, seed, actor: None, profile: Profile::default(), limits: Limits::default() }
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
    let _profile = ProfileScope::enter(db, &ctx.limits, !own)?;
    if own {
        exec_sql(db, "BEGIN IMMEDIATE")?;
    }
    match run_inner(db, program, ctx, own, state) {
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
    db: *mut sys::sqlite3,
    program: &Program,
    ctx: &ExecContext,
    own: bool,
    state: &ExecState,
) -> Result<Outcome, ExecError> {
    exec_sql(db, &format!("SAVEPOINT {SAVEPOINT_NAME}"))?;
    let rejection = {
        let _det = DetScope::set(db, ctx)?;
        *state.actor.lock().unwrap() = ctx.actor.clone();
        let _actor = ActorScope(state);
        run_steps(db, program, ctx)?
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

fn run_steps(db: *mut sys::sqlite3, program: &Program, ctx: &ExecContext) -> Result<Option<(String, usize)>, ExecError> {
    let Profile::Consensus { schema_admin } = ctx.profile;
    for (index, statement) in program.steps.iter().enumerate() {
        match run_statement(db, statement, &ctx.limits, schema_admin) {
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
    if !(0..=253_402_300_799_999).contains(&ctx.now_ms) {
        return Err(ExecError::InvalidInput("now_ms is out of range".into()));
    }
    if let Some(actor) = &ctx.actor {
        if actor.is_empty() || actor.len() > 64 {
            return Err(ExecError::InvalidInput("actor must be 1 to 64 bytes".into()));
        }
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
    db: *mut sys::sqlite3,
    statement: &Statement,
    limits: &Limits,
    schema_admin: bool,
) -> Result<(), Fail> {
    let sql = statement.sql.as_bytes();
    if sql.contains(&0) {
        return Err(reject("prepare")); // SQL_INVALID_SOURCE
    }
    if sql.len() > limits.max_sql_bytes {
        return Err(reject("too_large"));
    }
    let auth = RefCell::new(Authorizer::new(AuthMode::Consensus, AuthOptions { schema_admin }));
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
    let auth = RefCell::new(Authorizer::new(mode, options));
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
    if unsafe { sys::sqlite3_step(stmt.0) } != sys::SQLITE_ROW {
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
    recursive_triggers: i64,
    foreign_keys_restore: Option<c_int>,
}

impl ProfileScope {
    fn enter(db: *mut sys::sqlite3, l: &Limits, in_transaction: bool) -> Result<Self, ExecError> {
        let mut scope = Self { db, limits: vec![], config: vec![], recursive_triggers: 0, foreign_keys_restore: None };
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
        scope.recursive_triggers = pragma_int(db, "recursive_triggers")?;
        if scope.recursive_triggers != 1 {
            exec_sql(db, "PRAGMA recursive_triggers = ON")?;
        }
        Ok(scope)
    }
}

impl Drop for ProfileScope {
    fn drop(&mut self) {
        let db = self.db;
        if self.recursive_triggers != 1 {
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
    fn set(db: *mut sys::sqlite3, ctx: &ExecContext) -> Result<Self, ExecError> {
        Self::set_raw(
            db,
            ctx.now_ms,
            ctx.seed,
            ffi::SQLITE_DOLTLITE_DET_UTC_LOCALTIME | ffi::SQLITE_DOLTLITE_DET_RESET_COUNTERS,
        )
    }

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

/// Per-connection state behind `chronolog_actor()`, stored as SQLite client
/// data so it is freed with the connection.
pub struct ExecState {
    actor: Mutex<Option<Vec<u8>>>,
}

struct ActorScope<'a>(&'a ExecState);

impl Drop for ActorScope<'_> {
    fn drop(&mut self) {
        *self.0.actor.lock().unwrap() = None;
    }
}

const CLIENT_DATA_KEY: &CStr = c"celld.dolt_exec.v1";

unsafe extern "C" fn drop_state(p: *mut c_void) {
    drop(Box::from_raw(p as *mut ExecState));
}

/// Register `chronolog_assert`, `chronolog_actor` and
/// `chronolog_policy_violation` on the connection. Idempotent. [`execute`]
/// calls it; call it yourself to use `chronolog_actor()` (returns NULL outside
/// an execution) in ordinary SQL such as a column DEFAULT.
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
        let state = Box::into_raw(Box::new(ExecState { actor: Mutex::new(None) }));
        let flags = sys::SQLITE_UTF8 | sys::SQLITE_INNOCUOUS;
        let defs: [(&CStr, c_int, c_int, unsafe extern "C" fn(*mut sys::sqlite3_context, c_int, *mut *mut sys::sqlite3_value)); 3] = [
            (c"chronolog_assert", 2, flags | sys::SQLITE_DETERMINISTIC, chronolog_assert),
            (c"chronolog_actor", 0, flags, chronolog_actor),
            (c"chronolog_policy_violation", 2, flags, chronolog_policy_violation),
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

/// `chronolog_actor()`: the signer of the program being executed, else NULL.
unsafe extern "C" fn chronolog_actor(ctx: *mut sys::sqlite3_context, _argc: c_int, _argv: *mut *mut sys::sqlite3_value) {
    let db = sys::sqlite3_context_db_handle(ctx);
    let p = ffi::sqlite3_get_clientdata(db, CLIENT_DATA_KEY.as_ptr());
    if p.is_null() {
        sys::sqlite3_result_null(ctx);
        return;
    }
    let state = &*(p as *const ExecState);
    match &*state.actor.lock().unwrap() {
        Some(actor) => sys::sqlite3_result_blob(ctx, actor.as_ptr() as *const c_void, actor.len() as c_int, sys::SQLITE_TRANSIENT()),
        None => sys::sqlite3_result_null(ctx),
    }
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
