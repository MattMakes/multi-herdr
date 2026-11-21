#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use horch_core::execution::legacy::Record;
use horch_core::execution::records::SessionLedger;
use horch_e2e::harness::{wait_for, Harness};
use serde_json::{json, Value};

const WORKSPACE: &str = "w1";
const ORCHESTRATOR: &str = "w1:p1";
const WORKER: &str = "w2:p1";
const ROLE: &str = "opus-1";
const RECORD: &str = "r-done";
const CLOSING: &str = "closing: reporting to the orchestrator";
const BUDGET: Duration = Duration::from_secs(2);
const BOUNDED: Duration = Duration::from_secs(10);

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn herdr(h: &Harness, args: &[&str]) -> Output {
    let mut cmd = Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    cmd.args(args).output().unwrap()
}

fn stick_orchestrator(h: &Harness) {
    let mut path = h.log.clone().into_os_string();
    path.push(".state.json");
    let path = PathBuf::from(path);
    let mut state: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for ws in state["workspaces"].as_array_mut().unwrap() {
        for pane in ws["panes"].as_array_mut().unwrap() {
            if pane["pane_id"] == ORCHESTRATOR {
                pane["agent"] = json!("claude");
                pane["agent_status"] = json!("unknown");
            }
        }
    }
    std::fs::write(&path, state.to_string()).unwrap();
}

fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_WORKSPACE_ID", WORKSPACE);
    for label in ["orchestrator", "worker"] {
        let o = herdr(&h, &["workspace", "create", "--label", label]);
        assert!(o.status.success(), "{}", text(&o));
    }
    let mailbox = h.tmp.join(format!("herdr-orchestration-{WORKSPACE}"));
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), ORCHESTRATOR).unwrap();
    std::fs::write(mailbox.join(format!("{ROLE}.id")), WORKER).unwrap();
    stick_orchestrator(&h);
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .insert(Record {
            record_id: RECORD.into(),
            agent: "claude".into(),
            tier: "opus".into(),
            model: "opus".into(),
            role: ROLE.into(),
            task: "work".into(),
            ..Record::default()
        })
        .unwrap();
    h
}

fn as_worker(h: &Harness, args: &[&str]) -> Command {
    let mut cmd = h.horch(args);
    cmd.env("HERDR_PANE_ID", WORKER)
        .env("HORCH_ROLE", ROLE)
        .env("HORCH_RECORD_ID", RECORD);
    cmd
}

fn short_budget(h: &mut Harness) {
    h.set(
        "HORCH_FAULT",
        format!("done-push-budget-ms:{}", BUDGET.as_millis()),
    );
}

fn assert_bounded(what: &str, took: Duration, o: &Output) {
    assert!(
        took >= BUDGET,
        "{what} took {took:?}, less than the {BUDGET:?} budget: {}",
        text(o)
    );
    assert!(took < BOUNDED, "{what} took {took:?}: {}", text(o));
}

fn record(h: &Harness) -> Value {
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .get(RECORD)
        .map(|r| serde_json::to_value(r).unwrap())
        .unwrap()
}

fn done_entries(h: &Harness) -> usize {
    record(h)["history"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["event"] == "done")
        .count()
}

fn pending(h: &Harness) -> Vec<String> {
    let dir = h.state.join(format!("mail/{WORKSPACE}/pending"));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| match std::fs::read_to_string(e.unwrap().path()) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("reading pending mail: {error}"),
        })
        .collect()
}

fn worker_pane_closed(h: &Harness) -> bool {
    !herdr(h, &["pane", "get", WORKER]).status.success()
}

#[test]
fn done_finishes_in_time_against_a_stuck_orchestrator() {
    let mut h = world("done-stuck");
    short_budget(&mut h);
    let started = Instant::now();
    let o = as_worker(&h, &["done", "finished the work"])
        .output()
        .unwrap();
    let took = started.elapsed();
    assert!(o.status.success(), "{}", text(&o));
    assert_bounded("done", took, &o);
    assert_eq!(
        String::from_utf8_lossy(&o.stderr).lines().next(),
        Some(CLOSING),
        "{}",
        text(&o)
    );
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("queued, not yet acknowledged")
            && String::from_utf8_lossy(&o.stderr)
                .contains("redelivered until horch inbox reads it"),
        "{}",
        text(&o)
    );
    let mail = pending(&h);
    assert_eq!(mail.len(), 1, "{mail:?}");
    assert!(
        mail[0].contains("[opus-1] DONE: finished the work"),
        "{mail:?}"
    );
    assert_eq!(record(&h)["status"], "done", "{}", record(&h));
    assert!(worker_pane_closed(&h), "the worker pane closes");
}

