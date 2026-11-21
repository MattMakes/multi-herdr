#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::taskledger::Store;
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use rusqlite::OptionalExtension;
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn fleet(name: &str, scenario: &str) -> Harness {
    fleet_with(name, scenario, None)
}

fn slow_fleet(name: &str, scenario: &str, first: &str) -> Harness {
    fleet_with(name, scenario, Some(first))
}

fn fleet_with(name: &str, scenario: &str, first: Option<&str>) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_FAKE_SCENARIO", scenario);
    let root = h.fleet_root();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("fleet.toml"),
        "health_max_load_per_cpu = 1000000.0\npass_period_s = 1\n",
    )
    .unwrap();
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
    std::os::unix::fs::symlink(Harness::horch_bin(), h.bin.join("horch")).unwrap();
    if let Some(first) = first {
        let fake = Harness::horch_bin().with_file_name("fake-claude");
        let script = format!(
            "#!/bin/sh\n[ \"$1\" = warm ] && exit 0\n\
             case \"$*\" in\n*/inbox/START.md*) /bin/sleep 5; {first} ;;\nesac\n\
             exec '{}' \"$@\"\n",
            fake.display()
        );
        let path = h.write_warmed("claude-slow", script.as_bytes());
        h.set("HORCH_CLAUDE_BIN", path.to_string_lossy());
    }
    let workspace = h.fleet_workspace();
    assert_eq!(workspace, "w1");
    h.set("HORCH_WORKSPACE_ID", workspace);
    h
}

fn queue_one(h: &Harness, slug: &str) {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = dir.join("t0.md");
    std::fs::write(&ticket, "# T0\n").unwrap();
    let run = seed_run(h, slug, Some(1), "w1:p1");
    seed_task(h, &run, &ticket, "opus");
    h.serve_fleetd();
}

fn pass() {
    std::thread::sleep(std::time::Duration::from_secs(1));
}

