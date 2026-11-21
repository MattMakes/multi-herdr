#![cfg(unix)]

use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use chrono::Utc;
use horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem;
use horch_core::fleetd::fleet_runtime::adapters::watchdog_duties::{DutyEffects, WatchdogDuties};
use horch_core::fleetd::fleet_runtime::application::pass::{
    Done, NoDuties, Outside, Pass, PassReport, PoolFacts, Ports, RunDuties, Settings,
};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::application::cancel;
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane, Recorded};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::harness::HarnessKind;
use horch_core::roster::{Roster, Teammate};
use horch_core::routing::policy::Policy;
use horch_core::routing::quota::{PoolReading, QuotaFile, QuotaView, Window};
use horch_core::runtime::fault::Faults;
use horch_core::taskledger::model::{Actor, WorkerKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run, Thresholds};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::testing::bind_live;
use horch_core::taskledger::watchdog::{Observation, Observer};
use horch_core::taskledger::workers::{self, NewWorker, Worker};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection, OptionalExtension};

struct World {
    dir: PathBuf,
    _temp: tempfile::TempDir,
    conn: Connection,
    run: Run,
    config: FleetConfig,
}

fn world() -> World {
    world_with(NewRun {
        slug: "r1".into(),
        ..Default::default()
    })
}

fn world_with(new_run: NewRun) -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let mut conn = Connection::open(dir.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let run = runs::create(&conn, &dir, &new_run).unwrap();
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
        _temp: temp,
        conn,
        run,
        config: FleetConfig::default(),
    }
}

impl World {
    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn settings(&self) -> Settings {
        let horch = self.file("job.sh");
        if !horch.exists() {
            std::fs::write(&horch, "#!/bin/sh\nexec sleep 30\n").unwrap();
            horch_platform::fs::make_executable(&horch).unwrap();
        }
        Settings {
            fleet_root: self.root(),
            generation: Generation(1),
            config: self.config.clone(),
            horch,
            faults: Faults::default(),
        }
    }

