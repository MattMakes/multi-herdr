//! The watchdog duties of the fleetd pass through the CLI: liveness from
//! herdr's status (#203), a SIGTERM to fleetd during a reap (#216), and the
//! workspace of the wakes of a fleetd that a pane of another workspace
//! starts (#218).
#![cfg(unix)]

use horch_core::taskledger::{
    model::Actor,
    runs::{self, NewRun, Run},
    tasks::{self, NewTask},
    workers::{self, NewWorker},
    Store,
};
use horch_e2e::harness::Harness;

const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;

/// A fake agent process; killed on drop.
struct Agent(std::process::Child);
impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn fake(h: &Harness, args: &[&str]) {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let o = cmd.args(args).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

/// A run in workspace `w1` with 1 working worker in pane `w1:p1`, claimed at
/// 2030-01-01T00:00:00Z, whose agent is a live fake process.
fn pinned_worker(name: &str) -> (Harness, Store, Run, String, Agent) {
    let mut h = Harness::new(name);
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_FAKE_SCENARIO", "stay");
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    let mut cmd = std::process::Command::new(h.bin.join("codex"));
    h.seal(&mut cmd);
    let child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let agent = Agent(child);
    let started = horch_e2e::harness::wait_for("fixture agent identity", || {
        horch_core::procid::start_time(pid)
    });
    let mut store = Store::open(&db).unwrap();
    let (run, worker) = store
        .immediate(|tx| {
            let run = runs::create(
                tx,
                &h.project,
                &NewRun {
                    slug: "pinned".into(),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
            )?;
            tasks::add(
                tx,
                &run,
                &NewTask::work("author", "author.md", "pi-worker"),
                Actor::Orchestrator,
            )?;
            let w = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "pi-worker".into(),
                    role: Some("worker-1".into()),
                    harness: Some("pi".into()),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
            )?;
            workers::supervisor_started(tx, &w, 1, Some(1))?;
            workers::agent_started(tx, &w, pid, Some(started))?;
            tasks::claim(tx, &w)?;
            Ok((run, w))
        })
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE workers SET pane_id='w1:p1', claimed_at='2030-01-01T00:00:00Z',
             last_progress_at='2030-01-01T00:00:00Z' WHERE worker_id=?1",
            [&worker],
        )
        .unwrap();
    (h, store, run, worker, agent)
}

/// 1 forced watchdog pass at the pinned time `at`; the worker's check.
fn pinned_check(h: &Harness, run: &Run, at: &str) -> serde_json::Value {
    let o = h
        .horch(&[
            "ledger",
            "watch",
            "--run",
            &run.run_id,
            "--once",
            "--force",
            "--json",
        ])
        .env("HORCH_NOW", at)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()[0].clone()
}

