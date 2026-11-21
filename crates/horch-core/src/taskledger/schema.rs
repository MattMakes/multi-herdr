use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use super::model::*;

pub const VERSION: i64 = 9;

fn native(expr: &str) -> String {
    format!("CHECK (origin = 'import' OR ({expr}))")
}

fn origin_columns() -> String {
    format!(
        "origin TEXT NOT NULL DEFAULT 'native' {},\n    import_raw_id INTEGER",
        Origin::check("origin")
    )
}

fn projects() -> String {
    r#"CREATE TABLE projects (
    project_id TEXT PRIMARY KEY,
    root TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);"#
    .to_string()
}

fn runs_table() -> String {
    let origin = origin_columns();
    format!(
        r#"CREATE TABLE runs (
    run_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(project_id),
    slug TEXT NOT NULL,
    orchestrator_worker_id TEXT,
    workspace_id TEXT,
    base_branch TEXT,
    run_branch TEXT,
    remote TEXT,
    pr_number INTEGER,
    pr_url TEXT,
    finish_steps TEXT NOT NULL DEFAULT '[]',
    check_every_s INTEGER NOT NULL DEFAULT 600,
    stall_rounds INTEGER NOT NULL DEFAULT 3,
    heartbeat_every_s INTEGER NOT NULL DEFAULT 30,
    heartbeat_stale_s INTEGER NOT NULL DEFAULT 120,
    timer_tolerance_s INTEGER NOT NULL DEFAULT 60,
    watchdog_pid INTEGER,
    watchdog_started INTEGER,
    watchdog_checkin_at TEXT,
    status TEXT NOT NULL {run_status},
    {origin},
    started_at TEXT NOT NULL,
    ended_at TEXT
);"#,
        run_status = RunStatus::check("status"),
    )
}

fn tasks_table() -> String {
    let origin = origin_columns();
    format!(
        r#"CREATE TABLE tasks (
    task_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    seq INTEGER NOT NULL,
    title TEXT NOT NULL,
    task_file TEXT,
    issue_url TEXT,
    spec_ref TEXT,
    primary_persona TEXT,
    fallback_persona TEXT,
    claimable_by TEXT,
    fallback_reason TEXT,
    phase TEXT,
    parallel INTEGER NOT NULL DEFAULT 1 CHECK (parallel IN (0, 1)),
    priority INTEGER NOT NULL DEFAULT 100,
    wave INTEGER,
    worktree_mode TEXT NOT NULL DEFAULT 'worktree' {worktree_mode},
    land_check TEXT,
    kind TEXT NOT NULL DEFAULT 'work' {task_kind},
    parent_task_id TEXT REFERENCES tasks(task_id),
    status TEXT NOT NULL {task_status},
    attempts INTEGER NOT NULL DEFAULT 0,
    current_worker_id TEXT,
    summary TEXT,
    {origin},
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    finished_at TEXT,
    UNIQUE (run_id, seq),
    {task_file_rule},
    {persona_rule},
    {claimable_rule}
);"#,
        worktree_mode = WorktreeMode::check("worktree_mode"),
        task_kind = TaskKind::check("kind"),
        task_status = TaskStatus::check("status"),
        task_file_rule = native("task_file IS NOT NULL"),
        persona_rule = native("primary_persona IS NOT NULL"),
        claimable_rule = native("claimable_by IS NOT NULL"),
    )
}

fn task_deps() -> String {
    r#"CREATE TABLE task_deps (
    task_id TEXT NOT NULL REFERENCES tasks(task_id),
    blocked_by TEXT NOT NULL REFERENCES tasks(task_id),
    PRIMARY KEY (task_id, blocked_by),
    CHECK (task_id <> blocked_by)
);"#
    .to_string()
}

fn task_files() -> String {
    format!(
        r#"CREATE TABLE task_files (
    task_id TEXT NOT NULL REFERENCES tasks(task_id),
    path TEXT NOT NULL,
    mode TEXT NOT NULL {file_mode},
    PRIMARY KEY (task_id, path, mode)
);"#,
        file_mode = FileMode::check("mode"),
    )
}

