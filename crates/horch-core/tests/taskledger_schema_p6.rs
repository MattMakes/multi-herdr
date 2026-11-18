//! The schema step of Phase 6 (docs/specs/fleetd/design.md, "Ledger" and
//! "Migration plan" item 8; docs/tickets/fleetd-p6/02-schema.md): the tables
//! `ideas`, `requirements`, `task_covers` and `trial_merges`, the idea of
//! each task (FR-12), `tasks.fix_rounds` (RR-3), `attempts.idea_id`, and the
//! release backfill that gives each run in flight 1 idea (MAP.md of
//! fleetd-p6, Q5, Q6 and Q11).

use std::path::Path;

use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::schema;
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::Store;
use rusqlite::Connection;

/// The version before the Phase 6 step.
const BEFORE: i64 = schema::PHASE_6_VERSION - 1;

/// A ledger at [`BEFORE`] at `path`, built by the real steps, with:
/// - run 1 `alpha` with its feature `F-0001`: 1 running task, 1 done task,
///   and 1 open question that the orchestrator of the run owns.
/// - run 2 `beta` with its feature `F-0002`: 1 done and 1 cancelled task.
/// - run 3 `gamma` with no run branch and no feature: 1 ready task.
/// - run 4 `delta` with no run branch and no feature: 1 done task.
/// - run 5 `alpha` with a run branch and no feature (a run of today's flow,
///   made after the Phase 4 release): 1 running task.
fn phase_5_file(path: &Path) {
    let mut conn = Connection::open(path).unwrap();
    schema::migrate_to(&mut conn, BEFORE).unwrap();
    conn.execute_batch(
        "INSERT INTO projects VALUES ('p', '/r', 'now');
         INSERT INTO runs (run_id, project_id, slug, base_branch, run_branch, status, started_at)
            VALUES ('r1', 'p', 'alpha', 'main', 'fleet/a', 'active', '2026-11-01T00:00:00Z'),
                   ('r2', 'p', 'beta', 'main', 'fleet/b', 'active', '2026-11-02T00:00:00Z'),
                   ('r3', 'p', 'gamma', 'dev', NULL, 'active', '2026-11-03T00:00:00Z'),
                   ('r4', 'p', 'delta', NULL, NULL, 'active', '2026-11-04T00:00:00Z'),
                   ('r5', 'p', 'alpha', 'main', 'fleet/e', 'active', '2026-11-05T00:00:00Z');
         INSERT INTO features (id, name, branch, primary_branch, state, created_at, updated_at)
            VALUES ('F-0001', 'alpha', 'fleet/a', 'main', 'open', 'then', 'then'),
                   ('F-0002', 'beta', 'fleet/b', 'main', 'open', 'then', 'then');
         INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by, status,
                created_at, updated_at)
            VALUES ('t1', 'r1', 1, 'a', 'a.md', 'opus', 'opus', 'running', 'now', 'now'),
                   ('t2', 'r1', 2, 'b', 'b.md', 'opus', 'opus', 'done', 'now', 'now'),
                   ('t3', 'r2', 1, 'c', 'c.md', 'opus', 'opus', 'done', 'now', 'now'),
                   ('t4', 'r2', 2, 'd', 'd.md', 'opus', 'opus', 'cancelled', 'now', 'now'),
                   ('t5', 'r3', 1, 'e', 'e.md', 'opus', 'opus', 'ready', 'now', 'now'),
                   ('t6', 'r4', 1, 'f', 'f.md', 'opus', 'opus', 'done', 'now', 'now'),
                   ('t7', 'r5', 1, 'g', 'g.md', 'opus', 'opus', 'running', 'now', 'now');
         INSERT INTO questions (id, task_id, feature_id, owner, state, due_at, asked_at)
            VALUES ('t1/A1/Q1', 't1', 'r1', 'orchestrator', 'open', 100, 'now');",
    )
    .unwrap();
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

/// A [`phase_5_file`] ledger, migrated to the Phase 6 step with foreign
/// keys on, as `Store::open` sets them: its folder (kept alive) and a
/// connection.
fn migrated() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_5_file(&path);
    let mut conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_6_VERSION).unwrap();
    (dir, conn)
}

/// The text columns of each row of `sql`, in order.
fn rows(conn: &Connection, sql: &str) -> Vec<Vec<Option<String>>> {
    let mut stmt = conn.prepare(sql).unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

fn idea(conn: &Connection, id: &str, state: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
            VALUES (?1, 'F-0001', ?2, 'now', 'now')",
        [id, state],
    )
}

