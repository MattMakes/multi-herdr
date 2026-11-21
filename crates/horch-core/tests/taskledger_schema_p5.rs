use std::collections::BTreeMap;
use std::path::Path;

use horch_core::taskledger::schema;
use horch_core::taskledger::store::{self, migrate_refusal};
use rusqlite::types::Value;
use rusqlite::Connection;

const BEFORE: i64 = schema::PHASE_5_VERSION - 1;

fn phase_4_file(path: &Path) {
    let mut conn = Connection::open(path).unwrap();
    schema::migrate_to(&mut conn, BEFORE).unwrap();
    conn.execute_batch(
        "INSERT INTO projects VALUES ('p', '/r', 'now');
         INSERT INTO fleetd VALUES (1, 1, 10, 20, 'then', 5000, 'then');
         INSERT INTO runs (run_id, project_id, slug, base_branch, run_branch, status, started_at)
            VALUES ('r1', 'p', 'alpha', 'main', 'fleet/a', 'active', '2026-11-01T00:00:00Z');
         INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by, status,
                created_at, updated_at)
            VALUES ('t1', 'r1', 1, 'a', 'a.md', 'opus', 'opus', 'running', 'now', 'now');
         INSERT INTO workers (worker_id, run_id, kind, persona, host, task_id, attempt, status)
            VALUES ('w1', 'r1', 'worker', 'opus', 'h', 't1', 1, 'working');
         INSERT INTO attempts (id, task_id, worker_id, state, started_at)
            VALUES ('t1/A1', 't1', 'w1', 'live', '2026-11-01T02:00:00Z');",
    )
    .unwrap();
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn migrate(path: &Path) -> Connection {
    let mut conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_5_VERSION).unwrap();
    conn
}

fn migrated() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_4_file(&path);
    let conn = migrate(&path);
    (dir, conn)
}

