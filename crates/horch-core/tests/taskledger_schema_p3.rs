use std::collections::BTreeMap;
use std::path::Path;

use horch_core::taskledger::model::WorktreeStatus;
use horch_core::taskledger::schema;
use rusqlite::Connection;

const STATES: [&str; 8] = [
    "reserved", "created", "active", "landing", "landed", "removed", "orphaned", "rescued",
];

fn phase_2_file(path: &Path) {
    let mut conn = Connection::open(path).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_2_VERSION).unwrap();
    conn.execute_batch(
        "INSERT INTO projects VALUES ('p', '/r', 'now');
         INSERT INTO runs (run_id, project_id, slug, status, started_at) VALUES ('r1', 'p', 's', 'active', 'now');
         INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by, status,
                attempts, current_worker_id, created_at, updated_at)
            VALUES ('t1', 'r1', 1, 'a', 'a.md', 'opus', 'opus', 'running', 2, 'w1', 'now', 'now'),
                   ('t2', 'r1', 2, 'b', 'b.md', 'opus', 'opus', 'running', 1, 'w2', 'now', 'now'),
                   ('t3', 'r1', 3, 'c', 'c.md', 'opus', 'opus', 'running', 1, 'w3', 'now', 'now'),
                   ('t4', 'r1', 4, 'd', 'd.md', 'opus', 'opus', 'running', 1, 'w4', 'now', 'now');
         INSERT INTO workers (worker_id, run_id, kind, persona, host, task_id, attempt, status)
            VALUES ('w0', 'r1', 'worker', 'opus', 'h', 't1', 1, 'stopped'),
                   ('w1', 'r1', 'worker', 'opus', 'h', 't1', 2, 'working'),
                   ('w2', 'r1', 'worker', 'opus', 'h', 't2', 1, 'stopped'),
                   ('w3', 'r1', 'worker', 'opus', 'h', 't3', 1, 'working'),
                   ('w4', 'r1', 'worker', 'opus', 'h', 't4', 1, 'starting');
         INSERT INTO attempts (id, task_id, worker_id, state, started_at)
            VALUES ('t1/A1', 't1', 'w0', 'ended', '2026-11-17T09:00:00Z'),
                   ('t1/A2', 't1', 'w1', 'live', '2026-11-17T10:00:00Z'),
                   ('t2/A1', 't2', 'w2', 'ended', '2026-11-17T09:00:00Z'),
                   ('t3/A1', 't3', 'w3', 'live', '2026-11-17T10:00:00Z'),
                   ('t4/A1', 't4', 'w4', 'launching', NULL);",
    )
    .unwrap();
}

fn tree(
    conn: &Connection,
    id: &str,
    (task, worker): (Option<&str>, Option<&str>),
    status: &str,
    scratch: Option<bool>,
) {
    conn.execute(
        "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path, branch, base,
                base_commit, status, created_at)
            VALUES (?1, ?2, ?3, '/r', '/r/.worktrees/' || ?1, 'b-' || ?1, 'main', 'c0', ?4, 'then')",
        rusqlite::params![id, task, worker, status],
    )
    .unwrap();
    if let Some(scratch) = scratch {
        conn.execute(
            "INSERT INTO worktree_modes (worktree_id, run_id, name, scratch) VALUES (?1, 'r1', ?1, ?2)",
            rusqlite::params![id, scratch],
        )
        .unwrap();
    }
}

fn migrate(path: &Path) -> Connection {
    let mut conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_3_VERSION).unwrap();
    conn
}

fn migrated(add: impl FnOnce(&Connection)) -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_2_file(&path);
    add(&Connection::open(&path).unwrap());
    let conn = migrate(&path);
    (dir, conn)
}