    fn claimed_task(&self, agent: &Agent) -> (String, String) {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        let file = format!("tickets/t{n}.md");
        let mut new = NewTask::work(&format!("t{n}"), &file, "dev");
        new.worktree_mode = WorktreeMode::Shared;
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator).unwrap();
        let worker = NewWorker {
            run_id: self.run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: "dev".into(),
            ..Default::default()
        };
        let bound = bind_live(&self.conn, &task.task_id, &worker, Some(agent.recorded())).unwrap();
        (task.task_id, bound.worker_id)
    }

    fn set_clock(&self, s: i64) {
        self.conn
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [s])
            .unwrap();
    }

    fn clock(&self) -> i64 {
        self.conn
            .query_row("SELECT fleet_clock_s FROM fleetd", [], |r| r.get(0))
            .unwrap()
    }

    fn beat(&self, worker: &str) {
        self.conn
            .execute(
                "UPDATE workers SET last_checkin_at = ?2 WHERE worker_id = ?1",
                params![worker, util::now()],
            )
            .unwrap();
    }

    fn task(&self, task: &str) -> tasks::Task {
        tasks::get(&self.conn, task).unwrap()
    }

    fn attempts(&self, task: &str) -> Vec<(String, String, Option<String>)> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, state, outcome FROM attempts WHERE task_id = ?1 ORDER BY rowid")
            .unwrap();
        stmt.query_map([task], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn caused(&self, task: &str, cause: &str) -> i64 {
        self.conn
            .query_row(
                "SELECT count(*) FROM events WHERE task_id = ?1 AND cause = ?2",
                params![task, cause],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn pass_at(&mut self, clock: i64, worker: &str, outside: &dyn Outside) -> PassReport {
        self.pass_with(clock, worker, outside, &NoDuties)
    }

    fn pass_with(
        &mut self,
        clock: i64,
        worker: &str,
        outside: &dyn Outside,
        duties: &dyn RunDuties,
    ) -> PassReport {
        self.set_clock(clock);
        self.beat(worker);
        let ports = Ports {
            outside,
            duties,
            needs_you: &NeedsYouList,
            host: &Launcher,
        };
        let mut pass = Pass::new(self.settings(), ports);
        let report = pass.run(&mut self.conn).unwrap();
        assert!(report.failed.is_empty(), "{report:?}");
        report
    }
}

struct Agent(Child);

impl Agent {
    fn run(script: &str) -> Agent {
        let a = Agent(
            Command::new("/bin/sh")
                .args(["-c", script])
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        std::thread::sleep(Duration::from_millis(300));
        a
    }

    fn sleeping() -> Agent {
        Agent::run("while :; do sleep 0.1; done")
    }

    fn recorded(&self) -> Recorded {
        let pid = self.0.id();
        Recorded {
            pid,
            pid_start: horch_platform::process::start_time(pid),
        }
    }

    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }

    fn exits(&mut self) -> bool {
        wait_for(|| matches!(self.0.try_wait(), Ok(Some(_))))
    }

    fn alive(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(None))
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        // SAFETY: a signal to the process group that this test started.
        unsafe { libc::kill(-(self.0.id() as i32), libc::SIGKILL) };
        let _ = self.0.wait();
    }
}

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

struct Fake {
    claude_used: Option<f64>,
}

impl Fake {
    fn new(_w: &World) -> Fake {
        Fake { claude_used: None }
    }

    fn limited(_w: &World) -> Fake {
        Fake {
            claude_used: Some(1.0),
        }
    }
}

fn teammate(name: &str, agent: HarnessKind, model: &str, fallback: Option<&str>) -> Teammate {
    Teammate {
        name: name.to_string(),
        agent,
        model: Some(model.to_string()),
        fallbacks: fallback.map(str::to_string).into_iter().collect(),
        ..Default::default()
    }
}

fn reading(used: f64) -> PoolReading {
    PoolReading {
        windows: vec![Window::new(300, used, None, None)],
        observed_at: Some(util::now()),
        ..Default::default()
    }
}

impl Outside for Fake {
    fn panes(&self) -> Option<BTreeMap<String, InPane>> {
        None
    }

    fn close_pane(&self, _pane: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn holds(&self) -> Vec<Hold> {
        Vec::new()
    }

    fn worktree_folders(&self, _conn: &Connection) -> anyhow::Result<Vec<FolderItem>> {
        Ok(Vec::new())
    }

    fn attempt_spec(&self, attempt: &AttemptRow) -> anyhow::Result<LaunchSpec> {
        anyhow::bail!("no launch of {} in these tests", attempt.id)
    }

    fn pools(&self) -> Option<PoolFacts> {
        let used = self.claude_used?;
        let mut roster = Roster::builtin().unwrap();
        roster.insert_for_test(teammate(
            "dev",
            HarnessKind::Claude,
            "claude-opus-5-5",
            Some("alt"),
        ));
        roster.insert_for_test(teammate("alt", HarnessKind::Codex, "gpt-5.5", None));
        let mut pools = BTreeMap::new();
        pools.insert("claude".to_string(), reading(used));
        pools.insert("codex".to_string(), reading(0.0));
        let file = QuotaFile {
            schema: 1,
            written_at: None,
            pools,
            harnesses: BTreeMap::new(),
        };
        let quota = QuotaView::new(file, Utc::now(), Policy::default(), true);
        Some(PoolFacts { roster, quota })
    }
}

#[test]
fn result_in_the_pass_wins_over_the_lease_expiry() {
    let mut w = world();
    let mut agent = Agent::sleeping();
    let (task, worker) = w.claimed_task(&agent);
    w.conn
        .execute(
            "INSERT INTO questions (id, task_id, feature_id, owner, state, due_at, asked_at)
             VALUES ('Q1', ?1, 'f', 'operator', 'open', 0, ?2)",
            params![task, util::now()],
        )
        .unwrap();
    agent.kill();
    let fake = Fake::new(&w);
    w.pass_at(1_000, &worker, &fake);

    assert_eq!(w.task(&task).status, "blocked_on_user");
    let attempts = w.attempts(&task);
    assert_eq!(attempts.len(), 1, "{attempts:?}");
    assert_eq!(attempts[0].1, "ended");
    assert_eq!(attempts[0].2.as_deref(), Some("asked"));
    assert_eq!(w.caused(&task, "lease_expired"), 0, "the result won");
}

#[test]
fn idle_worker_is_stalled_and_queued_after_its_backoff() {
    let mut w = world();
    let mut agent = Agent::sleeping();
    let (task, worker) = w.claimed_task(&agent);
    let fake = Fake::new(&w);

    w.pass_at(100, &worker, &fake);
    assert_eq!(w.task(&task).status, "running");
    assert!(agent.alive());

    w.pass_at(310, &worker, &fake);
    let attempts = w.attempts(&task);
    assert_eq!(attempts[0].1, "ended", "{attempts:?}");
    assert_eq!(attempts[0].2.as_deref(), Some("stalled"));
    assert_eq!(
        w.task(&task).status,
        "stalled",
        "the stop comes after the commit"
    );
    assert!(agent.exits(), "the stop of rung 3 ended the agent");
    w.pass_at(315, &worker, &fake);
    let t = w.task(&task);
    assert_eq!(t.status, "ready", "rung 6 queued it again");
    let not_before = t.not_before.expect("a backoff");
    let clock = w.clock();
    assert!(not_before > clock, "{not_before} after {clock}");

    assert!(!claimable(&w), "startable in its backoff");
    w.set_clock(not_before);
    assert!(claimable(&w), "not startable after the backoff");
}

fn stops(report: &PassReport) -> Vec<&str> {
    report
        .done
        .iter()
        .filter_map(|d| match d {
            Done::Stop(s) => Some(s.as_str()),
            _ => None,
        })
        .collect()
}

fn claimable(w: &World) -> bool {
    !tasks::startable(&w.conn, &w.run.run_id).unwrap().is_empty()
}

#[test]
fn stalled_task_stays_stalled_while_the_old_process_lives() {
    let mut w = world();
    w.config.stop_grace_s = 600;
    let mut agent = Agent::run("trap '' TERM INT HUP; while :; do sleep 0.1; done");
    let (task, worker) = w.claimed_task(&agent);
    let fake = Fake::new(&w);

    let report = w.pass_at(1_000, &worker, &fake);
    assert_eq!(stops(&report).len(), 1, "1 polite stop: {report:?}");
    let attempts = w.attempts(&task);
    assert_eq!(attempts[0].2.as_deref(), Some("stalled"), "{attempts:?}");
    assert_eq!(w.task(&task).status, "stalled");
    assert!(agent.alive(), "it ignores the polite stop");

    let report = w.pass_at(1_120, &worker, &fake);
    assert!(stops(&report).is_empty(), "no stop again: {report:?}");
    assert!(agent.alive(), "it lives in its grace");
    assert_eq!(w.task(&task).status, "stalled", "its old process lives");
    assert!(!claimable(&w), "no attempt 2 beside a live attempt 1");
    assert_eq!(w.attempts(&task).len(), 1);

    let report = w.pass_at(1_600, &worker, &fake);
    assert_eq!(
        stops(&report),
        ["kill process ".to_string() + &agent.recorded().pid.to_string()]
    );
    w.pass_at(1_605, &worker, &fake);
    assert!(agent.exits(), "the hard stop ended it");

    let marks: i64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM events WHERE kind = 'stopped'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        marks, 2,
        "1 sent mark for the polite stop and 1 for the hard stop"
    );

    w.pass_at(1_610, &worker, &fake);
    let t = w.task(&task);
    assert_eq!(t.status, "ready", "rung 6 after the process is gone");
    assert_eq!(
        t.not_before,
        Some(w.clock() + 30),
        "the backoff starts at the move"
    );
    assert!(!claimable(&w), "in its backoff");
    w.set_clock(w.clock() + 30);
    assert!(claimable(&w));
}

#[test]
fn task_whose_old_process_is_gone_moves_in_the_same_pass() {
    let mut w = world();
    let mut agent = Agent::sleeping();
    let (task, worker) = w.claimed_task(&agent);
    agent.kill();
    let fake = Fake::new(&w);

    w.pass_at(1_000, &worker, &fake);
    let t = w.task(&task);
    assert_eq!(t.status, "ready", "{:?}", w.attempts(&task));
    assert_eq!(t.not_before, Some(1_030));
}

#[test]
fn verifying_task_is_not_stalled_by_an_old_lease() {
    let mut w = world();
    let agent = Agent::sleeping();
    let (task, worker) = w.claimed_task(&agent);
    w.conn
        .execute(
            "UPDATE tasks SET status = 'verifying' WHERE task_id = ?1",
            [&task],
        )
        .unwrap();
    let fake = Fake::new(&w);

    w.pass_at(1_000, &worker, &fake);
    assert_eq!(w.task(&task).status, "verifying");
    assert_eq!(w.caused(&task, "lease_expired"), 0);
    let attempts = w.attempts(&task);
    assert_ne!(attempts[0].2.as_deref(), Some("stalled"), "{attempts:?}");
}

#[test]
fn cancel_of_a_stalled_task_waits_for_its_old_process() {
    let mut w = world();
    w.config.stop_grace_s = 600;
    let mut agent = Agent::run("trap '' TERM INT HUP; while :; do sleep 0.1; done");
    let (task, worker) = w.claimed_task(&agent);
    let fake = Fake::new(&w);
    w.pass_at(1_000, &worker, &fake);
    assert_eq!(w.task(&task).status, "stalled");
    {
        let tx = w.conn.transaction().unwrap();
        let t = tasks::get(&tx, &task).unwrap();
        let out = cancel::request(&tx, &t, "the operator cancelled it").unwrap();
        assert!(matches!(out, cancel::Cancel::Stopping { .. }), "{out:?}");
        tx.commit().unwrap();
    }

    w.pass_at(1_010, &worker, &fake);
    assert!(agent.alive(), "it ignores the polite stop");
    assert_eq!(w.task(&task).status, "stalled", "its old process lives");

    agent.kill();
    w.pass_at(1_020, &worker, &fake);
    assert_eq!(w.task(&task).status, "cancelled");
}

#[test]
fn usage_limited_worker_restarts_on_the_fallback_and_burns_no_attempt() {
    let mut w = world();
    let mut agent = Agent::sleeping();
    let (task, worker) = w.claimed_task(&agent);
    let burned = |t: &tasks::Task| t.attempts - t.attempt_floor;
    assert_eq!(burned(&w.task(&task)), 1);
    let fake = Fake::limited(&w);

    w.pass_at(10, &worker, &fake);
    let attempts = w.attempts(&task);
    assert_eq!(attempts.len(), 1, "{attempts:?}");
    assert_eq!(attempts[0].1, "ended");
    assert_eq!(attempts[0].2.as_deref(), Some("quota"));
    let t = w.task(&task);
    assert_eq!(t.status, "ready", "ready for the fallback at once");
    assert_eq!(t.claimable_by.as_deref(), Some("alt"));
    assert_eq!(t.not_before, None, "a usage limit sets no backoff");
    assert_eq!(burned(&t), 0, "a usage limit burns no attempt");
    assert!(agent.exits(), "the stop of the old attempt ended the agent");
}

struct Quiet;

impl Observer for Quiet {
    fn observe(&self, _w: &Worker) -> Observation {
        Observation {
            agent_alive: true,
            herdr_status: Some("idle".into()),
            ..Default::default()
        }
    }
}

struct NoEffects;

impl DutyEffects for NoEffects {
    fn deliver(&self, _run: &Run) {}

    fn retry_cleanups(&self) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }
}

