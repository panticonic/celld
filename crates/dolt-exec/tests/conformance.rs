//! Chronolog Spec 10 conformance: a port of the SQL security tests
//! (`sql-security.test.ts`), the replicated-SQL integration tests
//! (`sql-materializer.integration.test.ts`) and a differential corpus, run
//! through `celld_dolt_exec::execute` on Dolt (prolly) stores.

// Test code: the workspace clippy.toml restrictions are for the celld runtime.
#![allow(clippy::disallowed_macros, clippy::disallowed_methods, clippy::excessive_precision)]

mod common;

use celld_dolt_exec::{
    check_statement, execute, execute_in_transaction, prng_seed_from_tx_id, AuthMode, AuthOptions, ExecContext,
    ExecError, Limits, Outcome, Parameter, Profile, Program, SqlValue, Statement,
};
use common::*;
use rusqlite::Connection;

// ───────────────────────────── helpers ─────────────────────────────

fn s(sql: &str) -> Statement {
    Statement::new(sql)
}

fn prog(steps: &[&str]) -> Program {
    Program::new(steps.iter().map(|x| s(x)).collect())
}

fn admin_ctx(n: i64) -> ExecContext {
    let mut c = ExecContext::new(T0 + n * 1000, n as u64 + 1);
    c.profile = Profile::Consensus { schema_admin: true };
    c.actor = Some(vec![n as u8 + 1; 32]);
    c
}

fn user_ctx(n: i64) -> ExecContext {
    let mut c = admin_ctx(n);
    c.profile = Profile::Consensus { schema_admin: false };
    c
}

fn accepted(conn: &Connection, p: &Program, ctx: &ExecContext) {
    let out = execute(conn, p, ctx).unwrap();
    assert_eq!(out, Outcome::Accepted, "{p:?}");
}

fn rejected(conn: &Connection, p: &Program, ctx: &ExecContext) -> (String, Option<usize>) {
    match execute(conn, p, ctx).unwrap() {
        Outcome::Rejected { code, statement_index } => (code, statement_index),
        Outcome::Accepted => panic!("expected a rejection: {p:?}"),
    }
}

/// The database the Chronolog security tests build, plus reserved names.
fn security_db() -> (tempfile::TempDir, Connection) {
    let (d, c) = fresh_dolt();
    c.execute_batch(
        "CREATE TABLE accounts (id INTEGER PRIMARY KEY AUTOINCREMENT, value TEXT NOT NULL);
         INSERT INTO accounts VALUES (1, 'one'), (2, 'two');
         CREATE TABLE \"DoltOn\" (id INTEGER PRIMARY KEY, value TEXT NOT NULL);
         INSERT INTO \"DoltOn\" VALUES (1, 'ordinary application table');
         CREATE TABLE chronolog_transactions (tx_id BLOB PRIMARY KEY, outcome TEXT NOT NULL);
         CREATE TABLE chronolog_materializer_metadata (key TEXT PRIMARY KEY, value TEXT);
         CREATE TABLE chronolog_checkpoints (prefix_length INTEGER PRIMARY KEY);",
    )
    .unwrap();
    (d, c)
}

const ADMIN: AuthOptions = AuthOptions { schema_admin: true };
const NO_ADMIN: AuthOptions = AuthOptions { schema_admin: false };

fn denied(c: &Connection, sql: &str, mode: AuthMode, opts: AuthOptions) {
    assert_eq!(check_statement(c, sql, mode, opts), Err("denied".to_string()), "{sql}");
}

fn allowed(c: &Connection, sql: &str, mode: AuthMode, opts: AuthOptions) {
    assert_eq!(check_statement(c, sql, mode, opts), Ok(()), "{sql}");
}

// ─────────────── ported: SQL authorization block-list ───────────────

#[test]
fn allows_ordinary_sqlite_in_consensus_including_formerly_gated_functions() {
    let (_d, c) = security_db();
    for sql in [
        "UPDATE accounts SET value = lower(value) WHERE sign(id) = 1",
        "UPDATE \"DoltOn\" SET value = upper(value) WHERE id = 1",
        "SELECT random(), randomblob(4), datetime('now'), changes(), last_insert_rowid(), printf('%.2f', 1.5)",
        "SELECT group_concat(value), sum(id), avg(id), total(id) FROM accounts GROUP BY value COLLATE NOCASE",
        "SELECT value, row_number() OVER (ORDER BY id) FROM accounts",
        "SELECT json_group_array(value) FROM accounts",
        "SELECT tx_id, outcome FROM chronolog_transactions",
        "SELECT name FROM pragma_table_info('accounts')",
        "PRAGMA table_info('accounts')",
        "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)",
        "CREATE INDEX accounts_value ON accounts (value)",
        "CREATE VIEW names AS SELECT value FROM accounts",
        "CREATE TRIGGER accounts_touch AFTER UPDATE ON accounts BEGIN SELECT RAISE(ABORT, 'no'); END",
        "ALTER TABLE accounts ADD COLUMN extra TEXT",
        "CREATE VIRTUAL TABLE docs USING fts5(body)",
        "WITH RECURSIVE n(v) AS (VALUES(1) UNION ALL SELECT v + 1 FROM n WHERE v < 5) SELECT count(*) FROM n",
    ] {
        allowed(&c, sql, AuthMode::Consensus, ADMIN);
    }
}

#[test]
fn allows_analyze_for_every_writer() {
    let (_d, c) = security_db();
    allowed(&c, "ANALYZE", AuthMode::Consensus, NO_ADMIN);
    allowed(&c, "REINDEX", AuthMode::Consensus, NO_ADMIN);
    accepted(&c, &prog(&["ANALYZE"]), &user_ctx(0));
}

#[test]
fn denies_escapes_from_the_database_and_the_pinned_engine() {
    let (_d, c) = security_db();
    for sql in [
        "ATTACH DATABASE ':memory:' AS other",
        "DETACH DATABASE other",
        "BEGIN",
        "COMMIT",
        "ROLLBACK",
        "SAVEPOINT nested",
        "RELEASE nested",
        "PRAGMA writable_schema = 1",
        "PRAGMA foreign_keys = OFF",
        "PRAGMA journal_mode = WAL",
        "CREATE TEMP TABLE scratch (x)",
        "CREATE TEMP VIEW scratch AS SELECT 1",
        "CREATE TEMP TRIGGER tt AFTER INSERT ON accounts BEGIN SELECT 1; END",
        "SELECT total_changes()",
        "SELECT load_extension('x')",
        "SELECT dolt_commit('-m', 'x')",
        "SELECT dolt_version()",
        "SELECT * FROM dolt_log",
        "SELECT * FROM DOLT_STATUS",
        "SELECT * FROM dolt_diff('HEAD', 'WORKING')",
        "CREATE VIRTUAL TABLE pages USING dbstat",
        "CREATE VIRTUAL TABLE pages USING sqlite_dbpage",
        // celld additions to Chronolog's list, see authorizer.rs:
        "CREATE VIRTUAL TABLE ann USING vec1(v)",
        "SELECT fts5_locale('en', 'text')",
        "DELETE FROM chronolog_transactions",
        "INSERT INTO chronolog_materializer_metadata VALUES ('a', 'b')",
        "DROP TABLE chronolog_checkpoints",
        "CREATE INDEX hijack ON chronolog_transactions (outcome)",
        "CREATE TABLE chronolog_extra (id INTEGER)",
        "ALTER TABLE chronolog_transactions ADD COLUMN x",
        "DELETE FROM dolt_ignore",
    ] {
        denied(&c, sql, AuthMode::Consensus, ADMIN);
    }
}

