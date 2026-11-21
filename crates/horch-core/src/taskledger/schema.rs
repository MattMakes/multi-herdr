//! The task ledger's tables (docs/specs/task-ledger.md section 4 and 11).
//!
//! The schema is built from the enums in [`super::model`], so each `CHECK`
//! list is the code's own list. `PRAGMA user_version` holds the version;
//! [`migrate`] brings an older file forward and never goes back.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};

use super::model::*;
use super::sql_words::word;

/// The version of the Phase 1 tables of fleetd (`v8`). A test that
/// tests that step builds or migrates to this version, not to [`VERSION`].
pub const PHASE_1_VERSION: i64 = 8;

/// The version of the Phase 2 tables and columns of fleetd (`phase_2`).
pub const PHASE_2_VERSION: i64 = PHASE_1_VERSION + 1;

/// The version of the Phase 3 columns of `worktrees` (`phase_3`).
pub const PHASE_3_VERSION: i64 = PHASE_2_VERSION + 1;

/// The version of the step that builds `events` again with the event
/// kinds of this binary (`events_kinds`).
pub const EVENT_KINDS_VERSION: i64 = PHASE_3_VERSION + 1;

/// The version of the step that builds `worktrees` again with its `path`
/// unique only among the rows that are not `removed` (`worktree_paths`).
pub const WORKTREE_PATHS_VERSION: i64 = EVENT_KINDS_VERSION + 1;

/// The version of the Phase 4 tables of fleetd and their release backfill
/// (`phase_4`).
pub const PHASE_4_VERSION: i64 = WORKTREE_PATHS_VERSION + 1;

/// The version of the Phase 5 tables of fleetd, `requests` and
/// `polite_stops` (`phase_5`).
pub const PHASE_5_VERSION: i64 = PHASE_4_VERSION + 1;

/// The version of the Phase 6 tables of fleetd, the idea of each task and
/// their release backfill (`phase_6`).
pub const PHASE_6_VERSION: i64 = PHASE_5_VERSION + 1;

/// The version of the Phase 7 step: `events` built again, so its kind
/// `CHECK` has the event words of the health of a profile (`phase_7`).
pub const PHASE_7_VERSION: i64 = PHASE_6_VERSION + 1;

/// The version of the step that rewrites each stored project root in the
/// spelling of [`super::runs::root_key`] and merges 2 rows of 1 root
/// (`project_roots`, D266-F2).
pub const PROJECT_ROOTS_VERSION: i64 = PHASE_7_VERSION + 1;

/// The schema version this binary writes.
pub const VERSION: i64 = PROJECT_ROOTS_VERSION;

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

/// The `projects` table.
fn projects() -> String {
    r#"CREATE TABLE projects (
    project_id TEXT PRIMARY KEY,
    root TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);"#
    .to_string()
}

/// The `runs_table` statements.
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

/// The status `CHECK` of version 1 (the words of today), frozen: the Phase
/// 2 step builds `tasks` again with the 12 states of [`TaskStatus`].
const V1_TASK_STATUS: &str =
    "CHECK (status IN ('queued', 'running', 'landing', 'done', 'cancelled'))";

/// The `tasks_table` statements.
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
        task_status = V1_TASK_STATUS,
        task_file_rule = native("task_file IS NOT NULL"),
        persona_rule = native("primary_persona IS NOT NULL"),
        claimable_rule = native("claimable_by IS NOT NULL"),
    )
}

/// The `task_deps` statements.
fn task_deps() -> String {
    r#"CREATE TABLE task_deps (
    task_id TEXT NOT NULL REFERENCES tasks(task_id),
    blocked_by TEXT NOT NULL REFERENCES tasks(task_id),
    PRIMARY KEY (task_id, blocked_by),
    CHECK (task_id <> blocked_by)
);"#
    .to_string()
}

/// The `task_files` statements.
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

/// The `workers_table` statements.
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

/// The `checks_table` statements.
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

/// The `stops_table` statements.
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

/// `worktrees.status` as version 1 wrote it. Version 1 runs as a migration,
/// so its text keeps these words; version 5 adds the later ones.
const V1_WORKTREE_STATUS: &str = "CHECK (status IN ('open', 'landing', 'landed', 'cleanup_failed', 'cleaned', 'unlanded', 'abandoned'))";

/// The `worktrees_table` statements.
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

/// `events.kind` as versions 1 to 7 wrote it. Version 1 runs as a
/// migration, so its text keeps these words; version 8 adds the later ones.
const V1_EVENT_KIND: &str = "CHECK (kind IN ('run_created', 'added', 'claimed', 'no_task', 'progress', 'note', 'question', 'blocked', 'answered', 'fallback', 'step_started', 'step_passed', 'step_failed', 'land_started', 'land_red', 'landed', 'pr_updated', 'cleaned', 'cleanup_failed', 'done', 'unlanded', 'reaped', 'requeued', 'stopped', 'session', 'exited', 'cancelled', 'verdict', 'woke', 'imported', 'reopened', 'dismissed'))";

/// The `events_table` statements.
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
        event_kind = V1_EVENT_KIND,
    )
}

/// The `task_steps` statements.
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
        step_name = StepName::check("name"),
        step_status = StepStatus::check("status"),
    )
}

/// The `import_raw` statements.
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

/// The `import_findings` statements.
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

/// The statements of version 1.
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

/// Version 2: the waits of a worker, and the modes of a worktree.
const V2: &str = "CREATE TABLE worker_waits (
            worker_id TEXT NOT NULL REFERENCES workers(worker_id),
            kind TEXT NOT NULL CHECK(kind IN ('question','reviewers','background')),
            reason TEXT NOT NULL, started_at TEXT NOT NULL, deadline TEXT NOT NULL,
            task_ids TEXT NOT NULL DEFAULT '[]', PRIMARY KEY(worker_id,kind));
            CREATE TABLE worktree_modes (
                worktree_id TEXT PRIMARY KEY REFERENCES worktrees(worktree_id),
                run_id TEXT NOT NULL REFERENCES runs(run_id), name TEXT NOT NULL,
                scratch INTEGER NOT NULL DEFAULT 0 CHECK(scratch IN (0,1))); ";

/// Version 3: a held task stays `queued` with a reason, so the status CHECK
/// list keeps its words (#168). `attempt_floor` is the attempt count at the
/// last operator requeue: the limit counts the attempts after it.
const V3: &str = "ALTER TABLE runs ADD COLUMN max_attempts INTEGER NOT NULL DEFAULT 3;
             ALTER TABLE tasks ADD COLUMN held_reason TEXT;
             ALTER TABLE tasks ADD COLUMN attempt_floor INTEGER NOT NULL DEFAULT 0;";

/// Version 4: the version and the binary (a JSON `ExeIdentity`) of the
/// watchdog at its last checkin, so `ledger status` shows an old watchdog
/// (#216).
const V4: &str = "ALTER TABLE runs ADD COLUMN watchdog_version TEXT;
             ALTER TABLE runs ADD COLUMN watchdog_exe TEXT;";

/// Version 5: worktree triage (#308). The statuses `archived` and `trashed`,
/// and the columns of a decision. SQLite cannot change a `CHECK`, so the
/// table is built again under its own name and its rows copied.
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
    // The rows of `worktree_modes` point at `worktrees`. [`migrate`] turns
    // the foreign keys off around it and checks them before the commit.
    format!(
        "{table}
INSERT INTO worktrees_v5 ({columns}) SELECT {columns} FROM worktrees;
DROP TABLE worktrees;
ALTER TABLE worktrees_v5 RENAME TO worktrees;"
    )
}

/// Version 6: the disk a parked worktree held when `horch worktree triage`
/// last measured it, so `ledger status` sums a stored value and never walks
/// a folder (#308).
const V6: &str = "ALTER TABLE worktrees ADD COLUMN bytes INTEGER;
             ALTER TABLE worktrees ADD COLUMN bytes_measured_at TEXT;";

/// Version 7: every field of the trace-fixes plan (docs/specs/trace-fixes.md
/// section 4), in 1 migration so no 2 tickets race on the version. The
/// ticket fields of a task (C1), the run check, PR decline and orchestrator
/// respawn of a run (C5, C7), the run gates (C5), the wake outbox (C3),
/// the handoff of a worker (C4) and the session records that replace the
/// JSON session ledger (D10, TRF-45). `held_reason` and `end_reason` stay free
/// text: their new words live in [`HeldReason`] and [`EndReason`].
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

/// The comment at each `pid_start` column of version 8.
const PID_START_TYPE: &str = "-- the design text says TEXT; INTEGER matches workers.agent_started and horch_platform::process::start_time";

/// The `fleetd` table: 1 row, written at the first start of fleetd.
fn fleetd_table() -> String {
    format!(
        "CREATE TABLE fleetd (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    generation INTEGER NOT NULL,
    pid INTEGER NOT NULL,
    {PID_START_TYPE}
    pid_start INTEGER NOT NULL CHECK (typeof(pid_start) = 'integer'),
    started_at TEXT NOT NULL,
    fleet_clock_s INTEGER NOT NULL,
    clock_ticked_at TEXT NOT NULL
);"
    )
}

/// The `attempts` table. `task_id` is null for an orchestrator attempt.
fn attempts_table() -> String {
    format!(
        "CREATE TABLE attempts (
    id TEXT PRIMARY KEY,
    task_id TEXT REFERENCES tasks(task_id),
    worker_id TEXT NOT NULL REFERENCES workers(worker_id),
    mux_handle TEXT,
    pid INTEGER,
    {PID_START_TYPE}
    pid_start INTEGER CHECK (pid_start IS NULL OR typeof(pid_start) = 'integer'),
    state TEXT NOT NULL CHECK (state IN ('launching', 'live', 'ended')),
    heartbeat_seq INTEGER,
    started_at TEXT,
    ended_at TEXT,
    exit_code INTEGER,
    outcome TEXT
);"
    )
}