#[test]
fn stall_rounds_of_today_no_longer_reap_a_worker() {
    let mut w = world_with(NewRun {
        slug: "r1".into(),
        thresholds: Thresholds {
            check_every_s: Some(1),
            stall_rounds: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    let mut agent = Agent::sleeping();
    let (task, worker) = w.claimed_task(&agent);
    let hour = (Utc::now() - chrono::Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    w.conn
        .execute(
            "UPDATE workers SET status = 'working', claimed_at = ?2, last_progress_at = ?2
             WHERE worker_id = ?1",
            params![worker, hour],
        )
        .unwrap();
    let fake = Fake::new(&w);
    let duties = WatchdogDuties::new(&Quiet, &NoEffects).forced();

    for clock in [10, 20, 30] {
        w.pass_with(clock, &worker, &fake, &duties);
        std::thread::sleep(Duration::from_millis(1_100));
    }
    let checks: i64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM checks WHERE worker_id = ?1",
            [&worker],
            |r| r.get(0),
        )
        .unwrap();
    assert!(checks >= 3, "the checks still run: {checks}");
    let status: String = w
        .conn
        .query_row(
            "SELECT status FROM workers WHERE worker_id = ?1",
            [&worker],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "working");
    assert_eq!(w.task(&task).status, "running");
    let reaped: i64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM events WHERE kind = 'reaped' AND worker_id = ?1",
            [&worker],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(reaped, 0);
    assert!(agent.alive());
}

#[test]
fn background_wait_holds_while_the_agent_child_runs_and_ends_when_it_exits() {
    let mut w = world();
    let mut agent = Agent::run("sleep 600 & wait; exec sleep 3600");
    let (task, worker) = w.claimed_task(&agent);
    workers::set_wait(
        &w.conn,
        &worker,
        workers::BACKGROUND_WAIT,
        "build",
        &[],
        86_400,
    )
    .unwrap();
    let fake = Fake::new(&w);
    let wait_row = |w: &World| -> Option<String> {
        w.conn
            .query_row(
                "SELECT kind FROM worker_waits WHERE worker_id = ?1",
                [&worker],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
    };

    for clock in [100, 200, 300, 400, 500] {
        w.pass_at(clock, &worker, &fake);
        assert_eq!(w.task(&task).status, "running", "at {clock}");
        assert!(wait_row(&w).is_some(), "at {clock}");
    }

    let pgid = agent.recorded().pid;
    let child: Vec<u32> = horch_core::procid::group_members(pgid)
        .into_iter()
        .filter(|p| *p != pgid)
        .collect();
    assert_eq!(child.len(), 1, "{child:?}");
    // SAFETY: a signal to the child that this test started.
    unsafe { libc::kill(child[0] as i32, libc::SIGKILL) };
    assert!(wait_for(
        || horch_core::procid::group_members(pgid).len() == 1
    ));
    w.pass_at(510, &worker, &fake);
    assert!(wait_row(&w).is_none(), "the wait ended");
    assert_eq!(w.task(&task).status, "running");

    for clock in [610, 710] {
        w.pass_at(clock, &worker, &fake);
        assert_eq!(w.task(&task).status, "running", "at {clock}");
    }
    w.pass_at(810, &worker, &fake);
    let attempts = w.attempts(&task);
    assert_eq!(attempts[0].2.as_deref(), Some("stalled"), "{attempts:?}");
    assert!(agent.exits());
    w.pass_at(815, &worker, &fake);
    assert_eq!(w.task(&task).status, "ready");
}

#[test]
fn cancel_requested_task_is_cancelled_by_the_pass_after_its_process_is_gone() {
    let mut w = world();
    let mut agent = Agent::run("trap '' TERM INT HUP; while :; do sleep 0.1; done");
    let (task, worker) = w.claimed_task(&agent);
    {
        let tx = w.conn.transaction().unwrap();
        let t = tasks::get(&tx, &task).unwrap();
        cancel::request(&tx, &t, "the operator cancelled it").unwrap();
        tx.commit().unwrap();
    }
    let fake = Fake::new(&w);
    w.pass_at(10, &worker, &fake);
    assert!(agent.alive(), "it ignores the polite stop");
    assert_ne!(w.task(&task).status, "cancelled", "its process lives");

    agent.kill();
    w.pass_at(20, &worker, &fake);
    assert_eq!(w.task(&task).status, "cancelled");
}

#[test]
fn idle_author_of_a_verifying_task_keeps_its_item_while_off_the_page() {
    let mut w = world();
    let a = Agent::sleeping();
    let b = Agent::sleeping();
    let (task_a, worker_a) = w.claimed_task(&a);
    let (task_b, worker_b) = w.claimed_task(&b);
    let hour_ago = (Utc::now() - chrono::Duration::hours(1))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    for (task, worker) in [(&task_a, &worker_a), (&task_b, &worker_b)] {
        w.conn
            .execute(
                "UPDATE tasks SET status = 'verifying' WHERE task_id = ?1",
                [task],
            )
            .unwrap();
        w.conn
            .execute(
                "INSERT INTO checks (worker_id, at, verdict, round) VALUES (?1, ?2, 'no_progress', 1)",
                params![worker, hour_ago],
            )
            .unwrap();
    }
    let open = |w: &World| -> Vec<String> {
        let mut stmt = w
            .conn
            .prepare("SELECT key FROM needs_you WHERE kind = 'finish_idle' AND state = 'open' ORDER BY key")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let fake = Fake::new(&w);
    for clock in [10, 20] {
        w.set_clock(clock);
        let mut settings = w.settings();
        settings.config.pass_budget = 1;
        let ports = Ports {
            outside: &fake,
            duties: &NoDuties,
            needs_you: &NeedsYouList,
            host: &Launcher,
        };
        let report = Pass::new(settings, ports).run(&mut w.conn).unwrap();
        assert!(report.failed.is_empty(), "{report:?}");
    }
    let mut want = vec![
        format!("finish_idle:{task_a}"),
        format!("finish_idle:{task_b}"),
    ];
    want.sort();
    assert_eq!(
        open(&w),
        want,
        "an item stays while its author is off the page"
    );
    assert!(w.attempts(&task_a)[0].1 == "live" && w.attempts(&task_b)[0].1 == "live");
}

struct Bells {
    inner: Fake,
    typed: std::cell::RefCell<Vec<String>>,
    fail: std::cell::Cell<usize>,
}

impl Outside for Bells {
    fn panes(&self) -> Option<BTreeMap<String, InPane>> {
        None
    }

    fn close_pane(&self, _pane: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn holds(&self) -> Vec<Hold> {
        Vec::new()
    }

    fn worktree_folders(&self, _conn: &Connection) -> anyhow::Result<Vec<FolderItem>> {
        Ok(Vec::new())
    }

    fn attempt_spec(&self, attempt: &AttemptRow) -> anyhow::Result<LaunchSpec> {
        self.inner.attempt_spec(attempt)
    }

    fn observe(&self, _w: &Worker) -> Option<Observation> {
        Some(Observation {
            agent_alive: true,
            herdr_status: Some("idle".into()),
            ..Default::default()
        })
    }

    fn doorbell(&self, pane: &str) -> anyhow::Result<()> {
        if self.fail.get() > 0 {
            self.fail.set(self.fail.get() - 1);
            anyhow::bail!("lost before the send");
        }
        self.typed.borrow_mut().push(pane.to_string());
        Ok(())
    }
}

#[test]
fn a_doorbell_that_was_not_typed_is_typed_by_the_next_pass() {
    let mut w = world();
    let _agent = Agent::sleeping();
    let (task, worker) = w.claimed_task(&_agent);
    w.conn
        .execute(
            "UPDATE workers SET pane_id = 'w1:p2' WHERE worker_id = ?1",
            [&worker],
        )
        .unwrap();
    let bells = Bells {
        inner: Fake::new(&w),
        typed: Default::default(),
        fail: std::cell::Cell::new(1),
    };
    let doorbell_events = |w: &World| -> i64 {
        w.conn
            .query_row(
                "SELECT count(*) FROM events WHERE kind = 'doorbell'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    };
    let pass = |w: &mut World, clock: i64| -> PassReport {
        w.set_clock(clock);
        w.beat(&worker);
        let ports = Ports {
            outside: &bells,
            duties: &NoDuties,
            needs_you: &NeedsYouList,
            host: &Launcher,
        };
        Pass::new(w.settings(), ports).run(&mut w.conn).unwrap()
    };
    for clock in [100, 200, 290] {
        let report = pass(&mut w, clock);
        assert!(report.failed.is_empty(), "{report:?}");
    }
    assert_eq!(doorbell_events(&w), 0);
    let report = pass(&mut w, 310);
    assert_eq!(report.failed.len(), 1, "{report:?}");
    assert_eq!(doorbell_events(&w), 1);
    assert!(bells.typed.borrow().is_empty());
    for clock in [320, 330] {
        let report = pass(&mut w, clock);
        assert!(report.failed.is_empty(), "{report:?}");
    }
    assert_eq!(*bells.typed.borrow(), ["w1:p2"]);
    assert_eq!(doorbell_events(&w), 1);
    assert_eq!(w.task(&task).status, "running");
}

fn assert_no_lost_task(w: &World) {
    let lost: Vec<String> = w
        .conn
        .prepare(
            "SELECT task_id FROM tasks t WHERE status IN ('leased', 'running')
               AND NOT EXISTS (SELECT 1 FROM attempts a
                               WHERE a.task_id = t.task_id AND a.state != 'ended')
               AND NOT EXISTS (SELECT 1 FROM leases l WHERE l.task_id = t.task_id)",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        lost.is_empty(),
        "running with no attempt and no lease: {lost:?}"
    );
}

#[test]
fn a_launch_timeout_of_a_task_attempt_gives_the_task_a_retry() {
    use horch_core::fleetd::fleet_runtime::adapters::effects::{write_launch, LaunchRecord};
    use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;

    let mut w = world();
    let mut agent = Agent::sleeping();
    let (task, worker) = w.claimed_task(&agent);
    agent.kill();
    let attempt = w.attempts(&task)[0].0.clone();
    w.conn
        .execute(
            "UPDATE attempts SET state = 'launching', pid = NULL, pid_start = NULL WHERE id = ?1",
            [&attempt],
        )
        .unwrap();
    let record = LaunchRecord {
        attempt: attempt.clone(),
        launched_at: FleetSeconds(0),
        process: None,
    };
    write_launch(&w.root(), &record).unwrap();
    let fake = Fake::new(&w);

    w.pass_at(30, &worker, &fake);
    assert_eq!(w.attempts(&task)[0].1, "launching");
    assert_eq!(w.task(&task).status, "running");
    assert_no_lost_task(&w);

    for clock in [100, 160, 220] {
        w.pass_at(clock, &worker, &fake);
        assert_no_lost_task(&w);
    }
    let attempts = w.attempts(&task);
    assert_eq!(attempts[0].1, "ended", "{attempts:?}");
    assert_eq!(attempts[0].2.as_deref(), Some("timed_out"));
    let t = w.task(&task);
    assert_eq!(t.status, "ready", "rung 6 queued it again");
    assert_eq!(t.attempts, 1, "the timeout burned 1 attempt");
    assert!(t.not_before.is_some(), "a backoff");
    assert_eq!(i1_breaches(&w, &task), 0, "no I1 item");
}

fn i1_breaches(w: &World, task: &str) -> i64 {
    w.conn
        .query_row(
            "SELECT count(*) FROM needs_you WHERE key = ?1 AND state = 'open'",
            [format!("invariant_breach:I1:task:{task}")],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn a_running_task_with_no_attempt_and_no_lease_is_an_i1_breach() {
    let mut w = world();
    let mut agent = Agent::sleeping();
    let (task, worker) = w.claimed_task(&agent);
    agent.kill();
    let attempt = w.attempts(&task)[0].0.clone();
    w.conn
        .execute(
            "UPDATE attempts SET state = 'ended', outcome = 'timed_out' WHERE id = ?1",
            [&attempt],
        )
        .unwrap();
    w.conn
        .execute("DELETE FROM leases WHERE task_id = ?1", [&task])
        .unwrap();
    let fake = Fake::new(&w);

    w.pass_at(10, &worker, &fake);
    assert_eq!(w.task(&task).status, "running");
    assert_eq!(i1_breaches(&w, &task), 1, "the pass opened the I1 item");
}

#[test]
fn a_dead_attempt_of_a_leased_task_gives_the_task_a_retry() {
    let mut w = world();
    let mut agent = Agent::sleeping();
    let (task, worker) = w.claimed_task(&agent);
    w.conn
        .execute(
            "UPDATE tasks SET status = 'leased' WHERE task_id = ?1",
            [&task],
        )
        .unwrap();
    agent.kill();
    let fake = Fake::new(&w);

    for clock in [10, 20] {
        w.pass_at(clock, &worker, &fake);
        assert_no_lost_task(&w);
    }
    let attempts = w.attempts(&task);
    assert_eq!(attempts[0].1, "ended", "{attempts:?}");
    let t = w.task(&task);
    assert_eq!(t.status, "ready", "rung 6 queued it again");
    assert_eq!(i1_breaches(&w, &task), 0, "no I1 item");
}