#[test]
fn sqlite_and_dolt_reserved_names_are_refused_by_the_engine_itself() {
    let (_d, c) = security_db();
    // Rejected either way; SQLite gets there before the authorizer does.
    for sql in ["CREATE TABLE sqlite_extra (id INTEGER)", "CREATE TABLE dolt_extra (id INTEGER)"] {
        assert_eq!(check_statement(&c, sql, AuthMode::Consensus, ADMIN), Err("prepare".into()), "{sql}");
    }
}

#[test]
fn limits_ddl_to_schema_admins_and_protects_platform_objects() {
    let (_d, c) = security_db();
    c.execute_batch(
        "CREATE TABLE notes (id INTEGER PRIMARY KEY, chronolog_owner BLOB);
         CREATE TRIGGER chronolog_k_notes_upd BEFORE UPDATE ON notes BEGIN SELECT 1; END;
         CREATE VIEW chronolog_my_owner_ids_update AS SELECT 1 AS id",
    )
    .unwrap();
    for sql in [
        "CREATE TABLE t (id INTEGER PRIMARY KEY)",
        "CREATE INDEX notes_id ON notes (id)",
        "CREATE VIEW v AS SELECT 1",
        "CREATE TRIGGER app AFTER INSERT ON notes BEGIN SELECT 1; END",
        "CREATE VIRTUAL TABLE docs USING fts5(body)",
        "ALTER TABLE notes ADD COLUMN extra TEXT",
        "DROP TABLE notes",
    ] {
        denied(&c, sql, AuthMode::Consensus, NO_ADMIN);
        allowed(&c, sql, AuthMode::Consensus, ADMIN);
    }
    for sql in [
        "DROP TRIGGER chronolog_k_notes_upd",
        "DROP VIEW chronolog_my_owner_ids_update",
        "CREATE TRIGGER chronolog_k_x BEFORE INSERT ON notes BEGIN SELECT 1; END",
        "ALTER TABLE notes DROP COLUMN chronolog_owner",
        "SELECT chronolog_policy_violation('notes', 'update')",
        "UPDATE chronolog_transactions SET outcome = 1",
    ] {
        denied(&c, sql, AuthMode::Consensus, ADMIN);
    }
    // Dropping a table takes its platform triggers with it.
    allowed(&c, "DROP TABLE notes", AuthMode::Consensus, ADMIN);
    // Platform code may write and create chronolog_* objects.
    allowed(&c, "UPDATE chronolog_transactions SET outcome = 1", AuthMode::Platform, NO_ADMIN);
    allowed(&c, "CREATE TRIGGER chronolog_k_x BEFORE INSERT ON notes BEGIN SELECT 1; END", AuthMode::Platform, NO_ADMIN);
}

#[test]
fn app_triggers_fired_by_platform_statements_get_consensus_rules() {
    let (_d, c) = security_db();
    c.execute_batch(
        "CREATE TABLE notes (id INTEGER PRIMARY KEY, chronolog_owner BLOB);
         CREATE TRIGGER app_forge AFTER UPDATE ON notes BEGIN DELETE FROM chronolog_transactions; END;",
    )
    .unwrap();
    denied(&c, "UPDATE notes SET chronolog_owner = NULL", AuthMode::Platform, NO_ADMIN);
    c.execute_batch("DROP TRIGGER app_forge; CREATE TRIGGER app_touch AFTER UPDATE ON notes BEGIN SELECT 1; END").unwrap();
    allowed(&c, "UPDATE notes SET chronolog_owner = NULL", AuthMode::Platform, NO_ADMIN);
}

#[test]
fn local_reads_stay_read_only_while_allowing_ambient_functions() {
    let (_d, c) = security_db();
    allowed(&c, "SELECT datetime('now'), random(), total_changes()", AuthMode::LocalRead, NO_ADMIN);
    allowed(&c, "SELECT * FROM chronolog_transactions", AuthMode::LocalRead, NO_ADMIN);
    allowed(&c, "SELECT * FROM dolt_log", AuthMode::LocalRead, NO_ADMIN);
    for sql in ["DELETE FROM accounts", "CREATE TABLE x (id)", "SELECT load_extension('x')", "PRAGMA writable_schema = 1", "SELECT active_branch()"] {
        denied(&c, sql, AuthMode::LocalRead, NO_ADMIN);
    }
    // dolt_* functions are never available, even to local reads.
    denied(&c, "SELECT dolt_commit('-m', 'x')", AuthMode::LocalRead, NO_ADMIN);
}

#[test]
fn single_statement_check_uses_the_native_tail_not_string_slicing() {
    let (_d, c) = security_db();
    allowed(&c, "SELECT 'é'; -- allowed trailing trivia", AuthMode::LocalRead, NO_ADMIN);
    assert_eq!(check_statement(&c, "SELECT 'é'; DELETE FROM accounts", AuthMode::LocalRead, NO_ADMIN), Err("prepare".into()));
    assert_eq!(check_statement(&c, "SELECT 1\0; DELETE FROM accounts", AuthMode::LocalRead, NO_ADMIN), Err("prepare".into()));
    assert_eq!(check_statement(&c, "", AuthMode::LocalRead, NO_ADMIN), Err("prepare".into()));
    assert_eq!(check_statement(&c, "-- only a comment", AuthMode::LocalRead, NO_ADMIN), Err("prepare".into()));
}

// ───────── ported: execution (replicated SQL integration tests) ─────────

#[test]
fn executes_ddl_and_dml_in_order_and_keeps_the_state_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("application.db");
    {
        let c = open_dolt(&path, Some(GENESIS));
        let p = Program::new(vec![
            s("CREATE TABLE accounts (id INTEGER PRIMARY KEY, value TEXT NOT NULL) STRICT"),
            s("INSERT INTO accounts (id, value) VALUES (?, ?) RETURNING id")
                .bind(Parameter::Index(1), SqlValue::Integer(2))
                .bind(Parameter::Index(2), SqlValue::Text("two".into())),
            s("SELECT id, value FROM accounts ORDER BY id"),
        ]);
        accepted(&c, &p, &admin_ctx(1));
    }
    let c = open_dolt(&path, None);
    assert_eq!(rows_as_strings(&c, "SELECT id, value FROM accounts"), vec![vec!["2", "two"]]);
}

#[test]
fn rejects_with_the_assertion_code_and_rolls_back_schema_and_data() {
    let (_d, c) = fresh_dolt();
    accepted(
        &c,
        &prog(&[
            "CREATE TABLE accounts (id INTEGER PRIMARY KEY, balance INTEGER NOT NULL) STRICT",
            "INSERT INTO accounts (id, balance) VALUES (1, 10)",
        ]),
        &admin_ctx(1),
    );
    let before = state_digest(&c);
    let withdraw = Program::new(vec![
        s("CREATE TABLE audit (id INTEGER PRIMARY KEY, note TEXT)"),
        s("SELECT chronolog_assert((SELECT balance FROM accounts WHERE id = 1) >= :amount, 'insufficient_funds')")
            .bind(Parameter::Name("amount".into()), SqlValue::Integer(50)),
        s("UPDATE accounts SET balance = balance - 50 WHERE id = 1"),
    ]);
    assert_eq!(rejected(&c, &withdraw, &admin_ctx(5)), ("assert:insufficient_funds".to_string(), Some(1)));
    assert_eq!(q1::<i64>(&c, "SELECT count(*) FROM sqlite_schema WHERE name = 'audit'"), 0);
    assert_eq!(state_digest(&c), before, "a rejection leaves no trace");
    assert!(c.is_autocommit(), "execute() leaves no transaction open");
    // The same transaction replayed after a late deposit now passes.
    accepted(&c, &prog(&["UPDATE accounts SET balance = balance + 100 WHERE id = 1"]), &admin_ctx(3));
    accepted(&c, &withdraw, &admin_ctx(5));
    assert_eq!(q1::<i64>(&c, "SELECT balance FROM accounts"), 60);
    // Rejected programs and Dolt: the working set still commits normally afterwards.
    run(&c, "SELECT dolt_commit('-A','-m','after rejections')");
}

