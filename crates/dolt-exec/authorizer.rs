//! The consensus authorizer profile (Chronolog Spec 10 section 5), a port of
//! `createAuthorizer` in chronolog `materializer-doltlite/src/sql-profile.ts`.
//!
//! It is a block-list that fails closed: anything it does not recognise, such
//! as an action code a future SQLite adds, is denied. The decision function is
//! pure (strings in, verdict out) so the whole table can be tested without a
//! database; `exec` installs it as `sqlite3_set_authorizer`.

use std::collections::HashSet;

/// SQLite authorizer action codes (`sqlite3.h`).
pub mod action {
    pub const CREATE_INDEX: i32 = 1;
    pub const CREATE_TABLE: i32 = 2;
    pub const CREATE_TEMP_INDEX: i32 = 3;
    pub const CREATE_TEMP_TABLE: i32 = 4;
    pub const CREATE_TEMP_TRIGGER: i32 = 5;
    pub const CREATE_TEMP_VIEW: i32 = 6;
    pub const CREATE_TRIGGER: i32 = 7;
    pub const CREATE_VIEW: i32 = 8;
    pub const DELETE: i32 = 9;
    pub const DROP_INDEX: i32 = 10;
    pub const DROP_TABLE: i32 = 11;
    pub const DROP_TEMP_INDEX: i32 = 12;
    pub const DROP_TEMP_TABLE: i32 = 13;
    pub const DROP_TEMP_TRIGGER: i32 = 14;
    pub const DROP_TEMP_VIEW: i32 = 15;
    pub const DROP_TRIGGER: i32 = 16;
    pub const DROP_VIEW: i32 = 17;
    pub const INSERT: i32 = 18;
    pub const PRAGMA: i32 = 19;
    pub const READ: i32 = 20;
    pub const SELECT: i32 = 21;
    pub const TRANSACTION: i32 = 22;
    pub const UPDATE: i32 = 23;
    pub const ATTACH: i32 = 24;
    pub const DETACH: i32 = 25;
    pub const ALTER_TABLE: i32 = 26;
    pub const REINDEX: i32 = 27;
    pub const ANALYZE: i32 = 28;
    pub const CREATE_VTABLE: i32 = 29;
    pub const DROP_VTABLE: i32 = 30;
    pub const FUNCTION: i32 = 31;
    pub const SAVEPOINT: i32 = 32;
    pub const RECURSIVE: i32 = 33;
}

/// Raises a policy rejection; callable only from `chronolog_*` triggers and views.
pub const POLICY_VIOLATION_FUNCTION: &str = "chronolog_policy_violation";
/// The owner column every registered table carries (owner-rows, not enforced in v1).
pub const OWNER_COLUMN: &str = "chronolog_owner";

/// Read-only pragmas whose answers depend only on the schema and the engine
/// build. Allowed as statements and as table-valued functions.
pub const READ_ONLY_PRAGMAS: &[&str] = &[
    "collation_list",
    "compile_options",
    "foreign_key_list",
    "function_list",
    "index_info",
    "index_list",
    "index_xinfo",
    "module_list",
    "pragma_list",
    "table_info",
    "table_list",
    "table_xinfo",
    // SQLite runs quick_check itself while executing ALTER TABLE ... ADD COLUMN.
    "quick_check",
];

/// Functions whose result depends on connection history, physical storage,
/// Dolt history or the host.
///
/// `fts5_locale` is celld's addition to Chronolog's list: it embeds a
/// per-connection random header, so storing its result would diverge replicas.
pub const CONSENSUS_DENIED_FUNCTIONS: &[&str] = &[
    "load_extension",
    "total_changes",
    "sqlite_offset",
    "active_branch",
    "fts5_locale",
];

/// Virtual-table modules that read physical storage rather than SQL state.
///
/// `vec1` is celld's addition: its k-means index build seeds from the global
/// PRNG (and may use worker threads), so its shadow state is not reproducible.
pub const CONSENSUS_DENIED_MODULES: &[&str] = &["dbstat", "sqlite_dbpage", "sqlite_stmt", "vec1"];

/// Local SQL is not replayed, so only external effects and Dolt control are denied.
pub const FORBIDDEN_LOCAL_READ_FUNCTIONS: &[&str] = &["active_branch", "load_extension"];

