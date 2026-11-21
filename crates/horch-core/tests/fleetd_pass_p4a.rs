//! The pass of Phase 4, part 1: dispatch, the launch of push attempts, the
//! outbox scan and its ingest, the verifier, the review steps and the
//! repair attempts in the reconcile pass
//! (docs/tickets/fleetd-p4/46-pass-wiring-dispatch-and-verifier.md).
//!
//! `docs/specs/fleetd/design.md`, "The reconcile pass", "Order inside 1
//! pass", "A pass has a work budget", PL-5, RR-4 and FM-11.
//!
//! Each test makes its own temporary git repository, ledger and fleet root,
//! and runs the passes as library calls with no herdr snapshot. No test
//! starts a `horch` binary: a launch appends 1 line to a file, and a job
//! process is a script whose result file the test writes. No test reads the
//! load, the limits or the clock of this machine: there is no health hold,
//! the numbers come from a test `FleetConfig`, and the time of a budget
//! comes from a test clock.
#![cfg(unix)]

use std::cell::Cell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use horch_core::fleetd::fleet_runtime::adapters::job_result_file::{self, JobResult};
use horch_core::fleetd::fleet_runtime::adapters::jobs_store;
use horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::pass::{
    NoDuties, Outside, Pass, PassReport, Ports, Settings, TaskWork, ABORT_AFTER_COMMIT,
    ABORT_BEFORE_COMMIT,
};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind as JobKind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::fleetd::task_execution::application::dispatch::{bind, DispatchCtx};
use horch_core::fleetd::task_execution::application::launch::{Launches, PushLaunch};
use horch_core::fleetd::task_execution::domain::result::{ResultFile, ResultStatus};
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane};
use horch_core::fleetd::worker_profiles::adapters::check_sandbox::WorkerSandboxes;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::runtime::fault::{Faults, ABORT_EXIT_CODE};
use horch_core::taskledger::finish::{CodexSandbox, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, StepName, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::supervisor::{HeartbeatFile, HEARTBEAT_JSON};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection, OptionalExtension};

/// The folder of the world of a child process of a crash test.
const CHILD: &str = "HORCH_P4A_CHILD";

// ─── the world ──────────────────────────────────────────────────────────

/// A temp git repository `project` on `main`, a ledger at generation 1, a
/// fleet root and 1 run whose verification profile is the check alone.
struct World {
    _temp: Option<tempfile::TempDir>,
    dir: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
    tools: Tools,
    sandboxes: WorkerSandboxes,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let mut conn = open(&dir.join("ledger.sqlite3"));
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let project = dir.join("project");
    fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-q", "-b", "main"]);
    git(&project, &["config", "user.email", "t@example.com"]);
    git(&project, &["config", "user.name", "t"]);
    fs::write(project.join("a.txt"), "a\n").unwrap();
    git(&project, &["add", "a.txt"]);
    git(&project, &["commit", "-q", "-m", "a"]);
    let new_run = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        finish_steps: vec![StepName::Check, StepName::Land],
        ..Default::default()
    };
    let run = runs::create(&conn, &project, &new_run).unwrap();
    // A task needs an idea in flight of the feature of its run (FR-12).
    schema::run_idea(&conn, &run.run_id, &util::now(), true).unwrap();
    fs::create_dir_all(dir.join("fleet")).unwrap();
    drop(conn);
    let mut w = World::reopen(&dir);
    w._temp = Some(temp);
    w
}