/// The job kinds that the `kind` CHECK of `jobs` allows at version 8.
const V8_JOB_KINDS: &str = "'task_check', 'fast_gate', 'full_gate', 'trial_merge',
        'bundle_build', 'rebase', 'rescue', 'fetch', 'push',
        'worktree_add', 'worktree_remove', 'worktree_list', 'docs_sync', 'conformance'";

/// The job kind that the step of [`PHASE_5_VERSION`] adds to the `kind`
/// CHECK of `jobs`: the triage decisions that run git or gh (T-888).
const PHASE_5_JOB_KIND: &str = "'triage_apply'";

/// The `jobs` table and its index: at most 1 run of 1 kind and 1 subject
/// is queued or running.
fn jobs_table() -> String {
    format!(
        "CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ({V8_JOB_KINDS})),
    subject TEXT NOT NULL,
    repo TEXT,
    input_key TEXT NOT NULL,
    run INTEGER NOT NULL DEFAULT 1,
    abandoned_runs INTEGER NOT NULL DEFAULT 0,
    not_before INTEGER,
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'passed', 'failed', 'abandoned')),
    pid INTEGER,
    {PID_START_TYPE}
    pid_start INTEGER CHECK (pid_start IS NULL OR typeof(pid_start) = 'integer'),
    time_limit_s INTEGER NOT NULL,
    deadline INTEGER,
    result_json TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT,
    ended_at TEXT,
    UNIQUE (kind, subject, input_key, run)
);
CREATE UNIQUE INDEX jobs_one_active ON jobs (kind, subject) WHERE state IN ('queued', 'running');"
    )
}

/// The `pass_cursors` table.
fn pass_cursors_table() -> String {
    "CREATE TABLE pass_cursors (
    kind TEXT PRIMARY KEY,
    after_key TEXT,
    lap_started_at TEXT NOT NULL,
    prev_lap_started_at TEXT
);"
    .to_string()
}

/// The `needs_you` table.
fn needs_you_table() -> String {
    "CREATE TABLE needs_you (
    key TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    entity TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('open', 'resolved', 'superseded')),
    text TEXT NOT NULL,
    created_at TEXT NOT NULL,
    ended_at TEXT
);"
    .to_string()
}

/// `events` built again, as in `v5`, so its kind list has `transition`,
/// with the columns of a transition, null in each old row. The indexes go
/// with the old table and are made again after the rename.
fn events_v8() -> String {
    let old = "event_id, at, run_id, task_id, worker_id, actor, kind, text";
    let (table, indexes) = events_v8_ddl("events_v8");
    format!(
        "{table}
INSERT INTO events_v8 ({old}) SELECT {old} FROM events;
DROP TABLE events;
ALTER TABLE events_v8 RENAME TO events;
{indexes}"
    )
}

/// `events` as version 8 builds it, with the name `name` and the event
/// kinds of this binary: the table, then its indexes, which name `events`.
fn events_v8_ddl(name: &str) -> (String, String) {
    let table = events_table()
        .replace("CREATE TABLE events (", &format!("CREATE TABLE {name} ("))
        .replace(V1_EVENT_KIND, &EventKind::check("kind"))
        .replace(
            "\n    text TEXT NOT NULL DEFAULT ''\n);",
            "\n    text TEXT NOT NULL DEFAULT '',
    entity TEXT,
    entity_id TEXT,
    from_state TEXT,
    to_state TEXT,
    cause TEXT,
    data_json TEXT\n);",
        );
    let (table, indexes) = table.split_once("\n\n").expect("events has indexes");
    (table.to_string(), indexes.to_string())
}

/// Version [`EVENT_KINDS_VERSION`]: `events` built again, so its kind
/// `CHECK` is the list of this binary. A ledger keeps the kind list of the
/// binary that built its `events` table, and no later step built it again,
/// so a ledger at version 8 or more from an older binary refuses each
/// newer kind (`doorbell`, `hold_started`, `hold_ended`). A later step
/// that adds a kind runs it again with its own `version`
/// ([`PHASE_7_VERSION`]).
///
/// The new table comes from the text of the code ([`events_v8_ddl`]),
/// never from the stored text. Each row keeps its `event_id`, and the
/// `AUTOINCREMENT` counter keeps its value, so no id is used again. The
/// indexes go with the old table and are made again after the rename;
/// `events` has no trigger.
fn events_kinds(conn: &Connection, version: i64) -> Result<()> {
    let (table, indexes) = events_v8_ddl("events_kinds");
    conn.execute_batch(&table)?;
    let columns = column_names(conn, "events_kinds")?;
    if column_names(conn, "events")? != columns {
        bail!(
            "the events table does not have the columns of version {}",
            version - 1
        );
    }
    let seq: Option<i64> = conn
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'events'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    conn.execute_batch(&format!(
        "INSERT INTO events_kinds ({columns}) SELECT {columns} FROM events;
DROP TABLE events;
ALTER TABLE events_kinds RENAME TO events;
{indexes}",
        columns = columns.join(", "),
    ))?;
    if let Some(seq) = seq {
        conn.execute_batch(&format!(
            "DELETE FROM sqlite_sequence WHERE name = 'events';
INSERT INTO sqlite_sequence (name, seq) VALUES ('events', {seq});"
        ))?;
    }
    Ok(())
}

/// `worktrees` as version [`WORKTREE_PATHS_VERSION`] builds it, with the
/// name `name`: the table, then its index, which names `worktrees`. The
/// columns are those of [`v5`], [`V6`] and [`phase_3`], in that order, and
/// `path` is unique only among the rows that are not `removed`.
fn worktrees_paths_ddl(name: &str) -> (String, String) {
    let table = worktrees_table()
        .replace(
            "CREATE TABLE worktrees (",
            &format!("CREATE TABLE {name} ("),
        )
        .replace(
            "\n    path TEXT NOT NULL UNIQUE,",
            "\n    path TEXT NOT NULL,",
        )
        .replace(V1_WORKTREE_STATUS, &WorktreeStatus::check("status"))
        .replace(
            "\n    reason TEXT\n);",
            &format!(
                "\n    reason TEXT,
    archive_ref TEXT,
    decision TEXT {decision},
    decided_at TEXT,
    bytes_freed INTEGER,
    bytes INTEGER,
    bytes_measured_at TEXT,
    state TEXT NOT NULL DEFAULT 'orphaned' {WORKTREE_STATE},
    owner_attempt TEXT REFERENCES attempts(id),
    head_sha TEXT,
    dirty INTEGER NOT NULL DEFAULT 0,
    rescue_ref TEXT,
    seen_at TEXT\n);",
                decision = TriageDecision::check("decision")
            ),
        );
    let index =
        "CREATE UNIQUE INDEX worktrees_live_path ON worktrees (path) WHERE state <> 'removed';"
            .to_string();
    (table, index)
}

/// Version [`WORKTREE_PATHS_VERSION`]: `worktrees` built again, so 1 path
/// is unique only among the rows that are not `removed`. A removed row
/// keeps its path as history, and a restore makes a new row at that path
/// (fleetd-p3 ticket 13).
///
/// A row that today's triage `archived` or `trashed` has no folder: triage
/// writes those words only after the removal. The Phase 3 backfill made it
/// `orphaned`; this step makes it `removed`, from the rows alone, so the
/// path is free for a restore.
///
/// The new table comes from the text of the code
/// ([`worktrees_paths_ddl`]), never from the stored text. Each row keeps
/// each of its columns. `worktrees` has no index and no trigger of its
/// own before this step. The rows of `worktree_modes` point at
/// `worktrees`: [`migrate`] turns the foreign keys off around it and checks
/// them before the commit.
fn worktree_paths(conn: &Connection) -> Result<()> {
    let (table, index) = worktrees_paths_ddl("worktrees_paths");
    conn.execute_batch(&table)?;
    let columns = column_names(conn, "worktrees_paths")?;
    if column_names(conn, "worktrees")? != columns {
        bail!(
            "the worktrees table does not have the columns of version {}",
            WORKTREE_PATHS_VERSION - 1
        );
    }
    conn.execute_batch(&format!(
        "INSERT INTO worktrees_paths ({columns}) SELECT {columns} FROM worktrees;
DROP TABLE worktrees;
ALTER TABLE worktrees_paths RENAME TO worktrees;
UPDATE worktrees SET state = 'removed'
    WHERE status IN ('{archived}', '{trashed}') AND state <> 'removed';
{index}",
        columns = columns.join(", "),
        archived = WorktreeStatus::Archived,
        trashed = WorktreeStatus::Trashed,
    ))?;
    Ok(())
}

/// 1 live attempt, `<task id>/A<attempt>`, for each live worker with a task.
fn attempts_backfill() -> String {
    let live: Vec<String> = WorkerStatus::ALL
        .iter()
        .filter(|s| s.is_live())
        .map(|s| format!("'{s}'"))
        .collect();
    format!(
        "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at)
    SELECT task_id || '/A' || attempt, task_id, worker_id, agent_pid, agent_started, 'live',
        COALESCE(claimed_at, started_at, spawned_at)
    FROM workers
    WHERE status IN ({live}) AND task_id IS NOT NULL AND attempt IS NOT NULL;",
        live = live.join(", "),
    )
}

/// Version 8: the Phase 1 tables of fleetd (docs/specs/fleetd/design.md,
/// "Ledger"; docs/tickets/fleetd-p1/MAP.md, Q5 to Q7), `events` with the
/// columns of a transition, and the attempts of the live workers.
fn v8() -> String {
    [
        fleetd_table(),
        attempts_table(),
        jobs_table(),
        pass_cursors_table(),
        needs_you_table(),
        events_v8(),
        attempts_backfill(),
    ]
    .join("\n")
}

/// The `leases` table: at most 1 lease for each task (RR-2, FM-14).
/// `expires_at` is a deadline on the fleet clock (seconds).
fn leases_table() -> String {
    "CREATE TABLE leases (
    task_id TEXT PRIMARY KEY REFERENCES tasks(task_id),
    attempt_id TEXT NOT NULL REFERENCES attempts(id),
    expires_at INTEGER NOT NULL,
    renewed_at TEXT NOT NULL,
    renew_basis TEXT NOT NULL
);"
    .to_string()
}

