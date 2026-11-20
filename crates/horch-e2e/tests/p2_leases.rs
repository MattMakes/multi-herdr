//! The e2e tests of the Phase 2 gate (fleetd design, Migration plan item
//! 4): a stopped worker is caught within 1 lease (RR-1), an expired lease
//! never leaves a task assigned (RR-2), retries are bounded (RR-6), a cancel
//! stops the process of a live attempt before the task is `cancelled`, and
//! 2 workers never hold 1 task (FM-14).
//!
//! Each test runs the `horch` binary on its own ledger, with 1 forced pass
//! of fleetd for each `horch ledger watch --once`. The tests move 2 clocks
//! together, in steps, and no test sleeps for a lease:
//! - the calendar clock, with `HORCH_NOW` on each `horch` command. The age
//!   of the wrapper heartbeat is on this clock.
//! - the fleet clock, in the ledger ([`Harness::advance_fleet_clock`]).
//!   Each `horch` command is a new fleetd process, whose first tick adds 0.
//!
//! The stand-in worker has no supervisor: its last heartbeat is the start
//! of its agent. The tests read the stored task words
//! (`docs/tickets/fleetd-p2/MAP.md`, Q3).
//!
//! RR-1 is Unix only: `kill -STOP` has no Windows form (MAP.md, Q17). On
//! Windows the unit tests of the heartbeat and the stall ladder hold the
//! RR-1 row. Windows has no polite stop either, so FM-14 shows the old
//! process in its grace only on Unix.

use std::path::PathBuf;
use std::process::Output;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::ideas::application::questions::{self, Ask};
use horch_core::taskledger::Store;
use horch_e2e::bind::{bind, try_bind};
use horch_e2e::harness::{fixtures, wait_for, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};

const RUN: &str = "p2";

/// The design lease of a profile, in seconds of the fleet clock.
const LEASE_S: i64 = 120;

/// `heartbeat_max_age_s` of `fleet.toml`, the design default.
const HEARTBEAT_MAX_AGE_S: i64 = 30;

/// The step of the 2 clocks between 2 passes, in seconds.
const STEP_S: i64 = 10;

/// `stop_grace_s` of the tests: the time on the fleet clock from the polite
/// stop of a process to its kill.
const GRACE_S: i64 = 600;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A fleet with its own ledger file, a git repository and no watchdog, and
/// the 2 clocks of the test.
struct Fleet {
    h: Harness,
    /// The calendar time of the start of the test.
    base: String,
    /// The seconds that the test moved both clocks.
    offset: u64,
}

