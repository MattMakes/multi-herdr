//! Phase 0 red tests of worker liveness (fleetd design, Migration plan items
//! 1, 3 and 4): an idle claim (FM-3), a dead agent (FM-4), a usage limit
//! (FM-5) and a machine sleep. Each test is on the list of expected failures
//! in `expected-failures/p0_liveness.txt` until its phase makes it green.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Child, Output};
use std::time::{Duration, Instant};

use horch_core::taskledger::{runs, Store};
use horch_e2e::expected::check;
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{fixtures, Harness};
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    text(&o)
}

fn json_of(h: &Harness, args: &[&str]) -> Value {
    let o = h.run(args);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

fn ticket(h: &Harness, name: &str, body: &str) -> PathBuf {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.md"));
    std::fs::write(&p, body).unwrap();
    p
}

/// A harness with its own ledger file and a git repository, and no watchdog.
fn fleet(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set("HORCH_LEDGER_DB", db(&h).to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

fn db(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

/// Run `horch` as worker `worker`, with the extra environment `env`.
fn as_worker(h: &Harness, worker: &str, env: &[(&str, &str)], args: &[&str]) -> Output {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", worker);
    cmd.envs(env.iter().copied());
    cmd.output().unwrap()
}

/// 1 forced watchdog pass, as today's checks run it.
fn watch_once(h: &Harness, env: &[(&str, &str)]) -> (Output, Value) {
    let mut cmd = h.horch(&["ledger", "watch", "--once", "--force", "--json"]);
    cmd.envs(env.iter().copied());
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "horch ledger watch: {}", text(&o));
    let v = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
    (o, v)
}

/// The stand-in agent: a `sleep` process, killed and waited on drop, so a
/// failed assertion leaks no process.
struct Agent(Child);

impl Agent {
    fn start() -> Agent {
        Agent(
            std::process::Command::new("/bin/sleep")
                .arg("600")
                .spawn()
                .unwrap(),
        )
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    /// `SIGKILL`, then reap it, so that no zombie looks alive.
    fn kill(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.kill();
    }
}

/// 1 task of `persona` (with `fallback`, if any) in a new run, and 1 worker
/// of `persona` whose agent is a live `sleep` process. Returns the worker id.
fn worker_with_agent(
    h: &Harness,
    run: &[&str],
    persona: &str,
    fallback: Option<&str>,
    agent: &Agent,
) -> String {
    let mut new = vec!["ledger", "run", "new", "eps"];
    new.extend_from_slice(run);
    ok(h, &new);
    let p = ticket(h, "t0", "# T0\n");
    let p = p.to_string_lossy();
    let mut add = vec!["ledger", "add", "--file", &p, "--primary", persona];
    if let Some(f) = fallback {
        add.extend_from_slice(&["--fallback", f]);
    }
    ok(h, &add);
    let o = h.run(&["ledger", "worker", "new", "--persona", persona]);
    assert!(o.status.success(), "{}", text(&o));
    let w = String::from_utf8_lossy(&o.stdout).trim().to_string();
    horch_core::taskledger::supervisor::agent_started(&db(h), &w, agent.pid()).unwrap();
    w
}

/// The status and the attempt count of the only task of the ledger.
fn task_row(h: &Harness) -> (String, i64) {
    let store = Store::open(&db(h)).unwrap();
    store
        .conn()
        .query_row("SELECT status, attempts FROM tasks", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
}

#[test]
fn fm3_idle_claim_is_caught_in_under_10_min() {
    check(file!(), "fm3_idle_claim_is_caught_in_under_10_min", || {
        let h = fleet("p0-fm3");
        let o = ok(&h, &["ledger", "run", "new", "eps"]);
        let store = Store::open(&db(&h)).unwrap();
        let run = runs::of_project(store.conn(), &h.project)
            .unwrap()
            .pop()
            .expect("the run");
        // Today a worker with no progress is reaped after `stall_rounds`
        // checks of `check_every_s` each.
        let reap_s = run.check_every_s * run.stall_rounds;
        assert!(
            reap_s < 600,
            "an idle claim is reaped {reap_s} s after its last activity \
             (check_every_s {} x stall_rounds {}); the design catches it in \
             under 600 s (stall 300 s, lease 120 s)\n{o}",
            run.check_every_s,
            run.stall_rounds
        );
    });
}

#[test]
fn fm4_dead_agent_ends_its_attempt_within_30_s() {
    check(
        file!(),
        "fm4_dead_agent_ends_its_attempt_within_30_s",
        || {
            let h = fleet("p0-fm4");
            let mut agent = Agent::start();
            let w = worker_with_agent(&h, &[], "opus", None, &agent);
            let claim = as_worker(&h, &w, &[], &["ledger", "claim"]);
            assert!(claim.status.success(), "{}", text(&claim));
            // fleetd supervises the run, with the default policy.
            let mut fleetds = Fleetds::new(&h);
            fleetds.start(&h, "fleetd");
            agent.kill();
            // No other `horch` call: only fleetd can see the death.
            let killed = Instant::now();
            let mut status = task_row(&h).0;
            while killed.elapsed() < Duration::from_secs(35) {
                if status == "queued" || status == "ready" {
                    return;
                }
                std::thread::sleep(Duration::from_millis(500));
                status = task_row(&h).0;
            }
            panic!(
                "the task is still {status} {} s after its agent died; the design \
             ends the attempt within 30 s (a pass every 5 s)\nclaim: {}",
                killed.elapsed().as_secs(),
                text(&claim)
            );
        },
    );
}

#[test]
fn fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt() {
    check(
        file!(),
        "fm5_usage_limited_worker_is_not_reaped_and_burns_no_attempt",
        || {
            let mut h = fleet("p0-fm5");
            let quota = fixtures().join("quota/claude-exhausted-codex-ok.json");
            h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
            let agent = Agent::start();
            let run = ["--check-every", "1", "--stall-rounds", "1"];
            let w = worker_with_agent(&h, &run, "opus", Some("codex-sol"), &agent);
            let claim = as_worker(&h, &w, &[], &["ledger", "claim"]);
            assert!(claim.status.success(), "{}", text(&claim));
            let (_, attempts) = task_row(&h);
            // Run today's checks until a reap is due.
            let mut seen = Vec::new();
            for _ in 0..3 {
                std::thread::sleep(Duration::from_millis(1100));
                let (o, v) = watch_once(&h, &[]);
                let verdict = v[0]["verdict"].as_str().unwrap_or_default().to_string();
                assert!(
                    verdict != "dead",
                    "the usage-limited worker was reaped as dead; the design \
                     never reaps it (FM-5)\n{}",
                    text(&o)
                );
                seen.push(text(&o));
            }
            let alive = horch_core::procid::alive(agent.pid(), None);
            assert!(alive, "the agent was stopped\n{}", seen.join("\n"));
            let shown = json_of(&h, &["ledger", "show", "T-1", "--json"]);
            let (status, now) = task_row(&h);
            assert_eq!(now, attempts, "the usage limit burned an attempt\n{shown}");
            let persona = shown["task"]["claimable_by"].as_str().unwrap_or_default();
            assert!(
                status == "waiting_quota" || persona == "codex-sol",
                "the task is {status} for {persona}; the design moves it to \
                 the fallback codex-sol or to waiting_quota\n{shown}"
            );
        },
    );
}

#[test]
fn sleep_does_not_expire_a_lease() {
    check(file!(), "sleep_does_not_expire_a_lease", || {
        let h = fleet("p0-sleep");
        let agent = Agent::start();
        let w = worker_with_agent(&h, &[], "opus", None, &agent);
        let claimed = [("HORCH_NOW", "2030-01-01T00:00:00Z")];
        let claim = as_worker(&h, &w, &claimed, &["ledger", "claim"]);
        assert!(claim.status.success(), "{}", text(&claim));
        // The wall clock after a sleep of 2 h. The agent stays alive.
        let (o, _) = watch_once(&h, &[("HORCH_NOW", "2030-01-01T02:00:00Z")]);
        let store = Store::open(&db(&h)).unwrap();
        let left = store.conn().query_row(
            "SELECT l.expires_at - f.fleet_clock_s FROM leases l, fleetd f",
            [],
            |r| r.get::<_, i64>(0),
        );
        let left = left.unwrap_or_else(|e| {
            panic!(
                "the task has no lease on the fleet clock: {e}\n{}",
                text(&o)
            )
        });
        assert!(left > 0, "a sleep of 2 h expired the lease\n{}", text(&o));
        let (status, _) = task_row(&h);
        assert_eq!(status, "running", "{}", text(&o));
    });
}
