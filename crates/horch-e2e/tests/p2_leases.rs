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
//! Unix only: `kill -STOP` has no Windows form (MAP.md, Q17). On Windows the
//! unit tests of the heartbeat and the stall ladder hold the RR-1 row.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::ideas::application::questions::{self, Ask};
use horch_core::taskledger::Store;
use horch_e2e::harness::{fixtures, wait_for, Harness};

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

    /// 1 forced pass of fleetd.
    fn pass(&self) -> String {
        let o = self.run_as(None, &["ledger", "watch", "--once", "--force", "--json"]);
        assert!(o.status.success(), "horch ledger watch: {}", text(&o));
        text(&o)
    }

    /// Move both clocks `s` seconds forward.
    fn wait(&mut self, s: i64) {
        self.h.advance_fleet_clock(s);
        self.offset += u64::try_from(s).unwrap();
    }

    /// Move both clocks `s` seconds forward, then run 1 pass.
    fn pass_after(&mut self, s: i64) -> String {
        self.wait(s);
        self.pass()
    }

    /// 1 run with the extra flags `run` and 1 task T-1 of `opus`. 1 pass
    /// writes the `fleetd` row, so the fleet clock exists before the claim.
    fn run_with_task(&self, run: &[&str]) {
        let mut new = vec!["ledger", "run", "new", "p2"];
        new.extend_from_slice(run);
        self.ok(&new);
        let dir = self.h.project.join("tickets");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t1.md");
        std::fs::write(&p, "# T1\n").unwrap();
        let p = p.to_string_lossy();
        self.ok(&["ledger", "add", "--file", &p, "--primary", "opus"]);
        self.pass();
    }

    /// A new worker of `opus` whose agent is `agent`, with no supervisor.
    /// Returns its id.
    fn worker(&self, agent: &Agent) -> String {
        let o = self.run_as(None, &["ledger", "worker", "new", "--persona", "opus"]);
        assert!(o.status.success(), "{}", text(&o));
        let w = String::from_utf8_lossy(&o.stdout).trim().to_string();
        horch_core::taskledger::supervisor::agent_started(&self.db(), &w, agent.pid).unwrap();
        w
    }

    /// A new worker with `agent` that claims T-1.
    fn claimed(&self, agent: &Agent) -> String {
        let w = self.worker(agent);
        let claim = self.run_as(Some(&w), &["ledger", "claim"]);
        assert!(claim.status.success(), "{}", text(&claim));
        assert_eq!(self.task().status, "running", "{}", text(&claim));
        w
    }

    /// The stored facts of T-1.
    fn task(&self) -> Task {
        let store = Store::open(&self.db()).unwrap();
        store
            .conn()
            .query_row(
                "SELECT task_id, run_id, status, attempts, attempt_floor, replans, not_before
                 FROM tasks",
                [],
                |r| {
                    Ok(Task {
                        id: r.get(0)?,
                        run_id: r.get(1)?,
                        status: r.get(2)?,
                        attempts: r.get(3)?,
                        attempt_floor: r.get(4)?,
                        replans: r.get(5)?,
                        not_before: r.get(6)?,
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
    run_id: String,
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

/// The stand-in agent: a process that is not a child of the test, so its
/// end is seen at once and no zombie looks alive. Drop continues it and
/// kills it, so a failed assertion leaks no process.
struct Agent {
    pid: u32,
    start: u64,
}

impl Agent {
    /// A `sleep` process.
    fn start() -> Agent {
        Agent::spawn("/bin/sleep 600")
    }

    /// A process that ignores the polite stop (`SIGTERM`): only the kill
    /// after the grace ends it.
    fn stubborn() -> Agent {
        Agent::spawn("/bin/sh -c 'trap \"\" TERM; while :; do /bin/sleep 1; done'")
    }

    fn spawn(command: &str) -> Agent {
        let o = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                &format!("{command} </dev/null >/dev/null 2>&1 & echo $!"),
            ])
            .output()
            .unwrap();
        let pid: u32 = String::from_utf8_lossy(&o.stdout).trim().parse().unwrap();
        let start = horch_core::procid::start_time(pid).expect("the start time of the agent");
        Agent { pid, start }
    }

    fn alive(&self) -> bool {
        horch_core::procid::alive(self.pid, Some(self.start))
    }

    fn signal(&self, sig: libc::c_int) {
        if self.alive() {
            // SAFETY: kill(2) on the agent of this test, while it has the
            // start time that the test read.
            unsafe { libc::kill(self.pid as libc::pid_t, sig) };
        }
    }

    /// `kill -STOP`: the process stays, with no output and no progress.
    fn freeze(&self) {
        self.signal(libc::SIGSTOP);
    }

    /// Kill the agent and wait until it is gone.
    fn kill(&self) {
        self.signal(libc::SIGCONT);
        self.signal(libc::SIGKILL);
        wait_for("the end of the agent", || (!self.alive()).then_some(()));
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.signal(libc::SIGCONT);
        self.signal(libc::SIGKILL);
    }
}

#[test]
fn rr1_stopped_worker_is_caught_within_1_lease() {
    let mut f = Fleet::new("p2-rr1");
    f.run_with_task(&[]);
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
    f.run_with_task(&[]);
    let agent = Agent::start();
    let w = f.claimed(&agent);
    let (_, log) = f.passes_until_not_running(600, |_| {});
    let log = log.join("\n");
    let t = f.task();
    assert!(
        t.status == "ready" || t.status == "escalated",
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

#[test]
fn rr6_task_that_fails_each_attempt_escalates_after_max_attempts() {
    const MAX: i64 = 2;
    let mut f = Fleet::new("p2-rr6");
    f.run_with_task(&["--max-attempts", &MAX.to_string()]);
    let mut log = Vec::new();
    let mut replanned = false;
    // Each round is 1 attempt; 2 rounds of MAX attempts and 1 more round,
    // which must find the task escalated.
    for _ in 0..=(2 * MAX) {
        let t = f.task();
        match t.status.as_str() {
            "escalated" => break,
            "failed" => {
                assert!(
                    !replanned,
                    "a second replan of {}\n{}",
                    t.id,
                    log.join("\n")
                );
                assert_eq!(t.attempts - t.attempt_floor, MAX, "{}", log.join("\n"));
                log.push(f.ok(&["task", "requeue", "T-1", "replan"]));
                replanned = true;
            }
            "ready" => {}
            other => panic!("T-1 is {other}\n{}", log.join("\n")),
        }
        if let Some(nb) = f.task().not_before.filter(|nb| *nb > f.clock()) {
            // The backoff of rung 6 holds the claim.
            let early = Agent::start();
            let w = f.worker(&early);
            let o = f.run_as(Some(&w), &["ledger", "claim"]);
            assert!(!assigned(&f.task().status), "{}", text(&o));
            f.wait(nb - f.clock());
        }
        let agent = Agent::start();
        f.claimed(&agent);
        // The attempt exits with no result.
        agent.kill();
        log.push(f.pass());
        let t = f.task();
        assert!(
            t.attempts - t.attempt_floor <= MAX,
            "{} attempts since the last requeue, limit {MAX}\n{}",
            t.attempts - t.attempt_floor,
            log.join("\n")
        );
    }
    let log = log.join("\n");
    let t = f.task();
    assert_eq!(t.status, "escalated", "{t:?}\n{log}");
    assert!(replanned, "no replan before the escalation\n{log}");
    assert_eq!(t.replans, 1, "{t:?}\n{log}");
    assert_eq!(
        t.attempts,
        2 * MAX,
        "MAX attempts, the replan, MAX attempts\n{log}"
    );
    let items = f.number(
        "SELECT COUNT(*) FROM needs_you WHERE state = 'open' AND entity = 'task'
         AND entity_id = (SELECT task_id FROM tasks)",
    );
    assert_eq!(items, 1, "1 needs-you item names the escalated task\n{log}");
    // An escalated task gets no attempt by itself.
    let o = f.pass_after(3600);
    assert_eq!(f.task().status, "escalated", "{o}");
    assert_eq!(f.task().attempts, 2 * MAX, "{o}");
}

#[test]
fn cancel_of_a_running_task_stops_its_process_before_the_task_is_cancelled() {
    let f = Fleet::new("p2-cancel-running");
    f.run_with_task(&[]);
    let agent = Agent::start();
    let w = f.claimed(&agent);
    let (attempt, _) = f.attempt_of(&w);
    let o = f.ok(&["task", "cancel", "T-1", "not needed"]);
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
    f.run_with_task(&[]);
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
        run_id: &t.run_id,
        owner: "orchestrator",
        mail_id: "m1",
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
    let o = f.ok(&["task", "cancel", "T-1", "not needed"]);
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
    f.run_with_task(&[]);
    let agent = Agent::start();
    f.claimed(&agent);
    let o = f.pass();
    assert_eq!(f.task().status, "waiting_quota", "{o}");
    let o = f.ok(&["task", "cancel", "T-1", "not needed"]);
    assert!(o.contains("T-1 cancelled."), "{o}");
    assert_eq!(f.task().status, "cancelled", "{o}");
    assert_eq!(f.lease_rows(), 0, "{o}");
}

#[test]
fn fm14_no_second_attempt_starts_while_the_old_process_lives() {
    let mut f = Fleet::new("p2-fm14");
    f.run_with_task(&[]);
    let old = Agent::stubborn();
    let first = f.claimed(&old);
    let (_, log) = f.passes_until_not_running(600, |_| {});
    let mut log = log;
    let (_, state) = f.attempt_of(&first);
    assert_eq!(state, "ended", "{}", log.join("\n"));
    assert!(old.alive(), "the old process lives in its grace");
    // A new worker tries to claim after each pass, while the old process
    // lives in its grace and after the kill.
    let mut agents = Vec::new();
    for _ in 0..=(GRACE_S / 60 + 2) {
        log.push(f.pass_after(60));
        let agent = Agent::start();
        let w = f.worker(&agent);
        agents.push(agent);
        let claim = f.run_as(Some(&w), &["ledger", "claim"]);
        log.push(text(&claim));
        assert!(f.lease_rows() <= 1, "{}", log.join("\n"));
        assert!(f.open_attempts() <= 1, "{}", log.join("\n"));
        if assigned(&f.task().status) {
            assert!(
                !old.alive(),
                "a second attempt started while the process of the first lives\n{}",
                log.join("\n")
            );
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