/// The `questions` table. `feature_id` has no reference: the `features`
/// table is Phase 4, and before then it holds the run id (MAP.md, Q13).
fn questions_table() -> String {
    "CREATE TABLE questions (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(task_id),
    feature_id TEXT NOT NULL,
    owner TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('open', 'answered', 'escalated')),
    answer_case TEXT CHECK (answer_case IN ('covered', 'revise', 'replan', 'own_decision')),
    citation TEXT,
    due_at INTEGER NOT NULL,
    asked_at TEXT NOT NULL,
    answered_at TEXT
);"
    .to_string()
}

/// The new columns of Phase 2. Each `INTEGER` time is on the fleet clock:
/// the end of a retry backoff (`not_before`), the time limit of the current
/// state (`tasks.due_at`), the last activity of an attempt (`active_at`,
/// MAP.md Q7) and the cap `max_wait_s` of a wait (`worker_waits.due_at`).
/// `cancel_requested` holds a cancel that waits for the stop of a live
/// attempt (MAP.md, Q11).
const PHASE_2_COLUMNS: &str = "ALTER TABLE tasks ADD COLUMN not_before INTEGER;
ALTER TABLE tasks ADD COLUMN due_at INTEGER;
ALTER TABLE tasks ADD COLUMN replans INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN cancel_requested TEXT;
ALTER TABLE attempts ADD COLUMN active_at INTEGER;
ALTER TABLE worker_waits ADD COLUMN due_at INTEGER;
ALTER TABLE worker_waits ADD COLUMN question_id TEXT;";

/// The indexes of the watchdog duties: the tasks and the workers of 1 run
/// in 1 status, for the whole-run counts (T-322); the newest check of 1
/// worker (`watchdog::prev`) and the workers of 1 task in attempt order
/// (`workers::attempts_at`), for the per-row reads (T-443).
const PHASE_2_INDEXES: &str = "CREATE INDEX tasks_by_run_status ON tasks(run_id, status);
CREATE INDEX workers_by_run_status ON workers(run_id, status);
CREATE INDEX checks_by_worker ON checks(worker_id, check_id);
CREATE INDEX workers_by_task ON workers(task_id, attempt, claimed_at);";

/// The design state of each task row of today, in SQL (MAP.md, Q4). The
/// words of today are the frozen words of [`V1_TASK_STATUS`]. A dependency
/// is met when its task is done, as in `tasks::blockers`.
fn task_state_of_today() -> String {
    format!(
        "CASE
        WHEN status = 'queued' AND held_reason IS NOT NULL THEN {escalated}
        WHEN status = 'queued' AND EXISTS (SELECT 1 FROM task_deps d
            JOIN tasks b ON b.task_id = d.blocked_by
            WHERE d.task_id = tasks.task_id AND b.status <> 'done') THEN {pending}
        WHEN status = 'queued' THEN {ready}
        WHEN status = 'landing' THEN {verifying}
        ELSE status END",
        escalated = word(TaskStatus::Escalated),
        pending = word(TaskStatus::Pending),
        ready = word(TaskStatus::Ready),
        verifying = word(TaskStatus::Verifying),
    )
}

/// The columns that the steps after version 1 add to `tasks`, as their
/// `ALTER TABLE` statements write them: [`V3`], [`v7`] and
/// [`PHASE_2_COLUMNS`].
fn tasks_added_columns() -> Vec<String> {
    vec![
        "held_reason TEXT".into(),
        "attempt_floor INTEGER NOT NULL DEFAULT 0".into(),
        format!("work_kind TEXT {}", WorkKind::check("work_kind")),
        format!("review_mode TEXT {}", ReviewMode::check("review_mode")),
        "ticket_snapshot TEXT".into(),
        "ticket_sha TEXT".into(),
        "check_none INTEGER NOT NULL DEFAULT 0 CHECK (check_none IN (0, 1))".into(),
        "not_before INTEGER".into(),
        "due_at INTEGER".into(),
        "replans INTEGER NOT NULL DEFAULT 0".into(),
        "cancel_requested TEXT".into(),
    ]
}

/// The column names of `table`, in order.
fn column_names(conn: &Connection, table: &str) -> Result<Vec<String>> {
    Ok(conn
        .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")?
        .query_map([table], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

/// `tasks` built again, as `events` in [`v8`], so its status `CHECK` is
/// the list of the 12 states, and each row moves to its design state. The
/// new table comes from the text of the code: [`tasks_table`] and
/// [`tasks_added_columns`], never from the stored text. [`migrate`] turns
/// the foreign keys off around it and checks them before the commit.
fn tasks_12_states(conn: &Connection) -> Result<()> {
    let mut sql = vec![tasks_table()
        .replacen("CREATE TABLE tasks (", "CREATE TABLE tasks_p2 (", 1)
        .replace(V1_TASK_STATUS, &TaskStatus::check("status"))];
    sql.extend(
        tasks_added_columns()
            .iter()
            .map(|c| format!("ALTER TABLE tasks_p2 ADD COLUMN {c};")),
    );
    conn.execute_batch(&sql.join("\n"))?;
    let columns = column_names(conn, "tasks_p2")?;
    if column_names(conn, "tasks")? != columns {
        bail!(
            "the tasks table does not have the columns of version {}",
            PHASE_2_VERSION - 1
        );
    }
    let values: Vec<String> = columns
        .iter()
        .map(|c| match c.as_str() {
            "status" => task_state_of_today(),
            other => other.to_string(),
        })
        .collect();
    conn.execute_batch(&format!(
        "INSERT INTO tasks_p2 (rowid, {columns}) SELECT rowid, {values} FROM tasks;
DROP TABLE tasks;
ALTER TABLE tasks_p2 RENAME TO tasks;",
        columns = columns.join(", "),
        values = values.join(", "),
    ))?;
    Ok(())
}

/// Version [`PHASE_2_VERSION`]: the Phase 2 tables and columns of fleetd
/// (docs/specs/fleetd/design.md, "Ledger" and "Migration plan" item 4).
/// [`phase_2_backfill`] fills them for a run in flight. `tasks` is built
/// again by [`tasks_12_states`] between the columns and the indexes.
fn phase_2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        &[
            leases_table(),
            questions_table(),
            PHASE_2_COLUMNS.to_string(),
        ]
        .join("\n"),
    )?;
    tasks_12_states(conn)?;
    conn.execute_batch(PHASE_2_INDEXES)?;
    Ok(())
}

/// The default `liveness.lease_s` of the design: the lease of each held
/// task at the release.
const BACKFILL_LEASE_S: i64 = 120;

/// The default `liveness.max_wait_s` of the design: the cap of each open
/// wait at the release. Each persona of Phase 2 is interactive (MAP.md, Q10).
const BACKFILL_MAX_WAIT_S: i64 = 3600;

/// The fleet clock of the `fleetd` row. With no row it is 0: fleetd starts
/// the clock at 0 when it writes its first row.
const FLEET_CLOCK: &str = "COALESCE((SELECT fleet_clock_s FROM fleetd WHERE id = 1), 0)";

/// The release backfill of Phase 2 (MAP.md, Q21), with `now` as
/// `renewed_at`:
/// - each running task with a live attempt gets 1 lease of
///   [`BACKFILL_LEASE_S`]. With 2 live attempts, the lease goes to the
///   attempt of the task's current worker, else to the newest.
/// - each live attempt is active now, so no attempt is judged idle for the
///   time before the update.
/// - each wait gets the cap [`BACKFILL_MAX_WAIT_S`].
fn phase_2_backfill(conn: &Connection, now: &str) -> Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO leases (task_id, attempt_id, expires_at, renewed_at, renew_basis)
    SELECT task_id, id, {FLEET_CLOCK} + ?1, ?2, 'backfill'
    FROM (SELECT a.task_id, a.id, row_number() OVER (PARTITION BY a.task_id
                ORDER BY a.worker_id IS t.current_worker_id DESC, a.started_at DESC, a.rowid DESC) AS n
            FROM attempts a JOIN tasks t ON t.task_id = a.task_id
            WHERE a.state = 'live' AND t.status = '{running}')
    WHERE n = 1",
            running = TaskStatus::Running,
        ),
        rusqlite::params![BACKFILL_LEASE_S, now],
    )?;
    conn.execute(
        &format!("UPDATE attempts SET active_at = {FLEET_CLOCK} WHERE state = 'live'"),
        [],
    )?;
    conn.execute(
        &format!("UPDATE worker_waits SET due_at = {FLEET_CLOCK} + ?1"),
        [BACKFILL_MAX_WAIT_S],
    )?;
    Ok(())
}

/// The state of a worktree in the design ("Worktrees: leased, reconciled,
/// never lost"), beside `status`, which keeps today's words (MAP.md of
/// fleetd-p3, Q3).
const WORKTREE_STATE: &str = "CHECK (state IN ('reserved', 'created', 'active', 'landing', 'landed', 'removed', 'orphaned', 'rescued'))";

/// Version [`PHASE_3_VERSION`]: the state, owner and rescue columns of
/// `worktrees` (docs/specs/fleetd/design.md, "Ledger" and "Migration plan"
/// item 5; MAP.md of fleetd-p3, Q2). `base_commit` is the design's
/// `base_sha` and `bytes_on_disk` its `disk_bytes`.
///
/// The default `orphaned` of `state` is a bridge for the writers that do
/// not give a state yet, and is never the state of a row at the update
/// ([`phase_3_backfill`] sets each one). The ticket that makes each of
/// them write the state:
/// - `worktree_ops::create`: ticket 08.
/// - `finish::insert_worktree`: ticket 08.
/// - `runs.rs`, the run gate folder: ticket 12.
/// - `triage::apply::adopt`: ticket 13.
/// - `import.rs`, `insert_worktree` of an imported tree: no ticket.
///
/// An orphaned tree is rescued, and a rescue never removes a tree.
fn phase_3() -> String {
    format!(
        "ALTER TABLE worktrees ADD COLUMN state TEXT NOT NULL DEFAULT 'orphaned' {WORKTREE_STATE};
ALTER TABLE worktrees ADD COLUMN owner_attempt TEXT REFERENCES attempts(id);
ALTER TABLE worktrees ADD COLUMN head_sha TEXT;
ALTER TABLE worktrees ADD COLUMN dirty INTEGER NOT NULL DEFAULT 0;
ALTER TABLE worktrees ADD COLUMN rescue_ref TEXT;
ALTER TABLE worktrees ADD COLUMN seen_at TEXT;"
    )
}

