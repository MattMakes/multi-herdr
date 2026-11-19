//! Fleet acceptance at the durable messaging / ledger seam.
use horch_core::messaging::mail::{redelivery_offset, Mail};
use horch_e2e::harness::Harness;
use std::time::Duration;

fn worker_tell(h: &Harness, text: &str) -> std::process::Output {
    h.horch(&["tell", "orchestrator", text])
        .env("HORCH_ROLE", "worker-1")
        .output()
        .unwrap()
}
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name);
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_TEST_MAIL_POLL_MS", "10");
    h
}
/// The short id of the oldest open question in workspace `w1`.
fn open_id(state: &std::path::Path) -> String {
    Mail::under(state, "w1").open_questions().unwrap()[0]
        .id
        .short()
}
#[test]
fn g60_missing_registration_still_queues_worker_mail() {
    let h = world("g60-no-orchestrator");
    let o = worker_tell(&h, "NOTE: durable");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stderr).contains("queued, not yet acknowledged"));
    assert_eq!(Mail::under(&h.state, "w1").pending().unwrap().len(), 1);
}
#[test]
fn g60_redelivery_continues_two_hours_after_enqueue() {
    assert!(redelivery_offset(30).is_some_and(|d| d > Duration::from_secs(2 * 3600)));
}
#[test]
fn g70_question_survives_ack_without_a_task_run() {
    let h = world("g70-question");
    let _ = worker_tell(&h, "QUESTION: which option?");
    let mail = Mail::under(&h.state, "w1");
    let pending = mail.pending().unwrap();
    assert_eq!(
        pending.len(),
        1,
        "question is durable without a registered orchestrator"
    );
    let inbox = h.run(&["inbox"]);
    assert!(inbox.status.success());
    let inbox = h.run(&["inbox", "--roles"]);
    assert!(
        String::from_utf8_lossy(&inbox.stdout).contains("QUESTION: which option?"),
        "{}",
        String::from_utf8_lossy(&inbox.stdout)
    );
    fake(&h, &["workspace", "create", "--label", "answer"]);
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
            "claude",
        ],
    );
    let mb = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mb).unwrap();
    std::fs::write(mb.join("worker-1.id"), "w1:p1").unwrap();
    // TRF-25: a plain tell closes no question.
    assert!(h
        .run(&["tell", "worker-1", "Choose option A"])
        .status
        .success());
    let inbox = h.run(&["inbox", "--roles"]);
    assert!(String::from_utf8_lossy(&inbox.stdout).contains("QUESTION: which option?"));
    // An answer by id closes it.
    let id = open_id(&h.state);
    assert!(h
        .run(&["tell", "worker-1", "--answers", &id, "Choose option A"])
        .status
        .success());
    let inbox = h.run(&["inbox", "--roles"]);
    assert!(!String::from_utf8_lossy(&inbox.stdout).contains("QUESTION: which option?"));
}
#[test]
fn g59_peek_unknown_role_is_clear() {
    let h = world("g59-peek");
    let o = h.run(&["peek", "missing", "--lines", "5"]);
    assert!(!o.status.success());
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("role 'missing' is not registered"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}
#[test]
fn g62_login_screen_is_detected_before_typing() {
    let mut h = world("g62-login");
    h.set("HORCH_FAKE_SCENARIO", "mail_drop");
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    assert!(create
        .args(["workspace", "create", "--label", "login"])
        .output()
        .unwrap()
        .status
        .success());
    let mb = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mb).unwrap();
    std::fs::write(mb.join("orchestrator.id"), "w1:p1").unwrap();
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--text",
            "Claude Code\nPlease run /login\n",
            "--status",
            "idle",
            "--agent",
            "claude",
        ],
    );
    let o = worker_tell(&h, "NOTE: waiting for login");
    assert!(o.status.success());
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("needs /login"),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(Mail::under(&h.state, "w1").pending().unwrap().len(), 1);
    let _ = h.run(&["inbox"]);
}

fn fake(h: &Harness, args: &[&str]) {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let o = cmd.args(args).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn g59_statuses_and_five_line_peek_are_read_only() {
    let h = world("g59-status-tail");
    fake(&h, &["workspace", "create", "--label", "roles"]);
    let mb = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mb).unwrap();
    std::fs::write(mb.join("idle.id"), "w1:p1").unwrap();
    std::fs::write(mb.join("closed.id"), "w1:p99").unwrap();
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--text",
            "zero\none\ntwo\nthree\nfour\nfive\n",
            "--status",
            "idle",
            "--agent",
            "claude",
        ],
    );
    for status in ["idle", "working", "blocked", "done", "unknown"] {
        fake(&h, &["pane", "test-state", "w1:p1", "--status", status]);
        for args in [&["inbox", "--roles"][..], &["inbox"][..]] {
            let o = h.run(args);
            let text = String::from_utf8_lossy(&o.stdout);
            assert!(o.status.success());
            assert!(text.contains(&format!("idle: {status}")), "{text}");
            assert!(text.contains("pane closed"), "{text}");
        }
    }
    let before = h.calls_of("herdr").len();
    let o = h.run(&["peek", "idle", "--lines", "5"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        "one\ntwo\nthree\nfour\nfive\n"
    );
    for call in h.calls_of("herdr").iter().skip(before) {
        let args = call["argv"].to_string();
        assert!(
            !args.contains("prompt") && !args.contains("send-text") && !args.contains("send-keys"),
            "{call}"
        );
    }
}

#[test]
fn g62_courier_notifies_once_and_resumes_after_login() {
    use horch_e2e::harness::wait_for;
    let mut h = world("g62-login-resume");
    h.set("HORCH_TEST_MAIL_REDELIVERY_MS", "50");
    fake(&h, &["workspace", "create", "--label", "login"]);
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--text",
            "Please run /login",
            "--status",
            "idle",
            "--agent",
            "claude",
        ],
    );
    let mb = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mb).unwrap();
    std::fs::write(mb.join("orchestrator.id"), "w1:p1").unwrap();
    assert!(worker_tell(&h, "NOTE: login mail").status.success());
    std::thread::sleep(Duration::from_millis(150));
    let calls = h.calls_of("herdr");
    let notifications = |calls: &[serde_json::Value]| {
        calls
            .iter()
            .filter(|c| c["argv"][0] == "notification")
            .count()
    };
    assert_eq!(notifications(&calls), 1);
    assert!(!calls
        .iter()
        .any(|c| c["argv"][0] == "agent" && c["argv"][1] == "prompt"));
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
        ],
    );
    wait_for("resumed redelivery", || {
        h.calls_of("herdr")
            .iter()
            .any(|c| c["argv"][0] == "agent" && c["argv"][1] == "prompt")
            .then_some(())
    });
    assert_eq!(notifications(&h.calls_of("herdr")), 1);
    assert_eq!(Mail::under(&h.state, "w1").pending().unwrap().len(), 1);
    assert!(h.run(&["inbox"]).status.success());
}

