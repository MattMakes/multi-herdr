//! The schema step of Phase 4 (docs/specs/fleetd/design.md, "Ledger" and
//! "Migration plan" item 6; docs/tickets/fleetd-p4/02-schema-phase4.md): the
//! tables `features`, `branches`, `bundles`, `bundle_members`, `gate_runs`,
//! `remote_tips` and `ingested`, the column `attempts.via`, the I12 rule of
//! bundle members, and the release backfill of a run in flight (MAP.md, Q4
//! and Q5).

use std::collections::BTreeMap;
use std::path::Path;

use horch_core::taskledger::schema;
use horch_core::taskledger::store::{self, migrate_refusal};
use rusqlite::types::Value;
use rusqlite::Connection;

/// The version before the Phase 4 step.
const BEFORE: i64 = schema::PHASE_4_VERSION - 1;
/// The fleet clock of the `fleetd` row in [`phase_3_file`].
const CLOCK: i64 = 5000;
/// The task limit of a backfilled branch row: the `max_minutes` default of
/// the design, 60 min.
const TASK_LIMIT_S: i64 = 3600;

/// A ledger at [`BEFORE`] at `path`, built by the real steps. The runs, in
/// insert order (so `rowid` 1 to 6):
/// - `r1`: active, run branch `fleet/a`. Its tasks:
///   - `t1` done, its tree cleaned with a landed commit.
///   - `t2` running, its tree open.
///   - `t3` ready, its tree `landed`.
///   - `t4` running, 2 trees: `fleet/a/t4-old` landed, then `fleet/a/t4`
///     open.
///   - `t5` ready, no tree.
/// - `r2`: done, run branch `fleet/b`.
/// - `r3`: abandoned, run branch `fleet/c`.
/// - `r4`: active, no run branch. Its task `t6` has a tree.
/// - `r5`: done, slug `dup`, run branch `fleet/d`.
/// - `r6`: active, slug `dup`, run branch `fleet/d`. Its task `t7` has a tree.
///
/// `w1` has the live attempt `t2/A1` and the ended attempt `t1/A1`.
fn phase_3_file(path: &Path) {
    let mut conn = Connection::open(path).unwrap();
    schema::migrate_to(&mut conn, BEFORE).unwrap();
    conn.execute_batch(&format!(
        "INSERT INTO projects VALUES ('p', '/r', 'now');
         INSERT INTO fleetd VALUES (1, 1, 10, 20, 'then', {CLOCK}, 'then');
         INSERT INTO runs (run_id, project_id, slug, base_branch, run_branch, status, started_at)
            VALUES ('r1', 'p', 'alpha', 'main', 'fleet/a', 'active', '2026-11-01T00:00:00Z'),
                   ('r2', 'p', 'beta', 'main', 'fleet/b', 'done', '2026-11-02T00:00:00Z'),
                   ('r3', 'p', 'gamma', 'main', 'fleet/c', 'abandoned', '2026-11-03T00:00:00Z'),
                   ('r4', 'p', 'solo', 'main', NULL, 'active', '2026-11-04T00:00:00Z'),
                   ('r5', 'p', 'dup', 'master', 'fleet/d', 'done', '2026-11-05T00:00:00Z'),
                   ('r6', 'p', 'dup', 'master', 'fleet/d', 'active', '2026-11-06T00:00:00Z');
         INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by, status,
                created_at, updated_at)
            VALUES ('t1', 'r1', 1, 'a', 'a.md', 'opus', 'opus', 'done', 'now', 'now'),
                   ('t2', 'r1', 2, 'b', 'b.md', 'opus', 'opus', 'running', 'now', 'now'),
                   ('t3', 'r1', 3, 'c', 'c.md', 'opus', 'opus', 'ready', 'now', 'now'),
                   ('t4', 'r1', 4, 'd', 'd.md', 'opus', 'opus', 'running', 'now', 'now'),
                   ('t5', 'r1', 5, 'e', 'e.md', 'opus', 'opus', 'ready', 'now', 'now'),
                   ('t6', 'r4', 1, 'f', 'f.md', 'opus', 'opus', 'running', 'now', 'now'),
                   ('t7', 'r6', 1, 'g', 'g.md', 'opus', 'opus', 'running', 'now', 'now');
         INSERT INTO workers (worker_id, run_id, kind, persona, host, task_id, attempt, status)
            VALUES ('w1', 'r1', 'worker', 'opus', 'h', 't2', 1, 'working');
         INSERT INTO attempts (id, task_id, worker_id, state, started_at)
            VALUES ('t1/A1', 't1', 'w1', 'ended', '2026-11-01T01:00:00Z'),
                   ('t2/A1', 't2', 'w1', 'live', '2026-11-01T02:00:00Z');"
    ))
    .unwrap();
    for (id, task, branch, status, landed, at) in [
        (
            "wt1",
            "t1",
            "fleet/a/t1",
            "cleaned",
            Some("c1"),
            "2026-11-01T01:00:00Z",
        ),
        (
            "wt2",
            "t2",
            "fleet/a/t2",
            "open",
            None,
            "2026-11-01T02:00:00Z",
        ),
        (
            "wt3",
            "t3",
            "fleet/a/t3",
            "landed",
            Some("c3"),
            "2026-11-01T03:00:00Z",
        ),
        (
            "wt4a",
            "t4",
            "fleet/a/t4-old",
            "landed",
            Some("c4"),
            "2026-11-01T04:00:00Z",
        ),
        (
            "wt4b",
            "t4",
            "fleet/a/t4",
            "open",
            None,
            "2026-11-01T05:00:00Z",
        ),
        (
            "wt6",
            "t6",
            "fleet/solo/t6",
            "open",
            None,
            "2026-11-04T01:00:00Z",
        ),
        (
            "wt7",
            "t7",
            "fleet/d/t7",
            "open",
            None,
            "2026-11-06T01:00:00Z",
        ),
    ] {
        conn.execute(
            "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch, base,
                    base_commit, status, landed_commit, created_at, state)
                VALUES (?1, ?2, '/r', '/r/.worktrees/' || ?1, ?3, 'main', 'c0', ?4, ?5, ?6, 'orphaned')",
            rusqlite::params![id, task, branch, status, landed, at],
        )
        .unwrap();
    }
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

