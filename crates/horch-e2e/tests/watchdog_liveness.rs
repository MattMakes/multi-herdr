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
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::Harness;

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
            // FR-12: a task needs an idea in flight of the feature of its run.
            horch_core::taskledger::schema::run_idea(
                tx,
                &run.run_id,
                &horch_core::taskledger::util::now(),
                true,
            )?;
            tasks::add(
                tx,
                &run,
                &NewTask::work("author", "author.md", "pi-worker"),
                Actor::Orchestrator,
            )?;
            let w = bind_first(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "pi-worker".into(),
                    role: Some("worker-1".into()),
                    harness: Some("pi".into()),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
                horch_core::fleetd::task_execution::domain::task::Recorded {
                    pid: pid,
                    pid_start: Some(started),
                },
            )?;
            horch_core::taskledger::testing::record_supervisor(tx, &w, 1, Some(1))?;
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

/// The stall ladder for a busy pane with no timer (MAP.md of fleetd-p2,
/// Q14): herdr shows `working`, but no signal moves, so the attempt earns
/// no activity after its claim. The pass keeps it within stall_after_s (300
/// s), with a proxy check and no stop. Past it the agent is hung: rung 3
/// stops it at once, with no doorbell (the ladder rings only an idle
/// pane), and rung 6 queues the task again; the operator gets 1 needs-you item.
#[test]
fn a_busy_pi_worker_with_no_timer_is_kept_until_stall_after_then_stopped_at_rung_3() {
    let (h, store, run, worker, mut agent) = pinned_worker("wd203-busy-pi");
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
    let c = pinned_check(&h, &run, "2030-01-01T00:01:00Z");
    assert_eq!(c["verdict"], "proxy_checkin", "{c}");
    let note = c["note"].as_str().unwrap();
    assert!(
        note.contains("no timer format for harness pi; no transcript"),
        "{note}"
    );
    let task_status = || -> String {
        store
            .conn()
            .query_row("SELECT status FROM tasks", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(task_status(), "running");
    assert!(matches!(agent.0.try_wait(), Ok(None)), "the pass kept it");
    assert!(
        outbox(&store, &run).is_empty(),
        "{:?}",
        outbox(&store, &run)
    );

    // Past stall_after_s of the claim, with no activity: rung 3.
    h.advance_fleet_clock(400);
    pinned_check(&h, &run, "2030-01-01T00:08:00Z");
    let (state, outcome): (String, Option<String>) = store
        .conn()
        .query_row("SELECT state, outcome FROM attempts", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(
        (state.as_str(), outcome.as_deref()),
        ("ended", Some("stalled"))
    );
    let doorbells: i64 = store
        .conn()
        .query_row(
            "SELECT count(*) FROM events WHERE kind = 'doorbell'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(doorbells, 0, "a busy pane gets no doorbell");
    // The stall pass saw the old process alive: the task stays stalled, so
    // no claim starts a new attempt beside it (FM-14).
    assert_eq!(
        task_status(),
        "stalled",
        "its old process lived in the pass"
    );
    horch_e2e::harness::wait_for("the stop of rung 3 to end the agent", || {
        matches!(agent.0.try_wait(), Ok(Some(_))).then_some(())
    });
    // The next pass sees the old process gone: rung 6.
    pinned_check(&h, &run, "2030-01-01T00:08:00Z");
    assert_eq!(task_status(), "ready", "rung 6 queued it again");
    assert_eq!(
        workers::get(store.conn(), &worker).unwrap().status,
        "incomplete"
    );
    // FM-16 and ticket 46 of fleetd-p6: no wake goes to an orchestrator.
    // fleetd queues the task again, and the operator gets the fact as the
    // needs-you item `stuck:<feature>` of the feature of the run.
    let w = outbox(&store, &run);
    assert!(w.is_empty(), "{w:?}");
    let feature: String = store
        .conn()
        .query_row(
            "SELECT printf('F-%04d', rowid) FROM runs WHERE run_id = ?1",
            [&run.run_id],
            |r| r.get(0),
        )
        .unwrap();
    let (kind, entity, entity_id, state, text): (String, String, String, String, String) = store
        .conn()
        .query_row(
            "SELECT kind, entity, entity_id, state, text FROM needs_you WHERE key = ?1",
            [format!("stuck:{feature}")],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(
        (
            kind.as_str(),
            entity.as_str(),
            entity_id.as_str(),
            state.as_str()
        ),
        ("stuck", "feature", feature.as_str(), "open"),
        "{text}"
    );
}

/// Write `config` as the `fleet.toml` of the fleet root of `h`: the folder
/// `fleet` next to its ledger (`docs/tickets/fleetd-p1/MAP.md`, Q4).
fn fleet_toml(h: &Harness, config: &str) {
    let root = h.root.join("fleet");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("fleet.toml"), config).unwrap();
}

/// A SIGTERM to fleetd while it reaps a worker does not lose the reap. The
/// stall ladder ends the attempt of a hung agent (herdr shows `working`, no
/// activity past stall_after_s) and sends TERM to the agent, which ignores
/// it, and the stop grace runs.
/// fleetd ends between 2 passes, and the next fleetd kills the agent after
/// the grace. A reap that a SIGTERM lost would leave the agent, with the
/// task already queued again.
#[test]
fn a_sigterm_to_fleetd_during_a_reap_leaves_the_reap_to_the_next_fleetd() {
    let (h, store, _run, worker, _fake) = pinned_worker("wd216-term-mid-reap");
    fleet_toml(&h, "pass_period_s = 1\nstop_grace_s = 8\n");
    fake(&h, &["workspace", "create", "--label", "w1"]);
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
    store
        .conn()
        .execute(
            &format!(
                "UPDATE workers SET agent_pid = {agent_pid}, agent_started = {agent_started}
                 WHERE worker_id = ?1"
            ),
            [&worker],
        )
        .unwrap();
    let mut fleetds = Fleetds::new(&h);
    let first = fleetds.start(&h, "fleetd-1");
    let (pid, started) = fleetds.row().expect("the fleetd row");
    // No activity since the claim, past stall_after_s (300 s): the agent
    // is hung, and rung 3 stops it.
    h.advance_fleet_clock(400);
    // The stop sent TERM to the agent, and the stop grace runs.
    horch_e2e::harness::wait_for("the reap's TERM to the agent", || {
        got_term.exists().then_some(())
    });
    assert!(horch_core::procid::signal_same(pid, started, SIGTERM));
    assert!(
        agent_ended.try_recv().is_err(),
        "the SIGTERM came inside the stop grace"
    );
    let status = fleetds.wait_exit(first);
    assert!(status.success(), "fleetd ended with {status}");
    assert_eq!(
        workers::get(store.conn(), &worker).unwrap().status,
        "incomplete"
    );
    fleetds.start(&h, "fleetd-2");
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

#[test]
fn a_run_made_outside_herdr_has_no_workspace_and_the_first_pane_status_records_it() {
    let mut h = Harness::new("wd218-run-new").with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.unset("HORCH_WORKSPACE_ID");
    h.unset("HERDR_PANE_ID");
    horch_e2e::ledger_seed::seed_run(&h, "auth", None, "w1:p1");
    h.serve_fleetd();
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
    // A worker pane of another fleet does not claim the run. The status
    // exits 1 while the run has work left, so only its text counts.
    h.horch(&["ledger", "status"])
        .env("HORCH_WORKSPACE_ID", "w2")
        .env("HERDR_PANE_ID", "p_7")
        .env("HORCH_ROLE", "sonnet-2")
        .output()
        .unwrap();
    assert_eq!(workspace(), None);
    let plan = |ws: &str| {
        h.horch(&["ledger", "status"])
            .env("HORCH_WORKSPACE_ID", ws)
            .env("HERDR_PANE_ID", "p_1")
            .output()
            .unwrap()
    };
    let o = plan("w1");
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

/// fleetd binds the first startable task of the run and persona of `new`
/// to a new worker with the live agent `agent`, and gives the worker (FR-5).
/// The run has no git repository: the task works in the shared checkout.
fn bind_first(
    tx: &rusqlite::Connection,
    new: &horch_core::taskledger::workers::NewWorker,
    agent: horch_core::fleetd::task_execution::domain::task::Recorded,
) -> anyhow::Result<String> {
    let task = horch_core::taskledger::tasks::startable(tx, &new.run_id)?
        .into_iter()
        .find(|t| t.claimable_by.as_deref() == Some(new.persona.as_str()))
        .ok_or_else(|| anyhow::anyhow!("no startable task of {}", new.persona))?
        .task_id;
    tx.execute(
        "UPDATE tasks SET worktree_mode = 'shared' WHERE task_id = ?1",
        [&task],
    )?;
    Ok(horch_core::taskledger::testing::bind_live(tx, &task, new, Some(agent))?.worker_id)
}