#[test]
fn g62_login_matcher_has_a_saved_current_screen() {
    assert!(horch_core::messaging::mail::needs_login(include_str!(
        "../../horch-core/src/messaging/fixtures/claude-login.txt"
    )));
    assert!(!horch_core::messaging::mail::needs_login("Ready to work"));
}

fn ledger_worker() -> (
    tempfile::TempDir,
    horch_core::taskledger::Store,
    horch_core::taskledger::runs::Run,
    String,
) {
    use horch_core::taskledger::{
        model::Actor,
        runs::{self, NewRun},
        tasks::{self, NewTask},
        workers::NewWorker,
        Store,
    };
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let (run, worker) = store
        .immediate(|tx| {
            let run = runs::create(
                tx,
                std::path::Path::new("/project"),
                &NewRun {
                    slug: "waits".into(),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
            )?;
            tasks::add(
                tx,
                &run,
                &NewTask::work("author", "author.md", "codex-terra"),
                Actor::Orchestrator,
            )?;
            let worker = bind_first(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "codex-terra".into(),
                    role: Some("worker-1".into()),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
                horch_core::fleetd::task_execution::domain::task::Recorded {
                    pid: 2,
                    pid_start: Some(2),
                },
            )?;
            horch_core::taskledger::testing::record_supervisor(tx, &worker, 1, Some(1))?;
            Ok((run, worker))
        })
        .unwrap();
    (dir, store, run, worker)
}
struct Idle(bool);
impl horch_core::taskledger::watchdog::Observer for Idle {
    fn observe(
        &self,
        _: &horch_core::taskledger::workers::Worker,
    ) -> horch_core::taskledger::watchdog::Observation {
        horch_core::taskledger::watchdog::Observation {
            agent_alive: self.0,
            herdr_status: Some("idle".into()),
            ..Default::default()
        }
    }
}
fn check(
    store: &mut horch_core::taskledger::Store,
    run: &horch_core::taskledger::runs::Run,
    worker: &str,
    alive: bool,
) -> String {
    store
        .conn()
        .execute(
            "UPDATE workers SET last_progress_at='2000-01-01T00:00:00Z' WHERE worker_id=?1",
            [worker],
        )
        .unwrap();
    horch_core::taskledger::watchdog::check_run(store, run, &Idle(alive), true).unwrap()[0]
        .verdict
        .clone()
}
/// 3 checks of `worker` with no progress: each gives `no_progress` with a
/// rising stall round, none gives `dead`, and the worker stays `working`.
fn assert_no_reap(
    store: &mut horch_core::taskledger::Store,
    run: &horch_core::taskledger::runs::Run,
    worker: &str,
) {
    use horch_core::taskledger::workers;
    let mut rounds = Vec::new();
    for _ in 0..3 {
        store
            .conn()
            .execute(
                "UPDATE workers SET last_progress_at='2000-01-01T00:00:00Z' WHERE worker_id=?1",
                [worker],
            )
            .unwrap();
        let c = horch_core::taskledger::watchdog::check_run(store, run, &Idle(true), true)
            .unwrap()
            .remove(0);
        assert_eq!(c.verdict, "no_progress", "{c:?}");
        rounds.push(c.round);
    }
    assert!(rounds.windows(2).all(|r| r[1] > r[0]), "{rounds:?}");
    assert_eq!(
        workers::get(store.conn(), worker).unwrap().status,
        "working"
    );
}
#[test]
fn g70_reviewer_wait_holds_sixty_minutes_then_three_idle_checks_reap() {
    use horch_core::taskledger::{
        model::{Actor, TaskKind},
        tasks::{self, NewTask},
        workers,
    };
    let (_dir, mut store, run, worker) = ledger_worker();
    let review = store
        .immediate(|tx| {
            let mut t = NewTask::work("review", "review.md", "codex-terra");
            t.kind = TaskKind::Review;
            tasks::add(tx, &run, &t, Actor::Worker)
        })
        .unwrap();
    workers::set_wait(
        store.conn(),
        &worker,
        "reviewers",
        "waiting for reviewer tasks",
        std::slice::from_ref(&review.task_id),
        7 * 86400,
    )
    .unwrap();
    // Six ten-minute checks, sixty minutes from the original wait, with no proxy progress.
    for _ in 0..6 {
        store.conn().execute("UPDATE worker_waits SET started_at=strftime('%Y-%m-%dT%H:%M:%SZ', started_at, '-600 seconds')",[]).unwrap();
        assert_eq!(check(&mut store, &run, &worker, true), "progress");
        assert_eq!(workers::get(store.conn(), &worker).unwrap().stall_rounds, 0);
    }
    let board = horch_core::taskledger::status::render(
        &horch_core::taskledger::status::report(store.conn(), &run).unwrap(),
    );
    assert!(board.contains("waiting for reviewer tasks"), "{board}");
    store
        .conn()
        .execute(
            "UPDATE tasks SET status='done' WHERE task_id=?1",
            [&review.task_id],
        )
        .unwrap();
    // The checks of today see no progress, but no stall round reaps
    // (docs/tickets/fleetd-p2/MAP.md, Q14). The ladder proof: g70_pinned_cli_sixty_minute_review_wait_and_normal_reap_after_completion.
    assert_no_reap(&mut store, &run, &worker);
}
#[test]
fn g70_waiting_background_deadline_and_dead_process() {
    use horch_core::taskledger::{
        model::{EventKind, ProgressSource},
        workers,
    };
    let (_dir, mut store, run, worker) = ledger_worker();
    let (agent, child) = agent_with_child(&store, &worker);
    let wait = |store: &horch_core::taskledger::Store, text: &str| {
        workers::progress(
            store.conn(),
            &worker,
            ProgressSource::Agent,
            EventKind::Progress,
            text,
        )
        .unwrap();
    };
    wait(&store, "waiting: background build");
    assert_eq!(
        workers::waiting(store.conn(), &worker).unwrap()[0].reason,
        "background build"
    );
    assert_eq!(check(&mut store, &run, &worker, true), "progress");
    assert_eq!(check(&mut store, &run, &worker, false), "dead");
    // Past its deadline the wait ends. The checks see no progress, but no
    // stall round reaps (docs/tickets/fleetd-p2/MAP.md, Q14).
    store
        .conn()
        .execute(
            "UPDATE worker_waits SET deadline='2000-01-01T00:00:00Z'",
            [],
        )
        .unwrap();
    for _ in 0..3 {
        assert_eq!(check(&mut store, &run, &worker, true), "no_progress");
    }
    assert!(workers::waiting(store.conn(), &worker).unwrap().is_empty());
    // A new wait holds while the child runs, and ends at once when it
    // exits.
    wait(&store, "waiting: background test");
    assert_eq!(workers::waiting(store.conn(), &worker).unwrap().len(), 1);
    end_child(&agent, child);
    assert!(workers::waiting(store.conn(), &worker).unwrap().is_empty());
    assert_eq!(
        last_wait_end(&store, &worker),
        "child_gone: the background wait ended (background test)"
    );
    drop(agent);
}
#[test]
fn g60_pinned_two_hour_old_mail_redelivers_and_inbox_stops_courier() {
    use horch_e2e::harness::wait_for;
    let mut h = world("g60-two-hours");
    h.set("HORCH_NOW", "2030-01-01T02:00:00Z");
    fake(&h, &["workspace", "create", "--label", "courier"]);
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
            "claude",
        ],
    );
    let mb = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mb).unwrap();
    std::fs::write(mb.join("orchestrator.id"), "w1:p1").unwrap();
    let mail = Mail::under(&h.state, "w1");
    mail.enqueue("worker-1", "NOTE: two hours old").unwrap();
    for entry in std::fs::read_dir(mail.dir().join("pending")).unwrap() {
        let path = entry.unwrap().path();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["created_at"] =
            serde_json::json!({"secs_since_epoch":1893456000u64,"nanos_since_epoch":0});
        std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    let mut courier = h
        .horch(&["mail", "courier", "--workspace", "w1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    wait_for("two-hour redelivery", || {
        h.calls_of("herdr")
            .iter()
            .any(|c| {
                c["argv"][0] == "agent"
                    && c["argv"][1] == "prompt"
                    && c["argv"][3]
                        .as_str()
                        .is_some_and(|s| s == horch_core::messaging::delivery::MAIL_LINE)
            })
            .then_some(())
    });
    // FR-7: the courier types the mail line; the text stays in the mail.
    assert_eq!(mail.pending().unwrap().len(), 1);
    assert!(h.run(&["inbox"]).status.success());
    wait_for("courier exit after acknowledgement", || {
        courier.try_wait().unwrap()
    });
    assert!(mail.pending().unwrap().is_empty());
}

