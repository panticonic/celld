//! `local_query`: Chronolog Spec 10 local reads. Exact storage classes,
//! read-only enforcement, host and Dolt tables, budgets and result limits.

mod common;

use celld_dolt_exec::{local_query, Limits, Parameter, SqlValue, Statement};
use common::fresh_dolt;

fn q(sql: &str) -> Statement {
    Statement::new(sql)
}

#[test]
fn values_keep_their_storage_class() {
    let (_dir, conn) = fresh_dolt();
    conn.execute_batch(
        "CREATE TABLE t (i INTEGER, r REAL, s TEXT, b BLOB, n);
         INSERT INTO t VALUES (9007199254740993, 1.0, 'x', x'0001', NULL);",
    )
    .unwrap();
    let rows = local_query(&conn, &q("SELECT i, r, s, b, n FROM t"), &Limits::default(), 10).unwrap();
    assert_eq!(rows.columns, vec!["i", "r", "s", "b", "n"]);
    assert_eq!(
        rows.rows,
        vec![vec![
            SqlValue::Integer(9_007_199_254_740_993),
            SqlValue::Real(1.0),
            SqlValue::Text("x".into()),
            SqlValue::Blob(vec![0, 1]),
            SqlValue::Null,
        ]]
    );
}

#[test]
fn bindings_are_exact() {
    let (_dir, conn) = fresh_dolt();
    let statement = q("SELECT typeof(?1), ?1 + 0, typeof(?2)")
        .bind(Parameter::Index(1), SqlValue::Integer(i64::MAX))
        .bind(Parameter::Index(2), SqlValue::Real(2.0));
    let rows = local_query(&conn, &statement, &Limits::default(), 10).unwrap();
    assert_eq!(
        rows.rows[0],
        vec![SqlValue::Text("integer".into()), SqlValue::Integer(i64::MAX), SqlValue::Text("real".into())]
    );
}

#[test]
fn writes_are_refused_and_change_nothing() {
    let (_dir, conn) = fresh_dolt();
    conn.execute_batch("CREATE TABLE t (x); INSERT INTO t VALUES (1);").unwrap();
    for sql in [
        "INSERT INTO t VALUES (2)",
        "UPDATE t SET x = 3",
        "DELETE FROM t",
        "CREATE TABLE u (y)",
        "DROP TABLE t",
        "PRAGMA user_version = 7",
    ] {
        let error = local_query(&conn, &q(sql), &Limits::default(), 10).unwrap_err();
        assert_eq!(error, "SQL_PROFILE_VIOLATION", "{sql}");
    }
    let n: i64 = conn.query_row("SELECT count(*) FROM t WHERE x = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

#[test]
fn host_tables_are_unreadable_but_dolt_history_is_readable() {
    let (_dir, conn) = fresh_dolt();
    conn.execute_batch("CREATE TABLE _cf_KV (key TEXT, value BLOB);").unwrap();
    assert_eq!(
        local_query(&conn, &q("SELECT * FROM _cf_KV"), &Limits::default(), 10).unwrap_err(),
        "SQL_PROFILE_VIOLATION"
    );
    // Local reads may inspect history (consensus programs may not).
    let log = local_query(&conn, &q("SELECT count(*) FROM dolt_log"), &Limits::default(), 10).unwrap();
    assert!(matches!(log.rows[0][0], SqlValue::Integer(n) if n >= 1));
}

#[test]
fn multiple_statements_and_bad_sql_are_refused() {
    let (_dir, conn) = fresh_dolt();
    assert_eq!(
        local_query(&conn, &q("SELECT 1; SELECT 2"), &Limits::default(), 10).unwrap_err(),
        "SQL_MULTIPLE_STATEMENTS"
    );
    assert_eq!(
        local_query(&conn, &q("SELEKT 1"), &Limits::default(), 10).unwrap_err(),
        "SQL_PREPARE_FAILED"
    );
    // Trailing trivia is one statement.
    assert!(local_query(&conn, &q("SELECT 1; -- done\n"), &Limits::default(), 10).is_ok());
}

#[test]
fn budgets_and_row_limits_hold() {
    let (_dir, conn) = fresh_dolt();
    let limits = Limits { max_vm_steps: 10_000, progress_granularity: 100, ..Limits::default() };
    let runaway = q("WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT count(*) FROM c");
    assert_eq!(local_query(&conn, &runaway, &limits, 10).unwrap_err(), "SQL_STEP_LIMIT");
    let many = q("WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 20) SELECT x FROM c");
    assert_eq!(local_query(&conn, &many, &Limits::default(), 5).unwrap_err(), "SQL_RESULT_LIMIT");
    assert_eq!(local_query(&conn, &many, &Limits::default(), 20).unwrap().rows.len(), 20);
    // The connection is usable afterwards: the budget and authorizer are gone.
    conn.execute_batch("CREATE TABLE after_limits (x)").unwrap();
}
