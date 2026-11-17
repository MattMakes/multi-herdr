//! Schema v8 (docs/specs/fleetd/design.md, "Ledger" and "Migration plan";
//! docs/tickets/fleetd-p1/03-schema-v8.md): the Phase 1 tables, the new
//! columns of `events`, the backfill of `attempts`, and the 3 safety rules
//! of OR-6 (a backup first, a newer schema refused, no migration of the
//! shared ledger by a development build).

use std::path::Path;

use horch_core::taskledger::schema;
use horch_core::taskledger::store::{self, migrate_refusal};
use rusqlite::Connection;

/// A version 7 ledger at `path`, built by the migration of today, with rows
/// in `tasks`, `workers` and `events`:
/// - `w1` works on `t1` (attempt 2), so it is live.
/// - `w2` starts on `t2` (attempt 1), so it is live.
/// - `w3` stopped on `t3`, so it is not live.
/// - `w4` spawns with no task yet.
fn version_7_file(path: &Path) {
    let mut conn = Connection::open(path).unwrap();
    schema::migrate_to(&mut conn, 7).unwrap();
    conn.execute_batch(
        "INSERT INTO projects VALUES ('p', '/r', 'now');
         INSERT INTO runs (run_id, project_id, slug, status, started_at) VALUES ('r1', 'p', 's', 'active', 'now');
         INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by, status,
                attempts, created_at, updated_at)
            VALUES ('t1', 'r1', 1, 'a', 'a.md', 'opus', 'opus', 'running', 2, 'now', 'now'),
                   ('t2', 'r1', 2, 'b', 'b.md', 'opus', 'opus', 'running', 1, 'now', 'now'),
                   ('t3', 'r1', 3, 'c', 'c.md', 'opus', 'opus', 'queued', 1, 'now', 'now');
         INSERT INTO workers (worker_id, run_id, persona, host, task_id, attempt, status, agent_pid, agent_started)
            VALUES ('w1', 'r1', 'opus', 'h', 't1', 2, 'working', 101, 1700000001),
                   ('w2', 'r1', 'opus', 'h', 't2', 1, 'starting', 102, 1700000002),
                   ('w3', 'r1', 'opus', 'h', 't3', 1, 'stopped', 103, 1700000003),
                   ('w4', 'r1', 'opus', 'h', NULL, NULL, 'spawning', 104, 1700000004);
         INSERT INTO events (at, run_id, task_id, worker_id, actor, kind, text)
            VALUES ('then', 'r1', 't1', NULL, 'horch', 'added', 'x'),
                   ('now', 'r1', 't1', 'w1', 'worker', 'claimed', 'y');",
    )
    .unwrap();
    assert_eq!(user_version(&conn), 7);
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn columns(conn: &Connection, table: &str) -> Vec<String> {
    conn.prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
        .unwrap()
        .query_map([table], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// `migrate` on the file at `path`, as `Store::open` runs it.
fn migrate(path: &Path) -> Connection {
    let mut conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    schema::migrate(&mut conn).unwrap();
    conn
}

/// A version 7 ledger, migrated: its folder (kept alive) and a connection
/// with foreign keys on.
fn migrated_version_7() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    version_7_file(&path);
    let conn = migrate(&path);
    (dir, conn)
}

#[test]
fn migrate_from_version_7_adds_the_phase_1_tables_and_keeps_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    version_7_file(&path);
    let conn = migrate(&path);
    assert_eq!(user_version(&conn), 8);
    assert_eq!(schema::VERSION, 8);

    let want: [(&str, &[&str]); 5] = [
        (
            "fleetd",
            &[
                "id",
                "generation",
                "pid",
                "pid_start",
                "started_at",
                "fleet_clock_s",
                "clock_ticked_at",
            ],
        ),
        (
            "attempts",
            &[
                "id",
                "task_id",
                "worker_id",
                "mux_handle",
                "pid",
                "pid_start",
                "state",
                "heartbeat_seq",
                "started_at",
                "ended_at",
                "exit_code",
                "outcome",
            ],
        ),
        (
            "jobs",
            &[
                "id",
                "kind",
                "subject",
                "repo",
                "input_key",
                "run",
                "abandoned_runs",
                "not_before",
                "state",
                "pid",
                "pid_start",
                "time_limit_s",
                "deadline",
                "result_json",
                "created_at",
                "started_at",
                "ended_at",
            ],
        ),
        (
            "pass_cursors",
            &["kind", "after_key", "lap_started_at", "prev_lap_started_at"],
        ),
        (
            "needs_you",
            &[
                "key",
                "kind",
                "entity",
                "entity_id",
                "state",
                "text",
                "created_at",
                "ended_at",
            ],
        ),
    ];
    for (table, cols) in want {
        assert_eq!(columns(&conn, table), cols, "{table}");
    }
    assert_eq!(
        columns(&conn, "events"),
        [
            "event_id",
            "at",
            "run_id",
            "task_id",
            "worker_id",
            "actor",
            "kind",
            "text",
            "entity",
            "entity_id",
            "from_state",
            "to_state",
            "cause",
            "data_json",
        ]
    );

    // The old rows stay, with null in each new column.
    type Event = (i64, String, String, Option<String>, Option<String>);
    let events: Vec<Event> = conn
        .prepare("SELECT event_id, kind, text, entity, data_json FROM events ORDER BY event_id")
        .unwrap()
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        events,
        [
            (1, "added".into(), "x".into(), None, None),
            (2, "claimed".into(), "y".into(), None, None),
        ]
    );
    for (table, n) in [("tasks", 3), ("workers", 4), ("projects", 1), ("runs", 1)] {
        let got: i64 = conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(got, n, "{table}");
    }
    drop(conn);

    // A second migration changes nothing and writes no backup.
    let before = schema::backups_in(dir.path()).len();
    let conn = migrate(&path);
    assert_eq!(user_version(&conn), 8);
    assert_eq!(schema::backups_in(dir.path()).len(), before);
}

