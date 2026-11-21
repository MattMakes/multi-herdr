use std::path::Path;

use horch_core::fleetd::task_execution::domain::retry::Outcome;
use horch_core::taskledger::schema;
use rusqlite::types::ValueRef;
use rusqlite::Connection;

const BEFORE: i64 = schema::PHASE_7_VERSION - 1;

const HEALTH_WORDS: [&str; 2] = ["profile_unhealthy", "profile_healthy"];

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn stored_sql(conn: &Connection, name: &str) -> String {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE name = ?1",
        [name],
        |r| r.get(0),
    )
    .unwrap()
}

fn tables(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn phase_6_file(path: &Path) {
    let mut conn = Connection::open(path).unwrap();
    schema::migrate_to(&mut conn, BEFORE).unwrap();
    as_an_older_binary(&conn);
    conn.execute_batch(
        "INSERT INTO events (at, run_id, task_id, worker_id, actor, kind, text, cause, data_json)
            VALUES ('2026-11-01T00:00:00Z', 'r1', 't1', 'w1', 'horch', 'note', 'a', NULL, NULL),
                   ('2026-11-01T00:00:01Z', 'r1', 't1', 'w1', 'horch', 'hold_started', 'b', 'health', '{}'),
                   ('2026-11-01T00:00:02Z', NULL, NULL, NULL, 'horch', 'transition', 'c', 'c', '{\"n\":1}');
         DELETE FROM events WHERE text = 'c';
         PRAGMA foreign_keys = OFF;
         PRAGMA ignore_check_constraints = ON;",
    )
    .unwrap();
    for table in tables(&conn) {
        if table == "events" || table == "sqlite_sequence" {
            continue;
        }
        let columns: Vec<(String, String)> = conn
            .prepare("SELECT name, type FROM pragma_table_info(?1) ORDER BY cid")
            .unwrap()
            .query_map([&table], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let names: Vec<&str> = columns.iter().map(|(n, _)| n.as_str()).collect();
        let values: Vec<String> = columns
            .iter()
            .map(|(name, ty)| match ty.to_ascii_uppercase().as_str() {
                "INTEGER" => "7".to_string(),
                _ => format!("'{table}.{name}'"),
            })
            .collect();
        conn.execute(
            &format!(
                "INSERT INTO {table} ({}) VALUES ({})",
                names.join(", "),
                values.join(", ")
            ),
            [],
        )
        .unwrap_or_else(|e| panic!("{table}: {e}"));
    }
    conn.execute_batch("PRAGMA ignore_check_constraints = OFF;")
        .unwrap();
}

fn as_an_older_binary(conn: &Connection) {
    let table = stored_sql(conn, "events");
    let (_, columns) = table.split_once('(').unwrap();
    let mut older = format!("CREATE TABLE events_old ({columns}");
    for word in HEALTH_WORDS {
        older = older.replace(&format!(", '{word}'"), "");
    }
    assert!(older.contains("'wait_ended'"), "{older}");
    for word in HEALTH_WORDS {
        assert!(!older.contains(word), "{older}");
    }
    let indexes = [
        stored_sql(conn, "events_by_task"),
        stored_sql(conn, "events_by_worker"),
    ];
    conn.execute_batch(&format!(
        "{older};
DROP TABLE events;
ALTER TABLE events_old RENAME TO events;
{};",
        indexes.join(";\n")
    ))
    .unwrap();
}

fn before() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_6_file(&path);
    let conn = Connection::open(&path).unwrap();
    assert_eq!(user_version(&conn), BEFORE);
    (dir, conn)
}

fn migrated() -> (tempfile::TempDir, Connection) {
    let (dir, mut conn) = before();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_7_VERSION).unwrap();
    assert_eq!(user_version(&conn), schema::PHASE_7_VERSION);
    (dir, conn)
}

fn event(conn: &Connection, kind: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO events (at, actor, kind, text, entity, entity_id, cause)
            VALUES ('now', 'horch', ?1, '', 'profile', 'opus-hardening', 'conformance')",
        [kind],
    )
}