#[test]
fn pins_now_random_localtime_and_counters_identically_on_two_fresh_stores() {
    let probe = Program::new(vec![s("INSERT INTO probes (tx, now, local, r, blob, last_rowid, changed) VALUES
        ('probe', datetime('now'), datetime('now', 'localtime'), random(), randomblob(8), last_insert_rowid(), changes())")]);
    let setup = prog(&["CREATE TABLE probes (tx TEXT PRIMARY KEY, now TEXT, local TEXT, r INTEGER, blob BLOB, last_rowid INTEGER, changed INTEGER)"]);
    let late = prog(&["CREATE TABLE marker (id INTEGER PRIMARY KEY)", "INSERT INTO marker (id) VALUES (7)"]);
    let mut ctx = ExecContext::new(T0, prng_seed_from_tx_id(b"probe-tx-id"));
    ctx.profile = Profile::Consensus { schema_admin: true };

    let (_d1, first) = fresh_dolt();
    accepted(&first, &setup, &admin_ctx(1));
    // Dirty the connection's counters first: the program must not see them.
    first.execute_batch("CREATE TABLE scratch(a); INSERT INTO scratch VALUES (1),(2);").unwrap();
    accepted(&first, &probe, &ctx);
    accepted(&first, &late, &admin_ctx(2));

    let (_d2, second) = fresh_dolt();
    accepted(&second, &setup, &admin_ctx(1));
    accepted(&second, &late, &admin_ctx(2));
    accepted(&second, &probe, &ctx);

    let q = "SELECT now, local, r, hex(blob), last_rowid, changed FROM probes";
    assert_eq!(rows_as_strings(&first, q), rows_as_strings(&second, q));
    let row = &rows_as_strings(&first, q)[0];
    assert_eq!(row[0], "2023-11-14 22:13:20");
    assert_eq!(row[1], "2023-11-14 22:13:20", "localtime is UTC");
    assert_eq!((row[4].as_str(), row[5].as_str()), ("0", "0"));
    assert_eq!(state_digest(&first).len(), 64);
}

#[test]
fn accepts_ordinary_sqlite_aggregates_windows_json_triggers_views_generated_columns() {
    let (_d, c) = fresh_dolt();
    accepted(
        &c,
        &prog(&[
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT COLLATE NOCASE, price REAL, doubled REAL GENERATED ALWAYS AS (price * 2))",
            "INSERT INTO items (name, price) VALUES ('a', 1.5), ('A', 2.25), ('b', 3)",
            "CREATE TABLE totals (name TEXT PRIMARY KEY, total REAL, names TEXT, rank INTEGER)",
            "INSERT INTO totals (name, total, names, rank) SELECT name, sum(price), group_concat(name), rank() OVER (ORDER BY sum(price) DESC) FROM items GROUP BY name",
            "CREATE TABLE log (id INTEGER PRIMARY KEY, message TEXT)",
            "CREATE TRIGGER items_log AFTER INSERT ON items BEGIN INSERT INTO log (message) VALUES ('added ' || NEW.name); END",
            "CREATE VIEW cheap AS SELECT name FROM items WHERE price < 2",
            "ALTER TABLE items ADD COLUMN note TEXT",
            "INSERT INTO items (name, price) VALUES (json_extract('{\"n\":\"c\"}', '$.n'), 1.0)",
        ]),
        &admin_ctx(1),
    );
    assert_eq!(q1::<i64>(&c, "SELECT count(*) FROM totals"), 2);
    assert_eq!(q1::<String>(&c, "SELECT message FROM log"), "added c");
    assert_eq!(q1::<f64>(&c, "SELECT doubled FROM items WHERE name = 'c'"), 2.0);
}

#[test]
fn non_admins_cannot_run_ddl_and_the_program_is_rejected_as_denied() {
    let (_d, c) = fresh_dolt();
    assert_eq!(rejected(&c, &prog(&["SELECT 1", "CREATE TABLE t (id INTEGER)"]), &user_ctx(1)), ("denied".to_string(), Some(1)));
    assert_eq!(q1::<i64>(&c, "SELECT count(*) FROM sqlite_schema"), 0);
}

#[test]
fn rejects_denied_malformed_unbound_over_budget_and_constraint_violating_statements() {
    let (_d, c) = fresh_dolt();
    c.execute_batch("CREATE TABLE chronolog_transactions (tx_id BLOB PRIMARY KEY, outcome TEXT NOT NULL)").unwrap();
    accepted(&c, &prog(&["CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT NOT NULL)"]), &admin_ctx(0));
    let unbound = s("INSERT INTO t (id, v) VALUES (?, ?)").bind(Parameter::Index(1), SqlValue::Integer(1));
    let cases: Vec<(&str, Statement, &str)> = vec![
        ("attach", s("ATTACH DATABASE ':memory:' AS other"), "denied"),
        ("pragma", s("PRAGMA writable_schema = 1"), "denied"),
        ("log-write", s("DELETE FROM chronolog_transactions"), "denied"),
        ("temp", s("CREATE TEMP TABLE scratch (x)"), "denied"),
        ("dolt", s("SELECT dolt_commit('-m', 'x')"), "denied"),
        ("dolt-table", s("SELECT * FROM dolt_log"), "denied"),
        ("savepoint", s("SAVEPOINT inner_sp"), "denied"),
        ("begin", s("BEGIN"), "denied"),
        ("total-changes", s("SELECT total_changes()"), "denied"),
        ("syntax", s("SELEC 1"), "prepare"),
        ("no-such-table", s("SELECT * FROM missing"), "prepare"),
        ("empty", s("  "), "prepare"),
        ("two-statements", s("SELECT 1; SELECT 2"), "prepare"),
        ("double-quoted-string", s("SELECT \"not a column\""), "prepare"),
        ("unbound", unbound, "binding"),
        ("not-null", s("INSERT INTO t (id) VALUES (1)"), "constraint"),
        ("steps", s("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) SELECT count(*) FROM n"), "step_limit"),
        ("assert-short-code", s("SELECT chronolog_assert(0, 'x')"), "assert:x"),
        ("assert-null", s("SELECT chronolog_assert(NULL, 'null_is_false')"), "assert:null_is_false"),
        ("assert-text-zero", s("SELECT chronolog_assert('abc', 'text_is_false')"), "assert:text_is_false"),
        // A code containing "authoriz" is still an assertion: only SQLITE_AUTH is a denial.
        ("assert-authoriz", s("SELECT chronolog_assert(0, 'unauthorized')"), "assert:unauthorized"),
        ("assert-bad-code-type", s("SELECT chronolog_assert(1, 5)"), "error"),
        ("assert-empty-code", s("SELECT chronolog_assert(1, '')"), "error"),
        ("too-big-value", s("SELECT CAST(x'ff' AS TEXT) || zeroblob(2000000000)"), "too_large"),
        ("mismatch", s("INSERT INTO t (id, v) VALUES ('abc', 'x')"), "mismatch"),
    ];
    let mut limits = Limits::default();
    limits.max_vm_steps = 5_000;
    for (name, statement, code) in cases {
        let mut ctx = admin_ctx(1);
        ctx.limits = limits;
        let p = Program::new(vec![s("SELECT 1"), statement]);
        let got = rejected(&c, &p, &ctx);
        assert_eq!((name, got.0.as_str(), got.1), (name, code, Some(1)));
    }
}

#[test]
fn a_write_that_exhausts_the_step_budget_is_a_rejection_and_leaves_no_trace() {
    let (_d, c) = fresh_dolt();
    accepted(&c, &prog(&["CREATE TABLE t (n INTEGER PRIMARY KEY)", "INSERT INTO t VALUES (1)"]), &admin_ctx(0));
    let before = state_digest(&c);
    let mut ctx = admin_ctx(1);
    ctx.limits.max_vm_steps = 5_000;
    let p = prog(&[
        "INSERT INTO t VALUES (2)",
        "WITH RECURSIVE n(i) AS (SELECT 10 UNION ALL SELECT i + 1 FROM n) INSERT INTO t SELECT i FROM n",
    ]);
    // SQLite aborts the whole transaction when a write statement is interrupted.
    assert_eq!(rejected(&c, &p, &ctx), ("step_limit".to_string(), Some(1)));
    assert!(c.is_autocommit());
    assert_eq!(state_digest(&c), before);
    // The connection is fine: the next program runs.
    accepted(&c, &prog(&["INSERT INTO t VALUES (3)"]), &admin_ctx(2));
    assert_eq!(q1::<i64>(&c, "SELECT count(*) FROM t"), 2);
}

#[test]
fn trigger_bodies_are_authorized_when_they_fire() {
    let (_d, c) = fresh_dolt();
    accepted(
        &c,
        &prog(&[
            "CREATE TABLE t (id INTEGER PRIMARY KEY)",
            "CREATE TABLE log (m TEXT)",
            "CREATE TRIGGER sneaky AFTER INSERT ON t BEGIN INSERT INTO log VALUES (dolt_commit('-A','-m','x')); END",
        ]),
        &admin_ctx(0),
    );
    // Creating the trigger passes; the INSERT that fires it does not.
    assert_eq!(rejected(&c, &prog(&["INSERT INTO t VALUES (1)"]), &user_ctx(1)), ("denied".to_string(), Some(0)));
    assert_eq!(q1::<i64>(&c, "SELECT count(*) FROM t"), 0);
}

#[test]
fn trigger_recursion_is_a_deterministic_error_and_raise_abort_is_a_constraint() {
    let (_d, c) = fresh_dolt();
    accepted(
        &c,
        &prog(&[
            "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER)",
            "CREATE TRIGGER loop AFTER INSERT ON t BEGIN INSERT INTO t (n) VALUES (NEW.n + 1); END",
        ]),
        &admin_ctx(0),
    );
    assert_eq!(rejected(&c, &prog(&["INSERT INTO t (n) VALUES (0)"]), &admin_ctx(1)), ("error".to_string(), Some(0)));
    accepted(
        &c,
        &prog(&[
            "DROP TRIGGER loop",
            "CREATE TRIGGER nope BEFORE INSERT ON t BEGIN SELECT RAISE(ABORT, 'nope'); END",
        ]),
        &admin_ctx(2),
    );
    assert_eq!(rejected(&c, &prog(&["INSERT INTO t (n) VALUES (0)"]), &admin_ctx(3)), ("constraint".to_string(), Some(0)));
}

#[test]
fn recursive_triggers_are_on_so_replace_fires_delete_triggers() {
    let (_d, c) = fresh_dolt();
    accepted(
        &c,
        &prog(&[
            "CREATE TABLE t (k INTEGER PRIMARY KEY, v TEXT)",
            "CREATE TABLE gone (k INTEGER)",
            "CREATE TRIGGER t_del AFTER DELETE ON t BEGIN INSERT INTO gone VALUES (OLD.k); END",
            "INSERT INTO t VALUES (1, 'a')",
            "INSERT OR REPLACE INTO t VALUES (1, 'b')",
        ]),
        &admin_ctx(0),
    );
    assert_eq!(q1::<i64>(&c, "SELECT count(*) FROM gone"), 1);
    // The profile is restored: a plain connection has recursive_triggers off.
    assert_eq!(q1::<i64>(&c, "PRAGMA recursive_triggers"), 0);
}

#[test]
fn lets_a_transaction_depend_on_an_earlier_outcome() {
    let (_d, c) = fresh_dolt();
    // Chronolog's log table; here the test plays the materializer and records outcomes.
    c.execute_batch("CREATE TABLE chronolog_transactions (tx_id BLOB PRIMARY KEY, outcome TEXT NOT NULL)").unwrap();
    accepted(&c, &prog(&["CREATE TABLE t (id INTEGER PRIMARY KEY)"]), &admin_ctx(0));
    c.execute("INSERT INTO chronolog_transactions VALUES (X'6669727374', 'accepted')", []).unwrap();
    let second = Program::new(vec![
        s("SELECT chronolog_assert(EXISTS (SELECT 1 FROM chronolog_transactions WHERE tx_id = ? AND outcome = 'accepted'), 'needs_first')")
            .bind(Parameter::Index(1), SqlValue::Blob(b"first".to_vec())),
        s("INSERT INTO t (id) VALUES (1)"),
    ]);
    accepted(&c, &second, &admin_ctx(1));
    let orphan = Program::new(vec![s("SELECT chronolog_assert(EXISTS (SELECT 1 FROM chronolog_transactions WHERE tx_id = ?), 'needs_missing')")
        .bind(Parameter::Index(1), SqlValue::Blob(b"missing".to_vec()))]);
    assert_eq!(rejected(&c, &orphan, &admin_ctx(2)).0, "assert:needs_missing");
}

#[test]
fn binds_real_empty_blob_and_named_parameters_exactly() {
    let (_d, c) = fresh_dolt();
    let p = Program::new(vec![
        s("CREATE TABLE v (id INTEGER PRIMARY KEY, a ANY, b ANY, c ANY, d ANY, e ANY) STRICT"),
        s("INSERT INTO v (a, b, c, d, e) VALUES (:real, :int, :blob, :text, @n)")
            .bind(Parameter::Name("real".into()), SqlValue::Real(1.0))
            .bind(Parameter::Name(":int".into()), SqlValue::Integer(1))
            .bind(Parameter::Name("blob".into()), SqlValue::Blob(vec![]))
            .bind(Parameter::Name("text".into()), SqlValue::Text("é".into()))
            .bind(Parameter::Name("n".into()), SqlValue::Null),
    ]);
    accepted(&c, &p, &admin_ctx(1));
    assert_eq!(
        rows_as_strings(&c, "SELECT typeof(a), typeof(b), typeof(c), length(c), d, typeof(e) FROM v"),
        vec![vec!["real", "integer", "blob", "0", "é", "null"]]
    );
}

#[test]
fn binding_errors_are_rejections() {
    let (_d, c) = fresh_dolt();
    accepted(&c, &prog(&["CREATE TABLE t (a, b)"]), &admin_ctx(0));
    let two = |bindings: Vec<(Parameter, SqlValue)>| {
        let mut st = s("INSERT INTO t VALUES (?1, ?2)");
        for (p, v) in bindings {
            st = st.bind(p, v);
        }
        Program::new(vec![st])
    };
    use Parameter::Index;
    use SqlValue::Integer as I;
    for (name, bindings) in [
        ("duplicate", vec![(Index(1), I(1)), (Index(1), I(2)), (Index(2), I(3))]),
        ("out of range", vec![(Index(1), I(1)), (Index(2), I(2)), (Index(3), I(3))]),
        ("index zero", vec![(Index(0), I(1)), (Index(1), I(1)), (Index(2), I(2))]),
        ("unknown name", vec![(Index(1), I(1)), (Index(2), I(2)), (Parameter::Name("nope".into()), I(3))]),
        ("missing", vec![(Index(1), I(1))]),
        ("non-finite real", vec![(Index(1), SqlValue::Real(f64::NAN)), (Index(2), I(2))]),
        ("infinite real", vec![(Index(1), I(1)), (Index(2), SqlValue::Real(f64::INFINITY))]),
    ] {
        assert_eq!(rejected(&c, &two(bindings), &admin_ctx(1)), ("binding".to_string(), Some(0)), "{name}");
    }
    assert_eq!(q1::<i64>(&c, "SELECT count(*) FROM t"), 0);
}

#[test]
fn chronolog_actor_is_the_signer_during_execution_and_null_otherwise() {
    let (_d, c) = fresh_dolt();
    celld_dolt_exec::register_functions(&c).unwrap();
    assert_eq!(rows_as_strings(&c, "SELECT chronolog_actor()"), vec![vec!["NULL"]]);
    let mut ctx = admin_ctx(1);
    ctx.actor = Some(vec![0xaa; 32]);
    accepted(
        &c,
        &prog(&[
            "CREATE TABLE n (id INTEGER PRIMARY KEY, chronolog_owner BLOB NOT NULL DEFAULT (chronolog_actor()))",
            "INSERT INTO n (id) VALUES (1)",
            "INSERT INTO n (id, chronolog_owner) SELECT 2, chronolog_actor()",
        ]),
        &ctx,
    );
    assert_eq!(rows_as_strings(&c, "SELECT hex(chronolog_owner) FROM n ORDER BY id"), vec![vec!["AA".repeat(32)], vec!["AA".repeat(32)]]);
    assert_eq!(rows_as_strings(&c, "SELECT chronolog_actor()"), vec![vec!["NULL"]], "cleared after execution");
    ctx.actor = None;
    accepted(&c, &prog(&["INSERT INTO n (id, chronolog_owner) VALUES (3, x'00')", "SELECT chronolog_assert(chronolog_actor() IS NULL, 'no_actor')"]), &ctx);
}

#[test]
fn connection_profile_is_applied_for_the_program_and_restored_after() {
    let (_d, c) = fresh_dolt();
    // A plain connection: double-quoted strings work, ATTACH is allowed, limits are default.
    assert_eq!(q1::<String>(&c, "SELECT \"x\""), "x");
    let attached_before = unsafe { rusqlite::ffi::sqlite3_limit(c.handle(), 7, -1) };
    assert!(attached_before > 0);
    accepted(&c, &prog(&["SELECT 1"]), &user_ctx(1));
    assert_eq!(q1::<String>(&c, "SELECT \"x\""), "x");
    assert_eq!(unsafe { rusqlite::ffi::sqlite3_limit(c.handle(), 7, -1) }, attached_before);
    // Inside a program the profile holds: no double-quoted strings.
    assert_eq!(rejected(&c, &prog(&["SELECT \"x\""]), &user_ctx(2)).0, "prepare");
}

#[test]
fn transaction_ownership_is_enforced() {
    let (_d, c) = fresh_dolt();
    accepted(&c, &prog(&["CREATE TABLE t (a)"]), &admin_ctx(0));
    // execute() refuses to run inside a caller transaction ...
    c.execute_batch("BEGIN").unwrap();
    assert!(matches!(execute(&c, &prog(&["SELECT 1"]), &admin_ctx(1)), Err(ExecError::TransactionState(_))));
    c.execute_batch("ROLLBACK").unwrap();
    // ... and execute_in_transaction() needs one.
    assert!(matches!(execute_in_transaction(&c, &prog(&["SELECT 1"]), &admin_ctx(1)), Err(ExecError::TransactionState(_))));
    // Caller-managed: the program's effects and the caller's own writes commit together.
    c.execute_batch("CREATE TABLE log (m TEXT); BEGIN IMMEDIATE").unwrap();
    let out = execute_in_transaction(&c, &prog(&["INSERT INTO t VALUES (1)"]), &admin_ctx(1)).unwrap();
    assert!(out.is_accepted());
    c.execute("INSERT INTO log VALUES ('accepted')", []).unwrap();
    let out = execute_in_transaction(&c, &prog(&["INSERT INTO t VALUES (2)", "SELECT chronolog_assert(0, 'no')"]), &admin_ctx(2)).unwrap();
    assert!(!out.is_accepted());
    c.execute("INSERT INTO log VALUES ('rejected')", []).unwrap();
    c.execute_batch("COMMIT").unwrap();
    assert_eq!(rows_as_strings(&c, "SELECT a FROM t"), vec![vec!["1"]]);
    assert_eq!(rows_as_strings(&c, "SELECT m FROM log ORDER BY rowid"), vec![vec!["accepted"], vec!["rejected"]]);
}

#[test]
fn invalid_programs_and_contexts_are_errors_not_rejections() {
    let (_d, c) = fresh_dolt();
    let bad = |p: &Program, ctx: &ExecContext| matches!(execute(&c, p, ctx), Err(ExecError::InvalidInput(_)));
    assert!(bad(&Program::new(vec![]), &admin_ctx(1)));
    assert!(bad(&Program::new(vec![s("SELECT 1"); 257]), &admin_ctx(1)));
    assert!(bad(&prog(&["SELECT 1"]), &ExecContext::new(-1, 1)));
    let mut ctx = admin_ctx(1);
    ctx.actor = Some(vec![]);
    assert!(bad(&prog(&["SELECT 1"]), &ctx));
    ctx.actor = Some(vec![1; 65]);
    assert!(bad(&prog(&["SELECT 1"]), &ctx));
    ctx = admin_ctx(1);
    ctx.limits.progress_granularity = 10_000_000;
    assert!(bad(&prog(&["SELECT 1"]), &ctx));
    // 256 steps is allowed.
    assert!(execute(&c, &Program::new(vec![s("SELECT 1"); 256]), &admin_ctx(1)).unwrap().is_accepted());
}

#[test]
fn oversize_statements_are_rejected_before_they_reach_sqlite() {
    let (_d, c) = fresh_dolt();
    let mut ctx = admin_ctx(1);
    ctx.limits.max_sql_bytes = 64;
    let long = format!("SELECT '{}'", "x".repeat(100));
    assert_eq!(rejected(&c, &Program::new(vec![s(&long)]), &ctx), ("too_large".to_string(), Some(0)));
    assert_eq!(rejected(&c, &prog(&["SELECT 1\0"]), &admin_ctx(1)).0, "prepare");
}

// ───────── operational failures are errors, never rejections ─────────

#[test]
fn a_read_only_store_is_an_operational_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ro.db");
    {
        let c = open_dolt(&path, Some(GENESIS));
        c.execute_batch("CREATE TABLE t (a)").unwrap();
    }
    let ro = Connection::open_with_flags(
        celld_dolt_exec::prolly_uri(&path, None),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();
    let r = execute(&ro, &prog(&["INSERT INTO t VALUES (1)"]), &admin_ctx(1));
    println!("read-only result: {r:?}");
    match r {
        Err(ExecError::Operational { .. }) => {}
        // DoltLite's pager shim does not enforce SQLITE_OPEN_READONLY (see celld
        // spike notes); in that case the write simply lands.
        Ok(Outcome::Accepted) => {}
        other => panic!("a read-only store must not produce a rejection: {other:?}"),
    }
}

#[test]
fn an_interrupt_from_outside_is_operational_not_a_step_limit_rejection() {
    let (_d, c) = fresh_dolt();
    let handle = c.get_interrupt_handle();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let t = std::thread::spawn(move || {
        while !flag.load(std::sync::atomic::Ordering::Relaxed) {
            handle.interrupt();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });
    let mut ctx = admin_ctx(1);
    ctx.limits.max_vm_steps = 200_000_000; // effectively no budget
    ctx.limits.progress_granularity = 1_000_000;
    // A long read-only statement, so the interrupt lands mid-step.
    let long = prog(&["WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 400000000) SELECT count(*) FROM n"]);
    let r = execute(&c, &long, &ctx);
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    t.join().unwrap();
    match r {
        Err(ExecError::Operational { code, .. }) => assert_eq!(code & 0xff, 9, "SQLITE_INTERRUPT"),
        other => panic!("expected an operational interrupt: {other:?}"),
    }
    assert!(c.is_autocommit());
    // Nothing is left half-done: the connection executes the next program.
    assert!(execute(&c, &prog(&["SELECT 1"]), &admin_ctx(2)).unwrap().is_accepted());
}

// ─────────────────────────── differential corpus ───────────────────────────

fn register_vec_once() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        type Init = unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *const std::os::raw::c_char,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> std::os::raw::c_int;
        let init = unsafe { std::mem::transmute::<*const (), Init>(sqlite_vec::sqlite3_vec_init as *const ()) };
        let rc = unsafe { rusqlite::ffi::sqlite3_auto_extension(Some(init)) };
        assert_eq!(rc, 0);
    });
}