/// The Dolt functions a local read may call: they only read the commit graph
/// and name commits and tables by hash, so an application can find where two
/// branches diverged (`dolt_merge_base`) or pin a state (`dolt_hashof`) in a
/// read of `dolt_at_<t>(ref)`, `dolt_history_<t>`, `dolt_diff_<t>` or
/// `dolt_blame_<t>`. Every other Dolt function is denied in every mode: a
/// replicated program must not see per-replica history, and the versioning
/// functions run through `ctx.storage.dolt`.
pub const LOCAL_READ_DOLT_FUNCTIONS: &[&str] = &[
    "dolt_hashof",
    "dolt_hashof_table",
    "dolt_hashof_db",
    "dolt_hashof_catalog",
    "dolt_hashof_index",
    "dolt_merge_base",
];

/// Which rule set the authorizer applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthMode {
    /// Trusted setup code: everything except ATTACH/DETACH.
    InternalBootstrap,
    /// A replicated transaction's statements.
    Consensus,
    /// Trusted platform operations (owner-rows hook): may write `chronolog_*`
    /// objects. App triggers they fire stay under consensus rules.
    Platform,
    /// A local, unreplicated read.
    LocalRead,
}

/// Options for the consensus profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AuthOptions {
    /// The signer may run DDL (an active admin of the `group` collective in
    /// Chronolog). `ANALYZE` and `REINDEX` are open to every writer.
    pub schema_admin: bool,
}

/// Names owned by SQLite, Dolt, Chronolog or the celld host (`_cf_*`: KV,
/// alarms and other runtime tables kept inside a Dolt cell's store) rather
/// than the application.
pub fn is_reserved_schema_object_name(value: &str) -> bool {
    let key = ascii_lower(value);
    is_host_name(&key)
        || key.starts_with("chronolog_")
        || key.starts_with("sqlite_")
        || key.starts_with("dolt_")
        || key.starts_with("doltlite_")
        || key.starts_with("pragma_")
}

/// celld's runtime tables. They hold per-cell host state (KV, alarms, wake
/// records) that is not part of any replicated program history, so a program
/// may neither write nor read them.
fn is_host_name(lowered: &str) -> bool {
    lowered.starts_with("_cf_")
}

fn is_dolt_name(value: &str) -> bool {
    value.starts_with("dolt_") || value.starts_with("doltlite_")
}

/// ASCII-only case folding, as SQLite itself folds identifiers.
pub fn ascii_lower(value: &str) -> String {
    value.chars().map(|c| c.to_ascii_lowercase()).collect()
}

/// One authorizer per statement: it remembers which tables the statement
/// drops, because `DROP TABLE` reports a `DROP_TRIGGER` for each of the
/// table's triggers, including its `chronolog_*` platform triggers.
#[derive(Debug)]
pub struct Authorizer {
    mode: AuthMode,
    options: AuthOptions,
    dropped_tables: HashSet<String>,
    /// Set while the statement is being stepped (not prepared): authorizer
    /// calls then come from SQL a virtual-table module runs internally.
    pub in_step: bool,
    /// Whether this authorizer has denied anything. A failed statement with a
    /// denial behind it is a `denied` rejection, whatever message SQLite or a
    /// virtual-table module wrapped it in.
    pub denied: bool,
}

impl Authorizer {
    pub fn new(mode: AuthMode, options: AuthOptions) -> Self {
        Self { mode, options, dropped_tables: HashSet::new(), in_step: false, denied: false }
    }

    /// `true` to allow (SQLITE_OK), `false` to deny (SQLITE_DENY). Arguments
    /// are the raw callback arguments: `arg1`/`arg2` per action, `database`
    /// (arg3) and `trigger_or_view` (arg4).
    pub fn allow(
        &mut self,
        action_code: i32,
        arg1: Option<&str>,
        arg2: Option<&str>,
        database: Option<&str>,
        trigger_or_view: Option<&str>,
    ) -> bool {
        let verdict = self.decide(action_code, arg1, arg2, database, trigger_or_view);
        self.denied |= !verdict;
        verdict
    }