fn columns(conn: &Connection, table: &str) -> Vec<(String, String, bool, i64)> {
    conn.prepare("SELECT name, type, \"notnull\", pk FROM pragma_table_info(?1) ORDER BY cid")
        .unwrap()
        .query_map([table], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

type Dump = BTreeMap<String, Vec<Vec<Value>>>;

fn dump(conn: &Connection) -> Dump {
    let tables: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let mut out = Dump::new();
    for table in tables {
        let mut stmt = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let n = stmt.column_count();
        let rows = stmt
            .query_map([], |r| (0..n).map(|i| r.get::<_, Value>(i)).collect())
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        out.insert(table, rows);
    }
    out
}

fn request(conn: &Connection, id: &str, class: &str, state: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO requests (request_id, kind, payload_json, payload_sha256, caller, class, auth,
                state, result_json, reason, received_at, decided_at)
            VALUES (?1, 'task_cancel', '{}', 'h', 'u', ?2, NULL, ?3, NULL, NULL, 'now', NULL)",
        [id, class, state],
    )
}

#[test]
fn requests_table_has_the_columns_and_checks_of_the_design() {
    let (_dir, conn) = migrated();
    let text =
        |name: &str, not_null: bool, pk: i64| (name.to_string(), "TEXT".to_string(), not_null, pk);
    assert_eq!(
        columns(&conn, "requests"),
        vec![
            text("request_id", false, 1),
            text("kind", true, 0),
            text("payload_json", true, 0),
            text("payload_sha256", true, 0),
            text("caller", true, 0),
            text("class", true, 0),
            text("auth", false, 0),
            text("state", true, 0),
            text("result_json", false, 0),
            text("reason", false, 0),
            text("received_at", true, 0),
            text("decided_at", false, 0),
        ]
    );
    let n: i64 = conn
        .query_row("SELECT count(*) FROM requests", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "the release backfill of Phase 5 is empty");

    for (i, class) in ["operator", "worker"].into_iter().enumerate() {
        for (j, state) in ["pending", "applied", "refused"].into_iter().enumerate() {
            request(&conn, &format!("q{i}{j}"), class, state).unwrap();
        }
    }
    assert!(
        request(&conn, "a1", "admin", "pending").is_err(),
        "class admin"
    );
    assert!(
        request(&conn, "a2", "operator", "done").is_err(),
        "state done"
    );
    assert!(
        request(&conn, "q00", "operator", "pending").is_err(),
        "request_id is the key"
    );
    assert!(
        conn.execute(
            "INSERT INTO requests (request_id, kind, payload_json, payload_sha256, caller, class, state, received_at)
                VALUES ('a3', 'task_cancel', '{}', 'h', 'u', NULL, 'pending', 'now')",
            [],
        )
        .is_err(),
        "class is not null"
    );
    assert!(conn
        .execute(
            "UPDATE requests SET state = 'done' WHERE request_id = 'q00'",
            []
        )
        .is_err());
    conn.execute(
        "UPDATE requests SET state = 'applied', auth = 'token', result_json = '{}', decided_at = 'now'
            WHERE request_id = 'q00'",
        [],
    )
    .unwrap();
}

#[test]
fn polite_stops_key_is_pid_and_start() {
    let (_dir, conn) = migrated();
    assert_eq!(
        columns(&conn, "polite_stops"),
        vec![
            ("pid".to_string(), "INTEGER".to_string(), true, 1),
            ("pid_start".to_string(), "INTEGER".to_string(), true, 2),
            ("at_s".to_string(), "INTEGER".to_string(), true, 0),
            ("pane".to_string(), "TEXT".to_string(), false, 0),
        ]
    );
    let n: i64 = conn
        .query_row("SELECT count(*) FROM polite_stops", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "the migration reads no stop file");

    let stop = "INSERT INTO polite_stops (pid, pid_start, at_s, pane) VALUES (?1, ?2, ?3, ?4)";
    conn.execute(stop, rusqlite::params![42, 7, 100, "w1:p1"])
        .unwrap();
    conn.execute(stop, rusqlite::params![42, 8, 100, None::<String>])
        .unwrap();
    conn.execute(stop, rusqlite::params![43, 7, 100, None::<String>])
        .unwrap();
    assert!(
        conn.execute(stop, rusqlite::params![42, 7, 200, None::<String>])
            .is_err(),
        "1 row for each pid and start"
    );
    assert!(
        conn.execute(stop, rusqlite::params![44, "seven", 100, None::<String>])
            .is_err(),
        "pid_start is an integer"
    );
    assert!(
        conn.execute(stop, rusqlite::params![44, 7, None::<i64>, None::<String>])
            .is_err(),
        "at_s is not null"
    );
}

#[test]
fn migration_to_the_next_version_writes_a_backup_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_4_file(&path);
    let before = dump(&Connection::open(&path).unwrap());

    let conn = migrate(&path);
    assert_eq!(user_version(&conn), schema::PHASE_5_VERSION);
    let mut after = dump(&conn);
    assert_eq!(after.remove("requests"), Some(vec![]));
    assert_eq!(after.remove("polite_stops"), Some(vec![]));
    assert_eq!(
        after, before,
        "each row and each table of the old file is unchanged"
    );

    let backups = schema::backups_in(dir.path());
    assert_eq!(backups.len(), 1, "{backups:?}");
    let name = backups[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with(&format!("ledger.v{BEFORE}-backup-")),
        "{name}"
    );
    let copy = Connection::open(&backups[0]).unwrap();
    assert_eq!(
        user_version(&copy),
        BEFORE,
        "the backup is the file before the step"
    );
    assert_eq!(dump(&copy), before);
}

#[test]
fn newer_schema_is_still_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    {
        let mut conn = Connection::open(&path).unwrap();
        schema::migrate(&mut conn).unwrap();
        conn.pragma_update(None, "user_version", schema::VERSION + 1)
            .unwrap();
    }
    let mut conn = Connection::open(&path).unwrap();
    let err = format!("{:#}", schema::migrate(&mut conn).unwrap_err());
    assert!(
        err.contains(&format!(
            "is schema version {} and this horch knows only up to {}",
            schema::VERSION + 1,
            schema::VERSION
        )),
        "{err}"
    );
    assert_eq!(user_version(&conn), schema::VERSION + 1);
    assert!(schema::backups_in(dir.path()).is_empty());
}

#[test]
fn development_build_does_not_migrate_the_shared_ledger() {
    let home = tempfile::tempdir().unwrap();
    let path = store::default_path(home.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    phase_4_file(&path);
    let dev = Path::new("/Users/op/src/horch/target/debug/horch");
    let guard = |from: i64| match migrate_refusal(dev, &path, Some(home.path()), None, from) {
        Some(text) => anyhow::bail!(text),
        None => Ok(()),
    };

    let mut conn = Connection::open(&path).unwrap();
    let text = format!(
        "{:#}",
        schema::migrate_guarded(&mut conn, &guard).err().unwrap()
    );
    assert!(
        text.contains("refuses to migrate the shared task ledger"),
        "{text}"
    );
    assert_eq!(user_version(&conn), BEFORE);
    assert!(conn.prepare("SELECT * FROM requests").is_err());
    assert!(schema::backups_in(path.parent().unwrap()).is_empty());
}
