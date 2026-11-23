#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::Kind;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as integration, BranchFact, NewBranch, NewFeature,
};
use horch_core::fleetd::integration::application::remote;
use horch_core::fleetd::worker_profiles::domain::launch_env;
use horch_core::harness::launch::write_agent_hooks;
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, Store};
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use serde_json::Value;

const PERSONA: &str = "opus";

const FLEET_TOML: &str = "pass_period_s = 1\n";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

struct World {
    h: Harness,
}

impl World {
    fn fleet_root(&self) -> PathBuf {
        self.h.root.join("fleet")
    }

    fn task_files(&self, name: &str) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut todo = vec![self.fleet_root().join("tasks")];
        while let Some(dir) = todo.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    todo.push(path);
                } else if path.file_name().is_some_and(|n| n == name) {
                    found.push(path);
                }
            }
        }
        found
    }

    fn diagnostics(&self) -> String {
        let mut calls: Vec<String> = self
            .h
            .calls()
            .iter()
            .map(|c| format!("{} {}", c["fake"], c["argv"]))
            .collect();
        calls.dedup();
        calls.truncate(40);
        let read = |p: PathBuf| std::fs::read_to_string(p).unwrap_or_default();
        format!(
            "calls:\n{}\nfleetd.stderr:\n{}\nlaunch.log:\n{}\nledger:\n{:?}",
            calls.join("\n"),
            read(self.h.root.join("fleetd.stderr")),
            read(self.fleet_root().join("run/launch.log")),
            self.rows("SELECT id || ' ' || via || ' ' || state FROM attempts UNION ALL SELECT 'task ' || status FROM tasks"),
        )
    }

    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = rusqlite::Connection::open(self.h.ledger_db()).unwrap();
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }
}

