//! Declarations for the entry points celld's DoltLite patches add (patch
//! 0002), plus SQLite 3.44+ client-data calls the bundled bindings predate.
//!
//! Everything here is `unsafe` and used only from this crate's safe wrappers.

use rusqlite::ffi::sqlite3;
use std::os::raw::{c_char, c_int, c_uint, c_void};

pub const SQLITE_DOLTLITE_DET_UTC_LOCALTIME: c_uint = 0x01;
pub const SQLITE_DOLTLITE_DET_RESET_COUNTERS: c_uint = 0x02;

extern "C" {
    pub fn sqlite3_doltlite_set_det_context(
        db: *mut sqlite3,
        unix_ms: i64,
        seed: u64,
        flags: c_uint,
    ) -> c_int;
    pub fn sqlite3_doltlite_clear_det_context(db: *mut sqlite3) -> c_int;
    pub fn sqlite3_doltlite_det_context_active(db: *mut sqlite3) -> c_int;

    pub fn sqlite3_get_clientdata(db: *mut sqlite3, name: *const c_char) -> *mut c_void;
    pub fn sqlite3_set_clientdata(
        db: *mut sqlite3,
        name: *const c_char,
        data: *mut c_void,
        destructor: Option<unsafe extern "C" fn(*mut c_void)>,
    ) -> c_int;
}

// sqlite3_db_config verbs and SQLITE_LIMIT_* ids from the pinned sqlite3.h.
pub const DBCONFIG_ENABLE_FKEY: c_int = 1002;
pub const DBCONFIG_ENABLE_FTS3_TOKENIZER: c_int = 1004;
pub const DBCONFIG_ENABLE_LOAD_EXTENSION: c_int = 1005;
pub const DBCONFIG_ENABLE_QPSG: c_int = 1007;
pub const DBCONFIG_DEFENSIVE: c_int = 1010;
pub const DBCONFIG_WRITABLE_SCHEMA: c_int = 1011;
pub const DBCONFIG_DQS_DML: c_int = 1013;
pub const DBCONFIG_DQS_DDL: c_int = 1014;
pub const DBCONFIG_TRUSTED_SCHEMA: c_int = 1017;
pub const DBCONFIG_REVERSE_SCANORDER: c_int = 1019;
pub const DBCONFIG_ENABLE_ATTACH_CREATE: c_int = 1020;
pub const DBCONFIG_ENABLE_ATTACH_WRITE: c_int = 1021;
pub const DBCONFIG_FP_DIGITS: c_int = 1023;

pub const LIMIT_LENGTH: c_int = 0;
pub const LIMIT_SQL_LENGTH: c_int = 1;
pub const LIMIT_COLUMN: c_int = 2;
pub const LIMIT_EXPR_DEPTH: c_int = 3;
pub const LIMIT_COMPOUND_SELECT: c_int = 4;
pub const LIMIT_VDBE_OP: c_int = 5;
pub const LIMIT_FUNCTION_ARG: c_int = 6;
pub const LIMIT_ATTACHED: c_int = 7;
pub const LIMIT_LIKE_PATTERN_LENGTH: c_int = 8;
pub const LIMIT_VARIABLE_NUMBER: c_int = 9;
pub const LIMIT_TRIGGER_DEPTH: c_int = 10;
pub const LIMIT_WORKER_THREADS: c_int = 11;

/// SQLITE_CONSTRAINT | (4 << 8)
pub const SQLITE_CONSTRAINT_FUNCTION: c_int = 19 | (4 << 8);