#[cfg(unix)]
fn relay_delivery_case(agent: &str) {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    let mut h = world("g60-relay");
    let short = tempfile::Builder::new()
        .prefix("h60-")
        .tempdir_in("/tmp")
        .unwrap();
    h.state = short.path().to_path_buf();
    if agent == "codex" {
        horch_core::execution::records::SessionLedger::for_project(
            &h.state,
            &h.project.to_string_lossy(),
        )
        .insert(horch_core::execution::legacy::Record {
            record_id: "relay-orchestrator".into(),
            session_id: Some("thread-fleet18".into()),
            agent: "codex".into(),
            tier: "sol".into(),
            model: "gpt-6-sol".into(),
            role: "orchestrator".into(),
            ..Default::default()
        })
        .unwrap();
        h.set("HORCH_RECORD_ID", "relay-orchestrator");
        // macOS may scan a freshly linked executable longer than the relay's
        // real ten-second queue budget. Warm this exact sealed fake first.
        let mut command = std::process::Command::new(h.bin.join("codex"));
        h.seal(&mut command);
        let mut warm = Agent(
            command
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let status = horch_e2e::harness::wait_for("sealed Codex fixture startup", || {
            warm.0.try_wait().unwrap()
        });
        assert!(status.success());
    }
    let error_path = h.root.join("relay.stderr");
    let mut relay = h
        .horch(&["channel", "serve", "--agent", agent])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(&error_path).unwrap())
        .spawn()
        .unwrap();
    let mut input = relay.stdin.take().unwrap();
    let mut output = BufReader::new(relay.stdout.take().unwrap());
    writeln!(input,"{}",serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}})).unwrap();
    input.flush().unwrap();
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    writeln!(
        input,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    input.flush().unwrap();
    let socket = h.state.join("channels/w1.sock");
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while !socket.exists() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    if !socket.exists() {
        let _ = relay.kill();
        let _ = relay.wait();
        panic!(
            "relay did not bind: {}",
            std::fs::read_to_string(&error_path).unwrap()
        );
    }
    let sent = worker_tell(&h, "NOTE: relay is not acknowledgement");
    assert!(
        sent.status.success(),
        "{}",
        String::from_utf8_lossy(&sent.stderr)
    );
    assert!(String::from_utf8_lossy(&sent.stderr).contains("queued, not yet acknowledged"));
    if agent == "claude" {
        line.clear();
        output.read_line(&mut line).unwrap();
        assert!(line.contains("notifications/claude/channel"), "{line}");
    } else {
        assert!(
            h.calls_of("codex").iter().any(|c| c["argv"][0] == "queue"),
            "Codex queue confirmation was not exercised"
        );
    }
    assert_eq!(Mail::under(&h.state, "w1").pending().unwrap().len(), 1);
    assert!(h.run(&["inbox"]).status.success());
    assert!(Mail::under(&h.state, "w1").pending().unwrap().is_empty());
    drop(input);
    assert!(relay.wait().unwrap().success());
}

