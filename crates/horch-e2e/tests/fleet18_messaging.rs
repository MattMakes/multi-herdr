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
    h.set("HORCH_TEST_MAIL_NO_ORCHESTRATOR_MS", "20");
    h.set("HORCH_TEST_MAIL_POLL_MS", "10");
    h
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
    assert!(h
        .run(&["tell", "worker-1", "Choose option A"])
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
        workers::{self, NewWorker},
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
            let worker = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "codex-terra".into(),
                    role: Some("worker-1".into()),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
            )?;
            workers::supervisor_started(tx, &worker, 1, Some(1))?;
            workers::agent_started(tx, &worker, 2, Some(2))?;
            tasks::claim(tx, &worker)?;
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
    for verdict in ["no_progress", "no_progress", "dead"] {
        assert_eq!(check(&mut store, &run, &worker, true), verdict);
    }
}
#[test]
fn g70_waiting_background_deadline_and_dead_process() {
    use horch_core::taskledger::{
        model::{EventKind, ProgressSource},
        workers,
    };
    let (_dir, mut store, run, worker) = ledger_worker();
    workers::progress(
        store.conn(),
        &worker,
        ProgressSource::Agent,
        EventKind::Progress,
        "waiting: background build",
    )
    .unwrap();
    assert_eq!(
        workers::waiting(store.conn(), &worker).unwrap()[0].reason,
        "background build"
    );
    assert_eq!(check(&mut store, &run, &worker, true), "progress");
    assert_eq!(check(&mut store, &run, &worker, false), "dead");
    store
        .conn()
        .execute(
            "UPDATE worker_waits SET deadline='2000-01-01T00:00:00Z'",
            [],
        )
        .unwrap();
    for verdict in ["no_progress", "no_progress", "dead"] {
        assert_eq!(check(&mut store, &run, &worker, true), verdict);
    }
}
#[test]
fn g70_question_wait_clears_on_orchestrator_answer_and_stalls_restart() {
    use horch_core::taskledger::workers;
    let (dir, mut store, run, worker) = ledger_worker();
    let mail = Mail::under(dir.path(), "w1");
    let id = mail.enqueue("worker-1", "QUESTION: choose?").unwrap();
    mail.ack(&id).unwrap();
    workers::sync_questions(store.conn(), dir.path(), "w1").unwrap();
    for _ in 0..6 {
        assert_eq!(check(&mut store, &run, &worker, true), "progress");
    }
    mail.answer("worker-1").unwrap();
    workers::sync_questions(store.conn(), dir.path(), "w1").unwrap();
    assert!(workers::waiting(store.conn(), &worker).unwrap().is_empty());
    for verdict in ["no_progress", "no_progress", "dead"] {
        assert_eq!(check(&mut store, &run, &worker, true), verdict);
    }
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
                        .is_some_and(|s| s.contains("redelivery"))
            })
            .then_some(())
    });
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
        horch_core::execution::records::Ledger::for_project(&h.state, &h.project.to_string_lossy())
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