/// One corpus entry: a name, whether the signer is a schema admin, and steps.
struct Case {
    name: &'static str,
    admin: bool,
    steps: Vec<Statement>,
}

fn case(name: &'static str, admin: bool, steps: &[&str]) -> Case {
    Case { name, admin, steps: steps.iter().map(|x| s(x)).collect() }
}

fn corpus() -> Vec<Case> {
    let mut v = vec![
        case("schema", true, &[
            "CREATE TABLE accounts (id INTEGER PRIMARY KEY, name TEXT COLLATE NOCASE UNIQUE, balance INTEGER NOT NULL CHECK (balance >= 0)) STRICT",
            "INSERT INTO accounts (name, balance) VALUES ('alice', 100), ('Bob', 50), ('carol', 0) RETURNING id, name",
            "CREATE INDEX accounts_balance ON accounts (balance)",
        ]),
        case("transfer-ok", false, &[
            "SELECT chronolog_assert((SELECT balance FROM accounts WHERE name = 'alice') >= 30, 'insufficient_funds')",
            "UPDATE accounts SET balance = balance - 30 WHERE name = 'alice'",
            "UPDATE accounts SET balance = balance + 30 WHERE name = 'bob'",
        ]),
        case("transfer-rejected", false, &[
            "UPDATE accounts SET balance = balance - 1 WHERE name = 'bob'",
            "SELECT chronolog_assert((SELECT balance FROM accounts WHERE name = 'carol') >= 30, 'insufficient_funds')",
        ]),
        case("check-constraint", false, &["UPDATE accounts SET balance = balance - 1000 WHERE name = 'carol'"]),
        case("unique-nocase", false, &["INSERT INTO accounts (name, balance) VALUES ('ALICE', 1)"]),
        case("window-cte", true, &[
            "CREATE TABLE ranked (name TEXT, balance INTEGER, rank INTEGER, running INTEGER, names TEXT)",
            "INSERT INTO ranked SELECT name, balance, rank() OVER (ORDER BY balance DESC), sum(balance) OVER (ORDER BY balance DESC, name), group_concat(name, '|') OVER () FROM accounts",
            "WITH RECURSIVE n(v) AS (VALUES(1) UNION ALL SELECT v * 2 FROM n WHERE v < 1000) INSERT INTO ranked (name, balance) SELECT 'p' || v, v FROM n",
        ]),
        case("json", true, &[
            "CREATE TABLE docs (id INTEGER PRIMARY KEY, body TEXT)",
            "INSERT INTO docs (body) VALUES ('{\"a\":[1,2,{\"b\":3.5}],\"c\":\"é\"}'), ('[10,20,30]')",
            "CREATE TABLE flat (id INTEGER, k TEXT, v TEXT, t TEXT)",
            "INSERT INTO flat SELECT d.id, j.fullkey, j.value, j.type FROM docs d, json_tree(d.body) j",
            "INSERT INTO flat SELECT 0, 'agg', json_group_array(body), json_valid(body) FROM docs",
        ]),
        case("fts5", true, &[
            "CREATE VIRTUAL TABLE ft USING fts5(body, tokenize='unicode61 remove_diacritics 0')",
            "INSERT INTO ft (rowid, body) VALUES (1, 'the quick brown fox'), (2, 'jumps over the lazy dog'), (3, 'Ünïcode and FOXES')",
            "CREATE TABLE hits (rowid INTEGER, snip TEXT, rank REAL)",
            "INSERT INTO hits SELECT rowid, highlight(ft, 0, '[', ']'), bm25(ft) FROM ft WHERE ft MATCH 'fox OR lazy' ORDER BY rowid",
        ]),
        case("rtree", true, &[
            "CREATE VIRTUAL TABLE boxes USING rtree(id, x0, x1, y0, y1)",
            "INSERT INTO boxes VALUES (1, 0, 10, 0, 10), (2, 5, 15, 5, 15), (3, 20, 30, 20, 30)",
            "CREATE TABLE near (id INTEGER)",
            "INSERT INTO near SELECT id FROM boxes WHERE x0 <= 7 AND x1 >= 7 AND y0 <= 7 AND y1 >= 7",
        ]),
        case("triggers-views-fk", true, &[
            "CREATE TABLE parent (id INTEGER PRIMARY KEY, label TEXT)",
            "CREATE TABLE child (id INTEGER PRIMARY KEY, pid INTEGER NOT NULL REFERENCES parent(id) ON DELETE CASCADE ON UPDATE CASCADE, note TEXT)",
            "CREATE TABLE audit (id INTEGER PRIMARY KEY AUTOINCREMENT, what TEXT, at TEXT)",
            "CREATE TRIGGER child_ins AFTER INSERT ON child BEGIN INSERT INTO audit (what, at) VALUES ('child+' || NEW.id, datetime('now')); END",
            "CREATE TRIGGER child_del AFTER DELETE ON child BEGIN INSERT INTO audit (what, at) VALUES ('child-' || OLD.id, datetime('now')); END",
            "CREATE VIEW child_counts AS SELECT pid, count(*) AS n FROM child GROUP BY pid",
            "INSERT INTO parent VALUES (1, 'a'), (2, 'b')",
            "INSERT INTO child (pid, note) VALUES (1, 'x'), (1, 'y'), (2, 'z')",
            "UPDATE parent SET id = 10 WHERE id = 1",
            "DELETE FROM parent WHERE id = 2",
        ]),
        case("fk-violation", false, &["INSERT INTO child (pid, note) VALUES (999, 'orphan')"]),
        case("generated-and-alter", true, &[
            "CREATE TABLE g (id INTEGER PRIMARY KEY, a REAL, b REAL, s REAL GENERATED ALWAYS AS (a * b) VIRTUAL, t TEXT GENERATED ALWAYS AS (printf('%.3f', a / 3)) STORED)",
            "INSERT INTO g (a, b) VALUES (1.5, 2), (0.1, 0.2), (1e300, 1e300), (-0.0, 7)",
            "ALTER TABLE g ADD COLUMN note TEXT DEFAULT 'n/a'",
            "ALTER TABLE g RENAME COLUMN note TO remark",
            "ALTER TABLE g DROP COLUMN remark",
        ]),
        case("time-functions", true, &[
            "CREATE TABLE tm (k TEXT PRIMARY KEY, v)",
            "INSERT INTO tm VALUES ('now', datetime('now')), ('ts', CURRENT_TIMESTAMP), ('date', CURRENT_DATE), ('time', CURRENT_TIME)",
            "INSERT INTO tm VALUES ('unix', unixepoch('now')), ('unixsub', unixepoch('now', 'subsec')), ('julian', julianday('now'))",
            "INSERT INTO tm VALUES ('local', datetime('now', 'localtime')), ('utc', datetime('now', 'utc')), ('local-dst', datetime('2024-07-01 12:00:00', 'localtime'))",
            "INSERT INTO tm VALUES ('mods', datetime('now', '+1 month', 'start of month', 'weekday 0', '-3 hours')), ('strf', strftime('%Y-%j-%W-%w-%s-%f', 'now'))",
            "INSERT INTO tm VALUES ('far', datetime('2100-03-01 00:00:00', 'localtime')), ('past', datetime('1960-01-01 00:00:00', 'localtime')), ('e0', datetime(0, 'unixepoch', 'localtime'))",
        ]),
        case("random-functions", true, &[
            "CREATE TABLE rnd (k TEXT, v)",
            "INSERT INTO rnd VALUES ('r1', random()), ('r2', random()), ('blob', randomblob(150)), ('abs', abs(random()) % 1000), ('hex', hex(randomblob(4)))",
            "CREATE TABLE rowids (id INTEGER PRIMARY KEY, v TEXT)",
            "INSERT INTO rowids VALUES (9223372036854775807, 'max')",
            "INSERT INTO rowids (v) VALUES ('a'), ('b'), ('c')",
            "CREATE TABLE ctas AS SELECT 1 AS a, 2 AS a, 3 AS a, 4 AS a, 5 AS a, 6 AS a",
        ]),
        case("numeric-and-text", true, &[
            "CREATE TABLE nm (k TEXT, v)",
            "INSERT INTO nm VALUES ('add', 0.1 + 0.2), ('big', 9223372036854775807 + 1), ('neg', -9223372036854775808), ('div0', 1 / 0), ('mod0', 1 % 0), ('real-text', CAST(1.0 / 3 AS TEXT)), ('printf', printf('%.15g|%e|%5.2f|%05d|%x', 1.0/3, 12345.678, 3.14159, 42, 255))",
            "INSERT INTO nm VALUES ('round', round(2.5)), ('round2', round(-0.5)), ('cast', CAST('12abc' AS INTEGER)), ('typeof', typeof(1.0)), ('inf', 1e999), ('ninf', -1e999), ('hexfloat', hex(1.5))",
            "INSERT INTO nm VALUES ('upper', upper('straße é')), ('like', 'ABC' LIKE 'abc'), ('glob', 'abc' GLOB 'A*'), ('substr', substr('héllo', 2, 3)), ('instr', instr('héllo', 'l')), ('unicode', unicode('é')), ('quote', quote(x'00ff')), ('replace', replace('aaa', 'a', 'bb')), ('nocase', 'a' = 'A' COLLATE NOCASE), ('trim', trim('  x '))",
        ]),
        case("upsert-returning", true, &[
            "CREATE TABLE kv (k TEXT PRIMARY KEY, v INTEGER, n INTEGER DEFAULT 0) WITHOUT ROWID",
            "INSERT INTO kv (k, v) VALUES ('a', 1), ('b', 2)",
            "INSERT INTO kv (k, v) VALUES ('a', 10), ('c', 3) ON CONFLICT (k) DO UPDATE SET v = v + excluded.v, n = n + 1 RETURNING k, v, n",
            "INSERT OR IGNORE INTO kv (k, v) VALUES ('b', 99)",
            "INSERT OR REPLACE INTO kv (k, v) VALUES ('c', 33)",
        ]),
        case("autoincrement", true, &[
            "CREATE TABLE ai (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT)",
            "INSERT INTO ai (v) VALUES ('a'), ('b')",
            "DELETE FROM ai",
            "INSERT INTO ai (v) VALUES ('c')",
        ]),
        case("analyze", false, &["ANALYZE"]),
        case("counters", true, &[
            "CREATE TABLE cnt (k TEXT, v INTEGER)",
            "INSERT INTO cnt VALUES ('lir0', last_insert_rowid()), ('ch0', changes())",
            "INSERT INTO cnt SELECT 'lir1', last_insert_rowid()",
            "UPDATE cnt SET v = v + 1 WHERE k = 'ch0'",
            "INSERT INTO cnt VALUES ('ch1', changes())",
        ]),
        case("actor", true, &[
            "CREATE TABLE who (id INTEGER PRIMARY KEY, owner BLOB NOT NULL DEFAULT (chronolog_actor()))",
            "INSERT INTO who (id) VALUES (1), (2)",
        ]),
        case("denied-attach", false, &["SELECT 1", "ATTACH DATABASE ':memory:' AS x"]),
        case("denied-pragma", false, &["PRAGMA writable_schema = 1"]),
        case("denied-dolt", false, &["SELECT dolt_hashof('HEAD')"]),
        case("denied-dolt-table", false, &["SELECT count(*) FROM dolt_log"]),
        case("denied-ddl", false, &["CREATE TABLE sneaky (a)"]),
        case("denied-reserved", true, &["CREATE TABLE chronolog_sneaky (a)"]),
        case("denied-temp", true, &["CREATE TEMP TABLE t (a)"]),
        case("denied-total-changes", false, &["SELECT total_changes()"]),
        case("denied-fts5-locale", false, &["SELECT fts5_locale('en', 'x')"]),
        case("denied-vec1", true, &["CREATE VIRTUAL TABLE ann USING vec1(v)"]),
        case("syntax", false, &["SELEC 1"]),
        case("multi", false, &["SELECT 1; SELECT 2"]),
        case("trigger-loop", true, &[
            "CREATE TABLE lp (n INTEGER)",
            "CREATE TRIGGER lp_t AFTER INSERT ON lp BEGIN INSERT INTO lp VALUES (NEW.n + 1); END",
            "INSERT INTO lp VALUES (0)",
        ]),
        case("step-limit-read", false, &["WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) SELECT count(*) FROM n"]),
        case("step-limit-write", true, &[
            "INSERT INTO kv (k, v) VALUES ('w', 1)",
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n) INSERT INTO kv SELECT 'z' || i, i, 0 FROM n",
        ]),
        case("after-step-limit", false, &["INSERT INTO kv (k, v) VALUES ('after', 1)"]),
    ];
    // sqlite-vec through the same path (vec0 shadow tables are ordinary tables).
    v.push(case("vec0", true, &[
        "CREATE VIRTUAL TABLE vecs USING vec0(embedding float[4])",
        "INSERT INTO vecs (rowid, embedding) VALUES (1, '[1,2,3,4]'), (2, '[4,3,2,1]'), (3, '[0,0,0,1]')",
        "CREATE TABLE knn (rowid INTEGER, distance REAL)",
        "INSERT INTO knn SELECT rowid, distance FROM vecs WHERE embedding MATCH '[1,2,3,3]' AND k = 2",
    ]));
    v
}