#[test]
fn ideas_table_has_the_10_states_of_the_design() {
    let (_dir, conn) = migrated();
    let states = [
        "captured",
        "clarifying",
        "awaiting_approval",
        "specifying",
        "ticketing",
        "in_progress",
        "integrating",
        "merged",
        "parked",
        "cancelled",
    ];
    for (i, state) in states.into_iter().enumerate() {
        idea(&conn, &format!("I-9{i:03}"), state).unwrap_or_else(|e| panic!("{state}: {e}"));
    }
    let err = idea(&conn, "I-9999", "done").unwrap_err().to_string();
    assert!(err.contains("CHECK constraint failed"), "{err}");
    let err = conn
        .execute(
            "INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
                VALUES ('I-9998', 'F-0404', 'captured', 'now', 'now')",
            [],
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("FOREIGN KEY constraint failed"), "{err}");
    let (priority, attempts): (i64, i64) = conn
        .query_row(
            "SELECT priority, phase_attempts FROM ideas WHERE id = 'I-9000'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((priority, attempts), (0, 0));
}

#[test]
fn task_without_an_idea_is_refused_by_the_trigger() {
    let (_dir, conn) = migrated();
    let task = |id: &str, idea: Option<&str>, origin: &str| {
        conn.execute(
            "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by,
                    status, origin, created_at, updated_at, idea_id)
                VALUES (?1, 'r1', 10 + length(?1), 'x', 'x.md', 'opus', 'opus', 'ready', ?2,
                    'now', 'now', ?3)",
            rusqlite::params![id, origin, idea],
        )
    };
    let err = task("n1", None, "native").unwrap_err().to_string();
    assert!(err.contains("a task needs an idea (FR-12)"), "{err}");
    task("n2", Some("I-0001"), "native").unwrap();
    task("imp", None, "import").expect("an imported row keeps its history");

    let (fix_rounds, idea_id): (i64, String) = conn
        .query_row(
            "SELECT fix_rounds, idea_id FROM tasks WHERE task_id = 'n2'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((fix_rounds, idea_id.as_str()), (0, "I-0001"));

    conn.execute(
        "INSERT INTO workers (worker_id, run_id, kind, persona, host, status)
            VALUES ('lead', 'r1', 'worker', 'idea-lead', 'h', 'working')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO attempts (id, task_id, idea_id, worker_id, state)
            VALUES ('I-0001/clarifying/1', NULL, 'I-0001', 'lead', 'launching')",
        [],
    )
    .expect("a sub-orchestrator attempt has an idea and no task");
}

#[test]
fn backfill_gives_each_run_1_idea_in_its_feature() {
    let (_dir, conn) = migrated();
    assert_eq!(
        rows(
            &conn,
            "SELECT id, feature_id, state, approved_sha256, created_at FROM ideas ORDER BY id"
        ),
        vec![
            vec![
                s("I-0001"),
                s("F-0001"),
                s("in_progress"),
                s("release"),
                s("2026-11-01T00:00:00Z")
            ],
            vec![
                s("I-0002"),
                s("F-0002"),
                s("integrating"),
                s("release"),
                s("2026-11-02T00:00:00Z")
            ],
            vec![
                s("I-0003"),
                s("F-0003"),
                s("in_progress"),
                s("release"),
                s("2026-11-03T00:00:00Z")
            ],
            vec![
                s("I-0005"),
                s("F-0005"),
                s("in_progress"),
                s("release"),
                s("2026-11-05T00:00:00Z")
            ],
        ],
        "run 4 has no feature and no open task, so it gets no idea"
    );
    assert_eq!(
        rows(
            &conn,
            "SELECT id, name, branch, primary_branch, state FROM features ORDER BY id"
        ),
        vec![
            vec![s("F-0001"), s("alpha"), s("fleet/a"), s("main"), s("open")],
            vec![s("F-0002"), s("beta"), s("fleet/b"), s("main"), s("open")],
            vec![s("F-0003"), s("gamma"), None, s("dev"), s("open")],
            vec![
                s("F-0005"),
                s("alpha-5"),
                s("fleet/e"),
                s("main"),
                s("open")
            ],
        ]
    );
    assert_eq!(
        rows(
            &conn,
            "SELECT name, feature_id, task_id, owner FROM branches"
        ),
        vec![vec![s("fleet/e"), s("F-0005"), None, s("fleetd")]],
        "the new feature gets the branches row of its branch, as in Phase 4"
    );
    assert_eq!(
        rows(&conn, "SELECT task_id, idea_id FROM tasks ORDER BY task_id"),
        vec![
            vec![s("t1"), s("I-0001")],
            vec![s("t2"), s("I-0001")],
            vec![s("t3"), s("I-0002")],
            vec![s("t4"), s("I-0002")],
            vec![s("t5"), s("I-0003")],
            vec![s("t6"), None],
            vec![s("t7"), s("I-0005")],
        ]
    );
    let broken: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(broken, 0);
}

#[test]
fn backfill_idea_is_integrating_when_each_task_is_done_or_cancelled() {
    let (_dir, conn) = migrated();
    let state = |id: &str| -> String {
        conn.query_row("SELECT state FROM ideas WHERE id = ?1", [id], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(state("I-0001"), "in_progress", "t1 is running");
    assert_eq!(
        state("I-0002"),
        "integrating",
        "t3 is done, t4 is cancelled"
    );
}

#[test]
fn backfill_moves_each_question_to_the_feature_and_its_sub_orchestrator() {
    let (_dir, conn) = migrated();
    assert_eq!(
        rows(&conn, "SELECT id, feature_id, owner, state FROM questions"),
        vec![vec![
            s("t1/A1/Q1"),
            s("F-0001"),
            s("idea-lead:F-0001"),
            s("open")
        ]]
    );
}

#[test]
fn trial_merges_key_is_the_2_tips() {
    let (_dir, conn) = migrated();
    conn.execute(
        "INSERT INTO jobs (id, kind, subject, input_key, state, time_limit_s, created_at)
            VALUES (1, 'trial_merge', 'F-0001+F-0002', 'aaa+bbb', 'queued', 60, 'now')",
        [],
    )
    .unwrap();
    let merge = |left_ref: &str, left_tip: &str, right_tip: &str| {
        conn.execute(
            "INSERT INTO trial_merges (left_ref, right_ref, left_tip, right_tip, job_id, clean,
                    conflict_files, at)
                VALUES (?1, 'fleet/b', ?2, ?3, 1, NULL, NULL, 'now')",
            [left_ref, left_tip, right_tip],
        )
    };
    merge("fleet/a", "aaa", "bbb").unwrap();
    let err = merge("fleet/c", "aaa", "bbb").unwrap_err().to_string();
    assert!(err.contains("UNIQUE constraint failed"), "{err}");
    merge("fleet/a", "aaa", "ccc").expect("another right tip is another key");
    merge("fleet/a", "bbb", "aaa").expect("the order of the 2 tips is part of the key");
    let key: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('trial_merges') WHERE pk > 0 ORDER BY pk")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(key, ["left_tip", "right_tip"]);
    let err = conn
        .execute(
            "INSERT INTO trial_merges (left_tip, right_tip, job_id, at) VALUES ('x', 'y', NULL, 'now')",
            [],
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("NOT NULL constraint failed"), "{err}");
}

#[test]
fn requirements_and_task_covers_have_the_columns_of_the_design() {
    let (_dir, conn) = migrated();
    conn.execute(
        "INSERT INTO requirements (idea_id, req_id, text) VALUES ('I-0001', 'FR-1', 'x')",
        [],
    )
    .unwrap();
    let err = conn
        .execute(
            "INSERT INTO requirements (idea_id, req_id, text) VALUES ('I-0001', 'FR-1', 'y')",
            [],
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("UNIQUE constraint failed"), "{err}");
    conn.execute(
        "INSERT INTO task_covers (task_id, idea_id, req_id) VALUES ('t1', 'I-0001', 'FR-1')",
        [],
    )
    .unwrap();
}

#[test]
fn migration_to_the_next_version_writes_a_backup_first() {
    let (dir, conn) = migrated();
    assert_eq!(user_version(&conn), schema::PHASE_6_VERSION);
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
    assert!(copy.prepare("SELECT * FROM ideas").is_err());
    assert_eq!(
        rows(
            &copy,
            "SELECT id, feature_id, owner FROM questions ORDER BY id"
        ),
        vec![vec![s("t1/A1/Q1"), s("r1"), s("orchestrator")]]
    );
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

/// Until ticket 47 retires `horch ledger add`, `tasks::add` gives each new
/// task the idea of its run, and writes the feature and the idea of a run
/// that has none.
#[test]
fn ledger_add_in_a_new_run_gives_the_task_the_idea_of_its_run() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let run = store
        .immediate(|tx| {
            runs::create(
                tx,
                Path::new("/repo"),
                &NewRun {
                    slug: "omega".into(),
                    base_branch: Some("main".into()),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let add = |store: &mut Store, title: &str| {
        let t = NewTask::work(title, &format!("tickets/{title}.md"), "opus");
        store
            .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
            .unwrap()
            .task_id
    };
    let first = add(&mut store, "a");
    let second = add(&mut store, "b");
    let conn = store.conn();
    assert_eq!(
        rows(
            conn,
            "SELECT id, feature_id, state, approved_sha256 FROM ideas"
        ),
        vec![vec![
            s("I-0001"),
            s("F-0001"),
            s("in_progress"),
            s("release")
        ]]
    );
    assert_eq!(
        rows(
            conn,
            "SELECT id, name, branch, primary_branch, state FROM features"
        ),
        vec![vec![s("F-0001"), s("omega"), None, s("main"), s("open")]]
    );
    for task in [first, second] {
        let idea: Option<String> = conn
            .query_row(
                "SELECT idea_id FROM tasks WHERE task_id = ?1",
                [&task],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idea.as_deref(), Some("I-0001"));
    }
}