#[test]
fn g60_ledger_done_and_wake_queue_without_registration_then_courier_delivers() {
    use horch_core::{
        execution::{legacy::Record, records::Ledger},
        taskledger::{
            model::{Actor, TaskKind},
            runs::{self, NewRun},
            tasks::{self, NewTask},
            workers::{self, NewWorker},
            Store,
        },
    };
    use horch_e2e::harness::wait_for;
    let mut h = world("g60-ledger-wake").with_git();
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_TEST_MAIL_REDELIVERY_MS", "100");
    h.set("HORCH_TEST_MAIL_NO_ORCHESTRATOR_MS", "200");
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    fake(&h, &["workspace", "create", "--label", "wake"]);
    fake(&h, &["pane", "split", "w1:p1", "--direction", "right"]);
    let mut store = Store::open(&db).unwrap();
    let worker = store
        .immediate(|tx| {
            let run = runs::create(
                tx,
                &h.project,
                &NewRun {
                    slug: "wakes-on".into(),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
            )?;
            let mut t = NewTask::work("review", "review.md", "codex-terra");
            t.kind = TaskKind::Review;
            let findings = h.root.join("findings.md");
            std::fs::write(
                &findings,
                "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n",
            )?;
            t.spec_ref = Some(findings.to_string_lossy().into_owned());
            tasks::add(tx, &run, &t, Actor::Orchestrator)?;
            let w = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id,
                    persona: "codex-terra".into(),
                    role: Some("worker-1".into()),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
            )?;
            workers::supervisor_started(tx, &w, 1, Some(1))?;
            workers::agent_started(tx, &w, 2, Some(2))?;
            tasks::claim(tx, &w)?;
            Ok(w)
        })
        .unwrap();
    drop(store);
    Ledger::for_project(&h.state, &h.project.to_string_lossy())
        .insert(Record {
            record_id: "r-g60".into(),
            agent: "claude".into(),
            tier: "sonnet".into(),
            model: "sonnet".into(),
            role: "worker-1".into(),
            task: "review".into(),
            ..Default::default()
        })
        .unwrap();
    let done = h
        .horch(&["ledger", "done", "review complete"])
        .env("HORCH_WORKER_ID", &worker)
        .env("HORCH_ROLE", "worker-1")
        .env("HORCH_RECORD_ID", "r-g60")
        .env("HERDR_PANE_ID", "w1:p2")
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let mail = Mail::under(&h.state, "w1");
    let pending = mail.pending().unwrap();
    assert!(
        pending
            .iter()
            .any(|m| m.from == "horch" && m.text.contains("All tasks are done")),
        "{pending:?}"
    );
    assert!(
        pending
            .iter()
            .any(|m| m.from == "worker-1" && m.text.contains("DONE: review complete")),
        "{pending:?}"
    );
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
    let registered = h
        .horch(&["register", "orchestrator"])
        .env("HERDR_PANE_ID", "w1:p1")
        .output()
        .unwrap();
    assert!(
        registered.status.success(),
        "{}",
        String::from_utf8_lossy(&registered.stderr)
    );
    wait_for("after-done wake through courier", || {
        h.calls_of("herdr")
            .iter()
            .any(|c| {
                c["argv"][0] == "agent"
                    && c["argv"][3].as_str().is_some_and(|s| {
                        s.contains("All tasks are done") && s.contains("redelivery")
                    })
            })
            .then_some(())
    });
    let inbox = h.run(&["inbox"]);
    assert!(inbox.status.success());
    assert!(String::from_utf8_lossy(&inbox.stdout).contains("All tasks are done"));
}

