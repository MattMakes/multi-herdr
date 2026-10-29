//! The task-ledger watchdog through the CLI: liveness from herdr's status
//! (#203), its binary after an update (#216), and the identity of the
//! process that starts it (#218).
use horch_core::taskledger::{
    model::Actor,
    runs::{self, NewRun, Run},
    tasks::{self, NewTask},
    workers::{self, NewWorker},
    Store,
};
use horch_e2e::harness::Harness;

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
    assert!(wakes(&store, &run).is_empty(), "{:?}", wakes(&store, &run));
    let c = pinned_check(&h, &run, "2030-01-01T01:10:00Z");
    assert_eq!(c["verdict"], "suspect", "{c}");
    let w = wakes(&store, &run);
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
    let w = wakes(&store, &run);
    assert_eq!(
        w.iter().filter(|b| b.contains("may be stuck")).count(),
        1,
        "{w:?}"
    );
}

/// A project with 1 active run `zeta`, its own ledger, and the watchdog
/// restart on. Returns the run id.
fn watchdog_world(name: &str) -> (Harness, String) {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.unset("HORCH_NO_WATCHDOG");
    let o = h.run(&["ledger", "run", "new", "zeta"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let store = Store::open(&db).unwrap();
    let id: String = store
        .conn()
        .query_row("SELECT run_id FROM runs WHERE slug = 'zeta'", [], |r| {
            r.get(0)
        })
        .unwrap();
    (h, id)
}

fn locks(h: &Harness) -> std::path::PathBuf {
    h.root.join("locks")
}

/// A running `horch ledger watch` of the run; ends when the receiver hears.
struct Watchdog {
    pid: u32,
    started: u64,
    ended: std::sync::mpsc::Receiver<()>,
}

fn start_watchdog(h: &Harness, run_id: &str) -> Watchdog {
    let mut child = h
        .horch(&["ledger", "watch", "--run", run_id])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let (tx, ended) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = child.wait();
        let _ = tx.send(());
    });
    let started = horch_e2e::harness::wait_for("the watchdog's identity", || {
        horch_core::procid::start_time(pid)
    });
    horch_e2e::harness::wait_for("the watchdog's lock and binary record", || {
        watchdog::holder(&locks(h), run_id).filter(|w| w.pid == pid && w.exe.is_some())
    });
    Watchdog {
        pid,
        started,
        ended,
    }
}

/// End the live watchdog of the run, whoever started it.
fn stop_watchdog(h: &Harness, run_id: &str) {
    if let Some(w) = watchdog::holder(&locks(h), run_id) {
        horch_core::procid::signal_same(w.pid, w.started.unwrap(), 9);
    }
}

use horch_core::taskledger::watchdog;

#[test]
fn a_watchdog_of_another_build_is_stopped_by_pid_and_replaced() {
    let (h, run_id) = watchdog_world("wd216-other-build");
    let old = start_watchdog(&h, &run_id);
    // Simulate an older build: the lock names another binary.
    let other = horch_core::exeid::ExeIdentity {
        path: "/old/bin/horch".into(),
        len: 1,
        mtime_ns: 1,
    };
    std::fs::write(
        locks(&h).join(format!("watchdog-{run_id}.lock/exe")),
        serde_json::to_vec(&other).unwrap(),
    )
    .unwrap();
    let o = h.run(&["ledger", "status"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("restarted the watchdog of run zeta")
            && err.contains(&format!("PID {} ran another build", old.pid)),
        "{err}"
    );
    old.ended
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the old watchdog was stopped");
    let new = horch_e2e::harness::wait_for("the new watchdog", || {
        watchdog::holder(&locks(&h), &run_id).filter(|w| w.pid != old.pid && w.exe.is_some())
    });
    assert_eq!(
        new.exe,
        horch_core::exeid::exe_identity(&Harness::horch_bin()),
        "the new watchdog runs this build"
    );
    stop_watchdog(&h, &run_id);
}

#[test]
fn a_watchdog_of_this_build_is_left_alone() {
    let (h, run_id) = watchdog_world("wd216-same-build");
    let w = start_watchdog(&h, &run_id);
    let o = h.run(&["ledger", "status"]);
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!err.contains("restarted the watchdog"), "{err}");
    assert!(w
        .ended
        .recv_timeout(std::time::Duration::from_millis(500))
        .is_err());
    assert!(horch_core::procid::alive(w.pid, Some(w.started)));
    assert_eq!(watchdog::holder(&locks(&h), &run_id).unwrap().pid, w.pid);
    stop_watchdog(&h, &run_id);
}

#[test]
fn install_restarts_the_watchdog_of_an_active_run_on_the_installed_binary() {
    let (h, run_id) = watchdog_world("wd216-install");
    let old = start_watchdog(&h, &run_id);
    let dir = h.root.join("installed-bin");
    let o = h.run(&["install", "--dir", &dir.to_string_lossy()]);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        o.status.success(),
        "{out}\n{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let installed = dir.join(Harness::horch_bin().file_name().unwrap());
    assert!(
        out.contains(&format!(
            "restarted the watchdog of run zeta on {}: PID {} ran another build of horch",
            installed.display(),
            old.pid
        )),
        "{out}"
    );
    old.ended
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the old watchdog was stopped");
    let new = horch_e2e::harness::wait_for("the new watchdog", || {
        watchdog::holder(&locks(&h), &run_id).filter(|w| w.pid != old.pid && w.exe.is_some())
    });
    assert_eq!(new.exe, horch_core::exeid::exe_identity(&installed));
    stop_watchdog(&h, &run_id);
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

/// A watchdog that a w2 worker pane starts (with that pane's role, worker
/// id and private TMPDIR) for a run that w1 owns sends its wakes to w1.
#[test]
fn a_watchdog_started_from_another_workspace_wakes_the_runs_workspace() {
    let (mut h, store, run, worker, _agent) = pinned_worker("wd218-detached");
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
        err.contains("the watchdog of run pinned was not running; started it"),
        "{err}"
    );
    // The pane ends and removes its private TMPDIR; the watchdog goes on.
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
    stop_watchdog(&h, &run.run_id);
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
