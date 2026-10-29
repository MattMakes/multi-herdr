//! The task ledger's tables (docs/specs/task-ledger.md section 4 and 11).
//!
//! The schema is built from the enums in [`super::model`], so each `CHECK`
//! list is the code's own list. `PRAGMA user_version` holds the version;
//! [`migrate`] brings an older file forward and never goes back.

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use super::model::*;

/// The schema version this binary writes.
pub const VERSION: i64 = 2;

/// A rule that applies to rows this ledger writes, not to imported ones
/// (section 11.2): `CHECK (origin = 'import' OR <expr>)`.
fn native(expr: &str) -> String {
    format!("CHECK (origin = 'import' OR ({expr}))")
}

fn origin_columns() -> String {
    format!(
        "origin TEXT NOT NULL DEFAULT 'native' {},\n    import_raw_id INTEGER",
        Origin::check("origin")
    )
}

/// The statements of version 1.
fn v1() -> String {
    let origin = origin_columns();
    format!(
        r#"
CREATE TABLE projects (
    project_id TEXT PRIMARY KEY,
    root TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

CREATE TABLE runs (
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
);

CREATE TABLE tasks (
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
);

CREATE TABLE task_deps (
    task_id TEXT NOT NULL REFERENCES tasks(task_id),
    blocked_by TEXT NOT NULL REFERENCES tasks(task_id),
    PRIMARY KEY (task_id, blocked_by),
    CHECK (task_id <> blocked_by)
);

CREATE TABLE task_files (
    task_id TEXT NOT NULL REFERENCES tasks(task_id),
    path TEXT NOT NULL,
    mode TEXT NOT NULL {file_mode},
    PRIMARY KEY (task_id, path, mode)
);

CREATE TABLE workers (
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
    ON workers(task_id) WHERE status = 'working';

CREATE TABLE checks (
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
);

CREATE TABLE stops (
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
);

CREATE TABLE worktrees (
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
);

CREATE TABLE events (
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
CREATE INDEX events_by_worker ON events(worker_id, event_id);

CREATE TABLE task_steps (
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
);

CREATE TABLE import_raw (
    raw_id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_kind TEXT NOT NULL {source_kind},
    source_path TEXT NOT NULL,
    source_key TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    imported_at TEXT NOT NULL,
    mapped_to TEXT,
    map_error TEXT,
    UNIQUE (source_kind, source_key)
);

CREATE TABLE import_findings (
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
);
"#,
        run_status = RunStatus::check("status"),
        worktree_mode = WorktreeMode::check("worktree_mode"),
        task_kind = TaskKind::check("kind"),
        task_status = TaskStatus::check("status"),
        task_file_rule = native("task_file IS NOT NULL"),
        persona_rule = native("primary_persona IS NOT NULL"),
        claimable_rule = native("claimable_by IS NOT NULL"),
        file_mode = FileMode::check("mode"),
        worker_kind = WorkerKind::check("kind"),
        progress_source = ProgressSource::check("last_progress_source"),
        worker_status = WorkerStatus::check("status"),
        worker_rule = native("run_id IS NOT NULL AND persona IS NOT NULL AND host IS NOT NULL"),
        verdict = Verdict::check("verdict"),
        stop_requester = StopRequester::check("requested_by"),
        stop_target = StopTarget::check("target"),
        stop_signal = StopSignal::check("signal"),
        stop_result = StopResult::check("result"),
        worktree_status = WorktreeStatus::check("status"),
        actor = Actor::check("actor"),
        event_kind = EventKind::check("kind"),
        step_name = StepName::check("name"),
        step_status = StepStatus::check("status"),
        source_kind = SourceKind::check("source_kind"),
        finding_kind = FindingKind::check("kind"),
        finding_status = FindingStatus::check("status"),
    )
}

/// Bring the database to [`VERSION`]. A file from a newer binary is refused:
/// this one would not know its columns.
pub fn migrate(conn: &mut Connection) -> Result<()> {
    // The write lock first, then the version: 2 processes that open a fresh
    // file at once must not both create the tables.
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let current: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current > VERSION {
        bail!(
            "the task ledger is schema version {current}; this horch knows only up to {VERSION}. \
             Run horch self-update."
        );
    }
    if current == VERSION {
        return Ok(());
    }
    if current < 1 {
        tx.execute_batch(&v1())
            .context("creating the task ledger tables")?;
    }
    if current < 2 {
        tx.execute_batch(
            "CREATE TABLE worker_waits (
            worker_id TEXT NOT NULL REFERENCES workers(worker_id),
            kind TEXT NOT NULL CHECK(kind IN ('question','reviewers','background')),
            reason TEXT NOT NULL, started_at TEXT NOT NULL, deadline TEXT NOT NULL,
            task_ids TEXT NOT NULL DEFAULT '[]', PRIMARY KEY(worker_id,kind));
            CREATE TABLE worktree_modes (
                worktree_id TEXT PRIMARY KEY REFERENCES worktrees(worktree_id),
                run_id TEXT NOT NULL REFERENCES runs(run_id), name TEXT NOT NULL,
                scratch INTEGER NOT NULL DEFAULT 0 CHECK(scratch IN (0,1))); ",
        )?;
    }
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.commit()?;
    Ok(())
}
