//! The reconcile pass in 3 parts: the facts, 1 transaction of rows, then
//! the effects and the heartbeat (docs/specs/fleetd/design.md, "The
//! reconcile pass"; docs/tickets/fleetd-p1/21-reconcile-pass.md).
//!
//! Each test uses a temp ledger and a temp fleet root, and runs the pass as
//! a library call with fake herdr facts. A test that needs a crash runs the
//! pass in a child process: this test binary again, with the test
//! [`child_pass`] and the variable [`CHILD`].

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::effects::{self, LaunchRecord};
use horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem;
use horch_core::fleetd::fleet_runtime::adapters::{jobs_store, polite_stops_store};
use horch_core::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use horch_core::fleetd::fleet_runtime::application::pass::{
    Done, NoDuties, Outside, Pass, PassReport, Ports, Settings, ABORT_AFTER_COMMIT, HANG_PASS,
};
use horch_core::fleetd::fleet_runtime::domain::clock::{AwakeReading, FleetSeconds};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::heartbeat::{is_hung, is_older, Exe, Heartbeat};
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::adapters::sqlite as attempts;
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, AttemptState, Recorded};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::{
    LaunchError, LaunchSpec, Launched, ProcessHost,
};
use horch_core::runtime::fault::{Faults, ABORT_EXIT_CODE};
use horch_core::taskledger::model::{Actor, WorkerKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::testing::bind_live;
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use horch_platform::signal::{stop_same, Stop as How};
use rusqlite::{params, Connection, TransactionBehavior};

/// The variable that makes [`child_pass`] run: the folder of the world.
const CHILD: &str = "FLEETD_PASS_TEST_CHILD";
/// What the child does: `abort` (1 pass) or `hang` (passes until it hangs).
const CHILD_MODE: &str = "FLEETD_PASS_TEST_MODE";

/// The id of the attempt in `launching` of the tests.
const LAUNCHING: &str = "orchestrator/A2";

// ─── the world ──────────────────────────────────────────────────────────

/// A temp ledger at generation 1, a fleet root and 1 run.
struct World {
    dir: PathBuf,
    _temp: Option<tempfile::TempDir>,
    conn: Connection,
    run: Run,
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let mut conn = open(&dir.join("ledger.sqlite3"));
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = runs::create(&conn, &dir, &new_run).unwrap();
    // FR-12: a task needs an idea in flight of the feature of its run.
    horch_core::taskledger::schema::run_idea(
        &conn,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("fleet")).unwrap();
    World {
        dir,
        _temp: Some(temp),
        conn,
        run,
    }
}

/// The world of the parent, opened again in the child.
fn reopen(dir: &Path) -> World {
    let conn = open(&dir.join("ledger.sqlite3"));
    let run_id: String = conn
        .query_row("SELECT run_id FROM runs LIMIT 1", [], |r| r.get(0))
        .unwrap();
    let run = runs::get(&conn, &run_id).unwrap();
    World {
        dir: dir.to_path_buf(),
        _temp: None,
        conn,
        run,
    }
}

impl World {
    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// The lines of the file `name` of the world; none when it is missing.
    fn lines(&self, name: &str) -> usize {
        std::fs::read_to_string(self.file(name))
            .map(|t| t.lines().count())
            .unwrap_or(0)
    }

    fn settings(&self, faults: Faults) -> Settings {
        Settings {
            fleet_root: self.root(),
            generation: Generation(1),
            config: FleetConfig::default(),
            horch: self.job_program(),
            faults,
        }
    }

    /// The program of each job process: it sleeps.
    fn job_program(&self) -> PathBuf {
        let path = self.file("job.sh");
        if !path.exists() {
            std::fs::write(&path, "#!/bin/sh\nexec sleep 30\n").unwrap();
            horch_platform::fs::make_executable(&path).unwrap();
        }
        path
    }

    /// A worker of the run, in state `status`, with the agent `agent`.
    fn worker(&self, kind: WorkerKind, status: &str, agent: Option<Recorded>) -> String {
        let new = NewWorker {
            run_id: self.run.run_id.clone(),
            kind: Some(kind),
            persona: "dev".into(),
            ..Default::default()
        };
        let id = workers::insert(&self.conn, &new).unwrap();
        self.conn
            .execute(
                "UPDATE workers SET status = ?2, agent_pid = ?3, agent_started = ?4
                 WHERE worker_id = ?1",
                params![
                    id,
                    status,
                    agent.map(|a| a.pid),
                    agent.and_then(|a| a.pid_start).map(|s| s as i64)
                ],
            )
            .unwrap();
        id
    }

    /// The attempt [`LAUNCHING`] of a live orchestrator row: an intended
    /// launch.
    fn launching_attempt(&self) {
        let worker = self.worker(WorkerKind::Orchestrator, "working", None);
        let row = AttemptRow {
            id: LAUNCHING.into(),
            task_id: None,
            worker_id: worker,
            state: AttemptState::Launching,
            process: None,
            pane: None,
        };
        attempts::insert_attempt(&self.conn, &row, &util::now()).unwrap();
    }

    /// An ended worker row that names the live process `p`: an orphan to
    /// stop (I5).
    fn orphan(&self, p: &Proc) {
        self.worker(WorkerKind::Worker, "done", Some(p.recorded()));
    }

    /// A task bound to a worker with the agent `agent`: 1 live attempt.
    fn claimed_task(&self, agent: Recorded) -> String {
        let mut new = NewTask::work("a", "tickets/a.md", "dev");
        new.worktree_mode = WorktreeMode::Shared;
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator).unwrap();
        let worker = NewWorker {
            run_id: self.run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: "dev".into(),
            ..Default::default()
        };
        bind_live(&self.conn, &task.task_id, &worker, Some(agent)).unwrap();
        task.task_id
    }

    fn queue_job(&mut self, subject: &str) -> i64 {
        let config = FleetConfig::default();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let key = JobKey {
            kind: Kind::TaskCheck,
            subject,
            input_key: "k",
        };
        let queued = jobs::queue(&ctx, &tx, &key, None, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        match queued {
            Queued::New(id) => id,
            other => panic!("not queued: {other:?}"),
        }
    }

    fn job_state(&self, id: i64) -> State {
        jobs_store::get(&self.conn, id).unwrap().unwrap().state
    }

    fn attempt_state(&self, id: &str) -> (String, Option<String>) {
        self.conn
            .query_row(
                "SELECT state, outcome FROM attempts WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    fn set_clock(&self, s: i64) {
        self.conn
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [s])
            .unwrap();
    }

    /// Run this test binary as the child of [`child_pass`] in `mode`, with
    /// `fault` armed.
    fn child(&self, mode: &str, fault: &str) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "child_pass", "--nocapture", "--test-threads=1"])
            .env(CHILD, &self.dir)
            .env(CHILD_MODE, mode)
            .env("HORCH_FAULT", fault)
            .spawn()
            .unwrap()
    }
}