fn wakes(store: &Store, run: &Run) -> Vec<String> {
    let mut stmt = store
        .conn()
        .prepare("SELECT text FROM events WHERE run_id = ?1 AND kind = 'woke' ORDER BY event_id")
        .unwrap();
    stmt.query_map([&run.run_id], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

/// The wakes that the fleetd pass wrote to the outbox of `run`. The
/// outbox keeps them with `HORCH_NO_WAKE`, which writes no `woke` event.
fn outbox(store: &Store, run: &Run) -> Vec<String> {
    let mut stmt = store
        .conn()
        .prepare("SELECT text FROM outbox WHERE run_id = ?1 ORDER BY rowid")
        .unwrap();
    stmt.query_map([&run.run_id], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn a_busy_pi_worker_with_no_timer_is_kept_until_the_cap_then_one_suspect_wake_then_reaped() {
    let (h, store, run, worker, _agent) = pinned_worker("wd203-busy-pi");
    fake(&h, &["workspace", "create", "--label", "busy"]);
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--text",
            "running a long migration test",
            "--status",
            "working",
            "--agent",
            "pi",
        ],
    );
    // The default run: 3 stall rounds, so a cap of 6 herdr-only checks.
    for m in 1..=6 {
        let c = pinned_check(
            &h,
            &run,
            &format!("2030-01-01T{:02}:{:02}:00Z", m / 6, (m % 6) * 10),
        );
        assert_eq!(c["verdict"], "proxy_checkin", "check {m}: {c}");
        let note = c["note"].as_str().unwrap();
        assert!(
            note.contains("no timer format for harness pi; no transcript"),
            "{note}"
        );
    }
    assert!(
        outbox(&store, &run).is_empty(),
        "{:?}",
        outbox(&store, &run)
    );
    let c = pinned_check(&h, &run, "2030-01-01T01:10:00Z");
    assert_eq!(c["verdict"], "suspect", "{c}");
    let w = outbox(&store, &run);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(
        w[0].contains("worker-1") && w[0].contains("pane w1:p1"),
        "{}",
        w[0]
    );
    let c = pinned_check(&h, &run, "2030-01-01T01:20:00Z");
    assert_eq!(c["verdict"], "dead", "{c}");
    assert_eq!(
        workers::get(store.conn(), &worker).unwrap().status,
        "incomplete"
    );
    let w = outbox(&store, &run);
    assert_eq!(
        w.iter().filter(|b| b.contains("may be stuck")).count(),
        1,
        "{w:?}"
    );
}

/// Write `config` as the `fleet.toml` of the fleet root of `h`: the folder
/// `fleet` next to its ledger (`docs/tickets/fleetd-p1/MAP.md`, Q4).
fn fleet_toml(h: &Harness, config: &str) {
    let root = h.root.join("fleet");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("fleet.toml"), config).unwrap();
}

/// The fleetd processes of a test. Drop stops each one, and the fleetd that
/// the `fleetd` row names, so a failed assertion leaks none.
struct Fleetds {
    db: std::path::PathBuf,
    children: Vec<std::process::Child>,
}

impl Fleetds {
    fn new(h: &Harness) -> Fleetds {
        Fleetds {
            db: h.root.join("ledger.sqlite3"),
            children: Vec::new(),
        }
    }

    /// The PID and start time of the fleetd that the `fleetd` row names.
    fn row(&self) -> Option<(u32, u64)> {
        let store = Store::open(&self.db).ok()?;
        let (pid, started): (i64, i64) = store
            .conn()
            .query_row("SELECT pid, pid_start FROM fleetd WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .ok()?;
        Some((u32::try_from(pid).ok()?, u64::try_from(started).ok()?))
    }

    /// Start `horch fleetd` in the sealed environment of `h`, and wait
    /// until the `fleetd` row names it. Gives its index in `children`.
    fn start(&mut self, h: &Harness) -> usize {
        let mut cmd = std::process::Command::new(Harness::horch_bin());
        h.seal(&mut cmd);
        let child = cmd
            .arg("fleetd")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(
                std::fs::File::create(
                    h.root
                        .join(format!("fleetd-{}.stderr", self.children.len())),
                )
                .unwrap(),
            )
            .spawn()
            .unwrap();
        let pid = child.id();
        self.children.push(child);
        horch_e2e::harness::wait_for("the fleetd row to name the new fleetd", || {
            self.row().filter(|&(p, _)| p == pid).map(|_| ())
        });
        self.children.len() - 1
    }

    /// Kill the fleetd that the row names, and wait until it is gone.
    fn kill_current(&mut self) -> u32 {
        let (pid, started) = self.row().expect("the fleetd row");
        assert!(horch_core::procid::signal_same(pid, started, SIGKILL));
        for child in self.children.iter_mut().filter(|c| c.id() == pid) {
            let _ = child.wait();
        }
        horch_e2e::harness::wait_for("fleetd to end", || {
            (!horch_core::procid::alive(pid, Some(started))).then_some(())
        });
        pid
    }
}

impl Drop for Fleetds {
    fn drop(&mut self) {
        if let Some((pid, started)) = self.row() {
            horch_core::procid::signal_same(pid, started, SIGKILL);
        }
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A SIGTERM to fleetd while it reaps a worker does not lose the reap. The
/// pass sent TERM to the agent, which ignores it, and the stop grace runs.
/// fleetd ends between 2 passes, and the next fleetd kills the agent after
/// the grace. A reap that a SIGTERM lost would leave the agent, with the
/// task already queued again.
#[test]
fn a_sigterm_to_fleetd_during_a_reap_leaves_the_reap_to_the_next_fleetd() {
    let (h, store, run, worker, _fake) = pinned_worker("wd216-term-mid-reap");
    fleet_toml(&h, "pass_period_s = 1\nstop_grace_s = 8\n");
    fake(&h, &["workspace", "create", "--label", "w1"]);
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--text",
            "Ready",
            "--status",
            "idle",
            "--agent",
            "pi",
        ],
    );
    // An agent that ignores TERM and says when it got one.
    let got_term = h.root.join("got-term");
    let mut agent = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!(
            "trap 'touch {}' TERM; while :; do sleep 1; done",
            got_term.display()
        ))
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let agent_pid = agent.id();
    let agent_started = horch_e2e::harness::wait_for("the agent's identity", || {
        horch_core::procid::start_time(agent_pid)
    });
    let (tx, agent_ended) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = agent.wait();
        let _ = tx.send(());
    });
    // Due now, and its next stall round is its last: the first pass reaps.
    store
        .conn()
        .execute(
            "UPDATE runs SET check_every_s = 1 WHERE run_id = ?1",
            [&run.run_id],
        )
        .unwrap();
    store
        .conn()
        .execute(
            &format!(
                "UPDATE workers SET agent_pid = {agent_pid}, agent_started = {agent_started},
                 stall_rounds = 2,
                 claimed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 hour'),
                 last_progress_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 hour')
                 WHERE worker_id = ?1"
            ),
            [&worker],
        )
        .unwrap();
    let mut fleetds = Fleetds::new(&h);
    let first = fleetds.start(&h);
    let (pid, started) = fleetds.row().expect("the fleetd row");
    // The reap sent TERM to the agent, and the stop grace runs.
    horch_e2e::harness::wait_for("the reap's TERM to the agent", || {
        got_term.exists().then_some(())
    });
    assert!(horch_core::procid::signal_same(pid, started, SIGTERM));
    assert!(
        agent_ended.try_recv().is_err(),
        "the SIGTERM came inside the stop grace"
    );
    let status = horch_e2e::harness::wait_for("fleetd to end after its pass", || {
        fleetds.children[first].try_wait().unwrap()
    });
    assert!(status.success(), "fleetd ended with {status}");
    assert_eq!(
        workers::get(store.conn(), &worker).unwrap().status,
        "incomplete"
    );
    fleetds.start(&h);
    agent_ended
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the next fleetd killed the agent after the grace");
    // The pass has no process of the pane, so it keeps the pane and asks
    // the operator (ticket 26 closes the pane of a reap, FM-22).
    let kept: String = store
        .conn()
        .query_row(
            "SELECT state FROM needs_you WHERE key = 'pane_kept:w1:p1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, "open");
}