/// The release backfill of Phase 3 (MAP.md of fleetd-p3, Q19), from the
/// rows alone: it reads no process and no folder, and the first pass of
/// fleetd checks each fact again.
/// - a `cleaned` tree is `removed`.
/// - a tree whose task has a `live` attempt of the worker of the row is
///   `active`, with that attempt as owner. A scratch tree has no task: the
///   `live` attempt of its worker holds it. With 2, the newest holds it.
/// - each other tree is `orphaned`, with no owner.
fn phase_3_backfill() -> String {
    format!(
        "UPDATE worktrees SET owner_attempt = (
        SELECT a.id FROM attempts a
        WHERE a.state = 'live' AND a.worker_id = worktrees.worker_id
            AND (a.task_id = worktrees.task_id OR EXISTS (SELECT 1 FROM worktree_modes m
                WHERE m.worktree_id = worktrees.worktree_id AND m.scratch = 1))
        ORDER BY a.started_at DESC, a.rowid DESC LIMIT 1)
    WHERE status <> '{cleaned}';
UPDATE worktrees SET state = CASE
        WHEN status = '{cleaned}' THEN 'removed'
        WHEN owner_attempt IS NOT NULL THEN 'active'
        ELSE 'orphaned' END;",
        cleaned = WorktreeStatus::Cleaned,
    )
}

/// The bundle states that are not ended: a task branch is a member of at
/// most 1 bundle in these states (I12).
const OPEN_BUNDLE: &str = "('building', 'gating', 'passed')";

/// The Phase 4 tables of fleetd (docs/specs/fleetd/design.md, "Ledger").
/// The `branches` row of a feature branch has no task and no landing state.
fn integration_tables() -> String {
    "CREATE TABLE features (
    id TEXT PRIMARY KEY,
    name TEXT UNIQUE,
    branch TEXT UNIQUE,
    primary_branch TEXT NOT NULL,
    stacked_on TEXT REFERENCES features(id),
    state TEXT NOT NULL CHECK (state IN ('open', 'ready', 'merged', 'abandoned')),
    pr TEXT,
    push_state TEXT CHECK (push_state IN ('ok', 'failed')),
    push_failures INTEGER NOT NULL DEFAULT 0,
    push_not_before INTEGER,
    pr_state TEXT NOT NULL DEFAULT 'none' CHECK (pr_state IN ('none', 'open', 'closed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE branches (
    name TEXT PRIMARY KEY,
    feature_id TEXT NOT NULL REFERENCES features(id),
    task_id TEXT REFERENCES tasks(task_id),
    landing_state TEXT CHECK (landing_state IN ('building', 'verifying', 'ready', 'in_bundle',
        'landed', 'conflicted', 'gate_failed', 'abandoned')),
    owner TEXT NOT NULL,
    due_at INTEGER NOT NULL,
    queue_seq INTEGER,
    repair_attempts INTEGER NOT NULL DEFAULT 0,
    rescue_ref TEXT,
    seen_tip TEXT,
    seen_remote_tip TEXT,
    orphan INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE bundles (
    id INTEGER PRIMARY KEY,
    feature_id TEXT NOT NULL REFERENCES features(id),
    base_tip TEXT NOT NULL,
    candidate_ref TEXT,
    candidate_tip TEXT,
    parent_id INTEGER REFERENCES bundles(id),
    state TEXT NOT NULL CHECK (state IN ('building', 'gating', 'passed', 'landed', 'failed', 'dropped')),
    job_id INTEGER REFERENCES jobs(id),
    due_at INTEGER NOT NULL,
    result TEXT CHECK (result IN ('pass', 'fail')),
    at TEXT NOT NULL
);
CREATE TABLE bundle_members (
    bundle_id INTEGER NOT NULL REFERENCES bundles(id),
    branch TEXT NOT NULL REFERENCES branches(name),
    position INTEGER NOT NULL,
    PRIMARY KEY (bundle_id, branch)
);
CREATE INDEX bundle_members_by_branch ON bundle_members (branch);
CREATE TABLE gate_runs (
    id INTEGER PRIMARY KEY,
    feature_id TEXT NOT NULL REFERENCES features(id),
    gate TEXT NOT NULL CHECK (gate IN ('fast', 'full')),
    tip TEXT NOT NULL,
    base_tip TEXT NOT NULL,
    job_id INTEGER NOT NULL REFERENCES jobs(id),
    result TEXT CHECK (result IN ('pass', 'fail')),
    at TEXT NOT NULL
);
CREATE TABLE remote_tips (
    repo TEXT NOT NULL,
    branch TEXT NOT NULL,
    tip TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    stale_since TEXT,
    PRIMARY KEY (repo, branch)
);
CREATE TABLE ingested (
    path TEXT PRIMARY KEY,
    sha256 TEXT,
    log_offset INTEGER,
    outcome TEXT NOT NULL CHECK (outcome IN ('accepted', 'rejected')),
    reason TEXT,
    at TEXT NOT NULL
);"
    .to_string()
}

/// I12, second part, in SQL, so that no writer can break it: a task branch
/// is a member of at most 1 bundle that is not ended. A membership that is
/// added or moved into an open bundle, and an ended bundle that opens
/// again, are refused when 1 of its branches is in another open bundle.
fn one_open_bundle_triggers() -> String {
    let refuse = "SELECT RAISE(ABORT, 'I12: a task branch is a member of at most 1 bundle that is not ended');";
    // `skip` leaves out the row that an update moves.
    let member_rule = |skip: &str| {
        format!(
            "(SELECT state FROM bundles WHERE id = NEW.bundle_id) IN {OPEN_BUNDLE}
    AND EXISTS (SELECT 1 FROM bundle_members m JOIN bundles b ON b.id = m.bundle_id
        WHERE m.branch = NEW.branch AND m.bundle_id <> NEW.bundle_id{skip}
            AND b.state IN {OPEN_BUNDLE})"
        )
    };
    format!(
        "CREATE TRIGGER bundle_members_one_open_insert BEFORE INSERT ON bundle_members
WHEN {insert}
BEGIN {refuse} END;
CREATE TRIGGER bundle_members_one_open_update BEFORE UPDATE OF bundle_id, branch ON bundle_members
WHEN {update}
BEGIN {refuse} END;
CREATE TRIGGER bundles_one_open_reopen BEFORE UPDATE OF state ON bundles
WHEN NEW.state IN {OPEN_BUNDLE} AND OLD.state NOT IN {OPEN_BUNDLE}
    AND EXISTS (SELECT 1 FROM bundle_members m
        JOIN bundle_members o ON o.branch = m.branch AND o.bundle_id <> m.bundle_id
        JOIN bundles b ON b.id = o.bundle_id
        WHERE m.bundle_id = NEW.id AND b.state IN {OPEN_BUNDLE})
BEGIN {refuse} END;",
        insert = member_rule(""),
        update = member_rule(" AND m.rowid <> OLD.rowid"),
    )
}

/// Version [`PHASE_4_VERSION`]: the Phase 4 tables of fleetd, the I12
/// triggers, how an attempt began (`attempts.via`, MAP.md of fleetd-p4,
/// Q4: an attempt of today's claim path is `claim`), and the index of the
/// reader `needs_you::open_keys` (T-263). [`phase_4_backfill`] fills the
/// tables for a run in flight.
fn phase_4() -> String {
    [
        integration_tables(),
        one_open_bundle_triggers(),
        "ALTER TABLE attempts ADD COLUMN via TEXT NOT NULL DEFAULT 'claim' CHECK (via IN ('claim', 'push'));
CREATE INDEX needs_you_by_kind_state ON needs_you (kind, state);"
            .to_string(),
    ]
    .join("\n")
}

/// The time limit of each backfilled `branches` row: the default
/// `max_minutes` of a profile in the design, 60 min.
const BACKFILL_TASK_LIMIT_S: i64 = 3600;

/// The release backfill of Phase 4 (design, "What a release does to a run
/// in flight"; MAP.md of fleetd-p4, Q5), from the rows alone: it reads no
/// git and no process, and the first pass of fleetd checks each fact again.
///
/// Each run with a run branch and a base branch gets 1 feature row:
/// - `id` is `F-<run rowid>`, 4 digits, as `F-0003`: `runs` has no seq
///   column, and its rowid is the insert order.
/// - `name` is the run slug, and `branch` the run branch. Both are unique
///   in the design. When a newer run has the same slug or branch, the older
///   run gets the name `<slug>-<rowid>` and no branch.
/// - `state` is `merged` for a `done` run, `abandoned` for an `abandoned`
///   run, else `open`.
///
/// Each feature with a branch gets the `branches` row of that branch,
/// owned by fleetd. Each task of a run with a feature row gets the
/// `branches` row of its newest worktree row, owned by its worker:
/// `landed` when that tree is `landed` or the task is done with a landed
/// commit, else `building`. Each row is due [`BACKFILL_TASK_LIMIT_S`]
/// after the fleet clock now.
fn phase_4_backfill(conn: &Connection, now: &str) -> Result<()> {
    conn.execute(
        &format!(
            "WITH f AS (SELECT rowid AS n, slug, run_branch, base_branch, status, started_at
        FROM runs WHERE run_branch IS NOT NULL AND base_branch IS NOT NULL)
INSERT INTO features (id, name, branch, primary_branch, state, created_at, updated_at)
    SELECT printf('F-%04d', n),
        CASE WHEN EXISTS (SELECT 1 FROM f g WHERE g.slug = f.slug AND g.n > f.n)
            THEN slug || '-' || n ELSE slug END,
        CASE WHEN EXISTS (SELECT 1 FROM f g WHERE g.run_branch = f.run_branch AND g.n > f.n)
            THEN NULL ELSE run_branch END,
        base_branch,
        CASE status WHEN '{done}' THEN 'merged' WHEN '{abandoned}' THEN 'abandoned' ELSE 'open' END,
        started_at, ?1
    FROM f",
            done = RunStatus::Done,
            abandoned = RunStatus::Abandoned,
        ),
        [now],
    )?;
    conn.execute(
        &format!(
            "INSERT INTO branches (name, feature_id, owner, due_at)
    SELECT branch, id, 'fleetd', {FLEET_CLOCK} + ?1 FROM features WHERE branch IS NOT NULL"
        ),
        [BACKFILL_TASK_LIMIT_S],
    )?;
    conn.execute(
        &format!(
            "INSERT INTO branches (name, feature_id, task_id, landing_state, owner, due_at)
    SELECT branch, printf('F-%04d', run_n), task_id,
        CASE WHEN wt_status = '{landed}' OR (task_status = '{task_done}' AND landed_commit IS NOT NULL)
            THEN 'landed' ELSE 'building' END,
        'worker', {FLEET_CLOCK} + ?1
    FROM (SELECT w.branch, w.status AS wt_status, w.landed_commit, t.task_id,
                t.status AS task_status, r.rowid AS run_n,
                row_number() OVER (PARTITION BY t.task_id
                    ORDER BY w.created_at DESC, w.rowid DESC) AS k
            FROM worktrees w JOIN tasks t ON t.task_id = w.task_id
            JOIN runs r ON r.run_id = t.run_id
            JOIN features f ON f.id = printf('F-%04d', r.rowid))
    WHERE k = 1",
            landed = WorktreeStatus::Landed,
            task_done = TaskStatus::Done,
        ),
        [BACKFILL_TASK_LIMIT_S],
    )?;
    Ok(())
}

/// Version [`PHASE_5_VERSION`]: the Phase 5 tables of fleetd
/// (docs/specs/fleetd/design.md, "Ledger"). `requests` has 1 row for each
/// command request (RR-8), with the class of its kind and how fleetd
/// authorized it (SR-8). `polite_stops` has 1 row for each process that got
/// a polite stop, at the fleet clock (MAP.md of fleetd-p5, Q12). The release
/// backfill of Phase 5 is empty: both tables start empty.
fn phase_5() -> String {
    format!(
        "CREATE TABLE requests (
    request_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    payload_sha256 TEXT NOT NULL,
    caller TEXT NOT NULL,
    class TEXT NOT NULL CHECK (class IN ('operator','worker')),
    auth TEXT,
    state TEXT NOT NULL CHECK (state IN ('pending','applied','refused')),
    result_json TEXT,
    reason TEXT,
    received_at TEXT NOT NULL,
    decided_at TEXT
);
CREATE TABLE polite_stops (
    pid INTEGER NOT NULL,
    {PID_START_TYPE}
    pid_start INTEGER NOT NULL CHECK (typeof(pid_start) = 'integer'),
    at_s INTEGER NOT NULL,
    pane TEXT,
    PRIMARY KEY (pid, pid_start)
);"
    )
}