fn workers_table() -> String {
    let origin = origin_columns();
    format!(
        r#"CREATE TABLE workers (
    worker_id TEXT PRIMARY KEY,
    run_id TEXT REFERENCES runs(run_id),
    kind TEXT NOT NULL DEFAULT 'worker' {worker_kind},
    persona TEXT,
    role TEXT,
    harness TEXT,
    model TEXT,
    effort TEXT,
    host TEXT,
    workspace_id TEXT,
    pane_id TEXT,
    supervisor_pid INTEGER,
    supervisor_started INTEGER,
    agent_pid INTEGER,
    agent_started INTEGER,
    previous_worker_id TEXT REFERENCES workers(worker_id),
    resumed_from_session TEXT,
    session_id TEXT,
    transcript_path TEXT,
    legacy_record_id TEXT,
    last_checkin_at TEXT,
    last_progress_at TEXT,
    last_progress_source TEXT {progress_source},
    stall_rounds INTEGER NOT NULL DEFAULT 0,
    task_id TEXT REFERENCES tasks(task_id),
    attempt INTEGER,
    status TEXT NOT NULL {worker_status},
    exit_code INTEGER,
    end_reason TEXT,
    summary TEXT,
    {origin},
    spawned_at TEXT,
    started_at TEXT,
    claimed_at TEXT,
    ended_at TEXT,
    {worker_rule}
);

CREATE UNIQUE INDEX workers_one_working_per_task
    ON workers(task_id) WHERE status = 'working';"#,
        worker_kind = WorkerKind::check("kind"),
        progress_source = ProgressSource::check("last_progress_source"),
        worker_status = WorkerStatus::check("status"),
        worker_rule = native("run_id IS NOT NULL AND persona IS NOT NULL AND host IS NOT NULL"),
    )
}

fn checks_table() -> String {
    format!(
        r#"CREATE TABLE checks (
    check_id INTEGER PRIMARY KEY AUTOINCREMENT,
    worker_id TEXT NOT NULL REFERENCES workers(worker_id),
    at TEXT NOT NULL,
    elapsed_s INTEGER,
    checkin_age_s INTEGER,
    pid_alive INTEGER CHECK (pid_alive IN (0, 1)),
    herdr_status TEXT,
    herdr_seq INTEGER,
    transcript_bytes INTEGER,
    transcript_mtime TEXT,
    screen_timer_s INTEGER,
    timer_delta_s INTEGER,
    verdict TEXT NOT NULL {verdict},
    round INTEGER NOT NULL,
    note TEXT NOT NULL DEFAULT ''
);"#,
        verdict = Verdict::check("verdict"),
    )
}

fn stops_table() -> String {
    format!(
        r#"CREATE TABLE stops (
    stop_id INTEGER PRIMARY KEY AUTOINCREMENT,
    worker_id TEXT NOT NULL REFERENCES workers(worker_id),
    requested_by TEXT NOT NULL {stop_requester},
    reason TEXT NOT NULL,
    target TEXT NOT NULL {stop_target},
    pid INTEGER,
    pid_started INTEGER,
    signal TEXT {stop_signal},
    pane_id TEXT,
    result TEXT NOT NULL {stop_result},
    at TEXT NOT NULL
);"#,
        stop_requester = StopRequester::check("requested_by"),
        stop_target = StopTarget::check("target"),
        stop_signal = StopSignal::check("signal"),
        stop_result = StopResult::check("result"),
    )
}

const V1_WORKTREE_STATUS: &str = "CHECK (status IN ('open', 'landing', 'landed', 'cleanup_failed', 'cleaned', 'unlanded', 'abandoned'))";

fn worktrees_table() -> String {
    let origin = origin_columns();
    format!(
        r#"CREATE TABLE worktrees (
    worktree_id TEXT PRIMARY KEY,
    task_id TEXT REFERENCES tasks(task_id),
    worker_id TEXT REFERENCES workers(worker_id),
    repo_root TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    branch TEXT NOT NULL,
    base TEXT NOT NULL,
    base_commit TEXT,
    status TEXT NOT NULL {worktree_status},
    landed_commit TEXT,
    check_output_path TEXT,
    bytes_on_disk INTEGER,
    {origin},
    created_at TEXT NOT NULL,
    landed_at TEXT,
    cleaned_at TEXT,
    reason TEXT
);"#,
        worktree_status = V1_WORKTREE_STATUS,
    )
}

fn events_table() -> String {
    format!(
        r#"CREATE TABLE events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    run_id TEXT,
    task_id TEXT,
    worker_id TEXT,
    actor TEXT NOT NULL {actor},
    kind TEXT NOT NULL {event_kind},
    text TEXT NOT NULL DEFAULT ''
);

CREATE INDEX events_by_task ON events(task_id, event_id);
CREATE INDEX events_by_worker ON events(worker_id, event_id);"#,
        actor = Actor::check("actor"),
        event_kind = EventKind::check("kind"),
    )
}

fn task_steps() -> String {
    format!(
        r#"CREATE TABLE task_steps (
    task_id TEXT NOT NULL REFERENCES tasks(task_id),
    worker_id TEXT NOT NULL REFERENCES workers(worker_id),
    seq INTEGER NOT NULL,
    name TEXT NOT NULL {step_name},
    status TEXT NOT NULL {step_status},
    rounds INTEGER NOT NULL DEFAULT 0,
    evidence_path TEXT,
    verdict TEXT,
    review_task_ids TEXT,
    judged_rev TEXT,
    started_at TEXT,
    ended_at TEXT,
    note TEXT,
    PRIMARY KEY (task_id, worker_id, seq)
);"#,
        step_name = V1_STEP_NAME,
        step_status = StepStatus::check("status"),
    )
}