/// The outside of the tests: no herdr snapshot, no health hold, and a
/// launch spec that appends 1 line to `launches.txt`, then runs `then`.
struct Fake {
    launches: PathBuf,
    then: &'static str,
    asked: Cell<usize>,
    closed: RefCell<Vec<String>>,
}

impl Fake {
    fn new(w: &World) -> Fake {
        Fake::then(w, "exec sleep 30")
    }

    fn then(w: &World, then: &'static str) -> Fake {
        Fake {
            launches: w.file("launches.txt"),
            then,
            asked: Cell::new(0),
            closed: RefCell::new(Vec::new()),
        }
    }
}

impl Outside for Fake {
    fn panes(
        &self,
    ) -> Option<BTreeMap<String, horch_core::fleetd::task_execution::domain::task::InPane>> {
        None
    }

    fn close_pane(&self, pane: &str) -> anyhow::Result<()> {
        self.closed.borrow_mut().push(pane.to_string());
        Ok(())
    }

    fn holds(&self) -> Vec<Hold> {
        Vec::new()
    }

    fn worktree_folders(&self, _conn: &Connection) -> anyhow::Result<Vec<FolderItem>> {
        Ok(Vec::new())
    }

    fn attempt_spec(&self, _attempt: &AttemptRow) -> anyhow::Result<LaunchSpec> {
        self.asked.set(self.asked.get() + 1);
        let script = format!(
            "echo launched >> '{}'; {}",
            self.launches.display(),
            self.then
        );
        Ok(LaunchSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: None,
            env_rule: Default::default(),
            detach: true,
            output: None,
        })
    }
}

/// A process of a test, killed when the test ends.
struct Proc(Child);

impl Proc {
    fn sleep() -> Proc {
        Proc(Command::new("sleep").arg("60").spawn().unwrap())
    }

