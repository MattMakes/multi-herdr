#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;
use std::time::{Duration, Instant};

use horch_core::procid;
use horch_core::taskledger::Store;
use horch_e2e::harness::{copy_tree, fixtures, repo_root, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn push_fleet(name: &str, scenario: &str, tasks: &[&str]) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_FAKE_SCENARIO", scenario);
    h.set("HORCH_FAULT", "delivery-probe-ms:100");
    h.set("HORCH_HOSTNAME", "test-host");
    let ws = h.fleet_workspace();
    h.set("HORCH_WORKSPACE_ID", ws);
    let teammates = h.root.join("teammates");
    copy_tree(&repo_root().join("teammates"), &teammates);
    let terra = teammates.join("codex-terra.md");
    let md = std::fs::read_to_string(&terra).unwrap();
    let interactive = md.replacen("\nagent: codex\n", "\nagent: codex\nmode: interactive\n", 1);
    assert_ne!(interactive, md, "codex-terra.md names its agent");
    std::fs::write(&terra, interactive).unwrap();
    h.set("HORCH_TEAMMATES_DIR", teammates.to_string_lossy());
    let root = h.fleet_root();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("fleet.toml"),
        "pass_period_s = 1\nstop_grace_s = 2\nhealth_max_load_per_cpu = 1000000.0\n",
    )
    .unwrap();
    commit_fast_gate_so_fleetd_dispatches(&h);
    let run = seed_run(&h, "delta", Some(1), "w1:p1");
    for (n, title) in tasks.iter().enumerate() {
        let p = ticket(&h, &format!("t{n}"), &format!("# {title}\n"));
        seed_task(&h, &run, &p, "codex-terra");
    }
    h.serve_fleetd();
    h
}