const V1_STEP_NAME: &str = "CHECK (name IN ('check', 'code-sweep', 'complexity-sweep', 'recheck', 'land', 'pr', 'cleanup'))";

fn v8() -> String {
    let table = task_steps()
        .replace("CREATE TABLE task_steps (", "CREATE TABLE task_steps_v8 (")
        .replace(V1_STEP_NAME, &StepName::check("name"));
    format!(
        "{table}
INSERT INTO task_steps_v8 SELECT * FROM task_steps;
DROP TABLE task_steps;
ALTER TABLE task_steps_v8 RENAME TO task_steps;"
    )
}

fn import_raw() -> String {
    format!(
        r#"CREATE TABLE import_raw (
    raw_id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_kind TEXT NOT NULL {source_kind},
    source_path TEXT NOT NULL,
    source_key TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    imported_at TEXT NOT NULL,
    mapped_to TEXT,
    map_error TEXT,
    UNIQUE (source_kind, source_key)
);"#,
        source_kind = SourceKind::check("source_kind"),
    )
}

fn import_findings() -> String {
    format!(
        r#"CREATE TABLE import_findings (
    finding_id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT REFERENCES projects(project_id),
    run_id TEXT REFERENCES runs(run_id),
    kind TEXT NOT NULL {finding_kind},
    task_id TEXT,
    worker_id TEXT,
    worktree_id TEXT,
    detail TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open' {finding_status},
    decided_by TEXT,
    decided_at TEXT,
    decision_note TEXT,
    created_at TEXT NOT NULL
);"#,
        finding_kind = FindingKind::check("kind"),
        finding_status = FindingStatus::check("status"),
    )
}

fn v1() -> String {
    let tables = [
        projects(),
        runs_table(),
        tasks_table(),
        task_deps(),
        task_files(),
        workers_table(),
        checks_table(),
        stops_table(),
        worktrees_table(),
        events_table(),
        task_steps(),
        import_raw(),
        import_findings(),
    ];
    format!("\n{}\n", tables.join("\n\n"))
}

const V2: &str = "CREATE TABLE worker_waits (
            worker_id TEXT NOT NULL REFERENCES workers(worker_id),
            kind TEXT NOT NULL CHECK(kind IN ('question','reviewers','background')),
            reason TEXT NOT NULL, started_at TEXT NOT NULL, deadline TEXT NOT NULL,
            task_ids TEXT NOT NULL DEFAULT '[]', PRIMARY KEY(worker_id,kind));
            CREATE TABLE worktree_modes (
                worktree_id TEXT PRIMARY KEY REFERENCES worktrees(worktree_id),
                run_id TEXT NOT NULL REFERENCES runs(run_id), name TEXT NOT NULL,
                scratch INTEGER NOT NULL DEFAULT 0 CHECK(scratch IN (0,1))); ";

const V3: &str = "ALTER TABLE runs ADD COLUMN max_attempts INTEGER NOT NULL DEFAULT 3;
             ALTER TABLE tasks ADD COLUMN held_reason TEXT;
             ALTER TABLE tasks ADD COLUMN attempt_floor INTEGER NOT NULL DEFAULT 0;";

const V4: &str = "ALTER TABLE runs ADD COLUMN watchdog_version TEXT;
             ALTER TABLE runs ADD COLUMN watchdog_exe TEXT;";

fn v5() -> String {
    let columns = "worktree_id, task_id, worker_id, repo_root, path, branch, base, base_commit, \
        status, landed_commit, check_output_path, bytes_on_disk, origin, import_raw_id, \
        created_at, landed_at, cleaned_at, reason";
    let table = worktrees_table()
        .replace("CREATE TABLE worktrees (", "CREATE TABLE worktrees_v5 (")
        .replace(V1_WORKTREE_STATUS, &WorktreeStatus::check("status"))
        .replace(
            "\n    reason TEXT\n);",
            &format!(
                "\n    reason TEXT,\n    archive_ref TEXT,\n    decision TEXT {},\n    decided_at TEXT,\n    bytes_freed INTEGER\n);",
                TriageDecision::check("decision")
            ),
        );
    format!(
        "{table}
INSERT INTO worktrees_v5 ({columns}) SELECT {columns} FROM worktrees;
DROP TABLE worktrees;
ALTER TABLE worktrees_v5 RENAME TO worktrees;"
    )
}