/// The wakes of `run` in the outbox that no delivery took.
fn undelivered(store: &Store, run: &Run) -> Vec<String> {
    let mut stmt = store
        .conn()
        .prepare(
            "SELECT text FROM outbox WHERE run_id = ?1 AND delivered_at IS NULL ORDER BY rowid",
        )
        .unwrap();
    stmt.query_map([&run.run_id], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn mail_in(h: &Harness, workspace: &str) -> Vec<String> {
    horch_core::messaging::mail::Mail::under(&h.state, workspace)
        .pending()
        .unwrap_or_default()
        .into_iter()
        .map(|m| m.text)
        .collect()
}

/// An idle worker of a run that workspace w1 owns, checked by a watchdog
/// pass from a process of workspace w2: the stall wake goes to w1.
#[test]
fn a_wake_from_another_workspace_goes_to_the_runs_workspace() {
    let (mut h, store, run, _worker, _agent) = pinned_worker("wd218-wake-target");
    h.unset("HORCH_NO_WAKE");
    fake(&h, &["workspace", "create", "--label", "w1"]);
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--text",
            "Ready",
            "--status",
            "idle",
            "--agent",
            "pi",
        ],
    );
    let o = h
        .horch(&[
            "ledger",
            "watch",
            "--run",
            &run.run_id,
            "--once",
            "--force",
            "--json",
        ])
        .env("HORCH_NOW", "2030-01-01T00:10:00Z")
        .env("HORCH_WORKSPACE_ID", "w2")
        .env("HORCH_ROLE", "sonnet-2")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let c = &serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()[0];
    assert_eq!(c["verdict"], "no_progress", "{c}");
    let w = wakes(&store, &run);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("the run's workspace w1"), "{}", w[0]);
    assert!(
        mail_in(&h, "w1")
            .iter()
            .any(|m| m.contains("worker-1") && m.contains("shows no progress")),
        "{:?}",
        mail_in(&h, "w1")
    );
    assert!(mail_in(&h, "w2").is_empty(), "{:?}", mail_in(&h, "w2"));
}

