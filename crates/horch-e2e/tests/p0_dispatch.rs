#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Thresholds};
use horch_core::taskledger::schema;
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;
use horch_e2e::harness::{fixtures, wait_for, Harness};
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
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
    h
}

fn pass(h: &Harness) -> Output {
    std::thread::sleep(std::time::Duration::from_secs(1));
    h.run(&["ledger", "status", "--json"])
}

fn task_yaml_of(h: &Harness, attempt: &str) -> Option<(PathBuf, String)> {
    let want = format!("attempt: {attempt}");
    files_named(&h.fleet_root().join("tasks"), "task.yaml")
        .into_iter()
        .map(|f| {
            let y = std::fs::read_to_string(&f).unwrap_or_default();
            (f, y)
        })
        .find(|(_, y)| y.lines().any(|l| l.trim() == want))
}

fn yaml_key(y: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    y.lines()
        .find_map(|l| l.trim().strip_prefix(&prefix))
        .map(|v| v.trim().trim_matches('"').to_string())
}

fn fleet(name: &str, scenario: &str) -> Harness {
    let mut h = world(name);
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_FAKE_SCENARIO", scenario);
    let workspace = h.fleet_workspace();
    assert_eq!(workspace, "w1");
    h.set("HORCH_WORKSPACE_ID", workspace);
    h
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

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    out(&o).trim().to_string()
}

fn queue_one(h: &Harness, slug: &str, thresholds: Thresholds, persona: &str) {
    let p = ticket(h, "t0", "# T0\n");
    let base = git_in(h, &h.project, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let branch = format!("fleet/{slug}");
    git_in(h, &h.project, &["branch", &branch]);
    let store = Store::open(&h.root.join("ledger.sqlite3")).unwrap();
    let conn = store.conn();
    let run = runs::create(
        conn,
        &h.project,
        &NewRun {
            slug: slug.into(),
            workspace_id: Some("w1".into()),
            base_branch: Some(base),
            run_branch: Some(branch),
            thresholds,
            ..Default::default()
        },
    )
    .unwrap();
    let now = horch_core::clock::now_stamp();
    schema::run_idea(conn, &run.run_id, &now, true)
        .unwrap()
        .expect("the idea of the run");
    let orchestrator = workers::insert(
        conn,
        &NewWorker {
            run_id: run.run_id.clone(),
            kind: Some(WorkerKind::Orchestrator),
            persona: "orchestrator".into(),
            workspace_id: Some("w1".into()),
            pane_id: Some("w1:p1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    runs::set_orchestrator(conn, &run.run_id, &orchestrator).unwrap();
    let new = NewTask::work("t0", &p.to_string_lossy(), persona);
    tasks::add(conn, &run, &new, Actor::Orchestrator).unwrap();
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

fn the_worker(st: &Value) -> (String, String) {
    let w = &st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["worker"]["kind"] == "worker")
        .unwrap_or_else(|| panic!("no worker: {st}"))["worker"];
    let field = |k: &str| w[k].as_str().unwrap_or_default().to_string();
    (field("worker_id"), field("role"))
}

#[test]
fn fm2_agent_that_never_claims_has_its_task_bound_at_spawn() {
    horch_e2e::expected::check(
        file!(),
        "fm2_agent_that_never_claims_has_its_task_bound_at_spawn",
        || {
            let h = fleet("p0-fm2", "exec,bound");
            queue_one(&h, "alpha", Thresholds::default(), "opus");
            h.serve_fleetd();
            let tasks = h.fleet_root().join("tasks");
            let path = wait_for(
                &format!(
                    "the agent is not bound at entry: no entry.json under {}",
                    tasks.display()
                ),
                || {
                    pass(&h);
                    files_named(&tasks, "entry.json").into_iter().next()
                },
            );
            let entry: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert!(
                entry["bound"] == true,
                "the agent is not bound at entry: {entry}"
            );
            assert!(
                entry["task_yaml_exists"] == true,
                "the task.yaml of the agent is missing at entry: {entry}"
            );
            let o = h.run(&["ledger", "status", "--json"]);
            let st: Value =
                serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
            let (id, label) = the_worker(&st);
            assert_eq!(entry["worker_id"], id.as_str(), "{entry}");
            let conn = rusqlite::Connection::open_with_flags(
                h.ledger_db(),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let bound: Vec<(String, String)> = conn
                .prepare(
                    "SELECT w.role, a.via FROM attempts a
                     JOIN tasks t ON t.task_id = a.task_id
                     JOIN workers w ON w.worker_id = a.worker_id
                     WHERE t.seq = 1 AND a.worker_id = ?1",
                )
                .unwrap()
                .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(
                bound,
                [(label.clone(), "push".to_string())],
                "the task is not bound to worker {label}: {}",
                text(&o)
            );
        },
    );
}

#[test]
fn fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt() {
    horch_e2e::expected::check(
        file!(),
        "fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt",
        || {
            let h = fleet("p0-fm6", "exec,commit,exit_without_result");
            queue_one(
                &h,
                "beta",
                Thresholds {
                    check_every_s: Some(1),
                    stall_rounds: Some(1),
                    ..Default::default()
                },
                "opus",
            );
            h.serve_fleetd();
            let (_, a1) = wait_for("no task.yaml of A1", || {
                pass(&h);
                task_yaml_of(&h, "A1")
            });
            let path = PathBuf::from(yaml_key(&a1, "worktree").expect("the worktree of A1"));
            let commit = wait_for(&format!("no commit of A1 in {}", path.display()), || {
                pass(&h);
                let mut c = h.git_cmd(&["log", "-1", "--format=%H %s"]);
                c.current_dir(&path);
                let line = c.output().ok().map(|o| out(&o))?;
                let (head, subject) = line.trim().split_once(' ')?;
                subject.starts_with("fake ").then(|| head.to_string())
            });

            wait_for("the worker of A1 is still live", || {
                pass(&h);
                let st = json_of(&h, &["ledger", "status", "--json"]);
                let live = ["spawning", "starting", "claiming", "working"];
                let w = &st["workers"]
                    .as_array()?
                    .iter()
                    .find(|w| w["worker"]["kind"] == "worker")?["worker"];
                (!live.iter().any(|s| w["status"] == *s)).then_some(())
            });
            h.advance_fleet_clock(121);
            wait_for("the task of A1 is not queued again", || {
                pass(&h);
                let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
                (t["task"]["status"] == "queued").then_some(())
            });
            h.advance_fleet_clock(30);

            let (_, a2) = wait_for("no task.yaml of A2", || {
                pass(&h);
                task_yaml_of(&h, "A2")
            });
            assert_eq!(
                yaml_key(&a2, "worktree").as_deref(),
                Some(path.to_string_lossy().as_ref()),
                "the task.yaml of A2 names another worktree: {a2}"
            );
            git_in(&h, &path, &["merge-base", "--is-ancestor", &commit, "HEAD"]);
        },
    );
}