impl Fleet {
    fn new(name: &str) -> Fleet {
        let mut h = Harness::new(name).with_git();
        let db = h.root.join("ledger.sqlite3");
        h.set("HORCH_LEDGER_DB", db.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        let root = h.root.join("fleet");
        std::fs::create_dir_all(&root).unwrap();
        let config = format!("pass_period_s = 3600\nstop_grace_s = {GRACE_S}\n");
        std::fs::write(root.join("fleet.toml"), config).unwrap();
        h.serve_fleetd();
        Fleet {
            h,
            base: horch_core::clock::now_stamp(),
            offset: 0,
        }
    }

    fn db(&self) -> PathBuf {
        self.h.root.join("ledger.sqlite3")
    }

    /// The calendar time of the test: the start plus the moves.
    fn now(&self) -> String {
        let base = horch_core::clock::parse(&self.base).unwrap();
        horch_core::clock::stamp(base + Duration::from_secs(self.offset))
    }

    /// Run `horch` at the calendar time of the test, as `worker` if any.
    fn run_as(&self, worker: Option<&str>, args: &[&str]) -> Output {
        let mut cmd = self.h.horch(args);
        cmd.env("HORCH_NOW", self.now());
        if let Some(w) = worker {
            cmd.env("HORCH_WORKER_ID", w);
        }
        cmd.output().unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let o = self.run_as(None, args);
        assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
        text(&o)
    }

    /// 1 forced pass of fleetd, while the test fleetd is down.
    fn pass(&self) -> String {
        let o = self.h.without_fleetd(|| {
            self.run_as(
                None,
                &[
                    "ledger", "watch", "--once", "--force", "--json", "--run", RUN,
                ],
            )
        });
        assert!(o.status.success(), "horch ledger watch: {}", text(&o));
        text(&o)
    }

    /// Move both clocks `s` seconds forward.
    fn advance_clocks(&mut self, s: i64) {
        self.h.advance_fleet_clock(s);
        self.offset += u64::try_from(s).unwrap();
    }

    /// Move both clocks `s` seconds forward, then run 1 pass.
    fn pass_after(&mut self, s: i64) -> String {
        self.advance_clocks(s);
        self.pass()
    }

    fn run_with_task(&self, max_attempts: Option<i64>) {
        let run = seed_run(&self.h, RUN, None, "w1:p1");
        if let Some(max) = max_attempts {
            Store::open(&self.db())
                .unwrap()
                .conn()
                .execute(
                    "UPDATE runs SET max_attempts = ?1 WHERE run_id = ?2",
                    rusqlite::params![max, run.run_id],
                )
                .unwrap();
        }
        let dir = self.h.project.join("tickets");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t1.md");
        std::fs::write(&p, "# T1\n").unwrap();
        seed_task(&self.h, &run, &p, "opus");
        self.pass();
    }

    /// A new worker of `opus` with `agent` and no supervisor, bound to T-1
    /// (FR-5). Returns its id.
    fn claimed(&self, agent: &Agent) -> String {
        let w = bind(&self.h, "opus", Some(agent.pid)).worker_id;
        assert_eq!(self.task().status, "running");
        w
    }

    /// The stored facts of T-1.
    fn task(&self) -> Task {
        let store = Store::open(&self.db()).unwrap();
        store
            .conn()
            .query_row(
                "SELECT task_id, status, attempts, attempt_floor, replans, not_before
                 FROM tasks",
                [],
                |r| {
                    Ok(Task {
                        id: r.get(0)?,
                        status: r.get(1)?,
                        attempts: r.get(2)?,
                        attempt_floor: r.get(3)?,
                        replans: r.get(4)?,
                        not_before: r.get(5)?,
                    })
                },
            )
            .unwrap()
    }

    /// 1 number from the ledger.
    fn number(&self, sql: &str) -> i64 {
        let store = Store::open(&self.db()).unwrap();
        store.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn clock(&self) -> i64 {
        self.number("SELECT fleet_clock_s FROM fleetd")
    }

    fn lease_rows(&self) -> i64 {
        self.number("SELECT COUNT(*) FROM leases")
    }

    /// The fleet clock at which the lease of T-1 expires.
    fn lease_expiry(&self) -> i64 {
        self.number("SELECT expires_at FROM leases")
    }

    /// The attempt of worker `w` and its state.
    fn attempt_of(&self, w: &str) -> (String, String) {
        let store = Store::open(&self.db()).unwrap();
        store
            .conn()
            .query_row(
                "SELECT id, state FROM attempts WHERE worker_id = ?1",
                [w],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    /// The attempts of T-1 that are not ended.
    fn open_attempts(&self) -> i64 {
        self.number("SELECT COUNT(*) FROM attempts WHERE task_id IS NOT NULL AND state <> 'ended'")
    }

    /// The id of the first transition event of `entity` `id` to `to`.
    fn moved(&self, entity: &str, id: &str, to: &str) -> Option<i64> {
        let store = Store::open(&self.db()).unwrap();
        store
            .conn()
            .query_row(
                "SELECT event_id FROM events WHERE entity = ?1 AND entity_id = ?2
                 AND to_state = ?3 ORDER BY event_id LIMIT 1",
                [entity, id, to],
                |r| r.get(0),
            )
            .ok()
    }

    /// Passes, each [`STEP_S`] after the last, until T-1 is not `running`,
    /// at most `limit_s`. Before each pass, `before` sees the fleet. Gives
    /// the fleet clock of the pass that moved it, and the outputs.
    fn passes_until_not_running(
        &mut self,
        limit_s: i64,
        mut before: impl FnMut(&Fleet),
    ) -> (i64, Vec<String>) {
        let mut log = Vec::new();
        for _ in 0..limit_s / STEP_S {
            before(self);
            log.push(self.pass_after(STEP_S));
            if self.task().status != "running" {
                return (self.clock(), log);
            }
        }
        panic!(
            "T-1 is still running after {limit_s} s of the fleet clock\n{}",
            log.join("\n")
        );
    }
}

/// The stored facts of T-1.
#[derive(Debug)]
struct Task {
    id: String,
    status: String,
    attempts: i64,
    attempt_floor: i64,
    replans: i64,
    not_before: Option<i64>,
}

/// A task word that holds the task for an attempt.
fn assigned(status: &str) -> bool {
    matches!(status, "leased" | "running")
}

/// The stand-in agent: a process whose end is seen at once, because on
/// Unix a thread of the test reaps it and on Windows no zombie stays. On
/// Unix it leads its own process group, as a real agent does, so the stop
/// of fleetd reaches its whole group. Drop kills it (on Unix, its group),
/// so a failed assertion leaks no process.
struct Agent {
    pid: u32,
    start: u64,
}

impl Agent {
    /// A process that sleeps.
    fn start() -> Agent {
        #[cfg(unix)]
        return Agent::spawn("/bin/sleep 600");
        #[cfg(windows)]
        return Agent::spawn_windows();
    }

    /// A process group whose leader ignores the polite stop (`SIGTERM`):
    /// only the kill after the grace ends the group. Windows has no polite
    /// stop, so there it is [`Agent::start`].
    fn stubborn() -> Agent {
        #[cfg(unix)]
        return Agent::spawn("/bin/sh -c 'trap \"\" TERM; while :; do /bin/sleep 1; done'");
        #[cfg(windows)]
        return Agent::start();
    }

    /// `process_group(0)` starts the agent in its own process group. A
    /// thread waits on the child, so no zombie stays after its end. `exec`
    /// replaces `/bin/sh` with the command, so the command leads the group.
    /// Without `exec`, `dash` (the `/bin/sh` of Debian) stays the group
    /// leader. (`set -m` in `sh -c` makes no group in `dash`.)
    #[cfg(unix)]
    fn spawn(command: &str) -> Agent {
        use std::os::unix::process::CommandExt;
        use std::process::Stdio;
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", &format!("exec {command}")])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        std::thread::spawn(move || child.wait());
        Agent::of(pid)
    }

    /// `ping` for 600 s. Windows keeps no zombie, and the test holds no
    /// handle to it.
    #[cfg(windows)]
    fn spawn_windows() -> Agent {
        let child = std::process::Command::new("ping")
            .args(["-n", "600", "127.0.0.1"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        Agent::of(child.id())
    }

    fn of(pid: u32) -> Agent {
        let start = horch_core::procid::start_time(pid).expect("the start time of the agent");
        #[cfg(unix)]
        assert!(
            horch_core::procid::is_same_group(pid, start),
            "agent {pid} does not lead its own process group"
        );
        Agent { pid, start }
    }

    fn alive(&self) -> bool {
        horch_core::procid::alive(self.pid, Some(self.start))
    }

    /// Whether the agent and each process of its group are gone. Windows
    /// has no process groups: there it is the agent alone.
    fn gone(&self) -> bool {
        !self.alive() && horch_core::procid::group_members(self.pid).is_empty()
    }

    /// Send `sig` to the group of the agent, while the agent leads it.
    #[cfg(unix)]
    fn signal_group(&self, sig: libc::c_int) {
        if horch_core::procid::is_same_group(self.pid, self.start) {
            // SAFETY: kill(2) on the process group of the agent of this
            // test, while the agent with its start time leads it.
            unsafe { libc::kill(-(self.pid as libc::pid_t), sig) };
        }
    }

    /// `kill -STOP`: the process stays, with no output and no progress.
    #[cfg(unix)]
    fn freeze(&self) {
        self.signal_group(libc::SIGSTOP);
        wait_for("the stopped state of the agent", || {
            let o = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &self.pid.to_string()])
                .output()
                .unwrap();
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .starts_with('T')
                .then_some(())
        });
    }

    fn stop(&self) {
        #[cfg(unix)]
        {
            self.signal_group(libc::SIGCONT);
            self.signal_group(libc::SIGKILL);
        }
        #[cfg(windows)]
        horch_core::procid::stop_same(self.pid, self.start, horch_core::procid::Stop::Kill);
    }

    /// Kill the agent and wait until it is gone.
    fn kill(&self) {
        self.stop();
        wait_for("the end of the agent", || self.gone().then_some(()));
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(unix)]
#[test]
fn rr1_stopped_worker_is_caught_within_1_lease() {
    let mut f = Fleet::new("p2-rr1");
    f.run_with_task(None);
    let agent = Agent::start();
    let w = f.claimed(&agent);
    assert_eq!(
        f.lease_expiry() - f.clock(),
        LEASE_S,
        "the claim takes 1 lease"
    );
    agent.freeze();
    let frozen = f.clock();
    let mut expiry = f.lease_expiry();
    let (caught, log) = f.passes_until_not_running(600, |f| {
        expiry = f.lease_expiry();
    });
    let log = log.join("\n");
    let renewed = expiry - LEASE_S;
    assert!(
        caught - renewed <= LEASE_S + STEP_S,
        "caught {} s after the last renewal; 1 lease is {LEASE_S} s\n{log}",
        caught - renewed
    );
    assert!(
        caught - frozen <= HEARTBEAT_MAX_AGE_S + LEASE_S + STEP_S,
        "caught {} s after the kill -STOP\n{log}",
        caught - frozen
    );
    let t = f.task();
    assert!(
        matches!(
            t.status.as_str(),
            "stalled" | "ready" | "failed" | "escalated"
        ),
        "{} is {}\n{log}",
        t.id,
        t.status
    );
    let (_, state) = f.attempt_of(&w);
    assert_eq!(state, "ended", "{log}");
}

#[test]
fn rr2_expired_lease_never_leaves_the_task_assigned() {
    let mut f = Fleet::new("p2-rr2");
    f.run_with_task(None);
    let agent = Agent::start();
    let w = f.claimed(&agent);
    let (_, log) = f.passes_until_not_running(600, |_| {});
    let log = log.join("\n");
    let t = f.task();
    assert!(
        matches!(t.status.as_str(), "stalled" | "ready" | "escalated"),
        "after its lease expired, {} is {}\n{log}",
        t.id,
        t.status
    );
    assert_eq!(f.lease_rows(), 0, "the lease row stays\n{log}");
    let (attempt, state) = f.attempt_of(&w);
    assert_eq!(state, "ended", "attempt {attempt}\n{log}");
    assert_eq!(f.open_attempts(), 0, "{log}");
    // The next passes leave it unassigned and with no lease.
    for _ in 0..3 {
        let o = f.pass_after(STEP_S);
        let t = f.task();
        assert!(!assigned(&t.status), "{} is {}\n{o}", t.id, t.status);
        assert_eq!(f.lease_rows(), 0, "{o}");
    }
}

/// The phases of RR-6, each 1 failed attempt at a time.
impl Fleet {
    /// Wait out the backoff of rung 6. A bind in the backoff gets no task.
    fn through_backoff(&mut self) {
        let Some(nb) = self.task().not_before.filter(|nb| *nb > self.clock()) else {
            return;
        };
        let early = Agent::start();
        let o = try_bind(&self.h, "opus", Some(early.pid));
        assert!(o.is_err(), "{o:?}");
        assert!(!assigned(&self.task().status), "{o:?}");
        self.advance_clocks(nb - self.clock());
    }

    fn failed_attempt(&mut self) -> String {
        self.through_backoff();
        let agent = Agent::start();
        self.claimed(&agent);
        agent.kill();
        let mut log = self.pass();
        wait_for("the rescue of the tree of the failed attempt", || {
            let orphaned = self.number("SELECT COUNT(*) FROM worktrees WHERE state = 'orphaned'");
            if orphaned == 0 {
                return Some(());
            }
            log.push_str(&self.pass_after(STEP_S));
            None
        });
        log
    }

    /// Fail attempts until T-1 is not `ready`. Each status on the way is
    /// asserted against the limit `max`.
    fn fail_until_not_ready(&mut self, max: i64, log: &mut Vec<String>) {
        while self.task().status == "ready" {
            log.push(self.failed_attempt());
            let t = self.task();
            assert!(
                t.attempts - t.attempt_floor <= max,
                "{} attempts since the last requeue, limit {max}\n{}",
                t.attempts - t.attempt_floor,
                log.join("\n")
            );
        }
    }
}

#[test]
fn rr6_task_that_fails_each_attempt_escalates_after_max_attempts() {
    const MAX: i64 = 2;
    let mut f = Fleet::new("p2-rr6");
    f.run_with_task(Some(MAX));
    let mut log = Vec::new();
    // The first MAX attempts fail: the task is `failed`.
    f.fail_until_not_ready(MAX, &mut log);
    let t = f.task();
    assert_eq!(t.status, "failed", "{t:?}\n{}", log.join("\n"));
    assert_eq!(t.attempts, MAX, "{t:?}\n{}", log.join("\n"));
    // The replan.
    log.push(f.ok(&["task", "requeue", "T-1", "replan", "--run", RUN]));
    // The next MAX attempts fail: the task is `escalated`.
    f.fail_until_not_ready(MAX, &mut log);
    let log = log.join("\n");
    let t = f.task();
    assert_eq!(t.status, "escalated", "{t:?}\n{log}");
    assert_eq!(t.replans, 1, "{t:?}\n{log}");
    assert_eq!(
        t.attempts,
        2 * MAX,
        "MAX attempts, the replan, MAX attempts\n{log}"
    );
    let c = rusqlite::Connection::open(f.db()).unwrap();
    let kinds: Vec<String> = c
        .prepare(
            "SELECT kind FROM needs_you WHERE state = 'open' AND entity = 'task'
             AND entity_id = (SELECT task_id FROM tasks) ORDER BY kind",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        kinds,
        ["held", "task_escalated"],
        "the escalation item and the question of the held task name the escalated task, and no other item does\n{log}"
    );
    // An escalated task gets no attempt by itself.
    let o = f.pass_after(3600);
    assert_eq!(f.task().status, "escalated", "{o}");
    assert_eq!(f.task().attempts, 2 * MAX, "{o}");
}

#[test]
fn cancel_of_a_running_task_stops_its_process_before_the_task_is_cancelled() {
    let f = Fleet::new("p2-cancel-running");
    f.run_with_task(None);
    let agent = Agent::start();
    let w = f.claimed(&agent);
    let (attempt, _) = f.attempt_of(&w);
    let mut o = f.ok(&["task", "cancel", "T-1", "not needed", "--run", RUN]);
    assert!(o.contains("T-1 is not cancelled yet"), "{o}");
    wait_for("the cancel of the task by a pass", || {
        o.push_str(&f.pass());
        (f.task().status == "cancelled").then_some(())
    });
    let t = f.task();
    assert_eq!(t.status, "cancelled", "{o}");
    assert!(!agent.alive(), "the agent outlived the cancel\n{o}");
    let ended = f.moved("attempt", &attempt, "ended");
    let cancelled = f.moved("task", &t.id, "cancelled");
    assert!(
        ended.is_some() && ended < cancelled,
        "the attempt end (event {ended:?}) is not before the cancel (event {cancelled:?})\n{o}"
    );
    assert_eq!(f.lease_rows(), 0, "{o}");
}

#[test]
fn cancel_of_a_blocked_on_user_task_is_immediate() {
    let f = Fleet::new("p2-cancel-blocked");
    f.run_with_task(None);
    let agent = Agent::start();
    let w = f.claimed(&agent);
    let (attempt, _) = f.attempt_of(&w);
    let t = f.task();
    let clock = FleetSeconds(f.clock());
    // The worker asks a question, then exits: rung 5 gives `asked`.
    let now = f.now();
    let ask = Ask {
        task_id: &t.id,
        attempt_id: &attempt,
        mail_id: Some("m1"),
        text: "which way?",
        asked_at: &now,
    };
    let mut store = Store::open(&f.db()).unwrap();
    store
        .immediate(|tx| questions::ask(tx, &ask, clock, 900))
        .unwrap();
    drop(store);
    agent.kill();
    let o = f.pass();
    assert_eq!(f.task().status, "blocked_on_user", "{o}");
    let o = f.ok(&["task", "cancel", "T-1", "not needed", "--run", RUN]);
    assert!(o.contains("T-1 cancelled."), "{o}");
    assert_eq!(f.task().status, "cancelled", "{o}");
}

#[test]
fn cancel_of_a_waiting_quota_task_is_immediate() {
    let mut f = Fleet::new("p2-cancel-quota");
    // The claude pool of opus and the codex pool of its fallback are full
    // until 2099.
    let fixture = fixtures().join("quota/all-exhausted.json");
    let quota = std::fs::read_to_string(fixture)
        .unwrap()
        .replace("\"resets_at\": \"2026-", "\"resets_at\": \"2099-");
    let file = f.h.root.join("quota.json");
    std::fs::write(&file, quota).unwrap();
    f.h.set("HORCH_QUOTA_FILE", file.to_string_lossy());
    f.run_with_task(None);
    let agent = Agent::start();
    f.claimed(&agent);
    let o = f.pass();
    assert_eq!(f.task().status, "waiting_quota", "{o}");
    wait_for("the stop of the agent by the quota rule", || {
        agent.gone().then_some(())
    });
    let o = f.ok(&["task", "cancel", "T-1", "not needed", "--run", RUN]);
    assert!(o.contains("T-1 cancelled."), "{o}");
    assert_eq!(f.task().status, "cancelled", "{o}");
    assert_eq!(f.lease_rows(), 0, "{o}");
    assert!(agent.gone(), "the agent lives after the cancel\n{o}");
}

#[test]
fn fm14_no_second_attempt_starts_while_the_old_process_lives() {
    let mut f = Fleet::new("p2-fm14");
    f.run_with_task(None);
    let old = Agent::stubborn();
    let first = f.claimed(&old);
    let (_, log) = f.passes_until_not_running(600, |_| {});
    let mut log = log;
    let (_, state) = f.attempt_of(&first);
    assert_eq!(state, "ended", "{}", log.join("\n"));
    #[cfg(unix)]
    assert!(old.alive(), "the old process lives in its grace");
    // A new worker tries to bind after each pass, while the old process
    // lives in its grace and after the kill.
    let mut agents = Vec::new();
    for _ in 0..=(GRACE_S / 60 + 2) {
        log.push(f.pass_after(60));
        let agent = Agent::start();
        let bound = try_bind(&f.h, "opus", Some(agent.pid));
        agents.push(agent);
        log.push(format!("{bound:?}"));
        assert!(f.lease_rows() <= 1, "{}", log.join("\n"));
        assert!(f.open_attempts() <= 1, "{}", log.join("\n"));
        if assigned(&f.task().status) {
            assert!(
                old.gone(),
                "a second attempt started while a process of the first lives\n{}",
                log.join("\n")
            );
            let w = bound
                .expect("the task is assigned to the new bind")
                .worker_id;
            let (_, state) = f.attempt_of(&w);
            assert_ne!(state, "ended", "{}", log.join("\n"));
            assert_eq!(f.lease_rows(), 1, "{}", log.join("\n"));
            return;
        }
    }
    panic!(
        "no second attempt started after the old process was gone\n{}",
        log.join("\n")
    );
}