const V6: &str = "ALTER TABLE worktrees ADD COLUMN bytes INTEGER;
             ALTER TABLE worktrees ADD COLUMN bytes_measured_at TEXT;";

fn v7() -> String {
    format!(
        "ALTER TABLE tasks ADD COLUMN work_kind TEXT {work_kind};
ALTER TABLE tasks ADD COLUMN review_mode TEXT {review_mode};
ALTER TABLE tasks ADD COLUMN ticket_snapshot TEXT;
ALTER TABLE tasks ADD COLUMN ticket_sha TEXT;
ALTER TABLE tasks ADD COLUMN check_none INTEGER NOT NULL DEFAULT 0 CHECK (check_none IN (0, 1));
ALTER TABLE runs ADD COLUMN default_check TEXT;
ALTER TABLE runs ADD COLUMN pr_declined_reason TEXT;
ALTER TABLE runs ADD COLUMN orchestrator_stopped_at TEXT;
ALTER TABLE runs ADD COLUMN orchestrator_respawns INTEGER NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN last_respawn_at TEXT;
ALTER TABLE workers ADD COLUMN handoff_path TEXT;
CREATE TABLE run_gates (
    gate_id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    tip_sha TEXT NOT NULL,
    command TEXT NOT NULL,
    exit_code INTEGER,
    log_path TEXT,
    started_at TEXT NOT NULL,
    ended_at TEXT
);
CREATE INDEX run_gates_by_tip ON run_gates(run_id, tip_sha);
CREATE TABLE outbox (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    event TEXT NOT NULL {event},
    key TEXT NOT NULL UNIQUE,
    text TEXT NOT NULL,
    created_at TEXT NOT NULL,
    delivered_at TEXT
);
CREATE INDEX outbox_undelivered ON outbox(created_at) WHERE delivered_at IS NULL;
ALTER TABLE workers ADD COLUMN target_task_id TEXT REFERENCES tasks(task_id);
CREATE TABLE session_records (
    record_id TEXT PRIMARY KEY,
    worker_id TEXT,
    project_root TEXT NOT NULL,
    slug TEXT NOT NULL,
    record_json TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX session_records_by_slug ON session_records(slug);",
        work_kind = WorkKind::check("work_kind"),
        review_mode = ReviewMode::check("review_mode"),
        event = OutboxEvent::check("event"),
    )
}

const V9: &str = "ALTER TABLE workers ADD COLUMN backend TEXT NOT NULL DEFAULT 'herdr' CHECK (backend IN ('herdr', 'hc'));";

pub fn migrate(conn: &mut Connection) -> Result<()> {
    migrate_guarded(conn, &|_| Ok(()))
}

pub fn migrate_guarded(conn: &mut Connection, guard: &dyn Fn(i64) -> Result<()>) -> Result<()> {
    let keys: bool = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    conn.pragma_update(None, "foreign_keys", false)?;
    let migrated = migrate_unchecked(conn, guard);
    conn.pragma_update(None, "foreign_keys", keys)?;
    migrated
}

fn file_of(conn: &Connection) -> Option<PathBuf> {
    conn.path().filter(|p| !p.is_empty()).map(PathBuf::from)
}

pub const BACKUPS_KEPT: usize = 3;

pub fn backups_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.strip_prefix("ledger.v")
                .and_then(|rest| rest.split_once("-backup-"))
                .is_some_and(|(v, rest)| {
                    !v.is_empty()
                        && v.bytes().all(|b| b.is_ascii_digit())
                        && rest.ends_with(".sqlite3")
                })
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

fn backup(file: &Path, from: i64) -> Result<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let mut copy = dir.join(format!("ledger.v{from}-backup-{stamp}.sqlite3"));
    let mut n = 2;
    while copy.exists() {
        copy = dir.join(format!("ledger.v{from}-backup-{stamp}-{n}.sqlite3"));
        n += 1;
    }
    let source = Connection::open(file)
        .with_context(|| format!("opening {} for a backup", file.display()))?;
    source.busy_timeout(super::store::BUSY_TIMEOUT)?;
    source
        .execute("VACUUM INTO ?1", [copy.to_string_lossy()])
        .with_context(|| format!("writing the ledger backup {}", copy.display()))?;
    let all = backups_in(dir);
    for old in &all[..all.len().saturating_sub(BACKUPS_KEPT)] {
        let _ = std::fs::remove_file(old);
    }
    Ok(copy)
}

fn newer_schema(file: Option<&Path>, current: i64) -> anyhow::Error {
    let what = match file {
        Some(f) => format!("the task ledger {}", f.display()),
        None => "the task ledger".to_string(),
    };
    let newest = file
        .and_then(|f| f.parent())
        .and_then(|dir| backups_in(dir).pop());
    let todo = match newest {
        Some(b) => format!(
            "Run horch self-update, or stop every fleet and copy the backup {} over the ledger \
             (rows written after the backup are lost).",
            b.display()
        ),
        None => "Run horch self-update, or use the horch that migrated the file.".to_string(),
    };
    anyhow::anyhow!(
        "{what} is schema version {current} and this horch knows only up to {VERSION}: \
         a newer or development horch migrated the file.\n{todo}"
    )
}

