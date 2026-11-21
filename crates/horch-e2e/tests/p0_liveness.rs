#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Child, Output};
use std::time::{Duration, Instant};

use horch_core::taskledger::model::{Actor, WorkerStatus};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;
use horch_e2e::expected::check;
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{fixtures, wait_for, wait_for_or, Harness};
use horch_e2e::ledger_seed::seed_run;
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
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

const RUN: &str = "eps";

fn watch_once(h: &Harness, env: &[(&str, &str)]) -> (Output, Value) {
    let mut cmd = h.horch(&[
        "ledger", "watch", "--once", "--force", "--json", "--run", RUN,
    ]);
    cmd.envs(env.iter().copied());
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "horch ledger watch: {}", text(&o));
    let v = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
    (o, v)
}

struct Agent(Child);

impl Agent {
    fn start() -> Agent {
        use std::os::unix::process::CommandExt;
        Agent(
            std::process::Command::new("/bin/sleep")
                .arg("600")
                .process_group(0)
                .spawn()
                .unwrap(),
        )
    }

    fn ended(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(Some(_)))
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

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

fn queue(h: &Harness, persona: &str, fallback: Option<&str>) {
    let run = seed_run(h, RUN, None, "w1:p1");
    let p = ticket(h, "t0", "# T0\n");
    let new = NewTask {
        fallback: fallback.map(str::to_string),
        ..NewTask::work("T0", &p.to_string_lossy(), persona)
    };
    let mut store = Store::open(&db(h)).unwrap();
    store
        .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
        .unwrap();
}

fn bind(h: &Harness, persona: &str, agent: &Agent) -> String {
    horch_e2e::bind::bind(h, persona, Some(agent.pid())).worker_id
}

fn task_row(h: &Harness) -> (String, i64) {
    let store = Store::open(&db(h)).unwrap();
    store
        .conn()
        .query_row(
            "SELECT status, attempts - attempt_floor FROM tasks",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

fn fleet_clock(h: &Harness) -> i64 {
    let store = Store::open(&db(h)).unwrap();
    store
        .conn()
        .query_row("SELECT fleet_clock_s FROM fleetd WHERE id = 1", [], |r| {
            r.get(0)
        })
        .unwrap()
}

fn causes_to(h: &Harness, to: &str) -> Vec<String> {
    let store = Store::open(&db(h)).unwrap();
    let mut stmt = store
        .conn()
        .prepare("SELECT cause FROM events WHERE entity = 'task' AND to_state = ?1 ORDER BY rowid")
        .unwrap();
    stmt.query_map([to], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

const CATCH_S: i64 = 600;

const STEP_S: i64 = 30;

#[test]
fn fm3_idle_claim_is_caught_in_under_10_min() {
    check(file!(), "fm3_idle_claim_is_caught_in_under_10_min", || {
        let h = fleet("p0-fm3");
        let agent = Agent::start();
        queue(&h, "opus", None);
        watch_once(&h, &[]);
        bind(&h, "opus", &agent);
        let claimed = fleet_clock(&h);
        let mut seen = Vec::new();
        loop {
            h.advance_fleet_clock(STEP_S);
            let (o, _) = watch_once(&h, &[]);
            seen.push(text(&o));
            let waited = fleet_clock(&h) - claimed;
            let (status, _) = task_row(&h);
            assert!(
                waited < CATCH_S,
                "the idle claim is {status} {waited} s after its claim on the \
                 fleet clock; the design catches it in under {CATCH_S} s \
                 (stall 300 s, lease 120 s)\n{}",
                seen.join("\n")
            );
            if status == "running" {
                continue;
            }
            assert!(
                ["stalled", "ready", "escalated"].contains(&status.as_str()),
                "the idle claim left running to {status}\n{}",
                seen.join("\n")
            );
            assert!(
                !causes_to(&h, "stalled").is_empty(),
                "the task left running with no move to stalled\n{}",
                seen.join("\n")
            );
            return;
        }
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
            queue(&h, "opus", None);
            bind(&h, "opus", &agent);
            let mut fleetds = Fleetds::new(&h);
            fleetds.start(&h, "fleetd");
            agent.kill();
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
             ends the attempt within 30 s (a pass every 5 s)",
                killed.elapsed().as_secs(),
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
            let workspace = h.fleet_workspace();
            h.set("HORCH_WORKSPACE_ID", workspace);
            declare_gates(&h);
            let quota = fixtures().join("quota/claude-exhausted-codex-ok.json");
            h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
            h.set("HORCH_NOW", "2026-09-28T18:00:00Z");
            let mut agent = Agent::start();
            queue(&h, "opus", Some("codex-sol"));
            let w = bind(&h, "opus", &agent);
            let (_, attempts) = task_row(&h);
            let mut seen = Vec::new();
            for _ in 0..3 {
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
            let store = Store::open(&db(&h)).unwrap();
            let outcome: Option<String> = store
                .conn()
                .query_row(
                    "SELECT outcome FROM attempts WHERE worker_id = ?1",
                    [&w],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                outcome.as_deref(),
                Some("quota"),
                "the attempt of the usage-limited agent did not end on the \
                 usage limit\n{}",
                seen.join("\n")
            );
            wait_for("the stop of the usage-limited agent", || {
                agent.ended().then_some(())
            });
            let shown = json_of(&h, &["ledger", "show", "T-1", "--json"]);
            let (status, now) = task_row(&h);
            assert_eq!(now, attempts, "the usage limit burned an attempt\n{shown}");
            let persona = shown["task"]["claimable_by"].as_str().unwrap_or_default();
            assert_eq!(
                (status.as_str(), persona),
                ("leased", "codex-sol"),
                "the design moves the task to its fallback codex-sol\n{shown}"
            );
            let restart: Vec<String> = store
                .conn()
                .prepare(
                    "SELECT a.state FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
                     WHERE w.persona = 'codex-sol'",
                )
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert!(
                matches!(restart.as_slice(), [s] if s == "launching" || s == "live"),
                "the fallback has no restart attempt: {restart:?}\n{shown}"
            );
        },
    );
}

fn declare_gates(h: &Harness) {
    std::fs::write(
        h.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    for args in [
        &["add", "fleet.toml"][..],
        &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
    ] {
        let o = h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
    }
}

const WOKE_AT: &str = "2030-01-01T02:00:00Z";

fn lease_left(h: &Harness) -> i64 {
    let store = Store::open(&db(h)).unwrap();
    store
        .conn()
        .query_row(
            "SELECT l.expires_at - f.fleet_clock_s FROM leases l, fleetd f",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|e| panic!("the task has no lease on the fleet clock: {e}"))
}

#[test]
fn sleep_does_not_expire_a_lease() {
    check(file!(), "sleep_does_not_expire_a_lease", || {
        let mut h = fleet("p0-sleep");
        let mut agent = Agent::start();
        queue(&h, "opus", None);
        bind(&h, "opus", &agent);
        let before = 0;

        let root = h.root.join("fleet");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("fleet.toml"), "pass_period_s = 1\n").unwrap();
        h.set("HORCH_NOW", WOKE_AT);
        let awake = Instant::now();
        let mut fleetds = Fleetds::new(&h);
        fleetds.start(&h, "fleetd");
        wait_passes(&h, 3);
        fleetds.kill_current();
        let awake_s = i64::try_from(awake.elapsed().as_secs()).unwrap() + 1;
        let log = fleetds.output("fleetd", "stderr");

        let grew = fleet_clock(&h) - before;
        assert!(
            grew <= awake_s,
            "the fleet clock grew {grew} s in {awake_s} s of awake time: it \
             follows the wall clock\n{log}"
        );
        let left = lease_left(&h);
        assert!(left > 0, "a long sleep expired the lease\n{log}");
        assert_eq!(task_row(&h).0, "running", "{log}");
        assert!(causes_to(&h, "stalled").is_empty(), "{log}");
        assert!(!agent.ended(), "the agent was stopped\n{log}");

        h.advance_fleet_clock(left + 1);
        let (o, _) = watch_once(&h, &[]);
        assert_eq!(
            causes_to(&h, "stalled"),
            [horch_core::fleetd::task_execution::application::lease::LEASE_EXPIRED],
            "{}",
            text(&o)
        );
        assert_ne!(task_row(&h).0, "running", "{}", text(&o));
    });
}

fn pane_fleet(name: &str, config: &str) -> (Harness, Store, String) {
    let mut h = Harness::new(name);
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    remove_agent_clis_so_fleetd_queues_no_conformance_job(&h);
    h.set("HORCH_LEDGER_DB", db(&h).to_string_lossy());
    let root = h.root.join("fleet");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("fleet.toml"), config).unwrap();
    herdr(&h, &["workspace", "create", "--label", "panes"]);
    let mut store = Store::open(&db(&h)).unwrap();
    let run = store
        .immediate(|tx| {
            runs::create(
                tx,
                &h.project,
                &NewRun {
                    slug: "panes".into(),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    (h, store, run.run_id)
}

fn remove_agent_clis_so_fleetd_queues_no_conformance_job(h: &Harness) {
    for cli in ["claude", "codex", "agy"] {
        std::fs::remove_file(h.bin.join(cli)).unwrap();
    }
}

fn herdr(h: &Harness, args: &[&str]) -> String {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let o = cmd.args(args).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn panes(h: &Harness) -> Vec<String> {
    let v: Value = serde_json::from_str(&herdr(h, &["pane", "list", "--workspace", "w1"])).unwrap();
    v["result"]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["pane_id"].as_str().unwrap().to_string())
        .collect()
}

fn polite_script(h: &Harness, tag: &str) -> String {
    let pid = h.root.join(format!("{tag}.pid")).display().to_string();
    format!("echo $$ > {pid}.tmp\n/bin/mv {pid}.tmp {pid}\nexec /bin/sleep 600\n")
}

fn pid_of(h: &Harness, tag: &str) -> (u32, u64) {
    let path = h.root.join(format!("{tag}.pid"));
    wait_for("the pid of the stand-in worker", || {
        let pid: u32 = std::fs::read_to_string(&path).ok()?.trim().parse().ok()?;
        Some((pid, horch_core::procid::start_time(pid)?))
    })
}

fn run_in_pane(h: &Harness, pane: &str, tag: &str) -> (u32, u64) {
    herdr(h, &["pane", "run", pane, &polite_script(h, tag)]);
    pid_of(h, tag)
}

fn reaped_worker(store: &mut Store, run: &str, pane: &str, supervisor: (u32, u64)) {
    store
        .immediate(|tx| {
            let w = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.to_string(),
                    persona: "pi-worker".into(),
                    harness: Some("pi".into()),
                    workspace_id: Some("w1".into()),
                    pane_id: Some(pane.into()),
                    ..Default::default()
                },
            )?;
            workers::supervisor_started(tx, &w, supervisor.0, Some(supervisor.1))?;
            workers::end(tx, &w, WorkerStatus::Incomplete, "reaped", None)
        })
        .unwrap();
}

fn alive(p: (u32, u64)) -> bool {
    horch_core::procid::alive(p.0, Some(p.1))
}

fn open_items(store: &Store) -> Vec<String> {
    let mut stmt = store
        .conn()
        .prepare("SELECT key FROM needs_you WHERE state = 'open' ORDER BY key")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn wait_passes(h: &Harness, passes: usize) {
    let path = h.root.join("fleet").join("run").join("heartbeat.json");
    let at = || -> Option<u64> {
        let v: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
        v["awake"]["awake_ms"].as_u64()
    };
    let mut seen = Vec::new();
    wait_for("fleetd passes", || {
        let now = at();
        if now.is_some() && seen.last() != Some(&now) {
            seen.push(now);
        }
        (seen.len() > passes).then_some(())
    });
}

const NO_KILL: &str = "stop_grace_s = 600\n";

fn stopped_then_crashed(h: &Harness, store: &mut Store, run: &str) -> (Fleetds, (u32, u64)) {
    let shell = run_in_pane(h, "w1:p1", "w");
    reaped_worker(store, run, "w1:p1", shell);
    let mut fleetds = Fleetds::new(h);
    fleetds.start(h, "fleetd-1");
    wait_for_or(
        "the end of the worker on the polite stop",
        || (!alive(shell)).then_some(()),
        || fleetds.output("fleetd-1", "stderr"),
    );
    fleetds.kill_current();
    assert!(
        panes(h).contains(&"w1:p1".to_string()),
        "the first fleetd closed the pane before it died; the test needs a \
         longer pass period"
    );
    (fleetds, shell)
}

#[test]
fn a_reaped_worker_that_ends_on_the_polite_stop_has_its_pane_closed() {
    let config = format!("pass_period_s = 1\n{NO_KILL}");
    let (h, mut store, run) = pane_fleet("p0-polite-pane", &config);
    let shell = run_in_pane(&h, "w1:p1", "w");
    reaped_worker(&mut store, &run, "w1:p1", shell);
    let mut fleetds = Fleetds::new(&h);
    fleetds.start(&h, "fleetd");
    wait_for_or(
        "the close of the pane of the reaped worker",
        || (!panes(&h).contains(&"w1:p1".to_string())).then_some(()),
        || {
            format!(
                "worker alive {}, panes {:?}, items {:?}\n{}",
                alive(shell),
                panes(&h),
                open_items(&store),
                fleetds.output("fleetd", "stderr")
            )
        },
    );
    assert!(!alive(shell));
    assert_eq!(open_items(&store), Vec::<String>::new());
}

#[test]
fn a_reaped_worker_whose_pane_has_no_process_keeps_its_pane_with_1_item() {
    let config = format!("pass_period_s = 1\n{NO_KILL}");
    let (h, mut store, run) = pane_fleet("p0-polite-no-info", &config);
    let shell = {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(polite_script(&h, "o"))
            .stdin(std::process::Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        std::thread::spawn(move || child.wait());
        pid_of(&h, "o")
    };
    reaped_worker(&mut store, &run, "w1:p1", shell);
    let mut fleetds = Fleetds::new(&h);
    fleetds.start(&h, "fleetd");
    wait_for_or(
        "the end of the worker on the polite stop",
        || (!alive(shell)).then_some(()),
        || fleetds.output("fleetd", "stderr"),
    );
    wait_passes(&h, 3);
    assert!(panes(&h).contains(&"w1:p1".to_string()));
    assert_eq!(open_items(&store), ["pane_kept:w1:p1"]);
}

#[test]
fn a_fleetd_that_dies_after_the_polite_stop_leaves_the_pane_close_to_the_next_fleetd() {
    let config = format!("pass_period_s = 30\n{NO_KILL}");
    let (h, mut store, run) = pane_fleet("p0-polite-crash", &config);
    let (mut fleetds, _) = stopped_then_crashed(&h, &mut store, &run);
    fleetds.start(&h, "fleetd-2");
    wait_for_or(
        "the close of the pane by the next fleetd",
        || (!panes(&h).contains(&"w1:p1".to_string())).then_some(()),
        || {
            format!(
                "panes {:?}, items {:?}\n{}",
                panes(&h),
                open_items(&store),
                fleetds.output("fleetd-2", "stderr")
            )
        },
    );
    assert_eq!(open_items(&store), Vec::<String>::new());
}

#[test]
fn a_pane_that_names_another_process_after_the_polite_stop_is_kept_with_1_item() {
    let config = format!("pass_period_s = 30\n{NO_KILL}");
    let (h, mut store, run) = pane_fleet("p0-polite-reused", &config);
    let (mut fleetds, _) = stopped_then_crashed(&h, &mut store, &run);
    let foreign = run_in_pane(&h, "w1:p1", "f");
    fleetds.start(&h, "fleetd-2");
    wait_for_or(
        "the needs-you item of the kept pane",
        || (open_items(&store) == ["pane_kept:w1:p1"]).then_some(()),
        || {
            format!(
                "items {:?}\n{}",
                open_items(&store),
                fleetds.output("fleetd-2", "stderr")
            )
        },
    );
    assert!(panes(&h).contains(&"w1:p1".to_string()));
    assert!(alive(foreign), "fleetd stopped a process that no row names");
    let _ = horch_core::procid::signal_same(foreign.0, foreign.1, libc::SIGKILL);
}

#[test]
fn an_unreadable_old_record_of_the_polite_stop_is_removed_and_the_pane_is_kept() {
    let config = format!("pass_period_s = 30\n{NO_KILL}");
    let (h, mut store, run) = pane_fleet("p0-polite-unreadable", &config);
    let (mut fleetds, shell) = stopped_then_crashed(&h, &mut store, &run);
    let deleted = store
        .conn()
        .execute(
            "DELETE FROM polite_stops WHERE pid = ?1 AND pid_start = ?2",
            [i64::from(shell.0), shell.1 as i64],
        )
        .unwrap();
    assert_eq!(deleted, 1, "no row of the polite stop");
    let record = h
        .root
        .join("fleet/run/stops")
        .join(format!("{}-{}.json", shell.0, shell.1));
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(&record, "not json").unwrap();
    fleetds.start(&h, "fleetd-2");
    let shown = record.display().to_string();
    wait_for_or(
        "the fleetd log line of the unreadable record",
        || {
            fleetds
                .output("fleetd-2", "stderr")
                .contains(&shown)
                .then_some(())
        },
        || fleetds.output("fleetd-2", "stderr"),
    );
    assert!(!record.exists(), "fleetd removed the old record");
    assert!(panes(&h).contains(&"w1:p1".to_string()), "the pane is kept");
}

fn doctor(h: &Harness) -> (bool, String, String) {
    let o = h.run(&["doctor"]);
    let root = h.root.to_string_lossy().into_owned();
    let shown = |b: &[u8]| String::from_utf8_lossy(b).replace(&root, "<root>");
    (o.status.success(), shown(&o.stdout), shown(&o.stderr))
}

fn without_git_line(text: &str) -> Vec<&str> {
    text.lines().filter(|l| !l.starts_with("git: ")).collect()
}

#[test]
fn on_a_host_with_no_git_the_doctor_fails_only_its_git_line_and_fleetd_refuses_to_start() {
    let with_git = Harness::new("p0-a-git");
    let mut h = Harness::new("p0-no-git");
    h.without("HORCH_GIT_BIN");

    let (passed, out_git, err_git) = doctor(&with_git);
    assert!(passed, "{out_git}{err_git}");
    assert!(out_git.contains("git: git version 2.41.0"), "{out_git}");
    assert!(!without_git_line(&out_git).is_empty(), "{out_git}");
    let (passed, out, err) = doctor(&h);
    assert!(!passed, "{out}{err}");
    assert!(err.contains("git: git not found on PATH"), "{err}");
    assert_eq!(without_git_line(&out), without_git_line(&out_git), "{err}");

    let mut fleetds = Fleetds::new(&h);
    let pid = fleetds.spawn(&h, "fleetd");
    assert!(!fleetds.wait_exit(pid).success());
    let stderr = fleetds.output("fleetd", "stderr");
    assert!(
        stderr.contains("refused: git not found on PATH"),
        "{stderr}"
    );
    assert_eq!(fleetds.row(), None);
}