fn json_of(h: &Harness, args: &[&str]) -> Value {
    let o = h.run(args);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

fn show(h: &Harness) -> Value {
    json_of(h, &["ledger", "show", "T-1", "--json"])
}

fn task_status(h: &Harness) -> String {
    show(h)["task"]["status"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn fake_key(h: &Harness, key: &str) -> Option<Value> {
    let tasks = h.fleet_root().join("tasks");
    let tasks = tasks.to_string_lossy();
    h.calls_of("claude")
        .into_iter()
        .rev()
        .filter(|c| c["argv"].to_string().contains(tasks.as_ref()))
        .find_map(|c| c.get(key).cloned())
}

fn context(h: &Harness) -> String {
    format!(
        "attempt: {:?}\nshow: {}\nclaude: {:?}\ningested: {:?}\nheartbeats: {:?}",
        first_attempt(h),
        show(h),
        h.calls_of("claude"),
        ingested(h),
        files_named(&h.fleet_root().join("tasks"), "heartbeat.json")
            .iter()
            .map(|p| std::fs::read_to_string(p).unwrap_or_default())
            .collect::<Vec<_>>()
    )
}

fn ingested(h: &Harness) -> Vec<(String, String, Option<String>)> {
    let Ok(store) = Store::open_read_only(&h.ledger_db()) else {
        return Vec::new();
    };
    let conn = store.conn();
    let Ok(mut stmt) = conn.prepare("SELECT path, outcome, reason FROM ingested") else {
        return Vec::new();
    };
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
}

fn until<T>(h: &Harness, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    wait_for_or(
        what,
        || {
            pass();
            f()
        },
        || context(h),
    )
}

fn assert_ok(what: &str, out: &Value) {
    assert!(out["exit"] == 0, "{what} of the agent failed: {out}");
}

fn files_named(dir: &Path, name: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else if p.file_name().is_some_and(|n| n == name) {
                found.push(p);
            }
        }
    }
    found
}

#[derive(Debug)]
struct Attempt {
    state: String,
    outcome: Option<String>,
    pid: Option<i64>,
    pid_start: Option<i64>,
    worker: String,
    agent_pid: Option<i64>,
}

fn first_attempt(h: &Harness) -> Option<Attempt> {
    let store = Store::open_read_only(&h.ledger_db()).ok()?;
    store
        .conn()
        .query_row(
            "SELECT a.state, a.outcome, a.pid, a.pid_start, w.status, w.agent_pid
             FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
             WHERE a.task_id IS NOT NULL ORDER BY a.rowid LIMIT 1",
            [],
            |r| {
                Ok(Attempt {
                    state: r.get(0)?,
                    outcome: r.get(1)?,
                    pid: r.get(2)?,
                    pid_start: r.get(3)?,
                    worker: r.get(4)?,
                    agent_pid: r.get(5)?,
                })
            },
        )
        .optional()
        .ok()
        .flatten()
}

fn live_with_wrapper(h: &Harness) -> Attempt {
    until(h, "the attempt is not live with its wrapper", || {
        first_attempt(h).filter(|a| a.state == "live" && a.pid.is_some() && a.pid_start.is_some())
    })
}

fn ended(h: &Harness) -> Attempt {
    until(h, "the attempt did not end", || {
        first_attempt(h).filter(|a| a.state == "ended")
    })
}

fn assert_exit_path(h: &Harness, a: &Attempt) {
    let wrapper = a.pid.is_some() && a.pid_start.is_some();
    assert!(wrapper, "no wrapper: {a:?}\n{}", context(h));
    assert_eq!(a.outcome.as_deref(), Some("no_result"), "{a:?}");
    assert_ne!(a.worker, "launch_failed", "{a:?}");
}

#[test]
fn push_attempt_agent_sends_its_result() {
    let h = fleet("p5-push-result", "exec,commit,result");
    queue_one(&h, "push-result");
    let result = until(&h, "the agent ran no horch result", || {
        fake_key(&h, "result")
    });
    assert_ok("horch result", &result);
    let tasks = h.fleet_root().join("tasks");
    assert!(
        !files_named(&tasks, "result.json").is_empty(),
        "no result.json under {}",
        tasks.display()
    );
    let a = until(&h, "the verifier did not end the attempt", || {
        first_attempt(&h).filter(|a| a.state == "ended")
    });
    assert!(
        matches!(a.outcome.as_deref(), Some("verified" | "rejected")),
        "{a:?}"
    );
    assert!(a.pid.is_some(), "no wrapper: {a:?}");
    assert_ne!(a.worker, "launch_failed", "{a:?}");
}

#[test]
fn push_attempt_agent_asks_a_question() {
    let h = slow_fleet(
        "p5-push-ask",
        "exec,exit_without_result",
        "horch ask Which port?",
    );
    queue_one(&h, "push-ask");
    let tasks = h.fleet_root().join("tasks");
    until(&h, "the task is not blocked_on_user", || {
        (task_status(&h) == "blocked_on_user").then_some(())
    });
    let a = ended(&h);
    assert_eq!(a.outcome.as_deref(), Some("asked"), "{a:?}");
    let questions: Vec<PathBuf> = files_named(&tasks, "1.json")
        .into_iter()
        .filter(|p| p.parent().is_some_and(|d| d.ends_with("outbox/questions")))
        .collect();
    assert_eq!(
        questions.len(),
        1,
        "no question file under {}",
        tasks.display()
    );
}

#[test]
fn push_attempt_agent_that_exits_with_no_result_ends_by_its_exit() {
    let h = slow_fleet("p5-push-exit", "exec,exit_without_result", ":");
    queue_one(&h, "push-exit");
    let a = ended(&h);
    assert_exit_path(&h, &a);
    assert_eq!(a.worker, "incomplete", "{a:?}");
    until(&h, "the task does not wait for its next attempt", || {
        matches!(task_status(&h).as_str(), "ready" | "queued").then_some(())
    });
}

#[test]
fn push_attempt_agent_killed_while_it_works_ends_by_its_exit() {
    let h = slow_fleet("p5-push-kill", "exec", "exec /bin/sleep 600");
    queue_one(&h, "push-kill");
    live_with_wrapper(&h);
    let agent = until(&h, "the agent PID of the live attempt", || {
        first_attempt(&h)
            .filter(|a| a.state == "live")
            .and_then(|a| a.agent_pid)
    });
    let o = std::process::Command::new("kill")
        .args(["-KILL", &agent.to_string()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    let a = ended(&h);
    assert_exit_path(&h, &a);
    assert_eq!(a.worker, "incomplete", "{a:?}");
}