fn migrate_unchecked(conn: &mut Connection, guard: &dyn Fn(i64) -> Result<()>) -> Result<()> {
    let file = file_of(conn);
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let current: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current > VERSION {
        return Err(newer_schema(file.as_deref(), current));
    }
    if current == VERSION {
        return Ok(());
    }
    if current > 0 {
        guard(current)?;
        if let Some(file) = &file {
            backup(file, current)?;
        }
    }
    if current < 1 {
        tx.execute_batch(&v1())
            .context("creating the task ledger tables")?;
    }
    for (version, sql) in [(2, V2), (3, V3), (4, V4)] {
        if current < version {
            tx.execute_batch(sql)?;
        }
    }
    if current < 5 {
        tx.execute_batch(&v5())
            .context("adding the worktree triage columns")?;
        let broken: i64 =
            tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })?;
        if broken > 0 {
            bail!("the version 5 migration left {broken} rows with a broken reference");
        }
    }
    if current < 6 {
        tx.execute_batch(V6)?;
    }
    if current < 7 {
        tx.execute_batch(&v7())
            .context("adding the trace-fixes columns and tables")?;
    }
    if current < 8 {
        tx.execute_batch(&v8())
            .context("adding the comment-sweep finish step")?;
    }
    if current < 9 {
        tx.execute_batch(V9)
            .context("adding the worker pane host backend")?;
    }
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnv1a(text: &str) -> u64 {
        text.bytes().fold(0xcbf29ce484222325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        })
    }

    #[test]
    fn v1_text_is_pinned() {
        let sql = v1();
        assert_eq!((sql.len(), fnv1a(&sql)), (8616, 949560913304565573));
    }

    #[test]
    fn migrate_empty_database_reaches_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, VERSION);
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 18);
    }

    fn version_3_ledger() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&v1()).unwrap();
        conn.execute_batch(V2).unwrap();
        conn.execute_batch(V3).unwrap();
        conn.pragma_update(None, "user_version", 3).unwrap();
        conn.execute_batch(
            "INSERT INTO projects VALUES ('p', '/r', 'now');
             INSERT INTO runs (run_id, project_id, slug, status, started_at, watchdog_pid)
                VALUES ('r1', 'p', 's', 'active', 'now', 77);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn migrate_from_version_3_adds_the_watchdog_columns_and_keeps_rows() {
        let mut conn = version_3_ledger();
        assert!(conn.prepare("SELECT watchdog_version FROM runs").is_err());
        migrate(&mut conn).unwrap();
        let (pid, version, exe): (i64, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT watchdog_pid, watchdog_version, watchdog_exe FROM runs",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((pid, version, exe), (77, None, None));
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, VERSION);
        migrate(&mut conn).unwrap();
    }

    #[test]
    fn hcb_03_migrate_from_version_7_adds_backend_and_keeps_rows() {
        for start_version in [7_i64, 8_i64] {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute_batch(&v1()).unwrap();
            for sql in [V2, V3, V4] {
                conn.execute_batch(sql).unwrap();
            }
            conn.execute_batch(&v5()).unwrap();
            conn.execute_batch(V6).unwrap();
            conn.execute_batch(&v7()).unwrap();
            if start_version >= 8 {
                conn.execute_batch(&v8()).unwrap();
            }
            conn.pragma_update(None, "user_version", start_version)
                .unwrap();
            conn.execute_batch(
                "INSERT INTO projects VALUES ('p', '/r', 'now');
                 INSERT INTO runs (run_id, project_id, slug, status, started_at)
                    VALUES ('r1', 'p', 's', 'active', 'now');
                 INSERT INTO workers (worker_id, run_id, persona, host, workspace_id, pane_id, status)
                    VALUES ('w1', 'r1', 'opus', 'h', 'ws1', 'p1', 'working');",
            )
            .unwrap();
            let mut conn = conn;
            assert!(conn.prepare("SELECT backend FROM workers").is_err());

            migrate(&mut conn).unwrap();

            let v: i64 = conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(v, VERSION);
            let row = crate::taskledger::workers::get(&conn, "w1").unwrap();
            assert_eq!(row.workspace_id.as_deref(), Some("ws1"));
            assert_eq!(row.pane_id.as_deref(), Some("p1"));
            assert_eq!(row.backend, crate::runtime::Backend::Herdr);

            conn.execute(
                "UPDATE workers SET backend = 'hc' WHERE worker_id = 'w1'",
                [],
            )
            .unwrap();
            let updated = crate::taskledger::workers::get(&conn, "w1").unwrap();
            assert_eq!(updated.backend, crate::runtime::Backend::Hc);
            assert!(
                conn.execute(
                    "UPDATE workers SET backend = 'tmux' WHERE worker_id = 'w1'",
                    []
                )
                .is_err(),
                "CHECK rejects unknown backend"
            );
        }
    }
}