#[test]
fn events_v8_takes_a_transition_event_and_keeps_its_indexes() {
    let (_dir, conn) = migrated_version_7();
    // A transition event: the kind `transition` is in the CHECK list, and a
    // new event takes the next id.
    conn.execute(
        "INSERT INTO events (at, actor, kind, entity, entity_id, from_state, to_state, cause, data_json)
            VALUES ('now', 'horch', 'transition', 'task', 't3', 'queued', 'running', 'claimed', '{}')",
        [],
    )
    .unwrap();
    assert_eq!(conn.last_insert_rowid(), 3);
    assert!(conn
        .execute(
            "INSERT INTO events (at, actor, kind) VALUES ('now', 'horch', 'nope')",
            []
        )
        .is_err());
    // The indexes of `events` are back.
    let indexes: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND tbl_name = 'events' AND sql IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(indexes, 2);
}

#[test]
fn fleetd_starts_empty_and_its_pid_start_fits_i64() {
    let (_dir, conn) = migrated_version_7();
    // `fleetd` has no row: ticket 14 writes generation 1 at the first start.
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM fleetd", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);
    let fleetd = "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
        VALUES (?1, 1, 7, ?2, 'now', 0, 'now')";
    assert!(
        conn.execute(fleetd, rusqlite::params![2, 1]).is_err(),
        "id is 1"
    );
    // pid_start is an INTEGER that fits i64: a larger value is refused, not wrapped.
    assert!(conn
        .execute(fleetd, rusqlite::params![1, u64::MAX])
        .is_err());
    assert!(conn
        .execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                VALUES (1, 1, 7, 18446744073709551615, 'now', 0, 'now')",
            [],
        )
        .is_err());
    conn.execute(fleetd, rusqlite::params![1, i64::MAX])
        .unwrap();
}

#[test]
fn phase_1_check_lists_and_references_hold() {
    let (_dir, conn) = migrated_version_7();
    // The other CHECK lists hold.
    let state = |s: &str| {
        conn.execute(
            "INSERT INTO needs_you (key, kind, entity, entity_id, state, text, created_at)
                VALUES (?1, 'question', 'task', 't1', ?1, 'x', 'now')",
            [s],
        )
    };
    state("open").unwrap();
    assert!(state("closed").is_err());
    assert!(conn
        .execute(
            "INSERT INTO attempts (id, task_id, worker_id, state) VALUES ('t3/A2', 't3', 'w3', 'running')",
            [],
        )
        .is_err());
    assert!(conn
        .execute(
            "INSERT INTO attempts (id, task_id, worker_id, state) VALUES ('t3/A2', 't3', 'nope', 'launching')",
            [],
        )
        .is_err(), "worker_id names a worker");
    assert!(conn
        .execute(
            "INSERT INTO attempts (id, task_id, worker_id, state, pid_start) VALUES ('t3/A2', 't3', 'w3', 'launching', 18446744073709551615)",
            [],
        )
        .is_err(), "pid_start fits i64");
    // An orchestrator attempt has no task.
    conn.execute(
        "INSERT INTO attempts (id, task_id, worker_id, state) VALUES ('o/A1', NULL, 'w4', 'launching')",
        [],
    )
    .unwrap();
    assert!(conn
        .execute(
            "INSERT INTO jobs (kind, subject, input_key, state, time_limit_s, created_at)
                VALUES ('compile', 't1', 'k', 'queued', 60, 'now')",
            [],
        )
        .is_err());
    conn.execute(
        "INSERT INTO pass_cursors (kind, lap_started_at) VALUES ('dispatch', 'now')",
        [],
    )
    .unwrap();
}