    fn decide(
        &mut self,
        action_code: i32,
        arg1: Option<&str>,
        arg2: Option<&str>,
        database: Option<&str>,
        trigger_or_view: Option<&str>,
    ) -> bool {
        use action as a;
        let object = arg1.map(ascii_lower).unwrap_or_default();
        let secondary = arg2.map(ascii_lower).unwrap_or_default();
        let database = database.map(ascii_lower).unwrap_or_default();
        let caller = trigger_or_view.map(ascii_lower).unwrap_or_default();
        let mode = self.mode;

        if mode == AuthMode::InternalBootstrap {
            return action_code != a::ATTACH && action_code != a::DETACH;
        }
        // Platform code is trusted, and so are the platform's own triggers and
        // views it runs. App triggers fired by a platform statement are
        // compiled inside it; they get exactly the consensus rules.
        let platform = mode == AuthMode::Platform && (caller.is_empty() || caller.starts_with("chronolog_"));
        if platform {
            return action_code != a::ATTACH && action_code != a::DETACH && action_code != a::TRANSACTION;
        }
        if action_code == a::SELECT || action_code == a::RECURSIVE {
            return true;
        }
        if action_code == a::FUNCTION {
            let function = if secondary.is_empty() { &object } else { &secondary };
            if is_dolt_name(function) {
                return mode == AuthMode::LocalRead && LOCAL_READ_DOLT_FUNCTIONS.contains(&function.as_str());
            }
            // Only platform triggers and views may raise a policy rejection.
            if function == POLICY_VIOLATION_FUNCTION {
                return caller.starts_with("chronolog_");
            }
            if mode == AuthMode::LocalRead {
                return !FORBIDDEN_LOCAL_READ_FUNCTIONS.contains(&function.as_str());
            }
            if let Some(pragma) = function.strip_prefix("pragma_") {
                return READ_ONLY_PRAGMAS.contains(&pragma);
            }
            return !CONSENSUS_DENIED_FUNCTIONS.contains(&function.as_str());
        }
        if action_code == a::PRAGMA {
            // FTS5 (`PRAGMA <db>.data_version`) and R-Tree (`PRAGMA <db>.page_size`)
            // read these from inside CREATE VIRTUAL TABLE and queries. A
            // program's own PRAGMA is authorized while its statement is
            // prepared, never while it steps, so a read of them is internal.
            if self.in_step && secondary.is_empty() && (object == "data_version" || object == "page_size") {
                return true;
            }
            return READ_ONLY_PRAGMAS.contains(&object.as_str());
        }
        if action_code == a::READ {
            // SQLite may omit the database name on follow-on READ callbacks.
            if !database.is_empty() && database != "main" && database != "temp" {
                return false;
            }
            // Dolt system tables expose per-replica history (commit hashes, times).
            if mode != AuthMode::LocalRead && is_dolt_name(&object) {
                return false;
            }
            if is_host_name(&ascii_lower(&object)) {
                return false;
            }
            return true;
        }
        if mode == AuthMode::LocalRead {
            return false;
        }

        // ---- consensus writes ----
        if action_code == a::INSERT || action_code == a::UPDATE || action_code == a::DELETE {
            // SQLite maintains its own catalogs (sqlite_schema, sqlite_sequence,
            // sqlite_stat1, the temp catalog) while executing DDL and DML.
            if object.starts_with("sqlite_") {
                return database == "main" || database == "temp";
            }
            if database != "main" {
                return false;
            }
            return !is_reserved_schema_object_name(&object);
        }
        // The first ANALYZE creates SQLite's own statistics table; any writer may
        // ANALYZE. The first AUTOINCREMENT table (or a vec0 table, whose
        // `_chunks` shadow table uses one) creates sqlite_sequence; Chronolog's
        // materialized database already has it from bootstrap, a fresh Dolt
        // cell does not.
        if action_code == a::CREATE_TABLE
            && (object == "sqlite_stat1" || object == "sqlite_stat4" || object == "sqlite_sequence")
        {
            return database == "main";
        }
        // All other DDL, including triggers, is for schema administrators only.
        let is_ddl = matches!(
            action_code,
            a::CREATE_INDEX
                | a::CREATE_TABLE
                | a::CREATE_TRIGGER
                | a::CREATE_VIEW
                | a::DROP_INDEX
                | a::DROP_TABLE
                | a::DROP_TRIGGER
                | a::DROP_VIEW
                | a::CREATE_VTABLE
                | a::DROP_VTABLE
                | a::ALTER_TABLE
        );
        if is_ddl && !self.options.schema_admin {
            return false;
        }
        if matches!(
            action_code,
            a::CREATE_TABLE
                | a::CREATE_INDEX
                | a::CREATE_VIEW
                | a::CREATE_TRIGGER
                | a::DROP_TABLE
                | a::DROP_INDEX
                | a::DROP_VIEW
                | a::DROP_TRIGGER
        ) {
            if database != "main" {
                return false;
            }
            // CREATE INDEX reports the table as arg2; implicit autoindexes are SQLite's own.
            if action_code == a::CREATE_INDEX && object.starts_with("sqlite_autoindex_") {
                return !is_reserved_schema_object_name(&secondary);
            }
            // Dropping an application table drops its platform triggers with it.
            if action_code == a::DROP_TRIGGER
                && object.starts_with("chronolog_")
                && self.dropped_tables.contains(&secondary)
            {
                return true;
            }
            if is_reserved_schema_object_name(&object) {
                return false;
            }
            if action_code == a::DROP_TABLE {
                self.dropped_tables.insert(object.clone());
            }
            let on_table = matches!(
                action_code,
                a::CREATE_INDEX | a::CREATE_TRIGGER | a::DROP_INDEX | a::DROP_TRIGGER
            );
            return !(on_table && !secondary.is_empty() && is_reserved_schema_object_name(&secondary));
        }
        if action_code == a::CREATE_VTABLE || action_code == a::DROP_VTABLE {
            if database != "main" || is_reserved_schema_object_name(&object) {
                return false;
            }
            return !CONSENSUS_DENIED_MODULES.contains(&secondary.as_str());
        }
        // SQLITE_ALTER_TABLE reports the database and table through arg1/arg2,
        // and DROP COLUMN reports the dropped column as the third argument.
        if action_code == a::ALTER_TABLE {
            if database == OWNER_COLUMN {
                return false;
            }
            return object == "main" && !is_reserved_schema_object_name(&secondary);
        }
        // Both only rebuild derived structures (indexes, sqlite_stat1) from table contents.
        if action_code == a::REINDEX || action_code == a::ANALYZE {
            return true;
        }
        // Temporary objects, transaction control, savepoints, ATTACH/DETACH and
        // unknown future action codes fail closed.
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use action as a;

    fn allow(mode: AuthMode, admin: bool, code: i32, a1: Option<&str>, a2: Option<&str>, db: Option<&str>, tv: Option<&str>) -> bool {
        Authorizer::new(mode, AuthOptions { schema_admin: admin }).allow(code, a1, a2, db, tv)
    }

    #[test]
    fn unknown_action_codes_fail_closed() {
        for code in [0, 34, 99, 1000, -1] {
            assert!(!allow(AuthMode::Consensus, true, code, None, None, None, None), "{code}");
        }
    }

    #[test]
    fn reserved_names_are_case_insensitive() {
        for n in ["Chronolog_x", "SQLITE_master", "DOLT_log", "Doltlite_x", "PRAGMA_table_info"] {
            assert!(is_reserved_schema_object_name(n), "{n}");
        }
        assert!(!is_reserved_schema_object_name("accounts"));
        assert!(is_reserved_schema_object_name("_cf_KV"));
        assert!(is_reserved_schema_object_name("_CF_alarm"));
        // Only ASCII is folded: a Turkish dotted capital I does not make a reserved prefix.
        assert!(!is_reserved_schema_object_name("sqlıte_x"));
    }

    #[test]
    fn dolt_functions_denied_in_every_non_bootstrap_mode() {
        for mode in [AuthMode::Consensus, AuthMode::LocalRead] {
            assert!(!allow(mode, true, a::FUNCTION, None, Some("dolt_commit"), None, None));
            assert!(!allow(mode, true, a::FUNCTION, None, Some("DOLTLITE_x"), None, None));
            assert!(!allow(mode, true, a::FUNCTION, None, Some("dolt_conflicts_resolve"), None, None));
        }
        // The read-only ones a local read may call; a replicated program may not.
        for function in LOCAL_READ_DOLT_FUNCTIONS {
            assert!(allow(AuthMode::LocalRead, true, a::FUNCTION, None, Some(function), None, None));
            assert!(!allow(AuthMode::Consensus, true, a::FUNCTION, None, Some(function), None, None));
        }
        assert!(allow(AuthMode::InternalBootstrap, true, a::FUNCTION, None, Some("dolt_commit"), None, None));
    }

    #[test]
    fn drop_table_authorizes_its_platform_triggers() {
        let mut auth = Authorizer::new(AuthMode::Consensus, AuthOptions { schema_admin: true });
        assert!(auth.allow(a::DROP_TABLE, Some("notes"), None, Some("main"), None));
        assert!(auth.allow(a::DROP_TRIGGER, Some("chronolog_k_notes_upd"), Some("notes"), Some("main"), None));
        // A trigger on another table is still denied.
        assert!(!auth.allow(a::DROP_TRIGGER, Some("chronolog_k_other_upd"), Some("other"), Some("main"), None));
    }
}
