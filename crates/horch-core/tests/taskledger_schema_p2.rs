//! The schema step of Phase 2 (docs/specs/fleetd/design.md, "Ledger" and
//! "Migration plan" item 4; docs/tickets/fleetd-p2/02-schema-phase2.md): the
//! `leases` and `questions` tables, the new columns of `tasks`, `attempts`
//! and `worker_waits`, the release backfill (MAP.md, Q21), and the 3 safety
//! rules of OR-6 for this step.

use std::collections::BTreeMap;
use std::path::Path;

use horch_core::taskledger::schema;
use horch_core::taskledger::store::{self, migrate_refusal};
use rusqlite::types::Value;
use rusqlite::Connection;

/// The fleet clock of the `fleetd` row in [`phase_1_file`].
const CLOCK: i64 = 5000;
/// The default `liveness.lease_s` of the design.
const LEASE_S: i64 = 120;
/// The default `liveness.max_wait_s` of the design.
const MAX_WAIT_S: i64 = 3600;

/// A Phase 1 ledger at `path`, built by the real steps, with the fleet clock
/// at `clock` (no `fleetd` row for `None`):
/// - `t1` runs with the live attempt `t1/A2` of `w1`.
/// - `t2` runs with the attempt `t2/A1`, still `launching`.
/// - `t3` is queued with no attempt.
/// - `t4` runs, and its only attempt `t4/A1` ended.
/// - `t5` runs with 2 live attempts: `t5/A2` of its current worker `w5`, and
///   `t5/A1` of `w6`, which started later.
/// - `t6` lands with the live attempt `t6/A1`.
/// - `w8` is an orchestrator with the live attempt `o/1` and no task.
/// - `w1` has a question wait and a background wait.
fn phase_1_file(path: &Path, clock: Option<i64>) {
    let mut conn = Connection::open(path).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_1_VERSION).unwrap();
    conn.execute_batch(
        "INSERT INTO projects VALUES ('p', '/r', 'now');
         INSERT INTO runs (run_id, project_id, slug, status, started_at) VALUES ('r1', 'p', 's', 'active', 'now');
         INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by, status,
                attempts, current_worker_id, held_reason, created_at, updated_at)
            VALUES ('t1', 'r1', 1, 'a', 'a.md', 'opus', 'opus', 'running', 2, 'w1', NULL, 'now', 'now'),
                   ('t2', 'r1', 2, 'b', 'b.md', 'opus', 'opus', 'running', 1, 'w2', NULL, 'now', 'now'),
                   ('t3', 'r1', 3, 'c', 'c.md', 'opus', 'opus', 'queued', 0, NULL, 'held', 'now', 'now'),
                   ('t4', 'r1', 4, 'd', 'd.md', 'opus', 'opus', 'running', 1, 'w4', NULL, 'now', 'now'),
                   ('t5', 'r1', 5, 'e', 'e.md', 'opus', 'opus', 'running', 2, 'w5', NULL, 'now', 'now'),
                   ('t6', 'r1', 6, 'f', 'f.md', 'opus', 'opus', 'landing', 1, 'w7', NULL, 'now', 'now');
         INSERT INTO workers (worker_id, run_id, kind, persona, host, task_id, attempt, status)
            VALUES ('w1', 'r1', 'worker', 'opus', 'h', 't1', 2, 'working'),
                   ('w2', 'r1', 'worker', 'opus', 'h', 't2', 1, 'starting'),
                   ('w4', 'r1', 'worker', 'opus', 'h', 't4', 1, 'stopped'),
                   ('w5', 'r1', 'worker', 'opus', 'h', 't5', 2, 'working'),
                   ('w6', 'r1', 'worker', 'opus', 'h', 't5', 1, 'starting'),
                   ('w7', 'r1', 'worker', 'opus', 'h', 't6', 1, 'working'),
                   ('w8', 'r1', 'orchestrator', 'opus', 'h', NULL, NULL, 'working');
         INSERT INTO attempts (id, task_id, worker_id, state, started_at)
            VALUES ('t1/A2', 't1', 'w1', 'live', '2026-11-17T10:00:00Z'),
                   ('t2/A1', 't2', 'w2', 'launching', NULL),
                   ('t4/A1', 't4', 'w4', 'ended', '2026-11-17T09:00:00Z'),
                   ('t5/A2', 't5', 'w5', 'live', '2026-11-17T10:00:00Z'),
                   ('t5/A1', 't5', 'w6', 'live', '2026-11-17T11:00:00Z'),
                   ('t6/A1', 't6', 'w7', 'live', '2026-11-17T10:00:00Z'),
                   ('o/1', NULL, 'w8', 'live', '2026-11-17T10:00:00Z');
         INSERT INTO worker_waits (worker_id, kind, reason, started_at, deadline)
            VALUES ('w1', 'question', 'q', 'now', 'later'),
                   ('w1', 'background', 'b', 'now', 'later');
         INSERT INTO events (at, run_id, task_id, worker_id, actor, kind, text)
            VALUES ('now', 'r1', 't1', 'w1', 'worker', 'claimed', 'y');",
    )
    .unwrap();
    if let Some(clock) = clock {
        conn.execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                VALUES (1, 1, 7, 1, 'now', ?1, 'now')",
            [clock],
        )
        .unwrap();
    }
    assert_eq!(user_version(&conn), schema::PHASE_1_VERSION);
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