/// A fleetd that the check of a w2 worker pane starts (with that pane's
/// role, worker id and private TMPDIR) for a run that w1 owns sends its
/// wakes to w1.
#[test]
fn a_fleetd_started_from_another_workspace_wakes_the_runs_workspace() {
    let (mut h, store, run, worker, _agent) = pinned_worker("wd218-detached");
    // A fleetd ran and died. The check before the next command starts
    // fleetd again, from the w2 pane that runs that command.
    let mut fleetds = Fleetds::new(&h);
    fleetds.start(&h);
    let dead = fleetds.kill_current();
    h.unset("HORCH_NO_WAKE");
    h.unset("HORCH_NO_WATCHDOG");
    fake(&h, &["workspace", "create", "--label", "w1"]);
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--text",
            "Ready",
            "--status",
            "idle",
            "--agent",
            "pi",
        ],
    );
    // A real clock: due at once, and the loop ticks every second.
    store
        .conn()
        .execute(
            "UPDATE runs SET check_every_s = 1 WHERE run_id = ?1",
            [&run.run_id],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE workers SET claimed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 hour'),
             last_progress_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 hour') WHERE worker_id = ?1",
            [&worker],
        )
        .unwrap();
    let pane_tmp = h.state.join("tmp").join("sonnet-2");
    std::fs::create_dir_all(&pane_tmp).unwrap();
    let o = h
        .horch(&["ledger", "status", "--run", &run.run_id])
        .env("HORCH_WORKSPACE_ID", "w2")
        .env("HORCH_ROLE", "sonnet-2")
        .env("HORCH_WORKER_ID", "w2-worker")
        .env("HERDR_PANE_ID", "p_9")
        .env("TMPDIR", &pane_tmp)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains(&format!("fleetd PID {dead} is gone.")),
        "{err}"
    );
    // The pane ends and removes its private TMPDIR; fleetd goes on.
    std::fs::remove_dir_all(&pane_tmp).unwrap();
    horch_e2e::harness::wait_for("the stall wake in w1", || {
        mail_in(&h, "w1")
            .iter()
            .any(|m| m.contains("worker-1") && m.contains("shows no progress"))
            .then_some(())
    });
    assert!(mail_in(&h, "w2").is_empty(), "{:?}", mail_in(&h, "w2"));
    let w = wakes(&store, &run);
    assert!(
        w.iter().all(|b| b.contains("the run's workspace w1")),
        "{w:?}"
    );
}