#[cfg(test)]
mod v5_tests {
    use super::*;

    fn version_4_file(path: &std::path::Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(&v1()).unwrap();
        for sql in [V2, V3, V4] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 4).unwrap();
        conn.execute_batch(
            "INSERT INTO projects VALUES ('p', '/r', 'now');
             INSERT INTO runs (run_id, project_id, slug, status, started_at) VALUES ('r1', 'p', 's', 'active', 'now');
             INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, status, created_at, reason, bytes_on_disk)
                VALUES ('wt1', '/r', '/r/.worktrees/a', 'a', 'main', 'unlanded', 'then', 'check red', 99);
             INSERT INTO worktree_modes (worktree_id, run_id, name, scratch) VALUES ('wt1', 'r1', 'a', 0);",
        )
        .unwrap();
    }

    #[test]
    fn migrate_from_version_4_keeps_rows_and_adds_the_triage_words_and_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        version_4_file(&path);
        let store = crate::taskledger::Store::open(&path).unwrap();
        let conn = store.conn();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, VERSION);
        let kept: (String, String, i64, Option<String>) = conn
            .query_row(
                "SELECT status, reason, bytes_on_disk, decision FROM worktrees WHERE worktree_id = 'wt1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(kept, ("unlanded".into(), "check red".into(), 99, None));
        conn.execute(
            "UPDATE worktrees SET status = 'archived', decision = 'ARCHIVE', archive_ref = 'refs/horch/archive/a',
                decided_at = 'now', bytes_freed = 99 WHERE worktree_id = 'wt1'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE worktrees SET status = 'trashed' WHERE worktree_id = 'wt1'",
            [],
        )
        .unwrap();
        assert!(conn
            .execute(
                "UPDATE worktrees SET status = 'lost' WHERE worktree_id = 'wt1'",
                []
            )
            .is_err());
        assert!(conn
            .execute(
                "UPDATE worktrees SET decision = 'MAYBE' WHERE worktree_id = 'wt1'",
                []
            )
            .is_err());
        let fk: i64 = conn
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(fk, 0);
        assert!(conn
            .execute(
                "INSERT INTO worktree_modes (worktree_id, run_id, name) VALUES ('nope', 'r1', 'x')",
                []
            )
            .is_err());
        drop(store);
        crate::taskledger::Store::open(&path).unwrap();
    }
}

#[cfg(test)]
mod v6_tests {
    use super::*;

    fn version_5_file(path: &std::path::Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(&v1()).unwrap();
        for sql in [V2, V3, V4] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch(&v5()).unwrap();
        conn.pragma_update(None, "user_version", 5).unwrap();
        conn.execute_batch(
            "INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, status, created_at, reason, bytes_on_disk, decision)
                VALUES ('wt1', '/r', '/r/.worktrees/a', 'a', 'main', 'unlanded', 'then', 'check red', 99, 'KEEP');",
        )
        .unwrap();
    }

    #[test]
    fn migrate_from_version_5_keeps_rows_and_adds_the_measured_bytes_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        version_5_file(&path);
        let store = crate::taskledger::Store::open(&path).unwrap();
        let conn = store.conn();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, VERSION);
        let kept: (String, i64, Option<String>, Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT reason, bytes_on_disk, decision, bytes, bytes_measured_at FROM worktrees WHERE worktree_id = 'wt1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            kept,
            ("check red".into(), 99, Some("KEEP".into()), None, None)
        );
        conn.execute(
            "UPDATE worktrees SET bytes = 5, bytes_measured_at = 'now' WHERE worktree_id = 'wt1'",
            [],
        )
        .unwrap();
        drop(store);
        let store = crate::taskledger::Store::open(&path).unwrap();
        let bytes: i64 = store
            .conn()
            .query_row("SELECT bytes FROM worktrees", [], |r| r.get(0))
            .unwrap();
        assert_eq!(bytes, 5);
    }
}

#[cfg(test)]
mod v7_tests {
    use super::*;
    use rusqlite::types::Value;