fn all_rows(conn: &Connection) -> Vec<(String, Vec<String>)> {
    tables(conn)
        .into_iter()
        .map(|table| {
            let mut stmt = conn
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .unwrap();
            let n = stmt.column_count();
            let mut rows: Vec<String> = stmt
                .query_map([], |r| {
                    let cells: Vec<String> = (0..n)
                        .map(|i| match r.get_ref(i).unwrap() {
                            ValueRef::Text(t) => format!("'{}'", String::from_utf8_lossy(t)),
                            other => format!("{other:?}"),
                        })
                        .collect();
                    Ok(cells.join(" | "))
                })
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            rows.sort();
            (table, rows)
        })
        .collect()
}

#[test]
fn p7_migration_accepts_the_profile_health_words() {
    let (_dir, conn) = before();
    for word in HEALTH_WORDS {
        let err = event(&conn, word).unwrap_err().to_string();
        assert!(err.contains("CHECK constraint failed"), "{word}: {err}");
    }
    drop(conn);

    let (_dir, conn) = migrated();
    for word in HEALTH_WORDS {
        event(&conn, word).unwrap_or_else(|e| panic!("{word}: {e}"));
    }
    let err = event(&conn, "profile_sick").unwrap_err().to_string();
    assert!(err.contains("CHECK constraint failed"), "{err}");
    let words: Vec<String> = conn
        .prepare("SELECT kind FROM events WHERE entity = 'profile' ORDER BY event_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(words, HEALTH_WORDS);
}

#[test]
fn p7_migration_accepts_the_outcome_context_limit() {
    let word = Outcome::ALL
        .into_iter()
        .find(|o| o.as_str() == "context_limit")
        .expect("the attempt outcome context_limit");
    assert!(!word.burns_attempt(), "MAP.md of fleetd-p7, Q2");

    let (_dir, conn) = migrated();
    conn.execute_batch(
        "INSERT INTO projects VALUES ('p7', '/r', 'now');
         INSERT INTO runs (run_id, project_id, slug, status, started_at)
            VALUES ('r7', 'p7', 's', 'active', 'now');
         INSERT INTO workers (worker_id, run_id, persona, host, attempt, status)
            VALUES ('w7', 'r7', 'opus', 'h', 1, 'working');",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO attempts (id, worker_id, state, outcome) VALUES ('a7', 'w7', 'ended', ?1)",
        [word.as_str()],
    )
    .unwrap();
    let stored: String = conn
        .query_row("SELECT outcome FROM attempts WHERE id = 'a7'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(stored, "context_limit");
}

#[test]
fn p7_migration_changes_no_row() {
    let (dir, conn) = before();
    let rows = all_rows(&conn);
    for (table, table_rows) in &rows {
        assert!(!table_rows.is_empty(), "{table} has no row");
    }
    let schema_of_other_tables = |conn: &Connection| -> Vec<String> {
        conn.prepare(
            "SELECT sql FROM sqlite_master WHERE tbl_name <> 'events' AND sql IS NOT NULL ORDER BY name",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    };
    let other = schema_of_other_tables(&conn);
    drop(conn);

    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_7_VERSION).unwrap();
    assert_eq!(user_version(&conn), schema::PHASE_7_VERSION);
    let after = all_rows(&conn);
    assert_eq!(
        after.iter().map(|(t, _)| t).collect::<Vec<_>>(),
        rows.iter().map(|(t, _)| t).collect::<Vec<_>>()
    );
    for ((table, new), (_, old)) in after.iter().zip(&rows) {
        assert_eq!(new, old, "{table}");
    }
    assert_eq!(schema_of_other_tables(&conn), other);
    event(&conn, "profile_healthy").unwrap();
    let last: i64 = conn
        .query_row("SELECT max(event_id) FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(last, 4);
    let copies = schema::backups_in(dir.path());
    assert_eq!(copies.len(), 1, "{copies:?}");
    let name = copies[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with(&format!("ledger.v{BEFORE}-backup-")),
        "{name}"
    );
}

#[test]
fn p7_newer_schema_is_refused() {
    assert_eq!(schema::VERSION, schema::PHASE_7_VERSION);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    {
        let mut conn = Connection::open(&path).unwrap();
        schema::migrate(&mut conn).unwrap();
        for word in HEALTH_WORDS {
            event(&conn, word).unwrap_or_else(|e| panic!("{word}: {e}"));
        }
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