/// A fleetd that the check of a w2 worker pane starts, for a run with no
/// workspace, does not take the pane's workspace: the detached launch drops
/// the pane identity, so its wakes go to no workspace, never to w2.
#[test]
fn a_detached_fleetd_does_not_inherit_the_starting_panes_workspace() {
    let (mut h, store, run, worker, _agent) = pinned_worker("wd218-no-workspace");
    // A fleetd ran and died. The check before the next command starts
    // fleetd again, from the w2 pane that runs that command.
    let mut fleetds = Fleetds::new(&h);
    fleetds.start(&h);
    let dead = fleetds.kill_current();
    h.unset("HORCH_NO_WAKE");
    h.unset("HORCH_NO_WATCHDOG");
    h.unset("HORCH_WORKSPACE_ID");
    fake(&h, &["workspace", "create", "--label", "w1"]);
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--text",
            "Ready",
            "--status",
            "idle",
            "--agent",
            "pi",
        ],
    );
    store
        .conn()
        .execute(
            "UPDATE runs SET check_every_s = 1, workspace_id = NULL WHERE run_id = ?1",
            [&run.run_id],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE workers SET claimed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 hour'),
             last_progress_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-1 hour') WHERE worker_id = ?1",
            [&worker],
        )
        .unwrap();
    let o = h
        .horch(&["ledger", "status", "--run", &run.run_id])
        .env("HORCH_WORKSPACE_ID", "w2")
        .env("HORCH_ROLE", "sonnet-2")
        .env("HORCH_WORKER_ID", "w2-worker")
        .env("HERDR_PANE_ID", "p_9")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains(&format!("fleetd PID {dead} is gone.")),
        "{err}"
    );
    // With no workspace, the wake has no target: it stays in the outbox,
    // and no workspace gets mail.
    let w = horch_e2e::harness::wait_for("the stall wake", || {
        let w = undelivered(&store, &run);
        (!w.is_empty()).then_some(w)
    });
    assert!(w[0].contains("worker-1"), "{w:?}");
    for ws in ["w1", "w2"] {
        assert!(mail_in(&h, ws).is_empty(), "{ws}: {:?}", mail_in(&h, ws));
    }
}

#[test]
fn a_run_made_outside_herdr_warns_and_the_first_pane_plan_records_its_workspace() {
    let mut h = Harness::new("wd218-run-new").with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.unset("HORCH_WORKSPACE_ID");
    h.unset("HERDR_PANE_ID");
    let o = h.run(&["ledger", "run", "new", "auth"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("warning: run auth has no workspace")
            && err.contains(
                "Wakes go only to an orchestrator that runs a ledger command from its pane"
            ),
        "{err}"
    );
    let workspace = || -> Option<String> {
        Store::open(&db)
            .unwrap()
            .conn()
            .query_row(
                "SELECT workspace_id FROM runs WHERE slug = 'auth'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert_eq!(workspace(), None);
    // A worker pane of another fleet does not claim the run.
    let o = h
        .horch(&["ledger", "plan-workers"])
        .env("HORCH_WORKSPACE_ID", "w2")
        .env("HERDR_PANE_ID", "p_7")
        .env("HORCH_ROLE", "sonnet-2")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(workspace(), None);
    let plan = |ws: &str| {
        h.horch(&["ledger", "plan-workers"])
            .env("HORCH_WORKSPACE_ID", ws)
            .env("HERDR_PANE_ID", "p_1")
            .output()
            .unwrap()
    };
    let o = plan("w1");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(
        err.matches("NOTE: run auth had no workspace. It now has workspace w1")
            .count(),
        1,
        "{err}"
    );
    assert_eq!(workspace().as_deref(), Some("w1"));
    // The first one wins; the NOTE is printed once.
    let o = plan("w3");
    assert!(!String::from_utf8_lossy(&o.stderr).contains("NOTE: run auth"));
    assert_eq!(workspace().as_deref(), Some("w1"));
}