/// The step of [`PHASE_5_VERSION`] builds `jobs` again, so its kind `CHECK`
/// also allows [`PHASE_5_JOB_KIND`]. The new table comes from the text of
/// the code ([`jobs_table`]), never from the stored text. Each row keeps
/// each of its columns and its `id`. The index goes with the old table and
/// is made again after the rename; `jobs` has no trigger. The rows of
/// `bundles` and `gate_runs` point at `jobs`: [`migrate`] turns the
/// foreign keys off around it and checks them before the commit.
fn phase_5_jobs(conn: &Connection) -> Result<()> {
    let ddl = jobs_table()
        .replace("CREATE TABLE jobs (", "CREATE TABLE jobs_kinds (")
        .replace(V8_JOB_KINDS, &format!("{V8_JOB_KINDS}, {PHASE_5_JOB_KIND}"));
    let (table, index) = ddl
        .split_once("\nCREATE UNIQUE INDEX")
        .expect("jobs has its index");
    conn.execute_batch(table)?;
    let columns = column_names(conn, "jobs_kinds")?;
    if column_names(conn, "jobs")? != columns {
        bail!(
            "the jobs table does not have the columns of version {}",
            PHASE_5_VERSION - 1
        );
    }
    conn.execute_batch(&format!(
        "INSERT INTO jobs_kinds ({columns}) SELECT {columns} FROM jobs;
DROP TABLE jobs;
ALTER TABLE jobs_kinds RENAME TO jobs;
CREATE UNIQUE INDEX{index}",
        columns = columns.join(", "),
    ))?;
    Ok(())
}

/// Version [`PHASE_6_VERSION`]: the Phase 6 tables of fleetd, as the design
/// writes them (docs/specs/fleetd/design.md, "Ledger"), the idea of each
/// task (FR-12) and of each sub-orchestrator attempt, and the fix rounds of
/// a judged task (RR-3) and the task as its plan commits it (`spec_json`).
/// [`phase_6_backfill`] gives each run in flight its idea, and
/// [`tasks_need_an_idea`] then refuses a task with no idea.
const PHASE_6: &str = "CREATE TABLE ideas (
    id TEXT PRIMARY KEY,
    feature_id TEXT NOT NULL REFERENCES features(id),
    state TEXT NOT NULL CHECK (state IN
        ('captured', 'clarifying', 'awaiting_approval', 'specifying',
         'ticketing', 'in_progress', 'integrating', 'merged',
         'parked', 'cancelled')),
    priority INTEGER NOT NULL DEFAULT 0,
    pane TEXT,
    waiting_on_you_since TEXT,
    approved_sha256 TEXT,
    approval_sha256 TEXT,
    plan_sha256 TEXT,
    phase_attempts INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
CREATE TABLE requirements (
    idea_id TEXT REFERENCES ideas(id), req_id TEXT, text TEXT,
    PRIMARY KEY (idea_id, req_id)
);
CREATE TABLE task_covers (task_id TEXT, idea_id TEXT, req_id TEXT);
CREATE TABLE trial_merges (
    left_ref TEXT, right_ref TEXT, left_tip TEXT, right_tip TEXT,
    job_id INTEGER NOT NULL REFERENCES jobs(id),
    clean INTEGER, conflict_files TEXT, at TEXT NOT NULL,
    PRIMARY KEY (left_tip, right_tip)
);
ALTER TABLE tasks ADD COLUMN idea_id TEXT REFERENCES ideas(id);
ALTER TABLE tasks ADD COLUMN fix_rounds INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN spec_json TEXT;
ALTER TABLE attempts ADD COLUMN idea_id TEXT REFERENCES ideas(id);";

/// FR-12 in SQL: SQLite cannot add a `NOT NULL` column with no default, so
/// this trigger refuses a new task with no idea. An imported row keeps its
/// history (section 11.2).
fn tasks_need_an_idea() -> String {
    format!(
        "CREATE TRIGGER tasks_need_an_idea BEFORE INSERT ON tasks
WHEN NEW.idea_id IS NULL AND NEW.origin <> '{import}'
BEGIN SELECT RAISE(ABORT, 'a task needs an idea (FR-12)'); END;",
        import = Origin::Import,
    )
}

/// The marker in `ideas.approved_sha256` of the idea that the release gives
/// a run (MAP.md of fleetd-p6, Q6): the first pass of fleetd writes the
/// `approval.json` of the run's `MAP.md`.
const RELEASE_APPROVAL: &str = "release";

/// The release backfill of Phase 6 (design, "What a release does to a run
/// in flight"; MAP.md of fleetd-p6, Q5): [`run_idea`] for each run, oldest
/// first, so a feature name that a run takes is taken for the next one. It
/// reads no file and starts no process.
fn phase_6_backfill(conn: &Connection, now: &str) -> Result<()> {
    let runs: Vec<String> = conn
        .prepare("SELECT run_id FROM runs ORDER BY rowid")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for run in runs {
        run_idea(conn, &run, now, false)?;
    }
    Ok(())
}