/// The migration of the file at `path` up to the Phase 4 step, with foreign
/// keys on as `Store::open` sets them. A later step does not change what
/// these tests see.
fn migrate(path: &Path) -> Connection {
    let mut conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_4_VERSION).unwrap();
    conn
}

/// A [`phase_3_file`] ledger, migrated: its folder (kept alive) and a
/// connection with foreign keys on.
fn migrated() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_3_file(&path);
    let conn = migrate(&path);
    (dir, conn)
}

/// Each feature row: `id -> (name, branch, primary_branch, state, pr_state)`.
type Features = BTreeMap<String, (String, Option<String>, String, String, String)>;

fn features(conn: &Connection) -> Features {
    conn.prepare("SELECT id, name, branch, primary_branch, state, pr_state FROM features")
        .unwrap()
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?),
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// Each branch row: `name -> (feature_id, task_id, landing_state, owner,
/// due_at, seen_tip)`.
type Branches = BTreeMap<
    String,
    (
        String,
        Option<String>,
        Option<String>,
        String,
        i64,
        Option<String>,
    ),
>;

fn branches(conn: &Connection) -> Branches {
    conn.prepare(
        "SELECT name, feature_id, task_id, landing_state, owner, due_at, seen_tip FROM branches",
    )
    .unwrap()
    .query_map([], |r| {
        Ok((
            r.get(0)?,
            (
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ),
        ))
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
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

/// Each row of each table, as text, by table. With `only`, the columns of
/// each table in `only`.
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
fn phase3_ledger_migrates_with_1_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_3_file(&path);
    let before = dump(&Connection::open(&path).unwrap(), None);

    let conn = migrate(&path);
    assert_eq!(user_version(&conn), schema::PHASE_4_VERSION);
    for table in [
        "features",
        "branches",
        "bundles",
        "bundle_members",
        "gate_runs",
        "remote_tips",
        "ingested",
    ] {
        assert!(
            conn.prepare(&format!("SELECT * FROM {table}")).is_ok(),
            "{table} is missing"
        );
    }
    // Each row of each old table keeps each old column.
    assert_eq!(dump(&conn, Some(&before)), before);

    // The backup holds the ledger as it was before the step.
    let backups = schema::backups_in(dir.path());
    assert_eq!(backups.len(), 1, "{backups:?}");
    let name = backups[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert!(
        name.starts_with(&format!("ledger.v{BEFORE}-backup-")),
        "{name}"
    );
    let copy = Connection::open(&backups[0]).unwrap();
    assert_eq!(user_version(&copy), BEFORE);
    assert!(copy.prepare("SELECT * FROM features").is_err());
    assert_eq!(dump(&copy, None), before);
}

#[test]
fn each_run_with_a_run_branch_gets_1_open_feature_row() {
    let (_dir, conn) = migrated();
    let f = |name: &str, branch: Option<&str>, primary: &str, state: &str| {
        (
            name.to_string(),
            branch.map(String::from),
            primary.to_string(),
            state.to_string(),
            "none".to_string(),
        )
    };
    let want: Features = [
        ("F-0001", f("alpha", Some("fleet/a"), "main", "open")),
        ("F-0002", f("beta", Some("fleet/b"), "main", "merged")),
        ("F-0003", f("gamma", Some("fleet/c"), "main", "abandoned")),
        ("F-0005", f("dup-5", None, "master", "merged")),
        ("F-0006", f("dup", Some("fleet/d"), "master", "open")),
    ]
    .into_iter()
    .map(|(id, row)| (id.to_string(), row))
    .collect();
    // `r4` has no run branch, so it has no feature row (Q5).
    assert_eq!(features(&conn), want);
    let created: String = conn
        .query_row(
            "SELECT created_at FROM features WHERE id = 'F-0001'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(created, "2026-11-01T00:00:00Z");
}

#[test]
fn runs_that_share_a_branch_and_a_slug_get_1_row_each_and_the_newest_keeps_the_branch() {
    let (_dir, conn) = migrated();
    let all = features(&conn);
    // The older run `r5` keeps its feature row with no branch and a name of
    // its own; the newer run `r6` has the branch, its row and its tasks.
    assert_eq!(all["F-0005"].0, "dup-5");
    assert_eq!(all["F-0005"].1, None);
    assert_eq!(all["F-0006"].0, "dup");
    assert_eq!(all["F-0006"].1.as_deref(), Some("fleet/d"));
    let rows = branches(&conn);
    assert_eq!(rows["fleet/d"].0, "F-0006");
    assert_eq!(rows["fleet/d/t7"].0, "F-0006");
    assert_eq!(
        rows.values().filter(|r| r.0 == "F-0005").count(),
        0,
        "{rows:?}"
    );
}

#[test]
fn landed_task_gets_a_landed_branch_row_and_open_task_a_building_row() {
    let (_dir, conn) = migrated();
    let due = CLOCK + TASK_LIMIT_S;
    let b = |feature: &str, task: Option<&str>, state: Option<&str>, owner: &str| {
        (
            feature.to_string(),
            task.map(String::from),
            state.map(String::from),
            owner.to_string(),
            due,
            None,
        )
    };
    let want: Branches = [
        // The feature branches, with no task and no landing state.
        ("fleet/a", b("F-0001", None, None, "fleetd")),
        ("fleet/b", b("F-0002", None, None, "fleetd")),
        ("fleet/c", b("F-0003", None, None, "fleetd")),
        ("fleet/d", b("F-0006", None, None, "fleetd")),
        // Done with a landed commit, and a tree in `landed`: landed.
        (
            "fleet/a/t1",
            b("F-0001", Some("t1"), Some("landed"), "worker"),
        ),
        (
            "fleet/a/t3",
            b("F-0001", Some("t3"), Some("landed"), "worker"),
        ),
        // An open tree: building. `t4` gets the branch of its newest tree.
        (
            "fleet/a/t2",
            b("F-0001", Some("t2"), Some("building"), "worker"),
        ),
        (
            "fleet/a/t4",
            b("F-0001", Some("t4"), Some("building"), "worker"),
        ),
        (
            "fleet/d/t7",
            b("F-0006", Some("t7"), Some("building"), "worker"),
        ),
    ]
    .into_iter()
    .map(|(name, row)| (name.to_string(), row))
    .collect();
    // `t5` has no tree, and `t6` is in a run with no run branch.
    assert_eq!(branches(&conn), want);
}

#[test]
fn backfill_with_no_fleetd_row_counts_from_fleet_clock_0() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    phase_3_file(&path);
    Connection::open(&path)
        .unwrap()
        .execute("DELETE FROM fleetd", [])
        .unwrap();
    let conn = migrate(&path);
    let due: Vec<i64> = conn
        .prepare("SELECT DISTINCT due_at FROM branches")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(due, vec![TASK_LIMIT_S]);
}

#[test]
fn attempt_backfill_is_via_claim() {
    let (_dir, conn) = migrated();
    let via: Vec<(String, String)> = conn
        .prepare("SELECT id, via FROM attempts ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        via,
        vec![
            ("t1/A1".to_string(), "claim".to_string()),
            ("t2/A1".to_string(), "claim".to_string())
        ]
    );
    // A new attempt names how it began, and only `claim` or `push`.
    let insert = |id: &str, via: &str| {
        conn.execute(
            "INSERT INTO attempts (id, task_id, worker_id, state, via) VALUES (?1, 't5', 'w1', 'launching', ?2)",
            [id, via],
        )
    };
    insert("t5/A1", "push").unwrap();
    assert!(insert("t5/A2", "spawn").is_err());
}

/// A bundle `id` of feature `F-0001` in `state`.
fn bundle(conn: &Connection, id: i64, state: &str) {
    conn.execute(
        "INSERT INTO bundles (id, feature_id, base_tip, state, due_at, at) VALUES (?1, 'F-0001', 'c0', ?2, 0, 'now')",
        rusqlite::params![id, state],
    )
    .unwrap();
}

fn member(conn: &Connection, bundle: i64, branch: &str) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO bundle_members (bundle_id, branch, position) VALUES (?1, ?2, 1)",
        rusqlite::params![bundle, branch],
    )
}

#[test]
fn bundle_member_of_2_open_bundles_is_refused() {
    let (_dir, conn) = migrated();
    for (id, state) in [
        (1, "building"),
        (2, "gating"),
        (3, "passed"),
        (4, "failed"),
        (5, "dropped"),
        (6, "landed"),
    ] {
        bundle(&conn, id, state);
    }
    member(&conn, 1, "fleet/a/t2").unwrap();
    // A second open bundle is refused: gating or passed.
    for open in [2, 3] {
        let e = member(&conn, open, "fleet/a/t2").unwrap_err().to_string();
        assert!(e.contains("I12"), "{e}");
    }
    // An ended bundle takes the member: I12 counts only open bundles.
    for ended in [4, 5, 6] {
        member(&conn, ended, "fleet/a/t2").unwrap();
    }
    // A membership moves to another open bundle when its branch is in no
    // other open bundle.
    member(&conn, 2, "fleet/a/t4").unwrap();
    conn.execute(
        "UPDATE bundle_members SET bundle_id = 1 WHERE bundle_id = 2 AND branch = 'fleet/a/t4'",
        [],
    )
    .unwrap();
    member(&conn, 3, "fleet/a/t1").unwrap();
    let e = conn
        .execute(
            "UPDATE bundle_members SET branch = 'fleet/a/t2' WHERE bundle_id = 3",
            [],
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("I12"), "{e}");
    // An ended bundle that opens again while a member is in an open bundle
    // is refused.
    let e = conn
        .execute("UPDATE bundles SET state = 'building' WHERE id = 4", [])
        .unwrap_err()
        .to_string();
    assert!(e.contains("I12"), "{e}");
    // When the open bundle ends, the next one takes the branch.
    conn.execute("UPDATE bundles SET state = 'dropped' WHERE id = 1", [])
        .unwrap();
    member(&conn, 2, "fleet/a/t2").unwrap();
}

#[test]
fn needs_you_open_keys_uses_the_kind_state_index() {
    let (_dir, conn) = migrated();
    let plan: Vec<String> = conn
        .prepare(
            "EXPLAIN QUERY PLAN SELECT key FROM needs_you WHERE state = 'open' AND kind = 'k' ORDER BY key",
        )
        .unwrap()
        .query_map([], |r| r.get(3))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let plan = plan.join("\n");
    assert!(
        plan.contains("USING INDEX needs_you_by_kind_state (kind=? AND state=?)"),
        "{plan}"
    );
}

#[test]
fn new_tables_take_the_design_words_and_refuse_others() {
    let (_dir, conn) = migrated();
    let ok = |sql: &str| {
        conn.execute(sql, [])
            .unwrap_or_else(|e| panic!("{sql}: {e}"))
    };
    let refused = |sql: &str| assert!(conn.execute(sql, []).is_err(), "{sql}");
    for state in ["ready", "merged"] {
        ok(&format!(
            "UPDATE features SET state = '{state}' WHERE id = 'F-0001'"
        ));
    }
    refused("UPDATE features SET state = 'closed' WHERE id = 'F-0001'");
    ok("UPDATE features SET push_state = 'failed', push_failures = 1, push_not_before = 10, pr_state = 'closed' WHERE id = 'F-0001'");
    refused("UPDATE features SET pr_state = 'merged' WHERE id = 'F-0001'");
    for state in [
        "verifying",
        "ready",
        "in_bundle",
        "conflicted",
        "gate_failed",
        "abandoned",
    ] {
        ok(&format!(
            "UPDATE branches SET landing_state = '{state}' WHERE name = 'fleet/a/t2'"
        ));
    }
    refused("UPDATE branches SET landing_state = 'merged' WHERE name = 'fleet/a/t2'");
    ok("UPDATE branches SET queue_seq = 1, repair_attempts = 1, rescue_ref = 'r', seen_tip = 'a', seen_remote_tip = 'b', orphan = 1 WHERE name = 'fleet/a/t2'");
    ok("INSERT INTO jobs (id, kind, subject, input_key, state, time_limit_s, created_at) VALUES (1, 'fast_gate', 'F-0001', 'k', 'running', 60, 'now')");
    ok("INSERT INTO bundles (id, feature_id, base_tip, candidate_ref, candidate_tip, state, job_id, due_at, at) VALUES (1, 'F-0001', 'c0', 'refs/horch/bundle/1', 'c9', 'gating', 1, 0, 'now')");
    ok("INSERT INTO bundles (id, feature_id, base_tip, parent_id, state, due_at, at) VALUES (2, 'F-0001', 'c0', 1, 'building', 0, 'now')");
    refused("INSERT INTO bundles (id, feature_id, base_tip, state, due_at, at) VALUES (3, 'F-0001', 'c0', 'open', 0, 'now')");
    ok("INSERT INTO gate_runs (feature_id, gate, tip, base_tip, job_id, at) VALUES ('F-0001', 'fast', 'c9', 'c0', 1, 'now')");
    refused("INSERT INTO gate_runs (feature_id, gate, tip, base_tip, job_id, at) VALUES ('F-0001', 'slow', 'c9', 'c0', 1, 'now')");
    refused("UPDATE gate_runs SET result = 'skip'");
    ok("UPDATE gate_runs SET result = 'pass'");
    ok("INSERT INTO remote_tips (repo, branch, tip, fetched_at) VALUES ('/r', 'main', 'c0', 'now')");
    refused("INSERT INTO remote_tips (repo, branch, tip, fetched_at) VALUES ('/r', 'main', 'c1', 'now')");
    ok("INSERT INTO ingested (path, sha256, outcome, at) VALUES ('/a/result.json', 'h', 'accepted', 'now')");
    ok("INSERT INTO ingested (path, log_offset, outcome, reason, at) VALUES ('/a/progress.jsonl', 10, 'rejected', 'link', 'now')");
    refused("INSERT INTO ingested (path, outcome, at) VALUES ('/a/b', 'read', 'now')");
}

#[test]
fn newer_schema_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
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
    assert!(schema::backups_in(dir.path()).is_empty());
}

#[test]
fn development_build_does_not_migrate_the_shared_ledger() {
    let home = tempfile::tempdir().unwrap();
    let path = store::default_path(home.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    phase_3_file(&path);
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
    assert!(conn.prepare("SELECT * FROM features").is_err());
    assert!(schema::backups_in(path.parent().unwrap()).is_empty());
}