fn states(conn: &Connection) -> BTreeMap<String, (String, Option<String>)> {
    conn.prepare("SELECT worktree_id, state, owner_attempt FROM worktrees")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn state(conn: &Connection, id: &str) -> (String, Option<String>) {
    states(conn).remove(id).unwrap()
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn worktree_columns(conn: &Connection) -> BTreeMap<String, (String, bool, Option<String>)> {
    conn.prepare("SELECT name, type, \"notnull\", dflt_value FROM pragma_table_info('worktrees')")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn worktrees_gains_state_owner_head_dirty_rescue_and_seen_columns() {
    let (_dir, conn) = migrated(|_| {});
    assert_eq!(user_version(&conn), schema::PHASE_3_VERSION);
    let cols = worktree_columns(&conn);
    let col = |name: &str| {
        cols.get(name)
            .cloned()
            .unwrap_or_else(|| panic!("no column {name}"))
    };
    assert_eq!(col("state").0, "TEXT");
    assert!(col("state").1, "state is NOT NULL");
    assert_eq!(col("owner_attempt"), ("TEXT".into(), false, None));
    assert_eq!(col("head_sha"), ("TEXT".into(), false, None));
    assert_eq!(col("dirty"), ("INTEGER".into(), true, Some("0".into())));
    assert_eq!(col("rescue_ref"), ("TEXT".into(), false, None));
    assert_eq!(col("seen_at"), ("TEXT".into(), false, None));
    for kept in ["base_commit", "bytes_on_disk", "status", "task_id"] {
        assert!(cols.contains_key(kept), "{kept} is kept");
    }
    for absent in ["base_sha", "disk_bytes", "idea_id"] {
        assert!(!cols.contains_key(absent), "no column {absent}");
    }
    assert!(!col("task_id").1, "task_id stays nullable");

    let refs: Vec<(String, String, String)> = conn
        .prepare("SELECT \"from\", \"table\", \"to\" FROM pragma_foreign_key_list('worktrees')")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(refs.contains(&("owner_attempt".into(), "attempts".into(), "id".into())));

    tree(&conn, "wt", (Some("t1"), Some("w1")), "open", None);
    for word in STATES {
        conn.execute(
            "UPDATE worktrees SET state = ?1 WHERE worktree_id = 'wt'",
            [word],
        )
        .unwrap_or_else(|e| panic!("{word} is refused: {e}"));
    }
    let err = conn
        .execute(
            "UPDATE worktrees SET state = 'lost' WHERE worktree_id = 'wt'",
            [],
        )
        .unwrap_err();
    assert!(err.to_string().contains("CHECK"), "{err}");
    let err = conn
        .execute(
            "INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, status, state, created_at)
                VALUES ('wx', '/r', '/r/x', 'x', 'main', 'open', 'gone', 'now')",
            [],
        )
        .unwrap_err();
    assert!(err.to_string().contains("CHECK"), "{err}");
    let err = conn
        .execute(
            "UPDATE worktrees SET owner_attempt = 'no/A1' WHERE worktree_id = 'wt'",
            [],
        )
        .unwrap_err();
    assert!(err.to_string().contains("FOREIGN KEY"), "{err}");
}

#[test]
fn backfill_makes_a_tree_with_a_live_attempt_active_with_its_owner() {
    let (_dir, conn) = migrated(|c| {
        tree(c, "open", (Some("t1"), Some("w1")), "open", Some(false));
        tree(c, "landing", (Some("t3"), Some("w3")), "landing", None);
    });
    assert_eq!(
        state(&conn, "open"),
        ("active".into(), Some("t1/A2".into()))
    );
    assert_eq!(
        state(&conn, "landing"),
        ("active".into(), Some("t3/A1".into()))
    );
}

#[test]
fn backfill_makes_each_other_tree_orphaned() {
    let (_dir, conn) = migrated(|c| {
        for status in WorktreeStatus::ALL {
            if *status != WorktreeStatus::Cleaned {
                tree(
                    c,
                    &format!("s-{status}"),
                    (Some("t2"), Some("w2")),
                    status.as_str(),
                    None,
                );
            }
        }
        tree(c, "handed", (Some("t1"), Some("w0")), "open", None);
        tree(c, "launching", (Some("t4"), Some("w4")), "open", None);
        tree(c, "no-worker", (Some("t1"), None), "open", None);
        tree(c, "no-task", (None, Some("w1")), "abandoned", None);
        tree(
            c,
            "other-task",
            (Some("t1"), Some("w3")),
            "open",
            Some(false),
        );
    });
    let all = states(&conn);
    assert_eq!(all.len(), WorktreeStatus::ALL.len() - 1 + 5);
    for (id, (state, owner)) in &all {
        assert_eq!((state.as_str(), owner), ("orphaned", &None), "{id}");
    }
}

#[test]
fn backfill_makes_a_cleaned_tree_removed() {
    let (_dir, conn) = migrated(|c| {
        tree(c, "gone", (Some("t2"), Some("w2")), "cleaned", None);
        tree(c, "gone-live", (Some("t1"), Some("w1")), "cleaned", None);
        tree(c, "gone-scratch", (None, Some("w3")), "cleaned", Some(true));
    });
    for id in ["gone", "gone-live", "gone-scratch"] {
        assert_eq!(state(&conn, id), ("removed".into(), None), "{id}");
    }
}

#[test]
fn backfill_makes_a_scratch_tree_of_a_live_worker_active() {
    let (_dir, conn) = migrated(|c| {
        tree(c, "scratch-live", (None, Some("w3")), "open", Some(true));
        tree(c, "scratch-dead", (None, Some("w2")), "open", Some(true));
        tree(
            c,
            "scratch-launching",
            (None, Some("w4")),
            "open",
            Some(true),
        );
    });
    assert_eq!(
        state(&conn, "scratch-live"),
        ("active".into(), Some("t3/A1".into()))
    );
    assert_eq!(state(&conn, "scratch-dead"), ("orphaned".into(), None));
    assert_eq!(state(&conn, "scratch-launching"), ("orphaned".into(), None));
}

#[test]
fn migration_to_the_phase_3_version_writes_a_backup_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_2_file(&path);
    tree(
        &Connection::open(&path).unwrap(),
        "wt",
        (Some("t1"), Some("w1")),
        "open",
        None,
    );
    let conn = migrate(&path);
    assert_eq!(user_version(&conn), schema::PHASE_3_VERSION);

    let backups = schema::backups_in(dir.path());
    assert_eq!(backups.len(), 1);
    let name = backups[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with(&format!("ledger.v{}-backup-", schema::PHASE_2_VERSION)),
        "{name}"
    );
    let copy = Connection::open(&backups[0]).unwrap();
    assert_eq!(user_version(&copy), schema::PHASE_2_VERSION);
    assert!(!worktree_columns(&copy).contains_key("state"));
    let rows: i64 = copy
        .query_row("SELECT count(*) FROM worktrees", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1);
}

fn kept_fields(conn: &Connection) -> BTreeMap<String, (String, String, String)> {
    conn.prepare("SELECT worktree_id, status, path, branch FROM worktrees")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn migration_from_phase_2_keeps_every_worktree_row_and_its_status() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_2_file(&path);
    let before = {
        let c = Connection::open(&path).unwrap();
        for status in WorktreeStatus::ALL {
            tree(
                &c,
                &format!("s-{status}"),
                (Some("t2"), Some("w2")),
                status.as_str(),
                None,
            );
        }
        tree(&c, "live", (Some("t1"), Some("w1")), "open", Some(false));
        tree(&c, "scratch", (None, Some("w3")), "open", Some(true));
        kept_fields(&c)
    };
    assert_eq!(before.len(), WorktreeStatus::ALL.len() + 2);
    let conn = migrate(&path);
    assert_eq!(kept_fields(&conn), before);
    let modes: i64 = conn
        .query_row("SELECT count(*) FROM worktree_modes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(modes, 2);
}