/// The idea of the run `run_id`, `I-<run rowid>` (4 digits, as the feature
/// `F-<run rowid>` of Phase 4), written when it does not exist. `adding` is
/// true when the caller adds a task to the run. The release backfill uses
/// it, so a run of today's flow has 1 idea. `tasks::add` writes no idea
/// (ticket 47 of fleetd-p6).
///
/// - A run with no feature row gets one only when it has, or gets, a task
///   that is not done and not cancelled; else it gets no idea (`None`). The
///   new feature follows the rule of `phase_4_backfill` for 1 run: `name`
///   is the slug, or `<slug>-<rowid>` when a feature has that name;
///   `branch` is the run branch when no feature and no branch row has it,
///   with its `branches` row owned by fleetd; `primary_branch` is the base
///   branch, or `main` for a run that names none.
/// - The idea is `in_progress` when the run has, or gets, a task that is
///   not done and not cancelled, else `integrating`. Its `approved_sha256`
///   is `RELEASE_APPROVAL`.
/// - Each task of the run with no idea gets it. Each question of the run
///   moves to the feature, owned by `idea-lead:<feature id>` (MAP.md, Q11).
pub fn run_idea(
    conn: &Connection,
    run_id: &str,
    now: &str,
    adding: bool,
) -> Result<Option<String>> {
    let open_task = format!(
        "EXISTS (SELECT 1 FROM tasks t WHERE t.run_id = runs.run_id AND t.status NOT IN ({done}, {cancelled}))",
        done = word(TaskStatus::Done),
        cancelled = word(TaskStatus::Cancelled),
    );
    let (n, open): (i64, bool) = conn
        .query_row(
            &format!("SELECT rowid, {open_task} FROM runs WHERE run_id = ?1"),
            [run_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .with_context(|| format!("the task ledger has no run {run_id}"))?;
    let open = open || adding;
    let feature = format!("F-{n:04}");
    let idea = format!("I-{n:04}");
    let exists =
        |sql: &str, id: &str| -> Result<bool> { Ok(conn.query_row(sql, [id], |r| r.get(0))?) };
    if exists("SELECT EXISTS (SELECT 1 FROM ideas WHERE id = ?1)", &idea)? {
        return Ok(Some(idea));
    }
    if !exists(
        "SELECT EXISTS (SELECT 1 FROM features WHERE id = ?1)",
        &feature,
    )? {
        if !open {
            return Ok(None);
        }
        conn.execute(
            &format!(
                "INSERT INTO features (id, name, branch, primary_branch, state, created_at, updated_at)
    SELECT ?2,
        CASE WHEN NOT EXISTS (SELECT 1 FROM features WHERE name = slug) THEN slug
            WHEN NOT EXISTS (SELECT 1 FROM features WHERE name = slug || '-' || rowid)
                THEN slug || '-' || rowid END,
        CASE WHEN NOT EXISTS (SELECT 1 FROM features WHERE branch = run_branch)
            AND NOT EXISTS (SELECT 1 FROM branches WHERE name = run_branch) THEN run_branch END,
        COALESCE(base_branch, 'main'),
        CASE status WHEN '{done}' THEN 'merged' WHEN '{abandoned}' THEN 'abandoned' ELSE 'open' END,
        started_at, ?3
    FROM runs WHERE run_id = ?1",
                done = RunStatus::Done,
                abandoned = RunStatus::Abandoned,
            ),
            [run_id, &feature, now],
        )?;
        conn.execute(
            &format!(
                "INSERT INTO branches (name, feature_id, owner, due_at)
    SELECT branch, id, 'fleetd', {FLEET_CLOCK} + ?2 FROM features WHERE id = ?1 AND branch IS NOT NULL"
            ),
            rusqlite::params![feature, BACKFILL_TASK_LIMIT_S],
        )?;
    }
    conn.execute(
        "INSERT INTO ideas (id, feature_id, state, approved_sha256, created_at, updated_at)
    SELECT ?2, ?3, ?4, ?5, started_at, ?6 FROM runs WHERE run_id = ?1",
        rusqlite::params![
            run_id,
            idea,
            feature,
            if open { "in_progress" } else { "integrating" },
            RELEASE_APPROVAL,
            now
        ],
    )?;
    conn.execute(
        "UPDATE tasks SET idea_id = ?2 WHERE run_id = ?1 AND idea_id IS NULL",
        [run_id, &idea],
    )?;
    conn.execute(
        "UPDATE questions SET feature_id = ?2, owner = 'idea-lead:' || ?2
    WHERE task_id IN (SELECT task_id FROM tasks WHERE run_id = ?1)",
        [run_id, &feature],
    )?;
    Ok(Some(idea))
}

/// Bring the database to [`VERSION`]. A file from a newer binary is refused:
/// this one would not know its columns.
///
/// Foreign keys are off while it runs: a table that is built again
/// (`v5`) is dropped while other rows point at it. They are checked before
/// the commit, and turned back on after it.
pub fn migrate(conn: &mut Connection) -> Result<()> {
    migrate_guarded(conn, &|_| Ok(()))
}

/// [`migrate`], but an existing file (version 1 or more) is migrated only
/// when `guard(version)` is `Ok`, and only after a backup (`backup`).
pub fn migrate_guarded(conn: &mut Connection, guard: &dyn Fn(i64) -> Result<()>) -> Result<()> {
    migrate_guarded_to(conn, guard, VERSION)
}

/// [`migrate`], but only up to `target`: a test builds an older file with
/// the real steps, then migrates it.
pub fn migrate_to(conn: &mut Connection, target: i64) -> Result<()> {
    if !(1..=VERSION).contains(&target) {
        bail!("the task ledger has no schema version {target}");
    }
    migrate_guarded_to(conn, &|_| Ok(()), target)
}

fn migrate_guarded_to(
    conn: &mut Connection,
    guard: &dyn Fn(i64) -> Result<()>,
    target: i64,
) -> Result<()> {
    // The pragma is a no-op inside a transaction, so it is set before one.
    let keys: bool = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    conn.pragma_update(None, "foreign_keys", false)?;
    let migrated = migrate_unchecked(conn, guard, target);
    conn.pragma_update(None, "foreign_keys", keys)?;
    migrated
}

/// The file of `conn`, or `None` for an in-memory database.
fn file_of(conn: &Connection) -> Option<PathBuf> {
    conn.path().filter(|p| !p.is_empty()).map(PathBuf::from)
}

/// How many backups `backup` keeps in 1 folder.
pub const BACKUPS_KEPT: usize = 3;

/// The backups in `dir`, `ledger.v<n>-backup-<stamp>.sqlite3`, oldest
/// name first.
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

/// Copy the ledger `file` at version `from` to
/// `<dir>/ledger.v<from>-backup-<UTC yyyymmddTHHMMSSZ>.sqlite3`, then keep
/// the newest [`BACKUPS_KEPT`]. The caller holds the write lock, so the copy
/// is the state the migration starts from. `VACUUM INTO` cannot run inside a
/// transaction, so it runs on a second connection.
fn backup(file: &Path, from: i64) -> Result<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new("."));
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let mut copy = dir.join(format!("ledger.v{from}-backup-{stamp}.sqlite3"));
    // 2 migrations in 1 second: the second copy gets a number.
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

/// The error for a file at version `current` above [`VERSION`]: 1 sentence
/// and 1 instruction line.
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

fn migrate_unchecked(
    conn: &mut Connection,
    guard: &dyn Fn(i64) -> Result<()>,
    target: i64,
) -> Result<()> {
    // The write lock first, then the version: 2 processes that open a fresh
    // file at once must not both create the tables.
    let file = file_of(conn);
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let current: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current > VERSION {
        return Err(newer_schema(file.as_deref(), current));
    }
    if current >= target {
        return Ok(());
    }
    if current > 0 {
        guard(current)?;
        if let Some(file) = &file {
            backup(file, current)?;
        }
    }
    // A step runs when the file is below its version and `target` is not.
    let due = |version: i64| current < version && version <= target;
    if due(1) {
        tx.execute_batch(&v1())
            .context("creating the task ledger tables")?;
    }
    for (version, sql) in [(2, V2), (3, V3), (4, V4)] {
        if due(version) {
            tx.execute_batch(sql)?;
        }
    }
    if due(5) {
        tx.execute_batch(&v5())
            .context("adding the worktree triage columns")?;
        no_broken_reference(&tx, 5)?;
    }
    if due(6) {
        tx.execute_batch(V6)?;
    }
    if due(7) {
        tx.execute_batch(&v7())
            .context("adding the trace-fixes columns and tables")?;
    }
    if due(PHASE_1_VERSION) {
        tx.execute_batch(&v8())
            .context("adding the fleetd tables and the attempts of live workers")?;
        no_broken_reference(&tx, PHASE_1_VERSION)?;
    }
    if due(PHASE_2_VERSION) {
        phase_2(&tx)
            .context("adding the leases, the questions and the Phase 2 columns of fleetd")?;
        phase_2_backfill(&tx, &super::util::now())
            .context("backfilling the leases and the wait limits of a run in flight")?;
        no_broken_reference(&tx, PHASE_2_VERSION)?;
    }
    if due(PHASE_3_VERSION) {
        tx.execute_batch(&phase_3())
            .context("adding the state, owner and rescue columns of worktrees")?;
        tx.execute_batch(&phase_3_backfill())
            .context("backfilling the state and the owner of each worktree")?;
        no_broken_reference(&tx, PHASE_3_VERSION)?;
    }
    if due(EVENT_KINDS_VERSION) {
        events_kinds(&tx, EVENT_KINDS_VERSION)
            .context("building events again with the event kinds of this binary")?;
    }
    if due(WORKTREE_PATHS_VERSION) {
        worktree_paths(&tx).context("building worktrees again with 1 live row for each path")?;
        no_broken_reference(&tx, WORKTREE_PATHS_VERSION)?;
    }
    if due(PHASE_4_VERSION) {
        tx.execute_batch(&phase_4())
            .context("adding the features, branches, bundles and other Phase 4 tables of fleetd")?;
        phase_4_backfill(&tx, &super::util::now())
            .context("backfilling the feature and the branches of each run in flight")?;
        no_broken_reference(&tx, PHASE_4_VERSION)?;
    }
    if due(PHASE_5_VERSION) {
        tx.execute_batch(&phase_5())
            .context("adding the requests and polite_stops tables of fleetd")?;
        phase_5_jobs(&tx).context("building jobs again with the triage_apply kind")?;
        no_broken_reference(&tx, PHASE_5_VERSION)?;
    }
    if due(PHASE_6_VERSION) {
        tx.execute_batch(PHASE_6).context(
            "adding the ideas, requirements, task_covers and trial_merges tables of fleetd",
        )?;
        phase_6_backfill(&tx, &super::util::now())
            .context("backfilling the idea of each run in flight")?;
        tx.execute_batch(&tasks_need_an_idea())?;
        no_broken_reference(&tx, PHASE_6_VERSION)?;
    }
    if due(PHASE_7_VERSION) {
        // The release row of Phase 7: "Ledger: None". The step changes only
        // the words that the kind `CHECK` allows, and no row.
        events_kinds(&tx, PHASE_7_VERSION)
            .context("building events again with the event words of the health of a profile")?;
    }
    if due(PROJECT_ROOTS_VERSION) {
        project_roots(&tx).context(
            "rewriting each project root in the plain form and merging 2 rows of 1 root",
        )?;
        no_broken_reference(&tx, PROJECT_ROOTS_VERSION)?;
    }
    tx.pragma_update(None, "user_version", target)?;
    tx.commit()?;
    Ok(())
}

/// Version [`PROJECT_ROOTS_VERSION`]: each `projects.root` in the spelling
/// of [`super::runs::root_key`]. An older Windows horch stored a verbatim
/// root (`\\?\C:\repo`), and `root_key` now gives the plain form
/// (`C:\repo`), so a lookup missed the old project and made a second row
/// (D266-F2). 2 rows with 1 root merge onto the older row: each reference to
/// the newer id, in each table with a foreign key to `projects`, moves to
/// the older id, and the newer row goes. A second run changes nothing.
fn project_roots(conn: &Connection) -> Result<()> {
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT project_id, root FROM projects ORDER BY created_at, project_id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut kept: std::collections::HashMap<String, String> = Default::default();
    let mut merges = Vec::new();
    let mut rewrites = Vec::new();
    for (id, root) in rows {
        let key = super::runs::stored_root_key(&root);
        if let Some(older) = kept.get(&key) {
            merges.push((id, older.clone()));
            continue;
        }
        if key != root {
            rewrites.push((id.clone(), key.clone()));
        }
        kept.insert(key, id);
    }
    if merges.is_empty() && rewrites.is_empty() {
        return Ok(());
    }
    let references: Vec<(String, String)> = conn
        .prepare(
            "SELECT m.name, f.\"from\" FROM sqlite_master m, pragma_foreign_key_list(m.name) f
             WHERE m.type = 'table' AND f.\"table\" = 'projects' ORDER BY m.name",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (newer, older) in &merges {
        for (table, column) in &references {
            conn.execute(
                &format!("UPDATE \"{table}\" SET \"{column}\" = ?1 WHERE \"{column}\" = ?2"),
                [older, newer],
            )?;
        }
        conn.execute("DELETE FROM projects WHERE project_id = ?1", [newer])?;
    }
    for (id, key) in &rewrites {
        conn.execute(
            "UPDATE projects SET root = ?1 WHERE project_id = ?2",
            [key, id],
        )?;
    }
    Ok(())
}

/// Fail when the migration step `version` left a row whose reference
/// names no row. Foreign keys are off while a migration runs.
fn no_broken_reference(conn: &Connection, version: i64) -> Result<()> {
    let broken: i64 = conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
        r.get(0)
    })?;
    if broken > 0 {
        bail!("the version {version} migration left {broken} rows with a broken reference");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FNV-1a, so the pin below needs no copy of the 250-line text.
    fn fnv1a(text: &str) -> u64 {
        text.bytes().fold(0xcbf29ce484222325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        })
    }

    /// Version 1 runs as a migration on existing files: its text must not drift.
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
        assert_eq!(tables, 38);
    }

    /// A ledger file as version 3 left it, with 1 run.
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
        // A second run of the migration changes nothing.
        migrate(&mut conn).unwrap();
    }
}