struct Agent(std::process::Child);
impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn pinned_worker(
    name: &str,
) -> (
    Harness,
    horch_core::taskledger::Store,
    horch_core::taskledger::runs::Run,
    String,
    Agent,
) {
    use horch_core::taskledger::{
        model::Actor,
        runs::{self, NewRun},
        tasks::{self, NewTask},
        workers::NewWorker,
        Store,
    };
    let mut h = world(name);
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
                &NewTask::work("author", "author.md", "codex-terra"),
                Actor::Orchestrator,
            )?;
            let w = bind_first(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "codex-terra".into(),
                    role: Some("worker-1".into()),
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
    store.conn().execute("UPDATE workers SET claimed_at='2030-01-01T00:00:00Z', last_progress_at='2030-01-01T00:00:00Z' WHERE worker_id=?1",[&worker]).unwrap();
    (h, store, run, worker, agent)
}
fn pinned_check(
    h: &Harness,
    run: &horch_core::taskledger::runs::Run,
    at: &str,
) -> serde_json::Value {
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
/// The stall ladder ended the attempt of `worker` `stalled` and stopped
/// `agent` by its recorded PID. After `next_pass` sees the agent gone, the
/// task is queued again with its events and its wake (FM-14).
fn assert_ladder_stopped(
    store: &horch_core::taskledger::Store,
    worker: &str,
    agent: &mut Agent,
    next_pass: impl FnOnce(),
) {
    use horch_core::taskledger::workers;
    let attempt: (String, Option<String>) = store
        .conn()
        .query_row(
            "SELECT state, outcome FROM attempts WHERE worker_id = ?1",
            [worker],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(attempt, ("ended".to_string(), Some("stalled".to_string())));
    let w = workers::get(store.conn(), worker).unwrap();
    assert_eq!(w.status, "incomplete");
    assert_eq!(w.agent_pid, Some(i64::from(agent.0.id())));
    let task_status = || -> String {
        store
            .conn()
            .query_row(
                "SELECT t.status FROM tasks t JOIN attempts a ON a.task_id = t.task_id
                 WHERE a.worker_id = ?1",
                [worker],
                |r| r.get(0),
            )
            .unwrap()
    };
    // The stall pass saw the old process alive: the task stays stalled, so
    // no claim starts a new attempt beside it.
    assert_eq!(
        task_status(),
        "stalled",
        "its old process lived in the pass"
    );
    horch_e2e::harness::wait_for("the stop of the agent by its PID", || {
        matches!(agent.0.try_wait(), Ok(Some(_))).then_some(())
    });
    next_pass();
    assert_eq!(task_status(), "ready", "rung 6 after the process is gone");
    let texts = |sql: &str| -> Vec<String> {
        let mut stmt = store.conn().prepare(sql).unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    let events = texts("SELECT text FROM events WHERE actor = 'horch' ORDER BY event_id");
    for text in [
        "the stall ladder ended it: stalled",
        "working -> incomplete",
        "T-1 is queued again",
    ] {
        assert!(
            events.iter().any(|e| e.contains(text)),
            "{text}: {events:?}"
        );
    }
    let wakes = texts("SELECT text FROM outbox ORDER BY rowid");
    assert!(
        wakes.iter().any(|w| w.contains("T-1: retry after 30 s")),
        "{wakes:?}"
    );
}
fn pin_wait(store: &horch_core::taskledger::Store, worker: &str, kind: &str, deadline: &str) {
    store.conn().execute("UPDATE worker_waits SET started_at='2030-01-01T00:00:00Z',deadline=?3 WHERE worker_id=?1 AND kind=?2",[worker,kind,deadline]).unwrap();
}
#[test]
fn g70_pinned_cli_sixty_minute_review_wait_and_normal_reap_after_completion() {
    use horch_core::taskledger::{
        model::{Actor, TaskKind},
        tasks::{self, NewTask},
        workers,
    };
    let (h, mut store, run, worker, mut agent) = pinned_worker("g70-pinned-review");
    let review = store
        .immediate(|tx| {
            let mut t = NewTask::work("review", "review.md", "codex-terra");
            t.kind = TaskKind::Review;
            tasks::add(tx, &run, &t, Actor::Orchestrator)
        })
        .unwrap();
    workers::set_wait(
        store.conn(),
        &worker,
        "reviewers",
        "slow code reviewers",
        std::slice::from_ref(&review.task_id),
        7 * 86400,
    )
    .unwrap();
    pin_wait(&store, &worker, "reviewers", "2030-01-08T00:00:00Z");
    for at in [
        "2030-01-01T00:10:00Z",
        "2030-01-01T00:20:00Z",
        "2030-01-01T00:30:00Z",
        "2030-01-01T00:40:00Z",
        "2030-01-01T00:50:00Z",
        "2030-01-01T01:00:00Z",
    ] {
        let c = pinned_check(&h, &run, at);
        assert_eq!(c["verdict"], "progress", "{c}");
        assert_eq!(
            workers::get(store.conn(), &worker).unwrap().status,
            "working"
        );
    }
    let status = h
        .horch(&["ledger", "status", "--run", &run.run_id])
        .env("HORCH_NOW", "2030-01-01T01:00:00Z")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&status.stdout).contains("slow code reviewers (1h0m ago"));
    store
        .conn()
        .execute(
            "UPDATE tasks SET status='done' WHERE task_id=?1",
            [&review.task_id],
        )
        .unwrap();
    // The reviews are in: the wait ends. The checks of today see no
    // progress, but no stall round reaps (docs/tickets/fleetd-p2/MAP.md,
    // Q14).
    for at in [
        "2030-01-01T01:10:00Z",
        "2030-01-01T01:20:00Z",
        "2030-01-01T01:30:00Z",
    ] {
        assert_eq!(pinned_check(&h, &run, at)["verdict"], "no_progress");
    }
    assert_eq!(
        workers::get(store.conn(), &worker).unwrap().status,
        "working"
    );
    assert!(matches!(agent.0.try_wait(), Ok(None)), "no check stops it");
    // No activity past stall_after_s (300 s) after the wait: the stall
    // ladder ends the attempt `stalled` and stops the agent by its PID.
    h.advance_fleet_clock(400);
    pinned_check(&h, &run, "2030-01-01T01:40:00Z");
    assert_ladder_stopped(&store, &worker, &mut agent, || {
        pinned_check(&h, &run, "2030-01-01T01:40:00Z");
    });
}
#[test]
fn g70_pinned_cli_question_reply_resets_stalls() {
    use horch_core::taskledger::workers;
    let (h, store, run, worker, mut agent) = pinned_worker("g70-pinned-question");
    fake(&h, &["workspace", "create", "--label", "question"]);
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
            "claude",
        ],
    );
    let mb = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mb).unwrap();
    std::fs::write(mb.join("worker-1.id"), "w1:p1").unwrap();
    let o = h
        .horch(&["tell", "orchestrator", "QUESTION: choose?"])
        .env("HORCH_ROLE", "worker-1")
        .env("HORCH_NOW", "2030-01-01T00:00:00Z")
        .output()
        .unwrap();
    assert!(o.status.success());
    for at in [
        "2030-01-01T00:10:00Z",
        "2030-01-01T01:00:00Z",
        "2030-01-07T00:00:00Z",
    ] {
        assert_eq!(pinned_check(&h, &run, at)["verdict"], "progress");
    }
    let id = open_id(&h.state);
    let reply = h
        .horch(&["tell", "worker-1", "--answers", &id, "Use A"])
        .env("HORCH_NOW", "2030-01-07T00:01:00Z")
        .output()
        .unwrap();
    assert!(
        reply.status.success(),
        "{}",
        String::from_utf8_lossy(&reply.stderr)
    );
    // The reply ends the question wait. The checks of today see no
    // progress, but no stall round reaps (docs/tickets/fleetd-p2/MAP.md,
    // Q14).
    for at in [
        "2030-01-07T00:10:00Z",
        "2030-01-07T00:20:00Z",
        "2030-01-07T00:30:00Z",
    ] {
        assert_eq!(pinned_check(&h, &run, at)["verdict"], "no_progress");
    }
    assert_eq!(
        workers::get(store.conn(), &worker).unwrap().status,
        "working"
    );
    assert!(matches!(agent.0.try_wait(), Ok(None)), "no check stops it");
    // No activity past stall_after_s (300 s) after the reply: the stall
    // ladder ends the attempt `stalled` and stops the agent by its PID.
    h.advance_fleet_clock(400);
    pinned_check(&h, &run, "2030-01-07T00:40:00Z");
    assert_ladder_stopped(&store, &worker, &mut agent, || {
        pinned_check(&h, &run, "2030-01-07T00:40:00Z");
    });
}
#[test]
fn g70_pinned_cli_wait_expires_and_dead_agent_is_immediate() {
    use horch_core::taskledger::workers;
    let (h, store, run, worker, _fake) = pinned_worker("g70-pinned-expiry");
    let (mut agent, child) = agent_with_child(&store, &worker);
    workers::set_wait(store.conn(), &worker, "background", "build", &[], 86400).unwrap();
    pin_wait(&store, &worker, "background", "2030-01-01T00:30:00Z");
    assert_eq!(
        pinned_check(&h, &run, "2030-01-01T00:10:00Z")["verdict"],
        "progress"
    );
    assert_eq!(
        pinned_check(&h, &run, "2030-01-01T00:40:00Z")["verdict"],
        "no_progress"
    );
    workers::set_wait(
        store.conn(),
        &worker,
        "background",
        "another build",
        &[],
        86400,
    )
    .unwrap();
    pin_wait(&store, &worker, "background", "2030-01-02T00:00:00Z");
    assert_eq!(
        pinned_check(&h, &run, "2030-01-01T00:42:00Z")["verdict"],
        "progress",
        "the wait holds while the child runs"
    );
    // The child exits: the wait ends at once, long before its deadline.
    end_child(&agent, child);
    assert_eq!(
        pinned_check(&h, &run, "2030-01-01T00:44:00Z")["verdict"],
        "no_progress"
    );
    assert!(workers::waiting(store.conn(), &worker).unwrap().is_empty());
    assert_eq!(
        last_wait_end(&store, &worker),
        "child_gone: the background wait ended (another build)"
    );
    agent.0.kill().unwrap();
    agent.0.wait().unwrap();
    // 1 pass ends the attempt of the dead agent at once, before the duty
    // check (I4).
    pinned_check(&h, &run, "2030-01-01T00:50:00Z");
    let attempt: (String, Option<String>) = store
        .conn()
        .query_row(
            "SELECT state, outcome FROM attempts WHERE worker_id = ?1",
            [&worker],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        attempt,
        ("ended".to_string(), Some("no_result".to_string()))
    );
    let reason = workers::get(store.conn(), &worker).unwrap().end_reason;
    let reason = reason.unwrap_or_default();
    assert!(
        reason.starts_with("dead: the process of attempt") && reason.ends_with("is gone"),
        "{reason}"
    );
    assert_eq!(
        workers::get(store.conn(), &worker).unwrap().status,
        "incomplete"
    );
}