/// Run the whole corpus on a fresh store: one line per case, with its outcome
/// and the state digest after it.
fn run_corpus() -> Vec<String> {
    register_vec_once();
    let (_d, c) = fresh_dolt();
    let mut lines = Vec::new();
    for (i, case) in corpus().into_iter().enumerate() {
        let mut ctx = ExecContext::new(T0 + 3_600_000 * (i as i64 + 1), prng_seed_from_tx_id(format!("tx-{i}").as_bytes()));
        ctx.profile = Profile::Consensus { schema_admin: case.admin };
        ctx.actor = Some(vec![(i as u8).wrapping_add(1); 32]);
        ctx.limits.max_vm_steps = 100_000;
        let outcome = execute(&c, &Program::new(case.steps), &ctx).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let outcome = match outcome {
            Outcome::Accepted => "accepted".to_string(),
            Outcome::Rejected { code, statement_index } => format!("rejected:{code}@{statement_index:?}"),
        };
        assert!(c.is_autocommit(), "{} left a transaction open", case.name);
        lines.push(format!("{}|{}|{}", case.name, outcome, state_digest(&c)));
    }
    lines
}

/// The expected outcome of each corpus case (the digests are checked for
/// equality between stores, zones and runs, not against fixed values).
const EXPECTED_OUTCOMES: &[(&str, &str)] = &[
    ("schema", "accepted"),
    ("transfer-ok", "accepted"),
    ("transfer-rejected", "rejected:assert:insufficient_funds@Some(1)"),
    ("check-constraint", "rejected:constraint@Some(0)"),
    ("unique-nocase", "rejected:constraint@Some(0)"),
    ("window-cte", "accepted"),
    ("json", "accepted"),
    ("fts5", "accepted"),
    ("rtree", "accepted"),
    ("triggers-views-fk", "accepted"),
    ("fk-violation", "rejected:constraint@Some(0)"),
    ("generated-and-alter", "accepted"),
    ("time-functions", "accepted"),
    ("random-functions", "accepted"),
    ("numeric-and-text", "accepted"),
    ("upsert-returning", "accepted"),
    ("autoincrement", "accepted"),
    ("analyze", "accepted"),
    ("counters", "accepted"),
    ("actor", "accepted"),
    ("denied-attach", "rejected:denied@Some(1)"),
    ("denied-pragma", "rejected:denied@Some(0)"),
    ("denied-dolt", "rejected:denied@Some(0)"),
    ("denied-dolt-table", "rejected:denied@Some(0)"),
    ("denied-ddl", "rejected:denied@Some(0)"),
    ("denied-reserved", "rejected:denied@Some(0)"),
    ("denied-temp", "rejected:denied@Some(0)"),
    ("denied-total-changes", "rejected:denied@Some(0)"),
    ("denied-fts5-locale", "rejected:denied@Some(0)"),
    ("denied-vec1", "rejected:denied@Some(0)"),
    ("syntax", "rejected:prepare@Some(0)"),
    ("multi", "rejected:prepare@Some(0)"),
    ("trigger-loop", "rejected:error@Some(2)"),
    ("step-limit-read", "rejected:step_limit@Some(0)"),
    ("step-limit-write", "rejected:step_limit@Some(1)"),
    ("after-step-limit", "accepted"),
    ("vec0", "accepted"),
];