fn ok(cmd: &mut Command) -> String {
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn finished_task(h: &Harness) -> String {
    ok(&mut h.horch(&[
        "ledger",
        "run",
        "new",
        "alpha",
        "--steps",
        "check,land,cleanup",
    ]));
    let ticket = h.project.join("01-a.md");
    std::fs::write(&ticket, "# 01: Feature\n").unwrap();
    let file = ticket.to_string_lossy();
    ok(&mut h.horch(&[
        "ledger",
        "add",
        "--file",
        &file,
        "--primary",
        "opus",
        "--check",
        "true",
    ]));
    let worker = ok(&mut h.horch(&["ledger", "worker", "new", "--persona", "opus"]));
    let worker = worker.trim();
    let mut claim = as_worker(h, &["ledger", "claim"]);
    ok(claim.env("HORCH_WORKER_ID", worker));
    let mut wt = as_worker(h, &["ledger", "worktree", "new", "feature"]);
    let wt = ok(wt.env("HORCH_WORKER_ID", worker));
    let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
    std::fs::write(path.join("feature.txt"), "work\n").unwrap();
    for args in [
        &["add", "feature.txt"][..],
        &["commit", "--quiet", "--no-gpg-sign", "-m", "feature"],
    ] {
        ok(h.git_cmd(args).current_dir(&path));
    }
    let mut finish = as_worker(h, &["ledger", "finish"]);
    ok(finish.env("HORCH_WORKER_ID", worker));
    worker.to_string()
}

#[test]
fn ledger_done_finishes_in_time_against_a_stuck_orchestrator() {
    let mut h = world("ldone-stuck");
    let worker = finished_task(&h);
    short_budget(&mut h);

    let started = Instant::now();
    let mut done = as_worker(&h, &["ledger", "done", "finished the task"]);
    let o = done.env("HORCH_WORKER_ID", &worker).output().unwrap();
    let took = started.elapsed();
    assert!(o.status.success(), "{}", text(&o));
    assert_bounded("ledger done", took, &o);
    assert_eq!(
        String::from_utf8_lossy(&o.stderr).lines().next(),
        Some(CLOSING),
        "{}",
        text(&o)
    );
    let mail = pending(&h);
    assert_eq!(mail.len(), 2, "{mail:?}");
    assert!(
        mail.iter()
            .any(|m| m.contains("[horch] NOTE: All tasks are done.")),
        "{mail:?}"
    );
    assert!(
        mail.iter()
            .any(|m| m.contains("[opus-1] DONE: finished the task")),
        "{mail:?}"
    );
    assert_eq!(record(&h)["status"], "done", "{}", record(&h));
    assert!(worker_pane_closed(&h), "the worker pane closes");
}

#[test]
fn ledger_done_killed_during_the_wake_push_has_recorded_the_report() {
    let h = world("ldone-kill");
    let worker = finished_task(&h);
    let mut first = as_worker(&h, &["ledger", "done", "finished the task"]);
    let mut first = first
        .env("HORCH_WORKER_ID", &worker)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for("the wake to be queued", || {
        pending(&h)
            .iter()
            .any(|m| m.contains("[horch] NOTE: All tasks are done."))
            .then_some(())
    });
    assert!(
        first.try_wait().unwrap().is_none(),
        "ledger done is pushing"
    );
    first.kill().unwrap();
    first.wait().unwrap();
    assert_eq!(record(&h)["status"], "done", "{}", record(&h));
    let mail = pending(&h);
    assert!(
        mail.iter()
            .any(|m| m.contains("[opus-1] DONE: finished the task")),
        "{mail:?}"
    );
    assert!(
        !worker_pane_closed(&h),
        "the killed done left its pane open"
    );

    let mut retry = as_worker(&h, &["ledger", "done", "finished the task"]);
    ok(retry.env("HORCH_WORKER_ID", &worker));
    assert_eq!(done_entries(&h), 1, "{}", record(&h));
    assert_eq!(pending(&h).len(), 2, "{:?}", pending(&h));
    assert!(worker_pane_closed(&h), "the retry closes the pane");
}

#[test]
fn done_retried_after_a_kill_during_the_push_records_one_done() {
    let h = world("done-kill");
    let mut first = as_worker(&h, &["done", "finished the work"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for("the record to be done", || {
        (done_entries(&h) == 1).then_some(())
    });
    assert!(first.try_wait().unwrap().is_none(), "done is still pushing");
    first.kill().unwrap();
    first.wait().unwrap();
    assert!(
        !worker_pane_closed(&h),
        "the killed done left its pane open"
    );
    let lock_path = h.state.join(format!("mail/{WORKSPACE}/courier.lock"));
    wait_for("a courier for the pending report", || {
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&lock_path)
            .ok()?;
        if lock.try_lock().is_err() {
            return Some(());
        }
        let _ = lock.unlock();
        None
    });

    let o = as_worker(&h, &["done", "finished the work"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("an earlier horch done recorded this summary"),
        "{}",
        text(&o)
    );
    assert_eq!(done_entries(&h), 1, "{}", record(&h));
    assert_eq!(pending(&h).len(), 1, "{:?}", pending(&h));
    assert!(worker_pane_closed(&h), "the retry closes the pane");
}

#[test]
fn done_returns_in_time_when_herdr_hangs_on_pane_get() {
    let mut h = world("done-hung-get");
    h.unset("HORCH_WORKSPACE_ID");
    short_budget(&mut h);
    let fake = std::fs::canonicalize(h.bin.join("herdr")).unwrap();
    let flag = h.root.join("hang-pane-get");
    std::fs::write(&flag, "").unwrap();
    let stub = format!(
        "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\nif [ \"$1\" = pane ] && [ \"$2\" = get ] && [ -f '{}' ]; then exec /bin/sleep 60; fi\nexec '{}' \"$@\"\n",
        flag.display(),
        fake.display()
    );
    h.write_warmed("herdr", stub.as_bytes());
    let started = Instant::now();
    let o = as_worker(&h, &["done", "finished the work"])
        .output()
        .unwrap();
    let took = started.elapsed();
    std::fs::remove_file(&flag).unwrap();
    assert_bounded("done", took, &o);
}