/// An agent that leads its own process group; the group is killed on
/// drop, so a failed assertion leaks no child.
struct GroupAgent(std::process::Child);

/// Make the agent of `worker` and of its attempt a new process that leads
/// its own process group, with a live child: the background work that
/// holds its wait (MAP.md of fleetd-p2, Q23). Gives the agent and the pid
/// of its child.
fn agent_with_child(store: &horch_core::taskledger::Store, worker: &str) -> (GroupAgent, u32) {
    use std::os::unix::process::CommandExt;
    let agent = GroupAgent(
        std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 600 & wait; exec sleep 3600"])
            .process_group(0)
            .spawn()
            .unwrap(),
    );
    let pid = agent.0.id();
    let started = horch_e2e::harness::wait_for("the agent's identity", || {
        horch_core::procid::start_time(pid)
    });
    let child = horch_e2e::harness::wait_for("the agent's child", || {
        let members = horch_core::procid::group_members(pid);
        members.into_iter().find(|m| *m != pid)
    });
    for sql in [
        format!(
            "UPDATE workers SET agent_pid = {pid}, agent_started = {started} WHERE worker_id = ?1"
        ),
        format!("UPDATE attempts SET pid = {pid}, pid_start = {started} WHERE worker_id = ?1"),
    ] {
        store.conn().execute(&sql, [worker]).unwrap();
    }
    (agent, child)
}

/// Kill the child `child` of `agent`, and wait until it is gone.
fn end_child(agent: &GroupAgent, child: u32) {
    let pid = agent.0.id();
    // SAFETY: a signal to the child that this test started.
    unsafe { libc::kill(child as i32, libc::SIGKILL) };
    horch_e2e::harness::wait_for("the child to exit", || {
        (horch_core::procid::group_members(pid) == [pid]).then_some(())
    });
}

/// The text of the last `wait_ended` event of `worker`.
fn last_wait_end(store: &horch_core::taskledger::Store, worker: &str) -> String {
    store
        .conn()
        .query_row(
            "SELECT text FROM events WHERE kind = 'wait_ended' AND worker_id = ?1
             ORDER BY event_id DESC LIMIT 1",
            [worker],
            |r| r.get(0),
        )
        .unwrap()
}
impl Drop for GroupAgent {
    fn drop(&mut self) {
        // SAFETY: a signal to the process group that this test started.
        unsafe { libc::kill(-(self.0.id() as i32), libc::SIGKILL) };
        let _ = self.0.wait();
    }
}