#[test]
fn migrate_7_to_8_writes_a_backup_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    version_7_file(&path);
    assert!(schema::backups_in(dir.path()).is_empty());
    drop(migrate(&path));

    let copies = schema::backups_in(dir.path());
    assert_eq!(copies.len(), 1, "{copies:?}");
    let name = copies[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(name.starts_with("ledger.v7-backup-"), "{name}");
    let copy = Connection::open(&copies[0]).unwrap();
    assert_eq!(user_version(&copy), 7);
    // The backup is the state before the migration: no attempts table.
    assert!(copy.prepare("SELECT * FROM attempts").is_err());
    let workers: i64 = copy
        .query_row("SELECT count(*) FROM workers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(workers, 4);
}

#[test]
fn newer_schema_9_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    {
        let mut conn = Connection::open(&path).unwrap();
        schema::migrate(&mut conn).unwrap();
        conn.pragma_update(None, "user_version", 9).unwrap();
    }
    let mut conn = Connection::open(&path).unwrap();
    let text = format!("{:#}", schema::migrate(&mut conn).err().unwrap());
    assert!(
        text.contains("is schema version 9 and this horch knows only up to 8"),
        "{text}"
    );
    assert!(
        text.contains("a newer or development horch migrated the file"),
        "{text}"
    );
    assert!(text.contains("Run horch self-update"), "{text}");
    assert_eq!(text.lines().count(), 2, "{text}");
    assert_eq!(user_version(&conn), 9);
    assert!(schema::backups_in(dir.path()).is_empty());
}

#[test]
fn development_build_does_not_migrate_the_shared_ledger() {
    let home = tempfile::tempdir().unwrap();
    let path = store::default_path(home.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    version_7_file(&path);
    // The guard that `Store::open` passes, for a development build and a
    // ledger at `<passwd home>/.horch/ledger.sqlite3`.
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
    assert!(text.contains("from schema version 7 to 8"), "{text}");
    assert_eq!(user_version(&conn), 7);
    assert!(conn.prepare("SELECT * FROM attempts").is_err());
    // The guard runs before the backup.
    assert!(schema::backups_in(path.parent().unwrap()).is_empty());
}

#[test]
fn live_worker_gets_1_live_attempt_and_an_ended_worker_gets_none() {
    let (_dir, conn) = migrated_version_7();
    type Row = (
        String,
        Option<String>,
        String,
        Option<i64>,
        Option<i64>,
        String,
    );
    let attempts: Vec<Row> = conn
        .prepare("SELECT id, task_id, worker_id, pid, pid_start, state FROM attempts ORDER BY id")
        .unwrap()
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        attempts,
        [
            (
                "t1/A2".into(),
                Some("t1".into()),
                "w1".into(),
                Some(101),
                Some(1700000001),
                "live".into()
            ),
            (
                "t2/A1".into(),
                Some("t2".into()),
                "w2".into(),
                Some(102),
                Some(1700000002),
                "live".into()
            ),
        ]
    );
    let fk: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(fk, 0);
}

#[test]
fn jobs_one_active_refuses_a_second_queued_run_of_1_kind_and_subject() {
    let (_dir, conn) = migrated_version_7();
    let job = |run: i64, input: &str, state: &str, subject: &str| {
        conn.execute(
            "INSERT INTO jobs (kind, subject, input_key, run, state, time_limit_s, created_at)
                VALUES ('task_check', ?1, ?2, ?3, ?4, 1200, 'now')",
            rusqlite::params![subject, input, run, state],
        )
    };
    job(1, "k1", "queued", "t1").unwrap();
    assert!(job(1, "k2", "queued", "t1").is_err(), "a second queued run");
    assert!(
        job(1, "k2", "running", "t1").is_err(),
        "a running run beside a queued one"
    );
    // Another subject, and ended runs, are not active.
    job(1, "k1", "queued", "t2").unwrap();
    job(2, "k1", "passed", "t1").unwrap();
    job(3, "k1", "failed", "t1").unwrap();
    // UNIQUE (kind, subject, input_key, run).
    assert!(job(2, "k1", "abandoned", "t1").is_err());
    // Once the queued run ends, a new run may queue.
    conn.execute(
        "UPDATE jobs SET state = 'abandoned' WHERE subject = 't1' AND run = 1",
        [],
    )
    .unwrap();
    job(4, "k1", "queued", "t1").unwrap();
    // pid_start is an INTEGER that fits i64, as in `attempts` and `fleetd`.
    let start = "UPDATE jobs SET pid_start = ?1 WHERE subject = 't1' AND run = 4";
    conn.execute(start, [i64::MAX]).unwrap();
    assert!(conn.execute(start, [u64::MAX]).is_err());
    assert!(conn.execute(start, ["abc"]).is_err());
    let kinds: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'jobs_one_active'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kinds, 1);
}
