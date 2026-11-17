//! The task ledger's tables (docs/specs/task-ledger.md section 4 and 11).
//!
//! The schema is built from the enums in [`super::model`], so each `CHECK`
//! list is the code's own list. `PRAGMA user_version` holds the version;
//! [`migrate`] brings an older file forward and never goes back.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use super::model::*;

/// The version of the Phase 1 tables of fleetd (`v8`). A test that
/// tests that step builds or migrates to this version, not to [`VERSION`].
pub const PHASE_1_VERSION: i64 = 8;

/// The version of the Phase 2 tables and columns of fleetd (`phase_2`).
pub const PHASE_2_VERSION: i64 = PHASE_1_VERSION + 1;

/// The schema version this binary writes.
pub const VERSION: i64 = PHASE_2_VERSION;

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
        task_status = TaskStatus::check("status"),
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

/// The `jobs` table and its index: at most 1 run of 1 kind and 1 subject
/// is queued or running.
fn jobs_table() -> String {
    format!(
        "CREATE TABLE jobs (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('task_check', 'fast_gate', 'full_gate', 'trial_merge',
        'bundle_build', 'rebase', 'rescue', 'fetch', 'push',
        'worktree_add', 'worktree_remove', 'worktree_list', 'docs_sync', 'conformance')),
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
    let table = events_table()
        .replace("CREATE TABLE events (", "CREATE TABLE events_v8 (")
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
    format!(
        "{table}
INSERT INTO events_v8 ({old}) SELECT {old} FROM events;
DROP TABLE events;
ALTER TABLE events_v8 RENAME TO events;
{indexes}"
    )
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

/// The indexes of the count queries of the whole-run watchdog duties
/// (T-322): the tasks and the workers of 1 run in 1 status.
const PHASE_2_INDEXES: &str = "CREATE INDEX tasks_by_run_status ON tasks(run_id, status);
CREATE INDEX workers_by_run_status ON workers(run_id, status);";

/// Version [`PHASE_2_VERSION`]: the Phase 2 tables and columns of fleetd
/// (docs/specs/fleetd/design.md, "Ledger" and "Migration plan" item 4).
/// [`phase_2_backfill`] fills them for a run in flight.
fn phase_2() -> String {
    [
        leases_table(),
        questions_table(),
        PHASE_2_COLUMNS.to_string(),
        PHASE_2_INDEXES.to_string(),
    ]
    .join("\n")
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
        tx.execute_batch(&phase_2())
            .context("adding the leases, the questions and the Phase 2 columns of fleetd")?;
        phase_2_backfill(&tx, &super::util::now())
            .context("backfilling the leases and the wait limits of a run in flight")?;
        no_broken_reference(&tx, PHASE_2_VERSION)?;
    }
    tx.pragma_update(None, "user_version", target)?;
    tx.commit()?;
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
        assert_eq!(tables, 25);
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
            phase_2(),
        ];
        assert_eq!(steps.len() as i64, VERSION, "add the new step here");
        for sql in &steps[..version as usize] {
            conn.execute_batch(sql).unwrap();
        }
        conn.execute("INSERT INTO projects VALUES ('p', '/r', 'now')", [])
            .unwrap();
        conn.pragma_update(None, "user_version", version).unwrap();
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
}