    /// A process that appends 1 line to `file` when it gets the polite
    /// stop, then exits.
    fn polite(file: &Path) -> Proc {
        let script = format!(
            "trap 'echo stopped >> \"{}\"; exit 0' TERM; while :; do sleep 0.1; done",
            file.display()
        );
        let p = Proc(
            Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .spawn()
                .unwrap(),
        );
        // The trap is set before the loop: wait until the shell runs it.
        std::thread::sleep(Duration::from_millis(300));
        p
    }

    /// A process that ended and was reaped.
    fn dead() -> Recorded {
        let mut p = Proc::sleep();
        let r = p.recorded();
        p.0.kill().unwrap();
        p.0.wait().unwrap();
        r
    }

    fn recorded(&self) -> Recorded {
        let pid = self.0.id();
        Recorded {
            pid,
            pid_start: horch_platform::process::start_time(pid),
        }
    }

    /// Wait until it exits, at most 10 s.
    fn wait_exit(&mut self) -> bool {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if let Ok(Some(_)) = self.0.try_wait() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Kill the process of the launch record of `attempt`, if any.
fn kill_launched(w: &World, attempt: &str) {
    if let Ok(Some(LaunchRecord {
        process: Some(p), ..
    })) = effects::read_launch(&w.root(), attempt)
    {
        if let Some(start) = p.pid_start {
            stop_same(p.pid, start, How::Kill);
        }
    }
}

/// Kill the process of each job row.
fn kill_jobs(w: &World) {
    let mut stmt = w
        .conn
        .prepare("SELECT pid, pid_start FROM jobs WHERE pid IS NOT NULL")
        .unwrap();
    let rows: Vec<(u32, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for (pid, start) in rows {
        stop_same(pid, start as u64, How::Kill);
    }
}

/// Wait until `cond` holds, at most 10 s.
fn wait_for(mut cond: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn ports<'a>(outside: &'a dyn Outside, host: &'a dyn ProcessHost) -> Ports<'a> {
    Ports {
        outside,
        duties: &NoDuties,
        needs_you: &NeedsYouList,
        host,
    }
}

fn run(pass: &mut Pass<'_>, w: &mut World) -> PassReport {
    pass.run(&mut w.conn).unwrap()
}

// ─── the child process ──────────────────────────────────────────────────

/// Not a test of its own: the pass of a child process of a test. It runs
/// only with [`CHILD`].
#[test]
fn child_pass() {
    let Some(dir) = std::env::var_os(CHILD) else {
        return;
    };
    let mut w = reopen(Path::new(&dir));
    let faults = Faults::parse(std::env::var("HORCH_FAULT").ok().as_deref());
    let fake = Fake::new(&w);
    let mut pass = Pass::new(w.settings(faults), ports(&fake, &Launcher));
    match std::env::var(CHILD_MODE).unwrap().as_str() {
        // The fault ends the process after the commit.
        "abort" => {
            run(&mut pass, &mut w);
        }
        // The second pass hangs before its heartbeat.
        _ => loop {
            run(&mut pass, &mut w);
        },
    }
    panic!("the fault did not stop the child");
}

// ─── FM-11: a crash between the commit and the effects ──────────────────

#[test]
fn fleetd_killed_between_commit_and_effect_does_the_effect_once_on_restart() {
    let mut w = world();
    let mut orphan = Proc::polite(&w.file("stops.txt"));
    w.orphan(&orphan);
    w.launching_attempt();

    // The child commits part 2 and dies before part 3.
    let status = w.child("abort", ABORT_AFTER_COMMIT).wait().unwrap();
    assert_eq!(status.code(), Some(ABORT_EXIT_CODE));
    assert_eq!(w.lines("stops.txt"), 0, "no stop before the restart");
    assert_eq!(w.lines("launches.txt"), 0, "no launch before the restart");
    assert_eq!(w.attempt_state(LAUNCHING).0, "launching");

    // The restarted fleetd does each effect once.
    let fake = Fake::new(&w);
    let mut pass = Pass::new(w.settings(Faults::default()), ports(&fake, &Launcher));
    let report = run(&mut pass, &mut w);
    assert!(orphan.wait_exit(), "the orphan got its stop");
    assert!(wait_for(|| w.lines("launches.txt") == 1), "the launch ran");
    assert_eq!(
        report.done,
        vec![
            Done::Stop(format!("process {}", orphan.recorded().pid)),
            Done::Launch(LAUNCHING.into())
        ]
    );

    let report = run(&mut pass, &mut w);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(report.done, Vec::new(), "no second effect");
    assert_eq!(w.lines("stops.txt"), 1, "1 stop");
    assert_eq!(w.lines("launches.txt"), 1, "1 launch");
    assert_eq!(w.attempt_state(LAUNCHING).0, "live");
    assert_eq!(fake.asked.get(), 1);
    let record = effects::read_launch(&w.root(), LAUNCHING).unwrap().unwrap();
    let launched = record.process.expect("the launch recorded its process");
    let (pid, start): (Option<u32>, Option<i64>) = w
        .conn
        .query_row(
            "SELECT pid, pid_start FROM attempts WHERE id = ?1",
            [LAUNCHING],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(pid, Some(launched.pid), "the live attempt owns its process");
    assert_eq!(start, launched.pid_start.map(|s| s as i64));
    kill_launched(&w, LAUNCHING);
}

#[test]
fn fm_fleetd_killed_mid_reap_requeues_exactly_once() {
    let mut w = world();
    let task = w.claimed_task(Proc::dead());

    let status = w.child("abort", ABORT_AFTER_COMMIT).wait().unwrap();
    assert_eq!(status.code(), Some(ABORT_EXIT_CODE));

    let fake = Fake::new(&w);
    let mut pass = Pass::new(w.settings(Faults::default()), ports(&fake, &Launcher));
    run(&mut pass, &mut w);
    run(&mut pass, &mut w);

    let status: String = w
        .conn
        .query_row(
            "SELECT status FROM tasks WHERE task_id = ?1",
            [&task],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "ready");
    let ended: Vec<(String, Option<String>)> = {
        let mut stmt = w
            .conn
            .prepare("SELECT state, outcome FROM attempts WHERE task_id = ?1")
            .unwrap();
        stmt.query_map([&task], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(ended, vec![("ended".into(), Some("no_result".into()))]);
    let requeues: i64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM events WHERE entity = 'task' AND entity_id = ?1
             AND from_state = 'running' AND to_state = 'ready'",
            [&task],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(requeues, 1, "the task is queued again exactly once");
}

// ─── part 3: stops, launches, the time budget ───────────────────────────

#[test]
fn stops_come_before_launches_in_part_3() {
    let mut w = world();
    let job = w.queue_job("T-1");
    w.launching_attempt();
    let mut orphan = Proc::polite(&w.file("stops.txt"));
    w.orphan(&orphan);

    let fake = Fake::new(&w);
    let mut pass = Pass::new(w.settings(Faults::default()), ports(&fake, &Launcher));
    let report = run(&mut pass, &mut w);
    assert_eq!(
        report.done,
        vec![
            Done::Stop(format!("process {}", orphan.recorded().pid)),
            Done::Launch(LAUNCHING.into()),
            Done::StartJob(job),
        ]
    );
    assert!(orphan.wait_exit());
    kill_launched(&w, LAUNCHING);
    kill_jobs(&w);
}

#[test]
fn launching_attempt_with_no_process_past_the_start_timeout_ends_timed_out() {
    let mut w = world();
    w.launching_attempt();
    // The launched process exits at once: no process has the attempt id.
    let fake = Fake::then(&w, "exit 0");
    let mut pass = Pass::new(w.settings(Faults::default()), ports(&fake, &Launcher));
    run(&mut pass, &mut w);
    let record = effects::read_launch(&w.root(), LAUNCHING).unwrap().unwrap();
    let launched = record.process.expect("the record has the process");
    // A pass that scans a process that still runs ends its attempt `live`,
    // so no pass runs before the process has exited (a loaded host can
    // delay the exit past the next pass). The pass that follows reaps it.
    assert!(wait_for(|| exited(launched.pid)), "the process exits");
    assert!(wait_for(|| pass_reaps(&mut pass, &mut w)));

    w.set_clock(record.launched_at.0 + 80);
    run(&mut pass, &mut w);
    assert_eq!(w.attempt_state(LAUNCHING).0, "launching", "within 90 s");

    w.set_clock(record.launched_at.0 + 95);
    run(&mut pass, &mut w);
    assert_eq!(
        w.attempt_state(LAUNCHING),
        ("ended".into(), Some("timed_out".into()))
    );
    assert_eq!(fake.asked.get(), 1, "an attempt id is launched once");
}

/// Whether the process `pid` has exited: it is gone, or it is a zombie that
/// its parent has not reaped yet. No pass runs, so nothing reaps it.
fn exited(pid: u32) -> bool {
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&out.stdout);
    let stat = stat.trim();
    stat.is_empty() || stat.starts_with('Z')
}

/// Run 1 pass, which reaps the ended launches; then whether the launched
/// process of [`LAUNCHING`] is gone.
fn pass_reaps(pass: &mut Pass<'_>, w: &mut World) -> bool {
    run(pass, w);
    let record = effects::read_launch(&w.root(), LAUNCHING).unwrap().unwrap();
    let p = record.process.unwrap();
    // A start time that cannot be read is not "gone": a process that is
    // still starting, or that exited and is not reaped yet, gives none.
    if !horch_platform::process::alive(p.pid) {
        return true;
    }
    match (p.pid_start, horch_platform::process::start_time(p.pid)) {
        (Some(start), Some(now)) => now != start,
        _ => false,
    }
}

#[test]
fn launch_is_skipped_when_a_live_process_has_the_attempt_id() {
    let mut w = world();
    w.launching_attempt();
    let live = Proc::sleep();
    let record = LaunchRecord {
        attempt: LAUNCHING.into(),
        launched_at: FleetSeconds(0),
        process: Some(live.recorded()),
    };
    effects::write_launch(&w.root(), &record).unwrap();

    let fake = Fake::new(&w);
    let mut pass = Pass::new(w.settings(Faults::default()), ports(&fake, &Launcher));
    let report = run(&mut pass, &mut w);
    assert_eq!(report.done, Vec::new());
    assert_eq!(fake.asked.get(), 0, "no second process for the attempt id");
    assert_eq!(w.lines("launches.txt"), 0);
    assert_eq!(w.attempt_state(LAUNCHING).0, "live");
}

/// A launcher that moves the test clock 15 s at each launch.
struct SlowHost<'a> {
    now: &'a Cell<Instant>,
}

impl ProcessHost for SlowHost<'_> {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, LaunchError> {
        self.now.set(self.now.get() + Duration::from_secs(15));
        Launcher.launch(spec)
    }
}

#[test]
fn pass_ends_at_its_time_budget_and_the_rest_stay_intended() {
    let mut w = world();
    let ids: Vec<i64> = ["T-1", "T-2", "T-3"]
        .iter()
        .map(|s| w.queue_job(s))
        .collect();
    let now = Cell::new(Instant::now());
    let host = SlowHost { now: &now };
    let fake = Fake::new(&w);
    let mut settings = w.settings(Faults::default());
    settings.config.pass_time_budget_s = 20;
    let mut pass = Pass::new(settings, ports(&fake, &host)).with_clock(Box::new(|| now.get()));

    let report = run(&mut pass, &mut w);
    // 0 s: start; 15 s: start; 30 s: past the budget of 20 s.
    assert_eq!(
        report.done,
        vec![Done::StartJob(ids[0]), Done::StartJob(ids[1])]
    );
    assert_eq!(report.left, 1);
    assert!(
        report.heartbeat,
        "the pass ends in order, with its heartbeat"
    );
    assert_eq!(w.job_state(ids[2]), State::Queued, "the rest stay intended");

    let report = run(&mut pass, &mut w);
    assert_eq!(report.done, vec![Done::StartJob(ids[2])]);
    kill_jobs(&w);
}

// ─── the heartbeat (FM-12, PL-5) ────────────────────────────────────────

#[test]
fn heartbeat_is_written_after_each_full_pass_and_not_by_a_hung_pass() {
    let mut w = world();
    let fake = Fake::new(&w);
    let mut pass = Pass::new(w.settings(Faults::default()), ports(&fake, &Launcher));
    assert_eq!(effects::read_heartbeat(&w.root()).unwrap(), None);
    run(&mut pass, &mut w);
    let first = effects::read_heartbeat(&w.root()).unwrap().unwrap();
    assert_eq!(first.pid, std::process::id());
    assert_eq!(first.generation, Generation(1));
    assert_eq!(first.version, env!("CARGO_PKG_VERSION"));
    std::thread::sleep(Duration::from_millis(20));
    run(&mut pass, &mut w);
    let second = effects::read_heartbeat(&w.root()).unwrap().unwrap();
    assert!(
        second.awake.awake_ms > first.awake.awake_ms,
        "each full pass"
    );

    // A child fleetd: its first pass writes the heartbeat, its next pass
    // hangs before it.
    std::fs::remove_file(effects::heartbeat_path(&w.root())).unwrap();
    let mut child = w.child("hang", HANG_PASS);
    let pid = child.id();
    assert!(wait_for(|| effects::read_heartbeat(&w.root())
        .ok()
        .flatten()
        .is_some_and(|h| h.pid == pid)));
    let written = effects::read_heartbeat(&w.root()).unwrap().unwrap();
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(child.try_wait().unwrap(), None, "the child hangs");
    assert_eq!(
        effects::read_heartbeat(&w.root()).unwrap().unwrap(),
        written,
        "a hung pass writes no heartbeat"
    );
    child.kill().unwrap();
    child.wait().unwrap();
}

fn heartbeat(boot: &str, awake_ms: u64) -> Heartbeat {
    Heartbeat {
        pid: 7,
        pid_start: 9,
        generation: Generation(3),
        awake: AwakeReading {
            boot_id: boot.into(),
            awake_ms,
        },
        at: "2026-11-17T00:00:00Z".into(),
        exe: Exe {
            path: "/bin/horch".into(),
            len: 10,
            mtime_ns: 20,
        },
        version: "0.4.8".into(),
    }
}

#[test]
fn is_hung_after_30_s_of_awake_time_and_not_after_a_sleep() {
    let at = |boot: &str, ms: u64| AwakeReading {
        boot_id: boot.into(),
        awake_ms: ms,
    };
    let hb = heartbeat("b1", 100_000);
    assert!(!is_hung(&hb, &at("b1", 100_000), 30));
    assert!(!is_hung(&hb, &at("b1", 130_000), 30), "30 s is the limit");
    assert!(
        is_hung(&hb, &at("b1", 130_001), 30),
        "past 30 s of awake time"
    );
    // A sleep of 1 h does not move the awake clock: 5 s of awake time.
    assert!(!is_hung(&hb, &at("b1", 105_000), 30), "not after a sleep");
    assert!(
        is_hung(&hb, &at("b2", 1_000), 30),
        "a reading of another boot"
    );
}

#[test]
fn rebuilt_binary_is_older() {
    let hb = heartbeat("b1", 0);
    let same = hb.exe.clone();
    assert!(!is_older(&hb, &same));
    let rebuilt = Exe {
        mtime_ns: 21,
        ..same
    };
    assert!(is_older(&hb, &rebuilt), "the daemon runs another binary");
}

#[test]
fn heartbeat_file_round_trips() {
    let w = world();
    let hb = heartbeat("b1", 5);
    effects::write_heartbeat(&w.root(), &hb).unwrap();
    assert_eq!(effects::read_heartbeat(&w.root()).unwrap(), Some(hb));
}

#[test]
fn a_restart_keeps_the_grace_of_a_polite_stop() {
    let mut w = world();
    // A process that ignores the polite stop.
    let mut stubborn = Proc(
        Command::new("/bin/sh")
            .arg("-c")
            .arg("trap '' TERM; while :; do sleep 0.1; done")
            .spawn()
            .unwrap(),
    );
    std::thread::sleep(Duration::from_millis(300));
    w.orphan(&stubborn);
    let recorded = stubborn.recorded();
    let pid = recorded.pid;

    let fake = Fake::new(&w);
    let mut pass = Pass::new(w.settings(Faults::default()), ports(&fake, &Launcher));
    assert_eq!(
        run(&mut pass, &mut w).done,
        vec![Done::Stop(format!("process {pid}"))]
    );
    assert_eq!(run(&mut pass, &mut w).done, Vec::new(), "within its grace");
    let start = recorded.pid_start.unwrap();
    let has_row = |w: &World| {
        polite_stops_store::get(&w.conn, pid, start)
            .unwrap()
            .is_some()
    };

    // A pass whose process scan reaches nothing keeps the record.
    let mut blind = w.settings(Faults::default());
    blind.config.scan_budget_ms = 0;
    let mut pass = Pass::new(blind, ports(&fake, &Launcher));
    assert_eq!(run(&mut pass, &mut w).done, Vec::new());
    assert!(
        has_row(&w),
        "a process that the scan did not reach keeps its grace"
    );

    // A new fleetd keeps the time of the polite stop: past the grace of
    // 20 s, the kill comes.
    drop(pass);
    let clock: i64 = w
        .conn
        .query_row("SELECT fleet_clock_s FROM fleetd", [], |r| r.get(0))
        .unwrap();
    w.set_clock(clock + 25);
    let mut pass = Pass::new(w.settings(Faults::default()), ports(&fake, &Launcher));
    assert_eq!(
        run(&mut pass, &mut w).done,
        vec![Done::Stop(format!("kill process {pid}"))]
    );
    assert!(stubborn.wait_exit(), "the kill ended it");
    run(&mut pass, &mut w);
    assert!(!has_row(&w), "the row of a gone process is deleted");
}