#[test]
fn g70_long_multiline_questions_and_blocked_survive_transport_ack_and_reply() {
    let (h, store, run, worker, _agent) = pinned_worker("g70-long-lines");
    fake(&h, &["workspace", "create", "--label", "long"]);
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
            "claude",
        ],
    );
    let mb = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mb).unwrap();
    std::fs::write(mb.join("worker-1.id"), "w1:p1").unwrap();
    for (keyword, start, check, reply) in [
        (
            "QUESTION:",
            "2030-01-01T00:00:00Z",
            "2030-01-01T00:10:00Z",
            "2030-01-01T00:11:00Z",
        ),
        (
            "BLOCKED:",
            "2030-01-01T01:00:00Z",
            "2030-01-01T01:10:00Z",
            "2030-01-01T01:11:00Z",
        ),
    ] {
        let body = format!("NOTE: {}\n{keyword} choose A or B?", "details ".repeat(100));
        let o = h
            .horch(&["tell", "orchestrator", &body])
            .env("HORCH_ROLE", "worker-1")
            .env("HORCH_NOW", start)
            .output()
            .unwrap();
        assert!(o.status.success());
        let inbox = h
            .horch(&["inbox"])
            .env("HORCH_NOW", check)
            .output()
            .unwrap();
        assert!(inbox.status.success());
        let normal = String::from_utf8_lossy(&inbox.stdout);
        assert!(
            normal.contains(&format!("{keyword} choose A or B?"))
                && normal.contains("open (600s ago)"),
            "{normal}"
        );
        let roles = h
            .horch(&["inbox", "--roles"])
            .env("HORCH_NOW", check)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&roles.stdout);
        assert!(
            text.contains(&format!("{keyword} choose A or B?")),
            "{text}"
        );
        assert!(text.contains("open (600s ago)"), "{text}");
        assert!(text.contains("waiting:"), "{text}");
        assert_eq!(pinned_check(&h, &run, check)["verdict"], "progress");
        let status = h
            .horch(&["inbox", "--roles"])
            .env("HORCH_NOW", check)
            .output()
            .unwrap();
        let status = String::from_utf8_lossy(&status.stdout);
        assert!(
            status.contains("watchdog=progress") && status.contains("waiting:"),
            "{status}"
        );
        let id = open_id(&h.state);
        let answered = h
            .horch(&["tell", "worker-1", "--answers", &id, "Use A"])
            .env("HORCH_NOW", reply)
            .output()
            .unwrap();
        assert!(answered.status.success());
        assert!(Mail::under(&h.state, "w1")
            .open_questions()
            .unwrap()
            .is_empty());
        assert!(
            horch_core::taskledger::workers::waiting(store.conn(), &worker)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn g70_delayed_older_reply_cannot_reopen_newer_answered_question() {
    let h = world("g70-monotonic-answer");
    fake(&h, &["workspace", "create", "--label", "reply"]);
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
            "claude",
        ],
    );
    let mb = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mb).unwrap();
    std::fs::write(mb.join("worker-1.id"), "w1:p1").unwrap();
    let q = h
        .horch(&["tell", "orchestrator", "QUESTION: later question"])
        .env("HORCH_ROLE", "worker-1")
        .env("HORCH_NOW", "2030-01-01T00:10:00Z")
        .output()
        .unwrap();
    assert!(q.status.success());
    let id = open_id(&h.state);
    let o = h
        .horch(&["tell", "worker-1", "--answers", &id, "answer"])
        .env("HORCH_NOW", "2030-01-01T00:20:00Z")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // A closed question takes no second answer, and an older reply does not
    // reopen it.
    for (args, ok) in [
        (
            vec!["tell", "worker-1", "--answers", id.as_str(), "again"],
            false,
        ),
        (vec!["tell", "worker-1", "answer"], true),
    ] {
        let o = h
            .horch(&args)
            .env("HORCH_NOW", "2030-01-01T00:00:00Z")
            .output()
            .unwrap();
        assert_eq!(o.status.success(), ok, "{args:?}");
        assert!(
            Mail::under(&h.state, "w1")
                .open_questions()
                .unwrap()
                .is_empty(),
            "an older reply {args:?} reopened the question"
        );
    }
    // Older replies also overlap each other across independent CLI processes.
    let mut replies: Vec<_> = (0..8)
        .map(|minute| {
            Agent(
                h.horch(&["tell", "worker-1", "delayed answer"])
                    .env("HORCH_NOW", format!("2030-01-01T00:0{minute}:00Z"))
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap(),
            )
        })
        .collect();
    let until = std::time::Instant::now() + Duration::from_secs(10);
    for reply in &mut replies {
        loop {
            if let Some(status) = reply.0.try_wait().unwrap() {
                let mut stderr = String::new();
                if let Some(mut pipe) = reply.0.stderr.take() {
                    std::io::Read::read_to_string(&mut pipe, &mut stderr).unwrap();
                }
                assert!(status.success(), "{status}: {stderr}");
                break;
            }
            assert!(
                std::time::Instant::now() < until,
                "overlapping replies exceeded bounded completion"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    assert!(Mail::under(&h.state, "w1")
        .open_questions()
        .unwrap()
        .is_empty());
}

#[test]
fn g70_concurrent_expired_question_pruners_are_idempotent() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let mail = Arc::new(Mail::under(dir.path(), "w1"));
    for _ in 0..64 {
        mail.enqueue("worker-1", "QUESTION: old").unwrap();
    }
    for entry in std::fs::read_dir(mail.dir().join("questions")).unwrap() {
        let path = entry.unwrap().path();
        let mut message: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        message["created_at"] = serde_json::json!({"secs_since_epoch":0,"nanos_since_epoch":0});
        std::fs::write(path, serde_json::to_vec(&message).unwrap()).unwrap();
    }
    let barrier = Arc::new(Barrier::new(16));
    let readers: Vec<_> = (0..16)
        .map(|_| {
            let mail = mail.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                mail.open_questions()
            })
        })
        .collect();
    for reader in readers {
        assert!(reader.join().unwrap().unwrap().is_empty());
    }
    assert!(mail.open_questions().unwrap().is_empty());
}

fn clock_file_set(path: &std::path::Path, at: &str) {
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, at).unwrap();
    std::fs::rename(temp, path).unwrap();
}
fn prompt_count(h: &Harness) -> usize {
    h.calls_of("herdr")
        .iter()
        .filter(|c| c["argv"][0] == "agent" && c["argv"][1] == "prompt")
        .count()
}
fn wait_for_pane_state_observed(h: &Harness) {
    horch_e2e::harness::wait_for("courier observes pane fixture state", || {
        let calls = h.calls_of("herdr");
        let changed = calls
            .iter()
            .rposition(|c| c["argv"][0] == "pane" && c["argv"][1] == "test-state")?;
        (calls
            .iter()
            .skip(changed + 1)
            .filter(|c| c["argv"][0] == "pane" && c["argv"][1] == "read")
            .count()
            >= 2)
            .then_some(())
    });
}