fn world(name: &str, ticket: &str, env: &[(&str, &str)]) -> World {
    let mut h = Harness::new(name).with_git();
    let fleet = h.root.join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("FLEET_ROOT", fleet.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_FAKE_SCENARIO", "exec,stay");
    for (key, value) in env {
        h.set(key, *value);
    }
    declare_gates(&h);
    seed_task(&h, ticket);
    let workspace = h.fleet_workspace();
    assert_eq!(workspace, "w1");
    h.set("HORCH_WORKSPACE_ID", workspace);
    h.serve_fleetd();
    World { h }
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

fn seed_task(h: &Harness, ticket: &str) {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t0.md"), ticket).unwrap();
    let store = Store::open(&h.ledger_db()).unwrap();
    let conn = store.conn();
    let run = runs::create(
        conn,
        &h.project,
        &NewRun {
            slug: "r1".into(),
            base_branch: Some("main".into()),
            workspace_id: Some("w1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    schema::run_idea(conn, &run.run_id, &horch_core::clock::now_stamp(), true)
        .unwrap()
        .expect("the idea of r1");
    let seat = workers::insert(
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
    runs::set_orchestrator(conn, &run.run_id, &seat).unwrap();
    let mut new = NewTask::work("t0", "tickets/t0.md", PERSONA);
    new.ticket_snapshot = Some(ticket.to_string());
    tasks::add(conn, &run, &new, Actor::Orchestrator).unwrap();
}

fn attempt_call(w: &World) -> Value {
    wait_for_or(
        "the fake agent of the attempt to run",
        || {
            w.h.calls_of("claude").into_iter().find(|c| {
                c["exec_prompt"]
                    .as_str()
                    .is_some_and(|p| p.contains("Do the work of T0."))
            })
        },
        || w.diagnostics(),
    )
}

#[test]
fn fr5_worker_never_runs_a_task_search() {
    let w = world("p4-fr5", "# T0\nDo the work of T0.\n", &[]);
    let call = attempt_call(&w);
    let start = call["exec_prompt"].as_str().unwrap();
    assert!(start.contains("Do the work of T0."), "{start}");
    assert_eq!(w.task_files("task.yaml").len(), 1);
    assert_eq!(w.task_files("START.md").len(), 1);
    let via = w.rows("SELECT via FROM attempts WHERE task_id IS NOT NULL");
    assert!(!via.is_empty(), "no attempt of the task");
    assert!(via.iter().all(|v| v == "push"), "{via:?}");
    let status = w.rows("SELECT status FROM tasks");
    assert_eq!(status.len(), 1, "{status:?}");
    assert_ne!(status[0], "queued", "the task did not run");
    for call in w.h.calls() {
        let text = call.to_string();
        assert!(!text.contains("ledger claim"), "{text}");
    }
}

const SECRET: &str = "ghp_e2eSecretValue0123456789";

fn files_holding(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else if std::fs::read(&path)
                .is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes()))
            {
                found.push(path);
            }
        }
    }
    found
}

#[test]
fn sr2_profile_secret_never_reaches_task_yaml_or_start_md() {
    let clean = world(
        "p4-sr2-clean",
        "# T0\nDo the work of T0.\n",
        &[("GH_TOKEN", SECRET)],
    );
    attempt_call(&clean);
    assert_eq!(clean.task_files("task.yaml").len(), 1);
    assert_eq!(clean.task_files("START.md").len(), 1);
    let holding = files_holding(&clean.fleet_root().join("tasks"), SECRET);
    assert!(holding.is_empty(), "the secret is in {holding:?}");

    let ticket = format!("# T0\nSet DEPLOY_TOKEN={SECRET} first.\n");
    let dirty = world("p4-sr2-dirty", &ticket, &[("GH_TOKEN", SECRET)]);
    let stderr = dirty.h.root.join("fleetd.stderr");
    let refusal = wait_for_or(
        "fleetd to refuse the task files",
        || {
            let text = std::fs::read_to_string(&stderr).unwrap_or_default();
            text.lines()
                .find(|l| l.contains("SR-2"))
                .map(str::to_string)
        },
        || dirty.diagnostics(),
    );
    assert!(refusal.contains("*_TOKEN="), "{refusal}");
    assert!(!refusal.contains(SECRET), "{refusal}");
    assert!(dirty.task_files("task.yaml").is_empty());
    assert!(dirty.task_files("START.md").is_empty());
    assert!(
        dirty
            .h
            .calls_of("claude")
            .iter()
            .all(|c| c.get("exec_prompt").is_none()),
        "an agent started on a refused attempt"
    );
    let holding = files_holding(&dirty.fleet_root(), SECRET);
    assert!(holding.is_empty(), "the secret is in {holding:?}");
}

const FEATURE: &str = "F-0001";
const FEATURE_BRANCH: &str = "fleet/feature-1";
const REMOTE: &str = "origin";

fn git(h: &Harness, dir: &Path, args: &[&str], extra_env: &[(String, String)]) -> Output {
    let mut cmd = h.git_cmd(args);
    cmd.current_dir(dir);
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    cmd.output().unwrap()
}

fn git_ok(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let o = git(h, dir, args, &[]);
    assert!(o.status.success(), "git {args:?}: {}", text(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn remote_tip(h: &Harness, remote: &Path, branch: &str) -> Option<String> {
    let o = git(
        h,
        remote,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
        &[],
    );
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn agent_git_env(fleet_root: &Path) -> Vec<(String, String)> {
    launch_env::git_env(fleet_root).to_vec()
}

fn add_remote(h: &Harness) -> PathBuf {
    let remote = h.root.join("remote.git");
    git_ok(
        h,
        &h.root,
        &["init", "--quiet", "--bare", &remote.to_string_lossy()],
    );
    git_ok(
        h,
        &h.project,
        &["remote", "add", REMOTE, &remote.to_string_lossy()],
    );
    git_ok(h, &h.project, &["push", "--quiet", REMOTE, "main"]);
    remote
}

#[test]
fn sr3_agent_git_push_is_refused_and_fleetd_push_works() {
    let w = world("p4-sr3-agent", "# T0\nDo the work of T0.\n", &[]);
    let call = attempt_call(&w);
    let keys: Vec<&str> = call["env_keys"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for key in ["GIT_CONFIG_COUNT", "GIT_CONFIG_KEY_0", "GIT_CONFIG_VALUE_0"] {
        assert!(keys.contains(&key), "the agent has no {key}: {keys:?}");
    }
    let hook = launch_env::hooks_dir(&w.fleet_root()).join("pre-push");
    assert!(hook.is_file(), "fleetd wrote no {}", hook.display());
    let remote = add_remote(&w.h);
    git_ok(&w.h, &w.h.project, &["branch", "agent-work"]);
    let o = git(
        &w.h,
        &w.h.project,
        &["push", REMOTE, "agent-work"],
        &agent_git_env(&w.fleet_root()),
    );
    assert!(!o.status.success(), "an agent push passed: {}", text(&o));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains(launch_env::PRE_PUSH_REFUSAL),
        "{}",
        text(&o)
    );
    assert_eq!(remote_tip(&w.h, &remote, "agent-work"), None);
    git_ok(
        &w.h,
        &w.h.project,
        &["push", "--quiet", REMOTE, "agent-work"],
    );
    assert!(remote_tip(&w.h, &remote, "agent-work").is_some());

    let h = push_world("p4-sr3-fleetd");
    let remote = h.root.join("remote.git");
    let tip = git_ok(
        &h,
        &h.project,
        &["rev-parse", &format!("refs/heads/{FEATURE_BRANCH}")],
    );
    let refused = git(
        &h,
        &h.project,
        &[
            "push",
            REMOTE,
            &format!("{FEATURE_BRANCH}:{FEATURE_BRANCH}"),
        ],
        &agent_git_env(&h.root.join("fleet")),
    );
    assert!(!refused.status.success(), "{}", text(&refused));
    assert_eq!(remote_tip(&h, &remote, FEATURE_BRANCH), None);
    h.serve_fleetd();
    wait_for_or(
        "the push job to end",
        || {
            let conn = rusqlite::Connection::open(h.ledger_db()).ok()?;
            let state: String = conn
                .query_row("SELECT state FROM jobs WHERE kind = 'push'", [], |r| {
                    r.get(0)
                })
                .ok()?;
            matches!(state.as_str(), "passed" | "failed" | "abandoned").then_some(state)
        },
        String::new,
    );
    let conn = rusqlite::Connection::open(h.ledger_db()).unwrap();
    let (state, result): (String, Option<String>) = conn
        .query_row(
            "SELECT state, result_json FROM jobs WHERE kind = 'push'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "passed", "{result:?}");
    assert_eq!(remote_tip(&h, &remote, FEATURE_BRANCH), Some(tip.clone()));
    let push_state: String = conn
        .query_row(
            "SELECT push_state FROM features WHERE id = ?1",
            [FEATURE],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(push_state, "ok");
}

#[test]
fn sr3_agent_launch_denies_external_messages() {
    let w = world("p4-sr3-messages", "# T0\nDo the work of T0.\n", &[]);
    let call = attempt_call(&w);
    let argv = call["argv"].as_array().unwrap();
    let settings = argv
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|a| serde_json::from_str::<Value>(a).ok())
        .find(|v| v.get("permissions").is_some())
        .unwrap_or_else(|| panic!("no --settings in {argv:?}"));
    let deny: Vec<&str> = settings["permissions"]["deny"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for words in launch_env::DENIED_COMMANDS {
        let rule = launch_env::claude_deny_rule(words);
        assert!(deny.contains(&rule.as_str()), "{rule} not in {deny:?}");
    }
    for rule in [
        "Bash(gh issue comment:*)",
        "Bash(gh pr comment:*)",
        "Bash(gh pr review:*)",
        "Bash(gh discussion:*)",
    ] {
        assert!(deny.contains(&rule), "{rule} not in {deny:?}");
    }
}

fn push_world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let fleet = h.root.join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("FLEET_ROOT", fleet.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    add_remote(&h);
    git_ok(
        &h,
        &h.project,
        &["checkout", "--quiet", "-b", FEATURE_BRANCH],
    );
    std::fs::write(h.project.join("feature.txt"), "feature\n").unwrap();
    git_ok(&h, &h.project, &["add", "feature.txt"]);
    git_ok(
        &h,
        &h.project,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "feature"],
    );
    git_ok(&h, &h.project, &["checkout", "--quiet", "main"]);
    let tip = git_ok(
        &h,
        &h.project,
        &["rev-parse", &format!("refs/heads/{FEATURE_BRANCH}")],
    );
    seed_push_job(&h, &tip);
    write_agent_hooks(&fleet).unwrap();
    for (key, value) in agent_git_env(&fleet) {
        h.set(&key, value);
    }
    h
}

fn seed_push_job(h: &Harness, tip: &str) {
    let mut store = Store::open(&h.ledger_db()).unwrap();
    store
        .immediate(|tx| {
            let feature = runs::create_feature_run(tx, &h.project, "main")?;
            assert_eq!(feature, FEATURE);
            let cause = |text| Cause {
                actor: Actor::Horch,
                text,
            };
            let new = NewFeature {
                id: FEATURE,
                name: Some(FEATURE),
                branch: Some(FEATURE_BRANCH),
                primary_branch: "main",
            };
            integration::insert_feature(tx, None, &new, cause("the test adds the feature"))?;
            let branch = NewBranch {
                name: FEATURE_BRANCH,
                feature_id: FEATURE,
                task_id: None,
                owner: "fleetd",
                due_at: 0,
            };
            integration::insert_branch(tx, None, &branch, cause("the feature branch"))?;
            integration::set_branch_fact(
                tx,
                None,
                FEATURE_BRANCH,
                BranchFact::SeenTip(tip),
                cause("the tip of the feature branch"),
            )?;
            tx.execute(
                "INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
                 VALUES ('I-1', ?1, 'in_progress', '2026-11-18T12:00:00Z', '2026-11-18T12:00:00Z')",
                [FEATURE],
            )?;
            let config = FleetConfig::default();
            let ctx = JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            };
            let input_key = remote::push_key(REMOTE, tip, None);
            let key = JobKey {
                kind: Kind::Push,
                subject: FEATURE,
                input_key: &input_key,
            };
            let repo = h.project.to_string_lossy();
            jobs::queue(&ctx, tx, &key, Some(&repo), FleetSeconds(0))?;
            Ok(())
        })
        .unwrap();
}
