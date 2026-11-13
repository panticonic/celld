//! Prints compile options and the connection defaults that user SQL can see,
//! so two SQLite builds can be diffed.
use rusqlite::Connection;

fn main() {
    let dir = tempfile::tempdir().unwrap();
    let c = Connection::open(dir.path().join("d.db")).unwrap();
    let mut st = c.prepare("PRAGMA compile_options").unwrap();
    let mut opts: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
    opts.sort();
    for o in opts {
        println!("compile_option {o}");
    }
    for p in [
        "page_size", "auto_vacuum", "recursive_triggers", "foreign_keys", "cache_size", "synchronous",
        "temp_store", "journal_mode", "wal_autocheckpoint", "journal_size_limit", "max_page_count",
        "secure_delete", "locking_mode", "encoding", "mmap_size", "busy_timeout", "case_sensitive_like",
        "trusted_schema", "defer_foreign_keys", "threads",
    ] {
        let v: rusqlite::Result<String> = c.query_row(&format!("PRAGMA {p}"), [], |r| r.get::<_, rusqlite::types::Value>(0).map(|v| format!("{v:?}")));
        println!("pragma {p} = {v:?}");
    }
    c.pragma_update(None, "journal_mode", "WAL").unwrap();
    let v: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    println!("after WAL: {v}");
    for p in ["synchronous", "wal_autocheckpoint", "page_size"] {
        let v: i64 = c.query_row(&format!("PRAGMA {p}"), [], |r| r.get(0)).unwrap();
        println!("wal pragma {p} = {v}");
    }
    let v: String = c.query_row("SELECT sqlite_version()", [], |r| r.get(0)).unwrap();
    println!("sqlite_version {v}");
    let mut st = c.prepare("SELECT name FROM pragma_function_list ORDER BY 1").unwrap();
    let f: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
    println!("functions {}", f.join(","));
    let mut st = c.prepare("SELECT name FROM pragma_module_list ORDER BY 1").unwrap();
    let f: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
    println!("modules {}", f.join(","));
    let mut st = c.prepare("SELECT name FROM pragma_collation_list ORDER BY 1").unwrap();
    let f: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
    println!("collations {}", f.join(","));
    let mut st = c.prepare("SELECT name FROM pragma_pragma_list ORDER BY 1").unwrap();
    let f: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
    println!("pragmas {}", f.join(","));
}