fn wait_for_pass_after_last_prompt(h: &Harness) {
    horch_e2e::harness::wait_for("courier pass after completed attempt", || {
        let calls = h.calls_of("herdr");
        let last = calls
            .iter()
            .rposition(|c| c["argv"][0] == "agent" && c["argv"][1] == "prompt")?;
        calls
            .iter()
            .skip(last + 1)
            .any(|c| c["argv"][0] == "pane" && c["argv"][1] == "read")
            .then_some(())
    });
}

/// A world for a courier that runs under a pinned clock. The orchestrator
/// exists, so the courier must live until its mail is acknowledged. The
/// 20 ms no-orchestrator limit of [`world`] would stop it after 2 failed
/// `pane get` calls in a row, which a loaded machine produces.
fn pinned_clock_world(name: &str) -> (Harness, std::path::PathBuf) {
    let mut h = world(name);
    h.set("HORCH_NOW", "2030-01-01T00:00:00Z");
    h.set("HORCH_TEST_MAIL_POLL_MS", "20");
    let clock = h.root.join("clock.txt");
    clock_file_set(&clock, "2030-01-01T00:00:00Z");
    h.set("HORCH_TEST_MAIL_CLOCK_FILE", clock.to_string_lossy());
    (h, clock)
}

#[test]
fn g60_pinned_courier_survives_a_herdr_outage() {
    let (h, _clock) = pinned_clock_world("g60-outage");
    fake(&h, &["workspace", "create", "--label", "resume"]);
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
            "claude",
        ],
    );
    let mb = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mb).unwrap();
    std::fs::write(mb.join("orchestrator.id"), "w1:p1").unwrap();
    Mail::under(&h.state, "w1")
        .enqueue("worker-1", "NOTE: outage")
        .unwrap();
    let mut courier = Agent(
        h.horch(&["mail", "courier", "--workspace", "w1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for_pane_state_observed(&h);
    // The fake herdr answers `pane get` with an error while its state file
    // is away: the courier sees its orchestrator vanish for a moment.
    let state = std::path::PathBuf::from(format!("{}.state.json", h.log.display()));
    let away = state.with_extension("away");
    std::fs::rename(&state, &away).unwrap();
    let before = h.calls_of("herdr").len();
    horch_e2e::harness::wait_for("courier polls during the outage", || {
        (h.calls_of("herdr").len() >= before + 3).then_some(())
    });
    std::fs::rename(&away, &state).unwrap();
    let after = h.calls_of("herdr").len();
    horch_e2e::harness::wait_for("courier polls after the outage", || {
        (h.calls_of("herdr").len() >= after + 3).then_some(())
    });
    assert!(
        courier.0.try_wait().unwrap().is_none(),
        "the courier must outlive a herdr outage"
    );
}

#[test]
fn g60_pinned_running_courier_skips_busy_and_login_backlog_without_burst() {
    use horch_e2e::harness::wait_for;
    for held in ["busy", "login"] {
        let (h, clock) = pinned_clock_world(&format!("g60-resume-{held}"));
        fake(&h, &["workspace", "create", "--label", "resume"]);
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
                "claude",
            ],
        );
        let mb = h.state.join("herdr-orchestration-w1");
        std::fs::create_dir_all(&mb).unwrap();
        std::fs::write(mb.join("orchestrator.id"), "w1:p1").unwrap();
        let mail = Mail::under(&h.state, "w1");
        mail.enqueue("worker-1", "NOTE: no catch-up burst").unwrap();
        for e in std::fs::read_dir(mail.dir().join("pending")).unwrap() {
            let path = e.unwrap().path();
            let mut m: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            m["created_at"] =
                serde_json::json!({"secs_since_epoch":1893456000u64,"nanos_since_epoch":0});
            std::fs::write(path, serde_json::to_vec(&m).unwrap()).unwrap();
        }
        let mut courier = Agent(
            h.horch(&["mail", "courier", "--workspace", "w1"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        wait_for_pane_state_observed(&h);
        clock_file_set(&clock, "2030-01-01T00:00:30Z");
        wait_for("courier reads the pinned clock file", || {
            (prompt_count(&h) >= 1).then_some(())
        });
        assert_eq!(
            prompt_count(&h),
            1,
            "courier must read the pinned clock file"
        );
        wait_for_pass_after_last_prompt(&h);
        fake(
            &h,
            &[
                "pane",
                "test-state",
                "w1:p1",
                "--status",
                if held == "busy" { "working" } else { "idle" },
                "--text",
                if held == "login" {
                    "Please run /login"
                } else {
                    "Working"
                },
            ],
        );
        wait_for_pane_state_observed(&h);
        clock_file_set(&clock, "2030-01-01T02:00:00Z");
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(prompt_count(&h), 1);
        fake(
            &h,
            &[
                "pane",
                "test-state",
                "w1:p1",
                "--status",
                "done",
                "--text",
                "Ready",
            ],
        );
        wait_for("single resumed retry", || {
            (prompt_count(&h) >= 2).then_some(())
        });
        wait_for_pass_after_last_prompt(&h);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            prompt_count(&h),
            2,
            "missed retries must not burst after {held}"
        );
        clock_file_set(&clock, "2030-01-01T02:14:59Z");
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(prompt_count(&h), 2);
        clock_file_set(&clock, "2030-01-01T02:15:00Z");
        wait_for("fifteen-minute digest", || {
            (prompt_count(&h) >= 3).then_some(())
        });
        assert_eq!(prompt_count(&h), 3);
        assert!(h.run(&["inbox"]).status.success());
        wait_for("courier acknowledges and exits", || {
            courier.0.try_wait().unwrap()
        });
    }
}

#[test]
fn g70_question_retention_failed_reply_and_new_question_after_answer() {
    let h = world("g70-retention");
    let question = |at: &str, text: &str| {
        let o = h
            .horch(&["tell", "orchestrator", text])
            .env("HORCH_ROLE", "worker-1")
            .env("HORCH_NOW", at)
            .output()
            .unwrap();
        assert!(o.status.success());
    };
    question("2030-01-01T00:00:00Z", "QUESTION: first");
    assert!(h.run(&["inbox"]).status.success());
    let id = open_id(&h.state);
    let failed = h
        .horch(&["tell", "worker-1", "--answers", &id, "answer"])
        .env("HORCH_NOW", "2030-01-01T00:01:00Z")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert_eq!(
        Mail::under(&h.state, "w1").open_questions().unwrap().len(),
        1
    );
    fake(&h, &["workspace", "create", "--label", "retention"]);
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
            "claude",
        ],
    );
    let mb = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mb).unwrap();
    std::fs::write(mb.join("worker-1.id"), "w1:p1").unwrap();
    assert!(h
        .horch(&["tell", "worker-1", "--answers", &id, "answer"])
        .env("HORCH_NOW", "2030-01-01T00:02:00Z")
        .output()
        .unwrap()
        .status
        .success());
    question("2030-01-01T00:03:00Z", "BLOCKED: new after answer");
    assert!(h.run(&["inbox"]).status.success());
    for (at, present) in [
        ("2030-01-08T00:03:00Z", true),
        ("2030-01-08T00:03:01Z", false),
    ] {
        let o = h
            .horch(&["inbox", "--roles"])
            .env("HORCH_NOW", at)
            .output()
            .unwrap();
        assert!(o.status.success());
        let text = String::from_utf8_lossy(&o.stdout);
        assert_eq!(
            text.contains("BLOCKED: new after answer"),
            present,
            "{text}"
        );
    }
}