#[test]
fn corpus_outcomes_are_as_specified() {
    let lines = run_corpus();
    let got: Vec<(String, String)> = lines
        .iter()
        .map(|l| {
            let mut p = l.splitn(3, '|');
            (p.next().unwrap().to_string(), p.next().unwrap().to_string())
        })
        .collect();
    let want: Vec<(String, String)> = EXPECTED_OUTCOMES.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
    assert_eq!(got, want);
}

#[test]
fn rejected_cases_leave_the_state_digest_unchanged() {
    let lines = run_corpus();
    let digests: Vec<(&str, &str, &str)> = lines
        .iter()
        .map(|l| {
            let mut p = l.splitn(3, '|');
            (p.next().unwrap(), p.next().unwrap(), p.next().unwrap())
        })
        .collect();
    for w in digests.windows(2) {
        if w[1].1.starts_with("rejected") {
            assert_eq!(w[0].2, w[1].2, "{} changed state although rejected", w[1].0);
        }
    }
}

#[test]
fn two_fresh_stores_agree_on_every_outcome_and_state_digest() {
    let a = std::thread::spawn(run_corpus);
    let b = std::thread::spawn(run_corpus);
    let (a, b) = (a.join().unwrap(), b.join().unwrap());
    assert_eq!(a, b);
    // And the results are not trivially equal: different cases give different digests.
    let digests: std::collections::HashSet<_> = a.iter().map(|l| l.rsplit('|').next().unwrap().to_string()).collect();
    assert!(digests.len() >= 18, "{}", digests.len());
}