fn commit_fast_gate_so_fleetd_dispatches(h: &Harness) {
    std::fs::write(
        h.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    for args in [&["add", "-A"][..], &["commit", "-q", "-m", "gates"][..]] {
        let o = h.git_cmd(args).current_dir(&h.project).output().unwrap();
        assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    }
}

fn ticket(h: &Harness, name: &str, body: &str) -> PathBuf {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.md"));
    std::fs::write(&p, body).unwrap();
    p
}

fn json_of(h: &Harness, args: &[&str]) -> Value {
    let o = h.run(args);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn wait_push<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(120);
    while Instant::now() < until {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("timed out waiting for {what}");
}

struct Launched {
    id: String,
    agent: (u32, u64),
}

impl Launched {
    fn alive(&self) -> bool {
        procid::alive(self.agent.0, Some(self.agent.1))
    }
}

fn launched(h: &Harness, n: usize) -> Vec<Launched> {
    wait_push(&format!("{n} agent PIDs"), || {
        let st = json_of(h, &["ledger", "status", "--json"]);
        let ws: Vec<Value> = st["workers"]
            .as_array()?
            .iter()
            .map(|w| w["worker"].clone())
            .filter(|w| w["kind"] == "worker")
            .collect();
        if ws.len() != n || !ws.iter().all(|w| w["agent_started"].is_i64()) {
            return None;
        }
        let out = ws.iter().map(|w| Launched {
            id: w["worker_id"].as_str().unwrap().to_string(),
            agent: (
                w["agent_pid"].as_i64().unwrap() as u32,
                w["agent_started"].as_i64().unwrap() as u64,
            ),
        });
        Some(out.collect())
    })
}

fn stops(h: &Harness, id: &str) -> Vec<Value> {
    let shown = json_of(h, &["ledger", "show", id, "--json"]);
    shown["stops"].as_array().cloned().unwrap_or_default()
}

fn attempts_of(h: &Harness, id: &str) -> Vec<(String, Option<String>)> {
    let db = Store::open(&h.ledger_db()).unwrap();
    let mut stmt = db
        .conn()
        .prepare("SELECT state, outcome FROM attempts WHERE worker_id = ?1")
        .unwrap();
    stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn a_stop_of_a_push_worker_ends_its_attempt_and_its_agent_only() {
    let h = push_fleet("ps-stop-one", "exec,stay", &["T0", "T1"]);
    let ws = launched(&h, 2);
    let o = ok(
        &h,
        &["ledger", "stop", &ws[0].id, "the operator dropped it"],
    );
    assert!(o.contains("Stopped"), "{o}");
    let ended = attempts_of(&h, &ws[0].id);
    assert!(
        ended
            .iter()
            .all(|(state, outcome)| state == "ended" && outcome.as_deref() == Some("killed")),
        "the stop ends the live attempt of the worker: {ended:?}"
    );
    wait_push("the stopped agent is gone", || {
        (!ws[0].alive()).then_some(())
    });
    assert!(ws[1].alive(), "the other agent runs on");
    let other = attempts_of(&h, &ws[1].id);
    assert!(other.iter().any(|(s, _)| s == "live"), "{other:?}");
    ok(&h, &["ledger", "stop", &ws[1].id, "test teardown"]);
}

#[test]
fn each_stop_that_fleetd_sends_is_recorded_and_a_second_stop_sends_nothing() {
    let h = push_fleet("ps-stop-rows", "exec,stay", &["T0"]);
    let w = launched(&h, 1).remove(0);
    ok(&h, &["ledger", "stop", &w.id, "the operator dropped it"]);
    wait_push("the stopped agent is gone", || (!w.alive()).then_some(()));
    let rows = wait_push("the stop of the pane", || {
        let rows = stops(&h, &w.id);
        rows.iter().any(|s| s["target"] == "pane").then_some(rows)
    });
    let agent: Vec<&Value> = rows.iter().filter(|s| s["target"] == "agent").collect();
    assert_eq!(
        agent.len(),
        1,
        "1 row for the 1 TERM of the agent: {rows:?}"
    );
    assert_eq!(agent[0]["pid"].as_i64().unwrap() as u32, w.agent.0);
    assert_eq!(agent[0]["pid_started"].as_i64().unwrap() as u64, w.agent.1);
    assert_eq!(agent[0]["signal"], "TERM");
    assert_eq!(agent[0]["result"], "sent");
    assert!(
        rows.iter()
            .any(|s| s["target"] == "pane" && s["result"] == "pane_closed"),
        "{rows:?}"
    );
    let o = json_of(&h, &["ledger", "stop", &w.id, "again", "--json"]);
    let steps = o["steps"].as_array().expect("the steps of the stop");
    assert!(steps.iter().all(|s| s["result"] != "sent"), "{o}");
    std::thread::sleep(Duration::from_secs(3));
    let after = stops(&h, &w.id);
    assert_eq!(after.len(), rows.len(), "{after:?}");
}

#[test]
fn a_stalled_push_task_stays_stalled_while_its_old_agent_lives() {
    let h = push_fleet("ps-stall", "exec,stay", &["T0"]);
    let w = launched(&h, 1).remove(0);
    let task_status = || -> String {
        let db = Store::open(&h.ledger_db()).unwrap();
        db.conn()
            .query_row(
                "SELECT t.status FROM tasks t JOIN attempts a ON a.task_id = t.task_id
                 WHERE a.worker_id = ?1",
                [&w.id],
                |r| r.get(0),
            )
            .unwrap()
    };
    let status = h.without_fleetd(|| {
        assert!(w.alive(), "the agent lives before the stall pass");
        h.advance_fleet_clock(400);
        json_of(&h, &["ledger", "watch", "--once", "--force", "--json"]);
        task_status()
    });
    let ended = attempts_of(&h, &w.id);
    assert_eq!(
        ended,
        vec![("ended".to_string(), Some("stalled".to_string()))]
    );
    assert_eq!(status, "stalled", "its old agent lived in the pass");
    wait_push("the stop of the agent", || {
        let status = task_status();
        let alive = w.alive();
        assert!(
            !(alive && status == "ready"),
            "{} is ready while its old agent lives",
            w.id
        );
        (!alive).then_some(())
    });
    wait_push("rung 6 after the agent is gone", || {
        (task_status() == "ready").then_some(())
    });
}