    fn version_6_file(path: &std::path::Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(&v1()).unwrap();
        for sql in [V2, V3, V4] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute_batch(&v5()).unwrap();
        conn.execute_batch(V6).unwrap();
        conn.pragma_update(None, "user_version", 6).unwrap();
        conn.execute_batch(
            "INSERT INTO projects VALUES ('p', '/r', 'now');
             INSERT INTO runs (run_id, project_id, slug, workspace_id, orchestrator_worker_id, status, started_at)
                VALUES ('r1', 'p', 's', 'ws1', 'o1', 'active', 'now');
             INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by, status,
                    held_reason, created_at, updated_at)
                VALUES ('t1', 'r1', 1, 'a', 'a.md', 'opus', 'opus', 'queued', '2 attempts failed', 'now', 'now'),
                       ('t2', 'r1', 2, 'b', 'b.md', 'opus', 'opus', 'running', NULL, 'now', 'now');
             INSERT INTO task_deps VALUES ('t2', 't1');
             INSERT INTO task_files VALUES ('t1', 'src/a.rs', 'own');
             INSERT INTO workers (worker_id, run_id, persona, host, task_id, status, end_reason)
                VALUES ('w1', 'r1', 'opus', 'h', 't1', 'stopped', 'reaped');
             INSERT INTO checks (worker_id, at, verdict, round) VALUES ('w1', 'now', 'progress', 1);
             INSERT INTO stops (worker_id, requested_by, reason, target, result, at)
                VALUES ('w1', 'watchdog', 'stall', 'agent', 'sent', 'now');
             INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path, branch, base, status,
                    created_at, decision, bytes, bytes_measured_at)
                VALUES ('wt1', 't1', 'w1', '/r', '/r/.worktrees/a', 'a', 'main', 'unlanded', 'then', 'KEEP', 5, 'now');
             INSERT INTO events (at, run_id, task_id, actor, kind, text) VALUES ('now', 'r1', 't1', 'horch', 'added', 'x');
             INSERT INTO task_steps (task_id, worker_id, seq, name, status) VALUES ('t1', 'w1', 1, 'check', 'passed');
             INSERT INTO import_raw (source_kind, source_path, source_key, raw_json, imported_at)
                VALUES ('pending_mail', '/m', 'k', '{}', 'now');
             INSERT INTO import_findings (kind, detail, created_at) VALUES ('disk_heavy', 'd', 'now');
             INSERT INTO worker_waits VALUES ('w1', 'question', 'q', 'now', 'later', '[]');
             INSERT INTO worktree_modes VALUES ('wt1', 'r1', 'a', 0);",
        )
        .unwrap();
    }

    type Dump = Vec<(String, Vec<String>, Vec<Vec<Value>>)>;

    fn dump(conn: &Connection, only: Option<&Dump>) -> Dump {
        let tables: Vec<String> = match only {
            Some(d) => d.iter().map(|(t, _, _)| t.clone()).collect(),
            None => conn
                .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap(),
        };
        tables
            .into_iter()
            .map(|table| {
                let columns: Vec<String> = match only {
                    Some(d) => d.iter().find(|(t, _, _)| *t == table).unwrap().1.clone(),
                    None => conn
                        .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
                        .unwrap()
                        .query_map([&table], |r| r.get(0))
                        .unwrap()
                        .collect::<rusqlite::Result<_>>()
                        .unwrap(),
                };
                let list = columns.join(", ");
                let sql = format!("SELECT {list} FROM {table} ORDER BY {list}");
                let rows = conn
                    .prepare(&sql)
                    .unwrap()
                    .query_map([], |r| {
                        (0..columns.len()).map(|i| r.get::<_, Value>(i)).collect()
                    })
                    .unwrap()
                    .collect::<rusqlite::Result<_>>()
                    .unwrap();
                (table, columns, rows)
            })
            .collect()
    }

    #[test]
    fn trf_01_migration_keeps_every_row_and_defaults_the_new_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        version_6_file(&path);
        let before = {
            let conn = Connection::open(&path).unwrap();
            dump(&conn, None)
        };
        assert_eq!(before.len(), 15);
        assert!(
            before
                .iter()
                .all(|(t, _, rows)| !rows.is_empty() || t == "sqlite_sequence"),
            "every table holds a row: {before:?}"
        );

        let store = crate::taskledger::Store::open(&path).unwrap();
        let conn = store.conn();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, VERSION);
        assert_eq!(dump(conn, Some(&before)), before, "no row changed");

        type TaskRow = (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            i64,
        );
        let task: Vec<TaskRow> = conn
            .prepare(
                "SELECT work_kind, review_mode, ticket_snapshot, ticket_sha, check_none FROM tasks",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(task, vec![(None, None, None, None, 0); 2]);
        let run: (Option<String>, Option<String>, Option<String>, i64, Option<String>) = conn
            .query_row(
                "SELECT default_check, pr_declined_reason, orchestrator_stopped_at, orchestrator_respawns, last_respawn_at FROM runs",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(run, (None, None, None, 0, None));
        let handoff: Option<String> = conn
            .query_row("SELECT handoff_path FROM workers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(handoff, None);
        for table in ["run_gates", "outbox"] {
            let n: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 0, "{table} starts empty");
        }

        conn.execute(
            "UPDATE tasks SET work_kind = ?1, review_mode = ?2, held_reason = ?3 WHERE task_id = 't1'",
            [WorkKind::Security.as_str(), ReviewMode::NoReview.as_str(), HeldReason::Unlanded.as_str()],
        )
        .unwrap();
        assert!(conn
            .execute("UPDATE tasks SET work_kind = 'chore'", [])
            .is_err());
        assert!(conn
            .execute("UPDATE tasks SET review_mode = 'some'", [])
            .is_err());
        assert!(conn.execute("UPDATE tasks SET check_none = 2", []).is_err());
        conn.execute(
            "UPDATE workers SET end_reason = ?1, handoff_path = 'h.md'",
            [EndReason::HandedOff.as_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO run_gates (run_id, tip_sha, command, exit_code, log_path, started_at, ended_at)
                VALUES ('r1', 'abc', 'just gate', 0, '/l', 'now', 'now')",
            [],
        )
        .unwrap();
        let wake = "INSERT INTO outbox (id, run_id, event, key, text, created_at) VALUES (?1, 'r1', ?2, 'r1:settled:run', 'x', 'now')";
        conn.execute(wake, ["u1", OutboxEvent::Settled.as_str()])
            .unwrap();
        assert!(
            conn.execute(wake, ["u2", "settled"]).is_err(),
            "key is unique"
        );
        assert!(conn
            .execute("INSERT INTO outbox (id, run_id, event, key, text, created_at) VALUES ('u3', 'r1', 'woke', 'k3', 'x', 'now')", [])
            .is_err());
        assert!(conn
            .execute("INSERT INTO outbox (id, run_id, event, key, text, created_at) VALUES ('u4', 'nope', 'held', 'k4', 'x', 'now')", [])
            .is_err(), "run_id must name a run");
        drop(store);
        let store = crate::taskledger::Store::open(&path).unwrap();
        let n: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    fn file_at(path: &std::path::Path, version: i64) {
        let conn = Connection::open(path).unwrap();
        let steps = [
            v1(),
            V2.into(),
            V3.into(),
            V4.into(),
            v5(),
            V6.into(),
            v7(),
            v8(),
            V9.into(),
        ];
        assert_eq!(steps.len() as i64, VERSION, "add the new step here");
        for sql in &steps[..version as usize] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute("INSERT INTO projects VALUES ('p', '/r', 'now')", [])
            .unwrap();
        conn.pragma_update(None, "user_version", version).unwrap();
    }

    fn schema_text(conn: &Connection) -> Vec<(String, String, Option<String>)> {
        conn.prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn cms_07_migration_keeps_the_step_rows_and_takes_the_comment_sweep_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        version_6_file(&path);
        let store = crate::taskledger::Store::open(&path).unwrap();
        let conn = store.conn();
        let old: (String, String) = conn
            .query_row("SELECT name, status FROM task_steps", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(old, ("check".to_string(), "passed".to_string()));
        conn.execute(
            "INSERT INTO task_steps (task_id, worker_id, seq, name, status)
             VALUES ('t1', 'w1', 2, 'comment-sweep', 'pending')",
            [],
        )
        .unwrap();
        assert!(conn
            .execute(
                "INSERT INTO task_steps (task_id, worker_id, seq, name, status)
                 VALUES ('t1', 'w1', 3, 'no-such-step', 'pending')",
                [],
            )
            .is_err());
    }

    #[test]
    fn trf_48_every_old_version_migrates_to_current() {
        let fresh_dir = tempfile::tempdir().unwrap();
        let fresh = crate::taskledger::Store::open(&fresh_dir.path().join("l.sqlite3")).unwrap();
        let want = schema_text(fresh.conn());
        for version in 1..VERSION {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("ledger.sqlite3");
            file_at(&path, version);
            let store = crate::taskledger::Store::open(&path)
                .unwrap_or_else(|e| panic!("version {version}: {e:#}"));
            let v: i64 = store
                .conn()
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(v, VERSION, "version {version}");
            assert_eq!(schema_text(store.conn()), want, "version {version}");
            let rows: i64 = store
                .conn()
                .query_row("SELECT count(*) FROM projects", [], |r| r.get(0))
                .unwrap();
            assert_eq!(rows, 1, "version {version}");
            let copies = backups_in(dir.path());
            assert_eq!(copies.len(), 1, "version {version}: {copies:?}");
            let name = copies[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            assert!(
                name.starts_with(&format!("ledger.v{version}-backup-")),
                "{name}"
            );
        }
    }
}