#[cfg(unix)]
#[test]
fn g60_claude_relay_notification_stays_pending_until_inbox() {
    relay_delivery_case("claude");
}
#[cfg(unix)]
#[test]
fn g60_codex_queue_confirmation_stays_pending_until_inbox() {
    relay_delivery_case("codex");
}

#[test]
fn g60_plain_done_and_tell_without_registration_arrive_after_late_registration() {
    use horch_core::execution::{legacy::Record, records::SessionLedger};
    use horch_e2e::harness::wait_for;
    let mut h = world("g60-plain-done");
    h.set("HORCH_TEST_MAIL_REDELIVERY_MS", "50");
    fake(&h, &["workspace", "create", "--label", "plain-done"]);
    fake(&h, &["pane", "split", "w1:p1", "--direction", "right"]);
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .insert(Record {
            record_id: "plain-worker".into(),
            agent: "claude".into(),
            tier: "sonnet".into(),
            model: "sonnet".into(),
            role: "worker-1".into(),
            task: "plain work".into(),
            ..Default::default()
        })
        .unwrap();
    let sent = worker_tell(&h, "NOTE: before registration");
    assert!(sent.status.success());
    assert!(
        String::from_utf8_lossy(&sent.stderr).contains("redelivered until horch inbox reads it")
    );
    let done = h
        .horch(&["done", "plain work complete"])
        .env("HORCH_ROLE", "worker-1")
        .env("HORCH_RECORD_ID", "plain-worker")
        .env("HERDR_PANE_ID", "w1:p2")
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let mail = Mail::under(&h.state, "w1");
    assert_eq!(mail.pending().unwrap().len(), 2);
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--text",
            "Ready",
            "--status",
            "done",
            "--agent",
            "claude",
        ],
    );
    let registered = h
        .horch(&["register", "orchestrator"])
        .env("HERDR_PANE_ID", "w1:p1")
        .output()
        .unwrap();
    assert!(registered.status.success());
    wait_for("the mail line typed after registration", || {
        h.calls_of("herdr")
            .iter()
            .any(|c| {
                c["argv"][0] == "agent"
                    && c["argv"][2] == "w1:p1"
                    && c["argv"][3]
                        .as_str()
                        .is_some_and(|s| s == horch_core::messaging::delivery::MAIL_LINE)
            })
            .then_some(())
    });
    // FR-7: both reports wait in the mail until inbox reads them.
    let pending: Vec<String> = mail
        .pending()
        .unwrap()
        .into_iter()
        .map(|m| m.text)
        .collect();
    assert!(
        pending
            .iter()
            .any(|t| t.contains("NOTE: before registration")),
        "{pending:?}"
    );
    assert!(
        pending
            .iter()
            .any(|t| t.contains("DONE: plain work complete")),
        "{pending:?}"
    );
    assert!(h.run(&["inbox"]).status.success());
    assert!(mail.pending().unwrap().is_empty());
}

#[test]
fn g60_optional_question_projection_error_does_not_abort_courier_startup() {
    let mut h = world("g60-broken-projection");
    let db = h.root.join("broken.sqlite3");
    std::fs::write(&db, "not sqlite").unwrap();
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    let o = worker_tell(&h, "QUESTION: still durable");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(Mail::under(&h.state, "w1").pending().unwrap().len(), 1);
    horch_e2e::harness::wait_for("courier startup after projection error", || {
        h.state.join("mail/w1/courier.lock").exists().then_some(())
    });
}

/// #177: a watchdog pass whose orchestrator pane stays `unknown` (an agent
/// herdr has not classified) ends within the push budget of #67, and its
/// stall wake waits as orchestrator mail.
#[test]
fn a_pass_with_an_unknown_orchestrator_ends_within_the_push_budget() {
    let (mut h, _store, run, _worker, _agent) = pinned_worker("t177-unknown-orchestrator");
    h.set("HORCH_NO_WAKE", "0");
    // A 2 s budget in place of the real 20 s.
    h.set("HORCH_FAULT", "done-push-budget-ms:2000");
    fake(&h, &["workspace", "create", "--label", "orchestrator"]);
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--status",
            "unknown",
            "--agent",
            "claude",
        ],
    );
    let mb = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mb).unwrap();
    std::fs::write(mb.join("orchestrator.id"), "w1:p1").unwrap();
    // macOS scans a new executable at its first exec: keep that out of the
    // timed pass.
    assert!(h.run(&["--version"]).status.success());

    let began = std::time::Instant::now();
    let c = pinned_check(&h, &run, "2030-01-01T00:10:00Z");
    let took = began.elapsed();

    assert_eq!(c["verdict"], "no_progress", "{c}");
    // The pass waits for the orchestrator until the 2 s budget ends, and
    // no longer: without a budget the readiness wait alone is 120 s, and
    // the real budget is 20 s.
    assert!(
        took >= Duration::from_secs(2),
        "the pass took {took:?}, so it did not wait out the budget"
    );
    assert!(took < Duration::from_secs(10), "the pass took {took:?}");
    let pending = Mail::under(&h.state, "w1").pending().unwrap();
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert!(
        pending[0].text.contains("worker-1 on") && pending[0].text.contains("shows no progress"),
        "{pending:?}"
    );
    assert!(h.run(&["inbox"]).status.success());
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
