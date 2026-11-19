//! The e2e tests of the safety rules of Phase 4 through the binary
//! (`docs/specs/fleetd/design.md`: SR-2, SR-3 and FR-5; ticket 58 of
//! `docs/tickets/fleetd-p4/MAP.md`).
//!
//! Each test makes its own ledger, fleet root and git project in a temp
//! folder, and runs `horch spawn` against a test `fleetd` with the fake
//! herdr and the fake agents. The tests read no real load, limit or clock:
//! the harness gives the machine through `HORCH_MACHINE_FILE`, and each
//! test waits for an event, never for a time.

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

/// The persona of each task. Its fake agent is `fake-claude`.
const PERSONA: &str = "opus";

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

// ─── the world of 1 test ────────────────────────────────────────────────

/// A fleet with 1 queued task, a test fleetd and an empty workspace `w1`
/// whose panes run their commands, like `horch fleet` leaves it.
struct World {
    h: Harness,
}

impl World {
    /// The folder of the fleet root.
    fn fleet_root(&self) -> PathBuf {
        self.h.root.join("fleet")
    }

    /// Each file named `name` under the folder of the tasks.
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

    /// What a failed wait shows: the argv of each fake call, the output of
    /// fleetd and the log of its launches.
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

    /// Rows of 1 text column of the ledger.
    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = rusqlite::Connection::open(self.h.ledger_db()).unwrap();
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }
}

/// A world whose task has the ticket text `ticket` and whose environment
/// has the extra variables `env`.
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
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", "exec,stay");
    for (key, value) in env {
        h.set(key, *value);
    }
    declare_gates(&h);
    seed_task(&h, ticket);
    h.serve_fleetd();
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    World { h }
}

/// The project declares its gates and commits the file, so no hold of a
/// missing fast gate stops the dispatch (Phase 4, Q-C).
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

/// Write the rows that the orchestrator's commands wrote before Phase 6
/// retired them: the run, its idea in flight (FR-12) and 1 queued task
/// whose ticket text is `ticket`. The test writes them before fleetd runs.
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
    // The pane that fleetd splits for the launch of an attempt.
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

/// The call of a fake `claude` that ran in exec mode, with the text of its
/// `START.md` in `exec_prompt`: the agent of a push attempt, started by
/// fleetd with no search for work.
fn attempt_call(w: &World) -> Value {
    wait_for_or(
        "the fake agent of the attempt to run",
        || {
            w.h.calls_of("claude")
                .into_iter()
                .find(|c| c.get("exec_prompt").is_some())
        },
        || w.diagnostics(),
    )
}

// ─── FR-5 ───────────────────────────────────────────────────────────────

/// FR-5: workers never search for work. fleetd binds the queued task to an
/// attempt before the pane exists, and the agent starts on the `START.md` of
/// that attempt. No attempt began with a claim, and no line told the agent
/// to search.
#[test]
fn fr5_worker_never_runs_a_task_search() {
    let w = world("p4-fr5", "# T0\nDo the work of T0.\n", &[]);
    let call = attempt_call(&w);
    let start = call["exec_prompt"].as_str().unwrap();
    assert!(start.contains("Do the work of T0."), "{start}");
    // The agent got its task from `task.yaml` and `START.md` of its attempt.
    assert_eq!(w.task_files("task.yaml").len(), 1);
    assert_eq!(w.task_files("START.md").len(), 1);
    // Each attempt began with a push: no claim, so no search.
    let via = w.rows("SELECT via FROM attempts WHERE task_id IS NOT NULL");
    assert!(!via.is_empty(), "no attempt of the task");
    assert!(via.iter().all(|v| v == "push"), "{via:?}");
    // The task left `queued` with no claim.
    let status = w.rows("SELECT status FROM tasks");
    assert_eq!(status.len(), 1, "{status:?}");
    assert_ne!(status[0], "queued", "the task did not run");
    // No line typed into a pane told an agent to search.
    for call in w.h.calls() {
        let text = call.to_string();
        assert!(!text.contains("ledger claim"), "{text}");
    }
}

// ─── SR-2 ───────────────────────────────────────────────────────────────

/// The value of the secret of the SR-2 tests.
const SECRET: &str = "ghp_e2eSecretValue0123456789";

/// Each file under `dir` that holds `needle`, with its path.
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

/// SR-2: secrets reach agents only through env vars, never through task
/// files. The environment of fleetd and of its children holds the secret
/// as `GH_TOKEN`.
/// - An attempt of a clean ticket gets its `task.yaml` and `START.md`, and
///   neither holds the secret.
/// - An attempt of a ticket that has a `DEPLOY_TOKEN=<secret>` line gets no
///   task file and no launch: the lint refuses the write, and its message
///   names the rule and never the value.
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

// ─── SR-3 ───────────────────────────────────────────────────────────────

/// The feature of the push world, its branch and its remote.
const FEATURE: &str = "F-0001";
const FEATURE_BRANCH: &str = "fleet/feature-1";
const REMOTE: &str = "origin";

/// Run git for the test in `dir`, in the sealed environment of `h`, and
/// give the output.
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

/// The tip of `branch` in the bare repository `remote`, or `None`.
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

/// The environment that the launch of an agent adds to its process
/// (`GIT_CONFIG_COUNT` and the hooks path of the agents of `fleet_root`).
fn agent_git_env(fleet_root: &Path) -> Vec<(String, String)> {
    launch_env::git_env(fleet_root).to_vec()
}

/// A bare repository next to the project, as its remote `origin`, with
/// `main` pushed to it. Gives its path.
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

/// SR-3: no agent pushes, and fleetd pushes the feature branch.
/// - fleetd launches the agent of a push attempt with the hooks path of
///   the agents in its environment, and the `pre-push` hook there refuses:
///   a `git push` with that environment fails and the remote keeps its
///   tips.
/// - A feature branch with a push job: the test fleetd runs the job with
///   that same environment in its own, and the push works: horch's own git
///   calls scrub the repository environment and set
///   `core.hooksPath=/dev/null` with `-c`.
#[test]
fn sr3_agent_git_push_is_refused_and_fleetd_push_works() {
    // The agent: its launch environment names the hooks path.
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
    // The same push without the agent environment works: only the agent is
    // refused.
    git_ok(
        &w.h,
        &w.h.project,
        &["push", "--quiet", REMOTE, "agent-work"],
    );
    assert!(remote_tip(&w.h, &remote, "agent-work").is_some());

    // fleetd: a push job runs with the hooks path of the agents in the
    // environment of fleetd itself, and still pushes.
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

/// A fleet with 1 feature whose branch is 1 commit past `main`, a remote
/// that has `main` only, and 1 queued push job of that branch. The
/// environment of fleetd and of its jobs holds the hooks path of the
/// agents, whose `pre-push` hook refuses. fleetd is not started: the test
/// starts it.
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

/// Write the rows of a feature whose branch has the tip `tip` and 1 queued
/// run of its push job to `origin`. The test writes them before fleetd runs.
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