#[cfg(test)]
mod v5_tests {
    use super::*;

    /// A ledger file as version 4 left it, with 1 worktree and its mode row.
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
        // The store opens with foreign keys on, as every real open does.
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
        // The mode row still points at the rebuilt table, and the key holds.
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
        // A second open migrates nothing.
        crate::taskledger::Store::open(&path).unwrap();
    }
}

#[cfg(test)]
mod v6_tests {
    use super::*;

    /// A ledger file as version 5 left it, with 1 parked worktree.
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
        // A second open migrates nothing and keeps the measurement.
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

    /// A ledger file as version 6 left it, with rows in every table.
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

    /// Every table, with its columns and rows, ordered so 2 dumps compare.
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
        // The Phase 2 step moves the held `t1` from `queued` to `escalated`.
        let mut before = before;
        let (_, columns, rows) = before.iter_mut().find(|(t, _, _)| t == "tasks").unwrap();
        let status = columns.iter().position(|c| c == "status").unwrap();
        for row in rows.iter_mut() {
            if row[status] == Value::Text("queued".into()) {
                row[status] = Value::Text(TaskStatus::Escalated.as_str().into());
            }
        }
        assert_eq!(dump(conn, Some(&before)), before, "no other row changed");

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

        // The new words are the enums' words, and the CHECK lists hold.
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
        // A second open migrates nothing and keeps the new rows.
        let store = crate::taskledger::Store::open(&path).unwrap();
        let n: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    /// A ledger file at `path` that the real steps 1..=`version` built, with
    /// 1 project row.
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
        ];
        // The Phase 2 step reads the stored text of `tasks`, so it runs on
        // the connection.
        assert_eq!(steps.len() as i64 + 9, VERSION, "add the new step here");
        for sql in steps.iter().take(version as usize) {
            conn.execute_batch(sql).unwrap();
        }
        if version >= PHASE_2_VERSION {
            phase_2(&conn).unwrap();
        }
        if version >= PHASE_3_VERSION {
            conn.execute_batch(&phase_3()).unwrap();
        }
        if version >= EVENT_KINDS_VERSION {
            events_kinds(&conn, EVENT_KINDS_VERSION).unwrap();
        }
        if version >= WORKTREE_PATHS_VERSION {
            worktree_paths(&conn).unwrap();
        }
        if version >= PHASE_4_VERSION {
            conn.execute_batch(&phase_4()).unwrap();
        }
        if version >= PHASE_5_VERSION {
            conn.execute_batch(&phase_5()).unwrap();
            phase_5_jobs(&conn).unwrap();
        }
        if version >= PHASE_6_VERSION {
            conn.execute_batch(PHASE_6).unwrap();
            conn.execute_batch(&tasks_need_an_idea()).unwrap();
        }
        if version >= PHASE_7_VERSION {
            events_kinds(&conn, PHASE_7_VERSION).unwrap();
        }
        if version >= PROJECT_ROOTS_VERSION {
            project_roots(&conn).unwrap();
        }
        conn.execute("INSERT INTO projects VALUES ('p', '/r', 'now')", [])
            .unwrap();
        conn.pragma_update(None, "user_version", version).unwrap();
    }

    /// D266-F2: a ledger of an older Windows horch at version
    /// [`PHASE_7_VERSION`], with the verbatim root of 1 project and a second
    /// row in the plain form that the upgraded horch made.
    fn verbatim_root_file(path: &std::path::Path) {
        file_at(path, PHASE_7_VERSION);
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(&format!(
            r"INSERT INTO projects VALUES ('old', '\\?\C:\repo', '2026-01-01T00:00:00Z');
             INSERT INTO projects VALUES ('new', 'C:\repo', '2026-06-01T00:00:00Z');
             INSERT INTO projects VALUES ('unc', '\\?\UNC\server\share\repo', '2026-01-02T00:00:00Z');
             INSERT INTO runs (run_id, project_id, slug, status, started_at)
                 VALUES ('r-old', 'old', 'build', 'active', '2026-01-01T00:00:01Z');
             INSERT INTO runs (run_id, project_id, slug, status, started_at)
                 VALUES ('r-new', 'new', 'other', 'active', '2026-06-01T00:00:01Z');
             INSERT INTO import_findings (project_id, kind, detail, created_at)
                 VALUES ('new', {kind}, 'd', '2026-06-01T00:00:02Z');",
            kind = word(FindingKind::EndedWithoutDone),
        ))
        .unwrap();
    }

    /// Every project row, as (id, root), by id.
    fn project_rows(conn: &Connection) -> Vec<(String, String)> {
        conn.prepare("SELECT project_id, root FROM projects ORDER BY project_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn d266_f2_a_verbatim_windows_root_migrates_to_1_project_with_its_old_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        verbatim_root_file(&path);
        let store = crate::taskledger::Store::open(&path).unwrap();
        let conn = store.conn();
        assert_eq!(
            project_rows(conn),
            [
                ("old".to_string(), r"C:\repo".to_string()),
                ("p".to_string(), "/r".to_string()),
                ("unc".to_string(), r"\\server\share\repo".to_string()),
            ]
        );
        let root = std::path::Path::new(r"C:\repo");
        let runs = super::super::runs::of_project(conn, root).unwrap();
        let ids: Vec<(&str, &str, &str)> = runs
            .iter()
            .map(|r| (r.run_id.as_str(), r.project_id.as_str(), r.status.as_str()))
            .collect();
        assert_eq!(
            ids,
            [("r-new", "old", "active"), ("r-old", "old", "active")]
        );
        assert_eq!(
            super::super::runs::ensure_project(conn, root).unwrap(),
            "old"
        );
        let finding: String = conn
            .query_row("SELECT project_id FROM import_findings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(finding, "old");
        let broken: i64 = conn
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(broken, 0);
    }

    #[test]
    fn d266_f2_the_project_roots_step_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        verbatim_root_file(&path);
        let conn = Connection::open(&path).unwrap();
        project_roots(&conn).unwrap();
        let once = project_rows(&conn);
        let runs_once: Vec<(String, String)> = conn
            .prepare("SELECT run_id, project_id FROM runs ORDER BY run_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        project_roots(&conn).unwrap();
        assert_eq!(project_rows(&conn), once);
        let runs_twice: Vec<(String, String)> = conn
            .prepare("SELECT run_id, project_id FROM runs ORDER BY run_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(runs_twice, runs_once);
    }

    /// Every table, index and trigger with its SQL text.
    fn schema_text(conn: &Connection) -> Vec<(String, String, Option<String>)> {
        conn.prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
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

    fn tasks_sql(conn: &Connection) -> String {
        conn.query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'tasks'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    /// The Phase 2 step builds `tasks` with the same text as the steps
    /// before it, with only the status CHECK of the 12 states.
    #[test]
    fn tasks_12_states_builds_the_table_of_the_steps_with_the_12_states() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("l.sqlite3");
        file_at(&path, PHASE_2_VERSION - 1);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(PHASE_2_COLUMNS).unwrap();
        let before = tasks_sql(&conn);
        tasks_12_states(&conn).unwrap();
        let want = before
            .replacen("CREATE TABLE tasks (", "CREATE TABLE \"tasks\" (", 1)
            .replace(V1_TASK_STATUS, &TaskStatus::check("status"));
        assert_eq!(tasks_sql(&conn), want);
    }

    /// The Phase 2 step never runs the stored text of `tasks`: a ledger
    /// whose stored text was changed gets the table of the code.
    #[test]
    fn tasks_12_states_does_not_run_the_stored_table_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("l.sqlite3");
        file_at(&path, PHASE_2_VERSION - 1);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(PHASE_2_COLUMNS).unwrap();
        let clean = tasks_sql(&conn);
        conn.pragma_update(None, "writable_schema", true).unwrap();
        conn.execute(
            "UPDATE sqlite_master SET sql = replace(sql, 'summary TEXT,', 'summary TEXT DEFAULT ''x'',')
             WHERE type = 'table' AND name = 'tasks'",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "writable_schema", false).unwrap();
        drop(conn);
        let conn = Connection::open(&path).unwrap();
        assert!(tasks_sql(&conn).contains("DEFAULT 'x'"));
        tasks_12_states(&conn).unwrap();
        let want = clean
            .replacen("CREATE TABLE tasks (", "CREATE TABLE \"tasks\" (", 1)
            .replace(V1_TASK_STATUS, &TaskStatus::check("status"));
        assert_eq!(tasks_sql(&conn), want);
    }

    /// The event kinds that the newest `events` step, [`PHASE_7_VERSION`],
    /// writes in its `CHECK`. A new kind needs a new step: a ledger keeps
    /// the `CHECK` of the binary that built its `events` table.
    #[test]
    fn event_kinds_are_pinned_to_the_schema_version() {
        let words: Vec<&str> = EventKind::ALL.iter().map(|k| k.as_str()).collect();
        let pinned = [
            "run_created",
            "added",
            "claimed",
            "no_task",
            "progress",
            "note",
            "question",
            "blocked",
            "answered",
            "fallback",
            "step_started",
            "step_passed",
            "step_failed",
            "land_started",
            "land_red",
            "landed",
            "pr_updated",
            "cleaned",
            "cleanup_failed",
            "done",
            "unlanded",
            "reaped",
            "requeued",
            "stopped",
            "session",
            "exited",
            "cancelled",
            "verdict",
            "woke",
            "imported",
            "reopened",
            "dismissed",
            "attempt_started",
            "transition",
            "hold_started",
            "hold_ended",
            "doorbell",
            "wait_ended",
            "profile_unhealthy",
            "profile_healthy",
        ];
        assert_eq!(
            (PHASE_7_VERSION, words.as_slice()),
            (PHASE_6_VERSION + 1, pinned.as_slice()),
            "the event kinds changed: add a schema step that rebuilds the events CHECK, \
             then pin the new kinds to the version of that step"
        );
    }

    /// A ledger at the version before [`EVENT_KINDS_VERSION`] whose
    /// `events` table an older binary built: its `CHECK` has no `doorbell`.
    /// It holds `events` events.
    fn older_events_ledger(dir: &Path, events: i64) -> Connection {
        let mut conn = Connection::open(dir.join("ledger.sqlite3")).unwrap();
        migrate_to(&mut conn, EVENT_KINDS_VERSION - 1).unwrap();
        let (table, indexes) = events_v8_ddl("events_old");
        let older = table.replace(", 'doorbell'", "");
        assert!(!older.contains("doorbell"));
        conn.execute_batch(&format!(
            "{older}
DROP TABLE events;
ALTER TABLE events_old RENAME TO events;
{indexes}"
        ))
        .unwrap();
        let tx = conn.transaction().unwrap();
        for n in 0..events {
            tx.execute(
                "INSERT INTO events (at, task_id, worker_id, actor, kind, text, cause, data_json)
                 VALUES ('now', ?1, ?2, 'horch', 'note', ?3, 'c', '{}')",
                rusqlite::params![format!("t{}", n % 7), format!("w{}", n % 5), n.to_string()],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        conn
    }

    /// Every event row, in id order.
    fn all_events(conn: &Connection) -> Vec<Vec<rusqlite::types::Value>> {
        let mut stmt = conn
            .prepare("SELECT * FROM events ORDER BY event_id")
            .unwrap();
        let n = stmt.column_count();
        stmt.query_map([], |r| (0..n).map(|i| r.get(i)).collect())
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn insert_doorbell(conn: &Connection) -> rusqlite::Result<i64> {
        conn.execute(
            "INSERT INTO events (at, actor, kind, text) VALUES ('now', 'horch', 'doorbell', 'x')",
            [],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Insert the worktree `id` at `path` in `state`.
    fn insert_tree(
        conn: &Connection,
        id: &str,
        path: &str,
        state: &str,
    ) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, status, state, created_at)
             VALUES (?1, '/r', ?2, 'b', 'main', 'cleaned', ?3, 'now')",
            rusqlite::params![id, path, state],
        )
    }

    #[test]
    fn worktree_paths_permits_removed_rows_and_1_live_row_of_a_path() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        insert_tree(&conn, "w1", "/r/.worktrees/a", "removed").unwrap();
        insert_tree(&conn, "w2", "/r/.worktrees/a", "removed").unwrap();
        insert_tree(&conn, "w3", "/r/.worktrees/a", "active").unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM worktrees", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);
        let index: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'worktrees_live_path'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(index.contains("WHERE state <> 'removed'"), "{index}");
    }

    #[test]
    fn worktree_paths_refuses_2_live_rows_of_1_path() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        insert_tree(&conn, "w1", "/r/.worktrees/a", "orphaned").unwrap();
        let e = insert_tree(&conn, "w2", "/r/.worktrees/a", "reserved").unwrap_err();
        assert!(e.to_string().contains("UNIQUE"), "{e}");
        // A live row cannot take the path of a live row by an update.
        insert_tree(&conn, "w3", "/r/.worktrees/a", "removed").unwrap();
        assert!(conn
            .execute(
                "UPDATE worktrees SET state = 'active' WHERE worktree_id = 'w3'",
                []
            )
            .is_err());
    }

    #[test]
    fn worktree_paths_step_keeps_each_row_and_column() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.sqlite3");
        file_at(&path, WORKTREE_PATHS_VERSION - 1);
        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, status, created_at,
                 decision, archive_ref, bytes, bytes_measured_at, state, head_sha, dirty, rescue_ref, seen_at)
             VALUES ('w1', '/r', '/r/.worktrees/a', 'a', 'main', 'archived', 'then', 'ARCHIVE',
                 'refs/horch/archive/a', 7, 'now', 'removed', 'abc', 1, 'refs/horch/rescue/w1/1', 'now');
             INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, status, created_at, state)
             VALUES ('w2', '/r', '/r/.worktrees/b', 'b', 'main', 'open', 'then', 'active');
             INSERT INTO runs (run_id, project_id, slug, status, started_at) VALUES ('r1', 'p', 's', 'active', 'now');
             INSERT INTO worktree_modes (worktree_id, run_id, name, scratch) VALUES ('w2', 'r1', 'b', 0);",
        )
        .unwrap();
        let rows = |conn: &Connection| -> Vec<Vec<Option<String>>> {
            let cols = column_names(conn, "worktrees").unwrap();
            let sql = format!(
                "SELECT {} FROM worktrees ORDER BY worktree_id",
                cols.iter()
                    .map(|c| format!("CAST({c} AS TEXT)"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let mut stmt = conn.prepare(&sql).unwrap();
            stmt.query_map([], |r| {
                (0..cols.len())
                    .map(|i| r.get(i))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
        };
        // A tree that today's triage archived has no folder: it is removed.
        conn.execute_batch(
            "INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, status, created_at, state)
             VALUES ('w4', '/r', '/r/.worktrees/c', 'c', 'main', 'trashed', 'then', 'orphaned');",
        )
        .unwrap();
        let mut before = rows(&conn);
        let columns = column_names(&conn, "worktrees").unwrap();
        let state = columns.iter().position(|c| c == "state").unwrap();
        before[2][state] = Some("removed".into());
        migrate(&mut conn).unwrap();
        assert_eq!(rows(&conn), before);
        assert_eq!(column_names(&conn, "worktrees").unwrap(), columns);
        let modes: i64 = conn
            .query_row("SELECT count(*) FROM worktree_modes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(modes, 1);
        // The new row of a restore takes the path of a removed row.
        insert_tree(&conn, "w3", "/r/.worktrees/a", "reserved").unwrap();
        insert_tree(&conn, "w5", "/r/.worktrees/c", "reserved").unwrap();
    }

    #[test]
    fn events_kinds_step_takes_the_new_kinds_and_keeps_each_row_and_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = older_events_ledger(dir.path(), 10_000);
        assert!(
            insert_doorbell(&conn).is_err(),
            "the older CHECK refuses doorbell"
        );
        // The newest event is deleted: its id is never used again.
        conn.execute("DELETE FROM events WHERE event_id = 10000", [])
            .unwrap();
        let before = all_events(&conn);
        assert_eq!(before.len(), 9_999);
        migrate_to(&mut conn, EVENT_KINDS_VERSION).unwrap();
        assert_eq!(all_events(&conn), before);
        assert_eq!(insert_doorbell(&conn).unwrap(), 10_001);
        let indexes: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = 'events'
                 ORDER BY name",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(indexes, ["events_by_task", "events_by_worker"]);
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, EVENT_KINDS_VERSION);
    }

    #[test]
    fn events_kinds_step_does_not_run_the_stored_table_text() {
        let dir = tempfile::tempdir().unwrap();
        let conn = older_events_ledger(dir.path(), 0);
        conn.execute_batch(
            "PRAGMA writable_schema = ON;
UPDATE sqlite_master SET sql = replace(sql, 'data_json TEXT', 'data_json TEXT DEFAULT 1')
    WHERE name = 'events';
PRAGMA writable_schema = OFF;",
        )
        .unwrap();
        drop(conn);
        let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
        let stored = |conn: &Connection| -> String {
            conn.query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'events'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert!(stored(&conn).contains("DEFAULT 1"));
        migrate(&mut conn).unwrap();
        let sql = stored(&conn);
        assert!(!sql.contains("DEFAULT 1"), "{sql}");
        assert!(sql.contains("'doorbell'"), "{sql}");
    }
}