/// The digest of the whole corpus (outcomes and state digests), printed by the
/// child so the parent can compare it across time zones.
fn corpus_digest(lines: &[String]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(lines.join("\n").as_bytes()))
}

#[test]
fn corpus_is_identical_under_different_time_zones() {
    const TEST: &str = "corpus_is_identical_under_different_time_zones";
    if !in_tz_child() {
        let utc = run_child_under_tz(TEST, "UTC", "CORPUS:");
        let la = run_child_under_tz(TEST, "America/Los_Angeles", "CORPUS:");
        let kolkata = run_child_under_tz(TEST, "Asia/Kolkata", "CORPUS:");
        assert_eq!(utc.len(), 1);
        assert_eq!(utc, la, "UTC vs America/Los_Angeles");
        assert_eq!(utc, kolkata, "UTC vs Asia/Kolkata");
        // The parent runs in the ambient zone and agrees too.
        assert_eq!(utc[0], corpus_digest(&run_corpus()));
        // The zone really does differ for unpinned SQL.
        let probe = |tz: &str| run_child_under_tz("zone_probe", tz, "ZONE:");
        assert_ne!(probe("UTC"), probe("America/Los_Angeles"));
        return;
    }
    println!("CORPUS:{}", corpus_digest(&run_corpus()));
}

/// Child helper for the test above: prints the unpinned local time.
#[test]
fn zone_probe() {
    if in_tz_child() {
        let c = Connection::open_in_memory().unwrap();
        println!("ZONE:{}", q1::<String>(&c, "SELECT datetime('2024-07-01 12:00:00', 'localtime')"));
    }
}

#[test]
fn programs_run_concurrently_on_many_threads_match_a_sequential_run() {
    // Each thread has its own store and its own contexts; the result must
    // equal the single-threaded run of the same corpus slice.
    let expected = run_corpus();
    let handles: Vec<_> = (0..4).map(|_| std::thread::spawn(run_corpus)).collect();
    for h in handles {
        assert_eq!(h.join().unwrap(), expected);
    }
}