/// #77: a registry that exists only in the old temp location, owned by
/// this user and mode 0700, is copied once into the state folder. A ledger
/// worker's `horch ledger done` then finds the orchestrator through the
/// copy, types its report there, and frees its role in both places.
#[cfg(unix)]
#[test]
fn ledger_done_finds_the_orchestrator_in_a_copied_old_registry() {
    use horch_core::{
        execution::{legacy::Record, records::Ledger},
        taskledger::{
            model::{Actor, TaskKind},
            runs::{self, NewRun},
            tasks::{self, NewTask},
            workers::{self, NewWorker},
            Store,
        },
    };
    use std::os::unix::fs::PermissionsExt;
    let mut h = world("p09-ledger-done").with_git();
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_NO_WAKE", "1");
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    fake(&h, &["workspace", "create", "--label", "legacy"]);
    fake(&h, &["pane", "split", "w1:p1", "--direction", "right"]);
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
    let legacy = h.tmp.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("orchestrator.id"), "w1:p1").unwrap();
    std::fs::write(legacy.join("worker-1.id"), "w1:p2").unwrap();
    std::fs::set_permissions(&legacy, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut store = Store::open(&db).unwrap();
    let worker = store
        .immediate(|tx| {
            let run = runs::create(
                tx,
                &h.project,
                &NewRun {
                    slug: "legacy".into(),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
            )?;
            let mut t = NewTask::work("review", "review.md", "codex-terra");
            t.kind = TaskKind::Review;
            let findings = h.root.join("findings.md");
            std::fs::write(
                &findings,
                "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n",
            )?;
            t.spec_ref = Some(findings.to_string_lossy().into_owned());
            tasks::add(tx, &run, &t, Actor::Orchestrator)?;
            let w = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id,
                    persona: "codex-terra".into(),
                    role: Some("worker-1".into()),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
            )?;
            workers::supervisor_started(tx, &w, 1, Some(1))?;
            workers::agent_started(tx, &w, 2, Some(2))?;
            tasks::claim(tx, &w)?;
            Ok(w)
        })
        .unwrap();
    drop(store);
    Ledger::for_project(&h.state, &h.project.to_string_lossy())
        .insert(Record {
            record_id: "r-p09".into(),
            agent: "claude".into(),
            tier: "sonnet".into(),
            model: "sonnet".into(),
            role: "worker-1".into(),
            task: "review".into(),
            ..Default::default()
        })
        .unwrap();

    let done = h
        .horch(&["ledger", "done", "review complete"])
        .env("HORCH_WORKER_ID", &worker)
        .env("HORCH_ROLE", "worker-1")
        .env("HORCH_RECORD_ID", "r-p09")
        .env("HERDR_PANE_ID", "w1:p2")
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&done.stderr);
    assert!(done.status.success(), "{stderr}");
    assert!(!stderr.contains("not used"), "{stderr}");
    let state = h.state.join("herdr-orchestration-w1");
    assert_eq!(
        std::fs::read_to_string(state.join("orchestrator.id")).unwrap(),
        "w1:p1",
        "the first command copies the registry"
    );
    let reported = h.calls_of("herdr").iter().any(|c| {
        c["argv"][0] == "agent"
            && c["argv"][1] == "prompt"
            && c["argv"][2] == "w1:p1"
            && c["argv"][3]
                .as_str()
                .is_some_and(|t| t.contains("DONE: review complete"))
    });
    assert!(
        reported,
        "the report is typed into the copied orchestrator pane"
    );
    assert!(!state.join("worker-1.id").exists());
    assert!(!legacy.join("worker-1.id").exists());
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
        workers::{self, NewWorker},
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
            let w = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "codex-terra".into(),
                    role: Some("worker-1".into()),
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
    let (h, mut store, run, worker, _agent) = pinned_worker("g70-pinned-review");
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
    for (at, want) in [
        ("2030-01-01T01:10:00Z", "no_progress"),
        ("2030-01-01T01:20:00Z", "no_progress"),
        ("2030-01-01T01:30:00Z", "dead"),
    ] {
        assert_eq!(pinned_check(&h, &run, at)["verdict"], want);
    }
    assert_eq!(
        workers::get(store.conn(), &worker).unwrap().status,
        "incomplete"
    );
}
#[test]
fn g70_pinned_cli_question_reply_resets_stalls() {
    use horch_core::taskledger::workers;
    let (h, store, run, worker, _agent) = pinned_worker("g70-pinned-question");
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
    let reply = h
        .horch(&["tell", "worker-1", "Use A"])
        .env("HORCH_NOW", "2030-01-07T00:01:00Z")
        .output()
        .unwrap();
    assert!(
        reply.status.success(),
        "{}",
        String::from_utf8_lossy(&reply.stderr)
    );
    for (at, want) in [
        ("2030-01-07T00:10:00Z", "no_progress"),
        ("2030-01-07T00:20:00Z", "no_progress"),
        ("2030-01-07T00:30:00Z", "dead"),
    ] {
        assert_eq!(pinned_check(&h, &run, at)["verdict"], want);
    }
    assert_eq!(
        workers::get(store.conn(), &worker).unwrap().status,
        "incomplete"
    );
}
#[test]
fn g70_pinned_cli_wait_expires_and_dead_agent_is_immediate() {
    use horch_core::taskledger::workers;
    let (h, store, run, worker, mut agent) = pinned_worker("g70-pinned-expiry");
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
    agent.0.kill().unwrap();
    agent.0.wait().unwrap();
    assert_eq!(
        pinned_check(&h, &run, "2030-01-01T00:50:00Z")["verdict"],
        "dead"
    );
    assert_eq!(
        workers::get(store.conn(), &worker).unwrap().status,
        "incomplete"
    );
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
        let answered = h
            .horch(&["tell", "worker-1", "Use A"])
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
    for at in ["2030-01-01T00:20:00Z", "2030-01-01T00:00:00Z"] {
        let o = h
            .horch(&["tell", "worker-1", "answer"])
            .env("HORCH_NOW", at)
            .output()
            .unwrap();
        assert!(o.status.success());
        assert!(
            Mail::under(&h.state, "w1")
                .open_questions()
                .unwrap()
                .is_empty(),
            "older completion at {at} reopened the question"
        );
    }
    // Older completions also overlap each other across independent CLI processes.
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

#[test]
fn g60_pinned_running_courier_skips_busy_and_login_backlog_without_burst() {
    use horch_e2e::harness::wait_for;
    for held in ["busy", "login"] {
        let mut h = world(&format!("g60-resume-{held}"));
        h.set("HORCH_NOW", "2030-01-01T00:00:00Z");
        h.set("HORCH_TEST_MAIL_POLL_MS", "20");
        let clock = h.root.join("clock.txt");
        clock_file_set(&clock, "2030-01-01T00:00:00Z");
        h.set("HORCH_TEST_MAIL_CLOCK_FILE", clock.to_string_lossy());
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
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while prompt_count(&h) < 1 && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
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
        clock_file_set(&clock, "2030-01-01T02:04:59Z");
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(prompt_count(&h), 2);
        clock_file_set(&clock, "2030-01-01T02:05:00Z");
        wait_for("five-minute fixed retry", || {
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
fn g70_finish_cli_creates_reviewer_wait_and_background_progress_cli_sets_deadline() {
    use horch_core::taskledger::{finish::codesweep::REVIEWERS, tasks, workers};
    let (h, store, run, worker, _agent) = pinned_worker("g70-finish-cli");
    let mut h = h.with_git();
    let git = |args: &[&str]| {
        let o = h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    git(&["checkout", "-b", "feature"]);
    std::fs::write(
        h.project.join("feature.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )
    .unwrap();
    git(&["add", "feature.rs"]);
    git(&["commit", "--quiet", "--no-gpg-sign", "-m", "feature"]);
    store
        .conn()
        .execute(
            "UPDATE runs SET base_branch='main',finish_steps='[\"code-sweep\"]' WHERE run_id=?1",
            [&run.run_id],
        )
        .unwrap();
    store
        .conn()
        .execute(
            "UPDATE tasks SET worktree_mode='shared' WHERE current_worker_id=?1",
            [&worker],
        )
        .unwrap();
    let sweep = h.root.join("code-sweep");
    std::fs::create_dir_all(sweep.join("reviewers")).unwrap();
    for (_, name, _) in REVIEWERS {
        std::fs::write(
            sweep.join("reviewers").join(format!("{name}.md")),
            format!("Review {name}.\n"),
        )
        .unwrap();
    }
    h.set("HORCH_CODE_SWEEP_DIR", sweep.to_string_lossy());
    let finish = h
        .horch(&["ledger", "finish", "--json"])
        .env("HORCH_WORKER_ID", &worker)
        .env("HORCH_NOW", "2030-01-01T00:00:00Z")
        .output()
        .unwrap();
    assert_eq!(
        finish.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&finish.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&finish.stdout).unwrap();
    assert_eq!(report["stopped_at"], "code-sweep", "{report}");
    let reviews = report["queued_reviews"].as_array().unwrap();
    assert!(reviews.len() >= 2);
    let waits = workers::waiting(store.conn(), &worker).unwrap();
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].kind, "reviewers");
    assert_eq!(waits[0].task_ids.len(), reviews.len());
    assert_eq!(waits[0].deadline, "2030-01-08T00:00:00Z");
    for at in [
        "2030-01-01T00:10:00Z",
        "2030-01-01T00:20:00Z",
        "2030-01-01T00:30:00Z",
        "2030-01-01T00:40:00Z",
        "2030-01-01T00:50:00Z",
        "2030-01-01T01:00:00Z",
    ] {
        assert_eq!(pinned_check(&h, &run, at)["verdict"], "progress");
    }
    for id in reviews {
        let review = tasks::get(store.conn(), id.as_str().unwrap()).unwrap();
        let created = h.run(&[
            "ledger",
            "worker",
            "new",
            "--persona",
            review.claimable_by.as_deref().unwrap(),
        ]);
        assert!(created.status.success());
        let reviewer = String::from_utf8_lossy(&created.stdout).trim().to_string();
        let claim = h
            .horch(&["ledger", "claim"])
            .env("HORCH_WORKER_ID", &reviewer)
            .output()
            .unwrap();
        assert!(claim.status.success());
        std::fs::write(
            review.spec_ref.as_deref().unwrap(),
            "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n",
        )
        .unwrap();
        let done = h
            .horch(&["ledger", "done", "review complete"])
            .env("HORCH_WORKER_ID", &reviewer)
            .env("HORCH_NOW", "2030-01-01T01:01:00Z")
            .output()
            .unwrap();
        assert!(
            done.status.success(),
            "{}",
            String::from_utf8_lossy(&done.stderr)
        );
    }
    // #177: the reviews are in, and the author has not run finish. It keeps
    // waiting, with the original deadline, and is not reaped before it.
    let waits = workers::waiting(store.conn(), &worker).unwrap();
    assert_eq!(waits.len(), 1, "{waits:?}");
    assert_eq!(waits[0].kind, "reviewers");
    assert!(waits[0].task_ids.is_empty(), "{waits:?}");
    assert_eq!(waits[0].deadline, "2030-01-08T00:00:00Z");
    for at in [
        "2030-01-01T01:10:00Z",
        "2030-01-01T01:20:00Z",
        "2030-01-01T01:30:00Z",
        "2030-01-07T23:50:00Z",
    ] {
        assert_eq!(pinned_check(&h, &run, at)["verdict"], "progress");
    }
    for (at, want) in [
        ("2030-01-08T00:10:00Z", "no_progress"),
        ("2030-01-08T00:20:00Z", "no_progress"),
        ("2030-01-08T00:30:00Z", "dead"),
    ] {
        assert_eq!(pinned_check(&h, &run, at)["verdict"], want);
    }
    assert_eq!(
        workers::get(store.conn(), &worker).unwrap().status,
        "incomplete"
    );

    // The background entry point is exercised independently of injected waits.
    let (h, store, run, worker, _agent) = pinned_worker("g70-progress-cli");
    let progress = h
        .horch(&["ledger", "progress", "waiting: background command"])
        .env("HORCH_WORKER_ID", &worker)
        .env("HORCH_NOW", "2030-01-01T00:00:00Z")
        .output()
        .unwrap();
    assert!(progress.status.success());
    let waits = workers::waiting(store.conn(), &worker).unwrap();
    assert_eq!(waits[0].reason, "background command");
    assert_eq!(waits[0].deadline, "2030-01-02T00:00:00Z");
    assert_eq!(
        pinned_check(&h, &run, "2030-01-01T23:59:59Z")["verdict"],
        "progress"
    );
    assert_eq!(
        pinned_check(&h, &run, "2030-01-02T00:00:01Z")["verdict"],
        "no_progress"
    );
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
    let failed = h
        .horch(&["tell", "worker-1", "answer"])
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
        .horch(&["tell", "worker-1", "answer"])
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
    use horch_core::execution::{legacy::Record, records::Ledger};
    use horch_e2e::harness::wait_for;
    let mut h = world("g60-plain-done");
    h.set("HORCH_TEST_MAIL_REDELIVERY_MS", "50");
    h.set("HORCH_TEST_MAIL_NO_ORCHESTRATOR_MS", "100");
    fake(&h, &["workspace", "create", "--label", "plain-done"]);
    fake(&h, &["pane", "split", "w1:p1", "--direction", "right"]);
    Ledger::for_project(&h.state, &h.project.to_string_lossy())
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
    wait_for("both reports redelivered after registration", || {
        let calls = h.calls_of("herdr");
        let typed: Vec<_> = calls
            .iter()
            .filter(|c| {
                c["argv"][0] == "agent"
                    && c["argv"][3]
                        .as_str()
                        .is_some_and(|s| s.contains("redelivery"))
            })
            .collect();
        (typed.iter().any(|c| {
            c["argv"][3]
                .as_str()
                .unwrap()
                .contains("NOTE: before registration")
        }) && typed.iter().any(|c| {
            c["argv"][3]
                .as_str()
                .unwrap()
                .contains("DONE: plain work complete")
        }))
        .then_some(())
    });
    assert!(h.run(&["inbox"]).status.success());
    assert!(mail.pending().unwrap().is_empty());
}

#[test]
fn g62_ledger_wake_uses_run_workspace_and_records_login_queue_cause() {
    let (mut h, store, run, worker, _agent) = pinned_worker("g62-wake-login");
    h.unset("HORCH_NO_WAKE");
    h.set("HORCH_WORKSPACE_ID", "conflicting-workspace");
    fake(&h, &["workspace", "create", "--label", "login-wake"]);
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
    let findings = h.root.join("findings.md");
    std::fs::write(
        &findings,
        "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n",
    )
    .unwrap();
    store
        .conn()
        .execute(
            "UPDATE tasks SET kind='review',spec_ref=?2 WHERE current_worker_id=?1",
            [&worker, &findings.to_string_lossy().into_owned()],
        )
        .unwrap();
    let done = h
        .horch(&["ledger", "done", "review complete"])
        .env("HORCH_WORKER_ID", &worker)
        .output()
        .unwrap();
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    let pending = Mail::under(&h.state, "w1").pending().unwrap();
    assert!(pending
        .iter()
        .any(|m| m.from == "horch" && m.text.contains("All tasks are done")));
    assert!(Mail::under(&h.state, "conflicting-workspace")
        .pending()
        .unwrap()
        .is_empty());
    let note:String=store.conn().query_row("SELECT text FROM events WHERE run_id=?1 AND kind='woke' ORDER BY event_id DESC LIMIT 1",[&run.run_id],|r|r.get(0)).unwrap();
    assert!(
        note.contains("queued, not yet acknowledged") && note.contains("needs /login"),
        "{note}"
    );
    let calls = h.calls_of("herdr");
    let notifications: Vec<_> = calls
        .iter()
        .filter(|c| c["argv"][0] == "notification")
        .collect();
    assert_eq!(notifications.len(), 1);
    let args = notifications[0]["argv"].as_array().unwrap();
    assert!(args[2].as_str().unwrap().contains("/login"));
    let body = args.windows(2).find(|a| a[0] == "--body").unwrap()[1]
        .as_str()
        .unwrap();
    assert!(body.contains("orchestrator") && body.contains("/login"));
    assert_eq!(prompt_count(&h), 0);
    assert!(h
        .horch(&["inbox"])
        .env("HORCH_WORKSPACE_ID", "w1")
        .output()
        .unwrap()
        .status
        .success());
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
    // The budget is 20 s; without it the readiness wait alone is 120 s.
    assert!(took < Duration::from_secs(60), "the pass took {took:?}");
    let pending = Mail::under(&h.state, "w1").pending().unwrap();
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert!(
        pending[0].text.contains("worker-1 on") && pending[0].text.contains("shows no progress"),
        "{pending:?}"
    );
    assert!(h.run(&["inbox"]).status.success());
}
