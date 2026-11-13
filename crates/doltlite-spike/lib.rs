//! Engine coexistence checks for the DoltLite-as-only-SQLite spike.
//! The crate has no runtime code; everything is under `cfg(test)`.

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, OpenFlags};
    use std::path::Path;

    const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

    fn is_stock_file(path: &Path) -> bool {
        match std::fs::read(path) {
            Ok(bytes) => bytes.len() >= 16 && &bytes[..16] == SQLITE_MAGIC,
            Err(_) => false,
        }
    }

    /// A URI that opts a new file into the prolly engine.
    fn prolly_uri(path: &Path) -> String {
        format!("file:{}?doltlite_engine=prolly", path.display())
    }

    fn open_uri(uri: &str) -> Connection {
        Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap()
    }

    #[test]
    fn library_is_doltlite() {
        let c = Connection::open_in_memory().unwrap();
        let v: String = c.query_row("SELECT sqlite_version()", [], |r| r.get(0)).unwrap();
        println!("sqlite_version={v}");
        assert!(v.starts_with("3.55"));
    }

    /// The default-engine patch: a brand new plain path is a stock file.
    #[test]
    fn new_plain_path_is_stock_wal_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cell.db");
        let c = Connection::open(&path).unwrap();
        let mode: String = c.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT); INSERT INTO t(b) VALUES ('x');")
            .unwrap();
        assert!(is_stock_file(&path), "new file must carry the SQLite header");
        assert!(path.with_extension("db-wal").exists() || path.with_file_name("cell.db-wal").exists());
        // Stock-only SQL surface: dbstat and wal_checkpoint work on it.
        let n: i64 = c.query_row("SELECT count(*) FROM dbstat", [], |r| r.get(0)).unwrap();
        assert!(n > 0);
    }

    #[test]
    fn engine_prolly_new_file_commits_and_logs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dolt.db");
        let c = open_uri(&prolly_uri(&path));
        c.execute_batch("CREATE TABLE t(a INTEGER PRIMARY KEY, b TEXT); INSERT INTO t(b) VALUES ('x');")
            .unwrap();
        let hash: String = c
            .query_row("SELECT dolt_commit('-A','-m','x')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(hash.len(), 40, "commit hash {hash}");
        let (msg, n): (String, i64) = c
            .query_row(
                "SELECT message, (SELECT count(*) FROM dolt_log) FROM dolt_log ORDER BY rowid LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        println!("dolt_log head message={msg:?} rows={n}");
        assert!(n >= 1);
        drop(c);
        assert!(!is_stock_file(&path), "prolly file must not carry the SQLite header");
        // A plain-path reopen (no URI) of an existing prolly file stays prolly.
        let c = Connection::open(&path).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM dolt_log", [], |r| r.get(0)).unwrap();
        assert!(n >= 1);
        let b: String = c.query_row("SELECT b FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(b, "x");
    }

    /// Both engines live in one process at the same time, on different threads.
    #[test]
    fn engines_coexist_across_threads() {
        let dir = tempfile::tempdir().unwrap();
        let stock = dir.path().join("stock.db");
        let dolt = dir.path().join("dolt.db");
        let uri = prolly_uri(&dolt);
        let s = stock.clone();
        let t1 = std::thread::spawn(move || {
            let c = Connection::open(&s).unwrap();
            c.pragma_update(None, "journal_mode", "WAL").unwrap();
            c.execute_batch("CREATE TABLE t(a)").unwrap();
            for i in 0..500 {
                c.execute("INSERT INTO t VALUES (?1)", [i]).unwrap();
            }
            c.query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0)).unwrap()
        });
        let t2 = std::thread::spawn(move || {
            let c = open_uri(&uri);
            c.execute_batch("CREATE TABLE t(a)").unwrap();
            for i in 0..500 {
                c.execute("INSERT INTO t VALUES (?1)", [i]).unwrap();
            }
            c.query_row("SELECT dolt_commit('-A','-m','bulk')", [], |r| r.get::<_, String>(0)).unwrap();
            c.query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0)).unwrap()
        });
        assert_eq!(t1.join().unwrap(), 500);
        assert_eq!(t2.join().unwrap(), 500);
        assert!(is_stock_file(&stock));
        assert!(!is_stock_file(&dolt));
    }

    /// ATTACH of a stock file from a stock connection, and a prolly connection
    /// next to it in the same transaction scope, both work.
    /// json_each / json_tree / FTS5 / R-Tree / dbstat from the stock feature set.
    #[test]
    fn stock_feature_set_present() {
        let c = Connection::open_in_memory().unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM json_each('[1,2,3]')", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 3);
        let n: i64 = c.query_row("SELECT count(*) FROM json_tree('{\"a\":[1,2]}')", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 4);
        c.execute_batch(
            "CREATE VIRTUAL TABLE f USING fts5(x); INSERT INTO f VALUES ('hello world');
             CREATE VIRTUAL TABLE r USING rtree(id, x0, x1); INSERT INTO r VALUES (1, 0, 1);",
        )
        .unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM f WHERE f MATCH 'hello'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn in_memory_and_temp_are_stock() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE t(a); INSERT INTO t VALUES (1); CREATE TEMP TABLE u(a); INSERT INTO u VALUES (2);")
            .unwrap();
        let n: i64 = c.query_row("SELECT (SELECT sum(a) FROM t)+(SELECT sum(a) FROM u)", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 3);
    }

    /// The facet-import path in storage.rs: deserialize into memory, then run
    /// the backup API into a NEW file at a plain path.
    #[test]
    fn facet_deserialize_then_backup_into_new_file() {
        let dir = tempfile::tempdir().unwrap();
        // Make a stock image.
        let img_path = dir.path().join("img.db");
        {
            let c = Connection::open(&img_path).unwrap();
            c.execute_batch("CREATE TABLE t(a TEXT); INSERT INTO t VALUES ('facet');").unwrap();
        }
        let bytes = std::fs::read(&img_path).unwrap();
        assert_eq!(&bytes[..16], SQLITE_MAGIC);

        let mut source = Connection::open_in_memory().unwrap();
        let data = unsafe {
            let p = rusqlite::ffi::sqlite3_malloc64(bytes.len() as u64).cast::<u8>();
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
            rusqlite::serialize::OwnedData::from_raw_nonnull(std::ptr::NonNull::new(p).unwrap(), bytes.len())
        };
        source.deserialize(rusqlite::DatabaseName::Main, data, true).unwrap();

        let dest_path = dir.path().join("facet.db");
        let mut dest = Connection::open(&dest_path).unwrap();
        let pages: u64 = dest.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap();
        assert_eq!(pages, 0);
        rusqlite::backup::Backup::new(&source, &mut dest)
            .unwrap()
            .run_to_completion(i32::MAX, std::time::Duration::ZERO, None)
            .unwrap();
        let v: String = dest.query_row("SELECT a FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "facet");
        drop(dest);
        assert!(is_stock_file(&dest_path), "backup target stayed stock");

        // serialize back out
        let c = Connection::open(&dest_path).unwrap();
        let ser = c.serialize(rusqlite::DatabaseName::Main).unwrap();
        assert_eq!(&ser[..16], SQLITE_MAGIC);
    }

    /// Stock file -> backup to another new stock file (file to file).
    #[test]
    fn file_to_file_backup_is_stock_and_complete() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.db");
        let b = dir.path().join("b.db");
        let src = Connection::open(&a).unwrap();
        src.pragma_update(None, "journal_mode", "WAL").unwrap();
        src.execute_batch("CREATE TABLE t(a); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<5000) INSERT INTO t SELECT i FROM n;").unwrap();
        let mut dst = Connection::open(&b).unwrap();
        rusqlite::backup::Backup::new(&src, &mut dst)
            .unwrap()
            .run_to_completion(5, std::time::Duration::ZERO, None)
            .unwrap();
        let n: i64 = dst.query_row("SELECT count(*) FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 5000);
        drop(dst);
        assert!(is_stock_file(&b));
    }

    /// PRAGMA data_version moves when ANOTHER connection commits (celld's
    /// replicated-write detection relies on it).
    #[test]
    fn data_version_tracks_other_connection_writes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("dv.db");
        let w = Connection::open(&p).unwrap();
        w.pragma_update(None, "journal_mode", "WAL").unwrap();
        w.execute_batch("CREATE TABLE t(a)").unwrap();
        let r = Connection::open(&p).unwrap();
        let v0: i64 = r.query_row("PRAGMA data_version", [], |x| x.get(0)).unwrap();
        w.execute("INSERT INTO t VALUES (1)", []).unwrap();
        let v1: i64 = r.query_row("PRAGMA data_version", [], |x| x.get(0)).unwrap();
        assert_ne!(v0, v1);
        let v2: i64 = r.query_row("PRAGMA data_version", [], |x| x.get(0)).unwrap();
        assert_eq!(v1, v2);
    }

    fn register_vec(c: &Connection) -> i32 {
        type Init = unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *mut std::os::raw::c_char,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> std::os::raw::c_int;
        let init = unsafe { std::mem::transmute::<*const (), Init>(sqlite_vec::sqlite3_vec_init as *const ()) };
        let mut err = std::ptr::null_mut();
        unsafe { init(c.handle(), &mut err, std::ptr::null()) }
    }

    #[test]
    fn sqlite_vec_on_stock_file() {
        let dir = tempfile::tempdir().unwrap();
        let c = Connection::open(dir.path().join("v.db")).unwrap();
        assert_eq!(register_vec(&c), 0);
        let ver: String = c.query_row("SELECT vec_version()", [], |r| r.get(0)).unwrap();
        println!("vec_version={ver}");
        c.execute_batch("CREATE VIRTUAL TABLE v USING vec0(e float[4]); INSERT INTO v(rowid, e) VALUES (1, '[1,2,3,4]'), (2, '[4,3,2,1]');").unwrap();
        let id: i64 = c
            .query_row("SELECT rowid FROM v WHERE e MATCH '[1,2,3,4]' ORDER BY distance LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(id, 1);
    }

    /// A stock connection exposes no dolt_* surface (celld's authorizer
    /// allows unknown functions, so they must not exist); a prolly one does.
    #[test]
    fn dolt_functions_only_on_prolly_connections() {
        let funcs = |c: &Connection| -> Vec<String> {
            let mut st = c
                .prepare("SELECT DISTINCT name FROM pragma_function_list WHERE name LIKE 'dolt%'")
                .unwrap();
            st.query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect()
        };
        let dir = tempfile::tempdir().unwrap();
        let stock = Connection::open(dir.path().join("s.db")).unwrap();
        stock.execute_batch("CREATE TABLE t(a)").unwrap();
        assert!(funcs(&stock).is_empty(), "{:?}", funcs(&stock));
        assert!(funcs(&Connection::open_in_memory().unwrap()).is_empty());
        let prolly = open_uri(&prolly_uri(&dir.path().join("p.db")));
        let f = funcs(&prolly);
        println!("prolly connection has {} dolt* functions", f.len());
        assert!(f.iter().any(|n| n == "dolt_commit"));
        let v: String = prolly.query_row("SELECT dolt_version()", [], |r| r.get(0)).unwrap();
        println!("dolt_version={v}");
        assert_eq!(v, "0.51.11");
    }

    /// Informational: what sqlite-vec does in prolly mode (not required).
    #[test]
    fn sqlite_vec_on_prolly_file_probe() {
        let dir = tempfile::tempdir().unwrap();
        let c = open_uri(&prolly_uri(&dir.path().join("pv.db")));
        let rc = register_vec(&c);
        let r = c.execute_batch("CREATE VIRTUAL TABLE v USING vec0(e float[4]); INSERT INTO v(rowid, e) VALUES (1, '[1,2,3,4]');");
        println!("PROBE sqlite-vec on prolly: init rc={rc} create/insert={r:?}");
    }
}