impl World {
    /// The world of the folder `dir` that [`world`] made.
    fn reopen(dir: &Path) -> World {
        let conn = open(&dir.join("ledger.sqlite3"));
        let run_id: String = conn
            .query_row("SELECT run_id FROM runs WHERE slug = 'r1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let run = runs::get(&conn, &run_id).unwrap();
        let tools = Tools {
            git: Git::new("git"),
            gh: dir.join("no-gh"),
            node: "/bin/sh".into(),
            complexity_cli: None,
            code_sweep_dir: None,
            evidence_root: dir.join("evidence"),
            shell: "/bin/sh".into(),
            reviewers_down: Vec::new(),
            codex: CodexSandbox::default(),
            jobs: None,
        };
        // Each persona has a check sandbox: the host facts do not decide a
        // test.
        let mut sandboxes = WorkerSandboxes::on_path(
            CodexSandbox::default(),
            dir.join("fleet"),
            std::env::var_os("PATH"),
        );
        sandboxes.gap = None;
        World {
            _temp: None,
            dir: dir.to_path_buf(),
            conn,
            run,
            config: FleetConfig::default(),
            tools,
            sandboxes,
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    fn launches_file(&self) -> PathBuf {
        self.dir.join("launches.txt")
    }

    fn settings(&self, faults: Faults) -> Settings {
        Settings {
            fleet_root: self.root(),
            generation: Generation(1),
            config: self.config.clone(),
            horch: self.job_program(),
            faults,
        }
    }

    /// The program of each job process: it runs for `JOB_S` seconds and
    /// writes no result file. The test writes the result file.
    fn job_program(&self) -> PathBuf {
        let path = self.dir.join("job.sh");
        if !path.exists() {
            fs::write(&path, "#!/bin/sh\nsleep 30\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    /// 1 ready task of the shared checkout with the check `true`.
    fn ready_task(&self, title: &str) -> String {
        let mut t = NewTask::work(title, &format!("{title}.md"), "dev");
        t.worktree_mode = WorktreeMode::Shared;
        t.land_check = Some("true".into());
        tasks::add(&self.conn, &self.run, &t, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    fn status(&self, task: &str) -> String {
        self.conn
            .query_row("SELECT status FROM tasks WHERE task_id = ?1", [task], |r| {
                r.get(0)
            })
            .unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// The push attempts of `task`: id and state, oldest first.
    fn attempts(&self, task: &str) -> Vec<(String, String)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, state FROM attempts WHERE task_id = ?1 AND via = 'push' ORDER BY rowid",
            )
            .unwrap();
        stmt.query_map([task], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// The 1 push attempt of `task` and its `A<n>`.
    fn attempt(&self, task: &str) -> (String, String) {
        let attempts = self.attempts(task);
        assert_eq!(attempts.len(), 1, "{attempts:?}");
        let id = attempts[0].0.clone();
        let n: i64 = self
            .conn
            .query_row(
                "SELECT w.attempt FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
                 WHERE a.id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        (id, format!("A{n}"))
    }

    fn launched(&self) -> Vec<String> {
        fs::read_to_string(self.launches_file())
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Wait until the launches file has `n` lines; the launch is a process.
    fn wait_launches(&self, n: usize) -> Vec<String> {
        let end = Instant::now() + Duration::from_secs(10);
        while self.launched().len() < n && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        self.launched()
    }

    /// The spawn of the attempt opened its pane, and the wrapper of the
    /// attempt writes its first heartbeat.
    fn heartbeat(&self, task: &str, attempt: &str, seq: u64) {
        self.conn
            .execute(
                "UPDATE workers SET pane_id = 'p-' || worker_id
                 WHERE worker_id = (SELECT worker_id FROM attempts
                     WHERE task_id = ?1 AND via = 'push' ORDER BY rowid DESC LIMIT 1)",
                [task],
            )
            .unwrap();
        let hb = HeartbeatFile {
            task: task.into(),
            attempt: attempt.into(),
            pid: None,
            pgid: None,
            mux_pane: None,
            proc_state: None,
            agent_state: None,
            last_output_at: None,
            tokens: None,
            seq,
            written_at: util::now(),
        };
        let path = attempt_dir(&self.root(), task, attempt).join(HEARTBEAT_JSON);
        write_0600(&path, serde_json::to_string(&hb).unwrap().as_bytes());
    }

    /// The agent of the attempt writes a valid `result.json`.
    fn result(&self, task: &str, attempt: &str) {
        let result = ResultFile {
            task: task.into(),
            attempt: attempt.into(),
            status: ResultStatus::Completed,
            summary: "done".into(),
            commits: Vec::new(),
            files_changed: Vec::new(),
            acceptance_self_check: Vec::new(),
            notes_for_next: None,
            handoff: None,
        };
        let outbox = attempt_dir(&self.root(), task, attempt).join("outbox");
        fs::create_dir_all(&outbox).unwrap();
        fs::set_permissions(&outbox, fs::Permissions::from_mode(0o700)).unwrap();
        write_0600(&outbox.join("result.json"), result.to_json().as_bytes());
    }

    /// The `task_check` runs of `task`.
    fn checks(&self, task: &str) -> Vec<jobs_store::JobRow> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM jobs WHERE kind = ?1 AND subject = ?2 ORDER BY id")
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map(params![JobKind::TaskCheck.as_str(), task], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        ids.into_iter()
            .map(|id| jobs_store::get(&self.conn, id).unwrap().unwrap())
            .collect()
    }

    /// The job process of the check of `task` ends: its result file.
    fn end_check(&self, task: &str, passed: bool) {
        let checks = self.checks(task);
        let row = checks.last().expect("a task_check run");
        assert_eq!(row.state, State::Running, "the pass started the job");
        let result = JobResult {
            job_id: row.id,
            run: row.run,
            passed,
            verdict: true,
            text: "`true`: exit 0".into(),
            data: None,
        };
        job_result_file::write(&self.root(), &result).unwrap();
    }
}

fn write_0600(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// The outside of the tests: no herdr snapshot and no health hold.
struct Fake;

impl Outside for Fake {
    fn panes(&self) -> Option<std::collections::BTreeMap<String, InPane>> {
        None
    }
    fn close_pane(&self, _pane: &str) -> Result<()> {
        Ok(())
    }
    fn holds(&self) -> Vec<Hold> {
        Vec::new()
    }
    fn worktree_folders(&self, _conn: &Connection) -> Result<Vec<FolderItem>> {
        Ok(Vec::new())
    }
    fn attempt_spec(&self, a: &AttemptRow) -> Result<LaunchSpec> {
        bail!("attempt {} is not a push attempt of this test", a.id)
    }
}

/// The spawn of a push attempt: 1 line with the attempt id in the
/// launches file.
struct Spawns {
    file: PathBuf,
}

impl Launches for Spawns {
    fn spec(&self, l: &PushLaunch) -> Result<LaunchSpec> {
        Ok(LaunchSpec {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                format!("echo {} >> '{}'", l.attempt, self.file.display()).into(),
            ],
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        })
    }
}

fn ports(fake: &Fake) -> Ports<'_> {
    Ports {
        outside: fake,
        duties: &NoDuties,
        needs_you: &NeedsYouList,
        host: &Launcher,
    }
}

fn pass<'a>(w: &'a World, spawns: &'a Spawns, faults: Faults) -> Pass<'a> {
    Pass::new(w.settings(faults), ports(&Fake)).with_task_work(TaskWork {
        tools: &w.tools,
        sandboxes: &w.sandboxes,
        launches: spawns,
    })
}

fn run(p: &mut Pass<'_>, conn: &mut Connection) -> PassReport {
    let report = p.run(conn).unwrap();
    assert!(report.heartbeat, "{report:?}");
    report
}

fn spawns(w: &World) -> Spawns {
    Spawns {
        file: w.launches_file(),
    }
}

// ─── bind and launch ────────────────────────────────────────────────────

/// A ready task is bound in part 2, and its task files and its launch
/// come after the commit, in part 3 of the same pass. The next pass does
/// not launch it again.
#[test]
fn pass_binds_a_ready_task_and_launches_it_after_the_commit() {
    let w = world();
    let task = w.ready_task("t1");
    let sp = spawns(&w);
    let mut conn = open(&w.dir.join("ledger.sqlite3"));
    let mut p = pass(&w, &sp, Faults::default());

    run(&mut p, &mut conn);

    assert_eq!(w.status(&task), "leased");
    let (id, n) = w.attempt(&task);
    assert_eq!(w.attempts(&task)[0].1, "launching");
    let dir = attempt_dir(&w.root(), &task, &n);
    assert!(dir.join("task.yaml").exists(), "task.yaml after the commit");
    assert!(
        dir.join("inbox").join("START.md").exists(),
        "START.md after the commit"
    );
    assert_eq!(w.wait_launches(1), vec![id.clone()], "1 launch");
    let dispatch: Option<String> = w
        .conn
        .query_row(
            "SELECT after_key FROM pass_cursors WHERE kind = 'dispatch'",
            [],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
        .flatten();
    assert_eq!(
        dispatch.as_deref(),
        Some(task.as_str()),
        "the dispatch cursor"
    );

    run(&mut p, &mut conn);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(w.launched(), vec![id], "no second launch");
}

/// A ready task goes through bind, launch, the first heartbeat, its
/// result, its check and the review steps to `done`, by passes alone.
#[test]
fn ready_task_reaches_done_by_passes_alone() {
    let w = world();
    let task = w.ready_task("t1");
    let sp = spawns(&w);
    let mut conn = open(&w.dir.join("ledger.sqlite3"));
    let mut p = pass(&w, &sp, Faults::default());

    run(&mut p, &mut conn);
    let (_, n) = w.attempt(&task);
    w.heartbeat(&task, &n, 1);
    run(&mut p, &mut conn);
    assert_eq!(w.status(&task), "running", "the first heartbeat");
    assert_eq!(w.attempts(&task)[0].1, "live");

    w.result(&task, &n);
    run(&mut p, &mut conn);
    assert_eq!(w.status(&task), "verifying", "the result");
    assert_eq!(w.checks(&task).len(), 1, "1 task_check run");

    w.end_check(&task, true);
    run(&mut p, &mut conn);
    assert_eq!(w.status(&task), "done", "the verifier moved it");
    let rows = w.count(&format!(
        "SELECT count(*) FROM events WHERE entity_id = '{task}' AND kind = 'verdict'"
    ));
    assert_eq!(rows, 1, "1 verdict row");
    let verdicts = w.root().join("tasks").join(&task);
    let files: Vec<_> = fs::read_dir(&verdicts)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("verdict"))
        .collect();
    assert_eq!(files.len(), 1, "1 verdict file");
}

// ─── order inside 1 pass ────────────────────────────────────────────────

/// A valid `result.json` of the pass wins over the expiry of the lease in
/// the same pass: the task goes to `verifying`, not `stalled`.
#[test]
fn result_in_the_pass_wins_over_the_lease_expiry() {
    let w = world();
    let task = w.ready_task("t1");
    let sp = spawns(&w);
    let mut conn = open(&w.dir.join("ledger.sqlite3"));
    let mut p = pass(&w, &sp, Faults::default());
    run(&mut p, &mut conn);
    let (_, n) = w.attempt(&task);
    w.heartbeat(&task, &n, 1);
    run(&mut p, &mut conn);
    assert_eq!(w.status(&task), "running");
    let leases = w.count(&format!(
        "SELECT count(*) FROM leases WHERE task_id = '{task}'"
    ));
    assert_eq!(leases, 1, "the attempt took the lease");

    // The lease is past its end at this pass, and the result is in.
    w.conn
        .execute(
            "UPDATE leases SET expires_at = 0 WHERE task_id = ?1",
            [&task],
        )
        .unwrap();
    w.result(&task, &n);
    run(&mut p, &mut conn);

    assert_eq!(w.status(&task), "verifying");
    let stalled = w.count(&format!(
        "SELECT count(*) FROM events WHERE entity_id = '{task}' AND to_state = 'stalled'"
    ));
    assert_eq!(stalled, 0, "no expiry moved the task");
}

// ─── PL-5 ───────────────────────────────────────────────────────────────

/// The pass queues the check as a `task_check` job, starts its process
/// after the commit, and returns while the job runs.
#[test]
fn pass_queues_the_check_and_does_not_wait() {
    let w = world();
    let task = w.ready_task("t1");
    let sp = spawns(&w);
    let mut conn = open(&w.dir.join("ledger.sqlite3"));
    let mut p = pass(&w, &sp, Faults::default());
    run(&mut p, &mut conn);
    let (_, n) = w.attempt(&task);
    w.heartbeat(&task, &n, 1);
    run(&mut p, &mut conn);
    w.result(&task, &n);

    // The job program runs for 30 s.
    let began = Instant::now();
    run(&mut p, &mut conn);
    assert!(
        began.elapsed() < Duration::from_secs(10),
        "the pass waited {:?}",
        began.elapsed()
    );
    let checks = w.checks(&task);
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].state, State::Running, "the job runs");
    assert_eq!(w.status(&task), "verifying", "the task waits for the job");

    // A pass while the job runs queues no second run.
    run(&mut p, &mut conn);
    assert_eq!(w.checks(&task).len(), 1);
    assert_eq!(w.status(&task), "verifying");
}

// ─── the outbox scan ────────────────────────────────────────────────────

/// The scan of outboxes reads at most its time budget in 1 pass. A new
/// fleetd continues at the cursor of the last: each outbox is read in
/// turn, and none is read twice.
#[test]
fn outbox_scan_continues_at_its_cursor_after_a_restart() {
    let mut w = world();
    // 6 bound attempts, each with a valid result. The 1 shared checkout
    // takes 1 task at a time, so the test binds them.
    let mut tasks = Vec::new();
    for i in 0..6 {
        let t = w.ready_task(&format!("t{i}"));
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &w.config,
                needs_you: &NeedsYouList,
            },
            clock: FleetSeconds(0),
            git: &w.tools.git,
        };
        let tx = w.conn.unchecked_transaction().unwrap();
        bind(&tx, &ctx, &t, "dev", &NewWorker::default()).unwrap();
        tx.commit().unwrap();
        tasks.push(t);
    }
    let sp = spawns(&w);
    let mut conn = open(&w.dir.join("ledger.sqlite3"));
    for t in &tasks {
        let (_, n) = w.attempt(t);
        w.result(t, &n);
    }

    // Each test clock step is 1 ms, and the scan budget is 3 ms: each scan
    // reads 2 items, then stops. File events are dropped, so only the
    // cursor gives the order.
    w.config.scan_budget_ms = 3;
    let ingested = |w: &World| w.count("SELECT count(*) FROM ingested");
    let stepper = || {
        let base = Instant::now();
        let n = Cell::new(0u64);
        Box::new(move || {
            n.set(n.get() + 1);
            base + Duration::from_millis(n.get())
        }) as Box<dyn FnMut() -> Instant>
    };
    let faults = Faults::parse(Some("drop-watch-events"));
    {
        let mut p = pass(&w, &sp, faults.clone()).with_clock(stepper());
        run(&mut p, &mut conn);
    }
    let first = ingested(&w);
    assert_eq!(first, 2, "the budget of 1 pass");
    let cursor: String = w
        .conn
        .query_row(
            "SELECT after_key FROM pass_cursors WHERE kind = 'outboxes'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    // A restart: a new fleetd goes on after the cursor.
    {
        let mut p = pass(&w, &sp, faults.clone()).with_clock(stepper());
        run(&mut p, &mut conn);
    }
    assert_eq!(ingested(&w), 4, "the new fleetd read 2 new outboxes");
    let after: String = w
        .conn
        .query_row(
            "SELECT after_key FROM pass_cursors WHERE kind = 'outboxes'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(after > cursor, "{after} after {cursor}");
    {
        let mut p = pass(&w, &sp, faults).with_clock(stepper());
        run(&mut p, &mut conn);
    }
    assert_eq!(ingested(&w), 6, "each outbox in turn");
}

// ─── horch ledger watch --once ──────────────────────────────────────────

/// `horch ledger watch --once` is 1 pass of a new fleetd: its start, then
/// 1 run. That 1 pass binds the ready task and launches it.
#[test]
fn watch_once_runs_the_phase4_pass() {
    let w = world();
    let task = w.ready_task("t1");
    let sp = spawns(&w);
    let mut conn = open(&w.dir.join("ledger.sqlite3"));
    let mut p = pass(&w, &sp, Faults::default());

    let failed = p.start(&mut conn).unwrap();
    assert!(failed.is_empty(), "{failed:?}");
    run(&mut p, &mut conn);

    assert_eq!(w.status(&task), "leased");
    let (id, _) = w.attempt(&task);
    assert_eq!(w.wait_launches(1), vec![id]);
}

// ─── crash points ───────────────────────────────────────────────────────

/// Not a test of its own: 1 pass of a child process of a crash test, with
/// the fault of `HORCH_FAULT`. It runs only with [`CHILD`].
#[test]
fn child_pass() {
    let Some(dir) = std::env::var_os(CHILD) else {
        return;
    };
    let w = World::reopen(Path::new(&dir));
    let faults = Faults::parse(std::env::var("HORCH_FAULT").ok().as_deref());
    let sp = spawns(&w);
    let mut conn = open(&w.dir.join("ledger.sqlite3"));
    let mut p = pass(&w, &sp, faults);
    let _ = p.run(&mut conn);
    panic!("the fault did not stop the child");
}

fn crash(w: &World, fault: &str) {
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_pass", "--nocapture", "--test-threads=1"])
        .env(CHILD, &w.dir)
        .env("HORCH_FAULT", fault)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(ABORT_EXIT_CODE), "the fault {fault}");
}

/// A crash before the commit of the bind leaves no row and no effect. The
/// next pass binds the task 1 time and launches it 1 time.
#[test]
fn crash_before_the_commit_of_the_bind_binds_and_launches_once_on_the_next_pass() {
    let w = world();
    let task = w.ready_task("t1");

    crash(&w, ABORT_BEFORE_COMMIT);
    assert_eq!(w.status(&task), "ready", "no row");
    assert!(w.attempts(&task).is_empty());
    assert!(w.launched().is_empty(), "no effect");

    let sp = spawns(&w);
    let mut conn = open(&w.dir.join("ledger.sqlite3"));
    let mut p = pass(&w, &sp, Faults::default());
    run(&mut p, &mut conn);
    let (id, n) = w.attempt(&task);
    assert!(attempt_dir(&w.root(), &task, &n).join("task.yaml").exists());
    assert_eq!(w.wait_launches(1), vec![id.clone()]);
    run(&mut p, &mut conn);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(w.launched(), vec![id], "1 launch");
}

/// A crash after the commit of the bind and before its effects leaves the
/// attempt in `launching` with no files and no launch. The next pass
/// writes its files and launches it 1 time, and binds no second attempt.
#[test]
fn crash_after_the_commit_of_the_bind_launches_once_on_the_next_pass() {
    let w = world();
    let task = w.ready_task("t1");

    crash(&w, ABORT_AFTER_COMMIT);
    assert_eq!(w.status(&task), "leased", "the row is committed");
    let (id, n) = w.attempt(&task);
    let dir = attempt_dir(&w.root(), &task, &n);
    assert!(
        !dir.join("task.yaml").exists(),
        "no effect before the crash"
    );
    assert!(w.launched().is_empty(), "no launch before the crash");

    let sp = spawns(&w);
    let mut conn = open(&w.dir.join("ledger.sqlite3"));
    let mut p = pass(&w, &sp, Faults::default());
    run(&mut p, &mut conn);
    assert!(
        dir.join("task.yaml").exists(),
        "the next pass wrote the files"
    );
    assert_eq!(w.wait_launches(1), vec![id.clone()]);
    run(&mut p, &mut conn);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(w.launched(), vec![id.clone()], "1 launch");
    assert_eq!(w.attempts(&task).len(), 1, "no second attempt");
}