/// A Phase 1 ledger with the fleet clock at [`CLOCK`], migrated: its folder
/// (kept alive) and a connection with foreign keys on.
fn migrated() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_1_file(&path, Some(CLOCK));
    let conn = migrate(&path);
    (dir, conn)
}

/// Each lease: `task_id -> (attempt_id, expires_at, renewed_at, renew_basis)`.
fn leases(conn: &Connection) -> BTreeMap<String, (String, i64, String, String)> {
    conn.prepare("SELECT task_id, attempt_id, expires_at, renewed_at, renew_basis FROM leases")
        .unwrap()
        .query_map([], |r| {
            Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// `active_at` of each attempt, by id.
fn active_at(conn: &Connection) -> BTreeMap<String, Option<i64>> {
    conn.prepare("SELECT id, active_at FROM attempts")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// `due_at` of each wait, by `(worker_id, kind)`.
fn wait_due_at(conn: &Connection) -> BTreeMap<(String, String), Option<i64>> {
    conn.prepare("SELECT worker_id, kind, due_at FROM worker_waits")
        .unwrap()
        .query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn phase2_migration_adds_leases_and_questions() {
    let (_dir, conn) = migrated();
    assert_eq!(user_version(&conn), schema::VERSION);

    assert_eq!(
        columns(&conn, "leases"),
        [
            "task_id",
            "attempt_id",
            "expires_at",
            "renewed_at",
            "renew_basis"
        ]
    );
    assert_eq!(
        columns(&conn, "questions"),
        [
            "id",
            "task_id",
            "feature_id",
            "owner",
            "state",
            "answer_case",
            "citation",
            "due_at",
            "asked_at",
            "answered_at",
        ]
    );
    let tail = |table: &str, n: usize| {
        let all = columns(&conn, table);
        all[all.len() - n..].to_vec()
    };
    assert_eq!(
        tail("tasks", 4),
        ["not_before", "due_at", "replans", "cancel_requested"]
    );
    assert_eq!(tail("attempts", 1), ["active_at"]);
    assert_eq!(tail("worker_waits", 2), ["due_at", "question_id"]);
    let replans: i64 = conn
        .query_row("SELECT replans FROM tasks WHERE task_id = 't1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(replans, 0);

    // The CHECK lists of `questions`. `feature_id` holds the run id, with no
    // reference (MAP.md, Q13).
    let ask = |id: &str, state: &str, case: Option<&str>| {
        conn.execute(
            "INSERT INTO questions (id, task_id, feature_id, owner, state, answer_case, due_at, asked_at)
                VALUES (?1, 't1', 'r1', 'orchestrator', ?2, ?3, 1, 'now')",
            rusqlite::params![id, state, case],
        )
    };
    for state in ["open", "answered", "escalated"] {
        ask(&format!("q-{state}"), state, None).unwrap();
    }
    for case in ["covered", "revise", "replan", "own_decision"] {
        ask(&format!("q-{case}"), "answered", Some(case)).unwrap();
    }
    assert!(ask("q-bad-state", "closed", None).is_err());
    assert!(ask("q-bad-case", "answered", Some("ignored")).is_err());
    assert!(
        conn.execute(
            "INSERT INTO questions (id, task_id, feature_id, owner, state, due_at, asked_at)
                VALUES ('q-no-task', 'nope', 'r1', 'o', 'open', 1, 'now')",
            [],
        )
        .is_err(),
        "task_id must name a task"
    );
    // The new step leaves no broken reference.
    let broken: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(broken, 0);
}

#[test]
fn held_task_with_a_live_attempt_gets_1_lease_with_a_full_time() {
    let (_dir, conn) = migrated();
    let leases = leases(&conn);
    let (attempt, expires_at, renewed_at, basis) = &leases["t1"];
    assert_eq!(attempt, "t1/A2");
    assert_eq!(*expires_at, CLOCK + LEASE_S);
    assert!(
        horch_core::clock::parse(renewed_at).is_some(),
        "renewed_at is a ledger stamp: {renewed_at}"
    );
    assert_eq!(basis, "backfill");
    // 2 live attempts: the lease goes to the attempt of the task's current
    // worker, not to the one that started later.
    assert_eq!(leases["t5"].0, "t5/A2");
    assert_eq!(leases["t5"].1, CLOCK + LEASE_S);
}

#[test]
fn task_with_no_live_attempt_gets_no_lease() {
    let (_dir, conn) = migrated();
    let held: Vec<String> = leases(&conn).into_keys().collect();
    // t2 (launching), t3 (no attempt), t4 (ended) and t6 (landing: no
    // lease, MAP.md Q20) get none.
    assert_eq!(held, ["t1", "t5"]);
}

#[test]
fn open_wait_gets_a_due_at_on_the_fleet_clock() {
    let (_dir, conn) = migrated();
    let due = wait_due_at(&conn);
    assert_eq!(due.len(), 2);
    for (wait, at) in &due {
        assert_eq!(*at, Some(CLOCK + MAX_WAIT_S), "{wait:?}");
    }
    let linked: i64 = conn
        .query_row(
            "SELECT count(*) FROM worker_waits WHERE question_id IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(linked, 0);
}

#[test]
fn live_attempt_gets_active_at_on_the_fleet_clock() {
    let (_dir, conn) = migrated();
    let got = active_at(&conn);
    for id in ["t1/A2", "t5/A1", "t5/A2", "t6/A1", "o/1"] {
        assert_eq!(got[id], Some(CLOCK), "{id}");
    }
    for id in ["t2/A1", "t4/A1"] {
        assert_eq!(got[id], None, "{id}");
    }
}

#[test]
fn backfill_with_no_fleetd_row_counts_from_fleet_clock_0() {
    // fleetd starts the fleet clock at 0 when it writes its first row.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_1_file(&path, None);
    let conn = migrate(&path);
    assert_eq!(leases(&conn)["t1"].1, LEASE_S);
    assert_eq!(active_at(&conn)["t1/A2"], Some(0));
    assert!(wait_due_at(&conn)
        .values()
        .all(|at| *at == Some(MAX_WAIT_S)));
}

#[test]
fn lease_is_unique_for_each_task() {
    let (_dir, conn) = migrated();
    let lease = |task: &str, attempt: &str| {
        conn.execute(
            "INSERT INTO leases (task_id, attempt_id, expires_at, renewed_at, renew_basis)
                VALUES (?1, ?2, 1, 'now', 'heartbeat')",
            [task, attempt],
        )
    };
    // FM-14: a second lease for 1 task is refused.
    assert!(lease("t1", "t1/A2").is_err());
    assert!(lease("t5", "t5/A1").is_err());
    assert_eq!(leases(&conn).len(), 2);
    // A lease names a task and an attempt that exist.
    assert!(lease("nope", "t1/A2").is_err());
    assert!(lease("t3", "nope").is_err());
    lease("t3", "t2/A1").unwrap();
}

#[test]
fn newer_schema_is_refused_and_backup_precedes_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_1_file(&path, Some(CLOCK));
    let before = dump(&Connection::open(&path).unwrap(), None);

    // The backup holds the Phase 1 file as it was before the step.
    let conn = migrate(&path);
    assert_eq!(user_version(&conn), schema::VERSION);
    let backups = schema::backups_in(dir.path());
    assert_eq!(backups.len(), 1);
    let name = backups[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert!(
        name.starts_with(&format!("ledger.v{}-backup-", schema::PHASE_1_VERSION)),
        "{name}"
    );
    let copy = Connection::open(&backups[0]).unwrap();
    assert_eq!(user_version(&copy), schema::PHASE_1_VERSION);
    assert!(copy.prepare("SELECT * FROM leases").is_err());
    assert_eq!(dump(&copy, None), before);
    drop(conn);

    // A file of a newer horch is refused, unchanged and with no backup.
    let newer = tempfile::tempdir().unwrap();
    let path = newer.path().join("ledger.sqlite3");
    {
        let mut conn = Connection::open(&path).unwrap();
        schema::migrate(&mut conn).unwrap();
        conn.pragma_update(None, "user_version", schema::VERSION + 1)
            .unwrap();
    }
    let mut conn = Connection::open(&path).unwrap();
    let text = format!("{:#}", schema::migrate(&mut conn).err().unwrap());
    assert!(
        text.contains(&format!(
            "is schema version {} and this horch knows only up to {}",
            schema::VERSION + 1,
            schema::VERSION
        )),
        "{text}"
    );
    assert!(text.contains("Run horch self-update"), "{text}");
    assert_eq!(user_version(&conn), schema::VERSION + 1);
    assert!(schema::backups_in(newer.path()).is_empty());
}

#[test]
fn development_build_does_not_migrate_a_phase_1_shared_ledger() {
    let home = tempfile::tempdir().unwrap();
    let path = store::default_path(home.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    phase_1_file(&path, Some(CLOCK));
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
    assert_eq!(user_version(&conn), schema::PHASE_1_VERSION);
    assert!(conn.prepare("SELECT * FROM leases").is_err());
    assert!(schema::backups_in(path.parent().unwrap()).is_empty());
}

/// Each row of each table, as text, by table. With `only`, the columns of
/// each table in `only`: a dump after the migration then reads the columns
/// that the dump before had.
type Dump = BTreeMap<String, (Vec<String>, Vec<Vec<String>>)>;

fn dump(conn: &Connection, only: Option<&Dump>) -> Dump {
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
        let cols = match only {
            Some(old) => match old.get(&table) {
                Some((cols, _)) => cols.clone(),
                None => continue,
            },
            None => columns(conn, &table),
        };
        let sql = format!("SELECT {} FROM {table} ORDER BY rowid", cols.join(", "));
        let rows = conn
            .prepare(&sql)
            .unwrap()
            .query_map([], |r| {
                (0..cols.len())
                    .map(|i| r.get::<_, Value>(i).map(|v| format!("{v:?}")))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        out.insert(table, (cols, rows));
    }
    out
}

#[test]
fn migration_from_the_phase1_version_keeps_each_row() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_1_file(&path, Some(CLOCK));
    let before = dump(&Connection::open(&path).unwrap(), None);
    assert!(before["tasks"].1.len() == 6 && before["attempts"].1.len() == 7);

    let conn = migrate(&path);
    assert!(
        conn.prepare("SELECT * FROM leases").is_ok(),
        "the migration runs the Phase 2 step"
    );
    let after = dump(&conn, Some(&before));
    assert_eq!(after, before);
}

/// The query plan of `sql`, 1 line for each step.
fn plan(conn: &Connection, sql: &str) -> String {
    conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap()
        .query_map([], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join("\n")
}

/// The count queries of the whole-run watchdog duties (T-322) read the run's
/// tasks and workers through an index on `(run_id, status)`, not by a scan.
#[test]
fn count_queries_of_the_watchdog_duties_use_the_run_status_indexes() {
    let (_dir, conn) = migrated();
    // `wakes::settled_line`.
    let settled = plan(
        &conn,
        "SELECT count(*) FROM tasks WHERE run_id = 'r1' AND status NOT IN ('done', 'cancelled')",
    );
    assert!(settled.contains("tasks_by_run_status"), "{settled}");
    // `watchdog::startable_rewake`.
    let startable = plan(
        &conn,
        "SELECT count(*) FROM tasks WHERE run_id = 'r1' AND status = 'queued' AND held_reason IS NULL",
    );
    assert!(startable.contains("tasks_by_run_status"), "{startable}");
    // `wakes::stuck_line`.
    let stuck = plan(
        &conn,
        "SELECT EXISTS (SELECT 1 FROM tasks WHERE run_id = 'r1' AND status IN ('queued', 'running', 'landing'))
            AND NOT EXISTS (SELECT 1 FROM workers
                WHERE run_id = 'r1' AND kind = 'worker' AND status = 'working')",
    );
    assert!(stuck.contains("tasks_by_run_status"), "{stuck}");
    assert!(stuck.contains("workers_by_run_status"), "{stuck}");
}
