//! Phase 0 red tests of fleetd (`docs/specs/fleetd/design.md`): a herdr
//! server restart (FM-13) and a worker killed with dirty and untracked files
//! in its worktree (FM-25). The tests began as Phase 0 red tests on the list
//! of expected failures, `expected-failures/p0_rescue.txt`. Phases 1 and 3
//! made them green, and they now run as normal tests.
//!
//! The test plays the agent: a `sleep` process stands in for it, and the
//! test runs the worker commands with `HORCH_WORKER_ID`. A herdr restart
//! ends every agent process and closes the panes of the fake herdr.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Output};
use std::time::{Duration, Instant};

use horch_core::taskledger::runs::Run;
use horch_core::taskledger::sql_words::WORKERS_LIVE;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{tasks, watchdog, workers, Store};
use horch_e2e::expected::check;
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{fixtures, wait_for, wait_for_or, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

/// The time in which the design catches a herdr restart and a dead worker
/// (FM-13 and FM-25: 5 to 30 s), with 5 s for a slow machine.
const CAUGHT_S: u64 = 35;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A harness in workspace `w1` (root pane `w1:p1`) with its own ledger file
/// and a git repository. The watchdog is off until a test turns it on.
fn fleet(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set("HORCH_LEDGER_DB", ledger(&h).to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_WORKSPACE_ID", "w1");
    herdr(&h, &["workspace", "create", "--label", name]);
    let root = h.fleet_root();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("fleet.toml"),
        "pass_period_s = 1\nworktree_list_period_s = 5\n",
    )
    .unwrap();
    h
}

fn ledger(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

fn herdr(h: &Harness, args: &[&str]) -> String {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let o = cmd.args(args).output().unwrap();
    assert!(o.status.success(), "herdr {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
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
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Kills the stand-in agents and the fleetds of the test when dropped, so a
/// failed assertion leaks none.
struct Teardown {
    agents: Vec<Child>,
    fleetds: Fleetds,
}

impl Drop for Teardown {
    fn drop(&mut self) {
        for agent in &mut self.agents {
            let _ = agent.kill();
            let _ = agent.wait();
        }
    }
}

fn held_task(h: &Harness, td: &mut Teardown, run: &Run, name: &str) -> (String, String, PathBuf) {
    let p = ticket(h, name, &format!("# {name}\n\n**Check:** none\n"));
    seed_task(h, run, &p, "opus");
    let split: Value = serde_json::from_str(&herdr(h, &["pane", "split", "w1:p1"])).unwrap();
    let pane = split["result"]["pane"]["pane_id"]
        .as_str()
        .expect("the new pane")
        .to_string();
    let agent = std::process::Command::new("/bin/sleep")
        .arg("600")
        .spawn()
        .unwrap();
    let worker = NewWorker {
        persona: "opus".into(),
        pane_id: Some(pane.clone()),
        ..Default::default()
    };
    let bound = horch_e2e::bind::bind_worker(h, worker, Some(agent.id()));
    td.agents.push(agent);
    let tree = PathBuf::from(
        &bound
            .worktree
            .expect("a worktree task binds a worktree")
            .path,
    );
    (bound.worker_id, pane, tree)
}

fn wait_tree(h: &Harness, tree: &Path) {
    wait_for(&format!("the worktree {}", tree.display()), || {
        let s = Store::open_read_only(&ledger(h)).unwrap();
        let states: Vec<String> = s
            .conn()
            .prepare("SELECT state FROM jobs WHERE kind = 'worktree_add'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .flatten()
            .collect();
        assert!(
            !states.iter().any(|s| s == "failed"),
            "the worktree_add job of {} failed",
            tree.display()
        );
        states.iter().any(|s| s == "passed").then_some(())
    });
}

fn set_stall_rounds(h: &Harness, run: &Run, rounds: i64) {
    Store::open(&ledger(h))
        .unwrap()
        .conn()
        .execute(
            "UPDATE runs SET stall_rounds = ?2 WHERE run_id = ?1",
            rusqlite::params![run.run_id, rounds],
        )
        .unwrap();
}

/// A herdr server restart: every agent process ends, and the panes are gone.
fn restart_herdr(h: &Harness, td: &mut Teardown, panes: &[String]) {
    for agent in &mut td.agents {
        agent.kill().unwrap();
        agent.wait().unwrap();
    }
    for pane in panes {
        herdr(h, &["pane", "close", pane]);
    }
}

/// 1 commit of `feature.txt` in the worktree `path`.
fn commit_feature(h: &Harness, path: &Path) -> String {
    std::fs::write(path.join("feature.txt"), "committed\n").unwrap();
    git_in(h, path, &["add", "feature.txt"]);
    git_in(
        h,
        path,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "feature"],
    );
    git_in(h, path, &["rev-parse", "HEAD"])
}

/// The refs under `refs/horch/rescue/`, as `<object> <ref>` lines.
fn rescue_refs(h: &Harness) -> Vec<String> {
    let project = h.project.clone();
    let refs = git_in(
        h,
        &project,
        &[
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            "refs/horch/rescue/",
        ],
    );
    refs.lines().map(str::to_string).collect()
}

fn evidence(h: &Harness) -> String {
    let s = Store::open_read_only(&ledger(h)).unwrap();
    [
        "SELECT 'tree ' || path || ' ' || state || ' ' || ifnull(rescue_ref, '-') FROM worktrees",
        "SELECT 'job ' || id || ' ' || kind || ' ' || state || ' ' || substr(ifnull(result_json, '-'), 1, 300) \
         FROM jobs WHERE kind NOT IN ('docs_sync', 'conformance', 'fetch')",
        "SELECT 'attempt ' || id || ' ' || state || ' ' || ifnull(outcome, '-') FROM attempts",
        "SELECT 'task ' || task_id || ' ' || status FROM tasks",
    ]
    .iter()
    .flat_map(|q| {
        let mut stmt = s.conn().prepare(q).unwrap();
        let rows: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .flatten()
            .collect();
        rows
    })
    .collect::<Vec<_>>()
    .join("\n")
}

fn the_rescue_ref(h: &Harness) -> String {
    let until = Instant::now() + Duration::from_secs(CAUGHT_S);
    let mut refs = rescue_refs(h);
    while refs.is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(500));
        refs = rescue_refs(h);
    }
    let all = git_in(
        h,
        &h.project.clone(),
        &["for-each-ref", "--format=%(refname)"],
    );
    assert!(
        refs.len() == 1,
        "no ref under refs/horch/rescue/ holds the work {CAUGHT_S} s after the death: \
         {refs:?}\nall refs:\n{all}\n{}",
        evidence(h)
    );
    refs[0].split(' ').nth(1).unwrap().to_string()
}

/// The bytes of `file` in the tree of `rev`.
fn file_at(h: &Harness, rev: &str, file: &str) -> Vec<u8> {
    let o = h
        .git_cmd(&["cat-file", "blob", &format!("{rev}:{file}")])
        .current_dir(&h.project)
        .output()
        .unwrap();
    assert!(o.status.success(), "{rev}:{file}: {}", text(&o));
    o.stdout
}

/// The active run, its tasks and its workers (not its orchestrator), read
/// from the ledger file without a `horch` call.
fn board(h: &Harness) -> (Run, Vec<tasks::Task>, Vec<workers::Worker>) {
    let s = Store::open_read_only(&ledger(h)).unwrap();
    let run = watchdog::active_runs(s.conn()).unwrap().remove(0);
    let ts = tasks::of_run(s.conn(), &run.run_id).unwrap();
    let mut ws = workers::of_run(s.conn(), &run.run_id).unwrap();
    ws.retain(|w| w.kind == "worker");
    (run, ts, ws)
}

fn live(w: &workers::Worker) -> bool {
    WORKERS_LIVE.iter().any(|s| s.as_str() == w.status)
}

#[test]
fn fm13_herdr_restart_ends_attempts_and_queues_tasks_again() {
    check(
        file!(),
        "fm13_herdr_restart_ends_attempts_and_queues_tasks_again",
        || {
            let h = fleet("p0-fm13-p1");
            // The default timers of the run: the design's numbers apply.
            let run = seed_run(&h, "fm13", None, "w1:p1");
            let mut td = Teardown {
                agents: Vec::new(),
                fleetds: Fleetds::new(&h),
            };
            let (_, p1, _) = held_task(&h, &mut td, &run, "t1");
            let (_, p2, _) = held_task(&h, &mut td, &run, "t2");
            let (run, ts, ws) = board(&h);
            // fleetd supervises the run.
            td.fleetds.start(&h, "fleetd");
            assert!(ts.iter().all(|t| t.status == "running"), "{ts:?}");
            assert_eq!(ws.iter().filter(|w| live(w)).count(), 2, "{ws:?}");

            restart_herdr(&h, &mut td, &[p1, p2]);
            // No `horch` call from here: fleetd alone must act.
            let until = Instant::now() + Duration::from_secs(CAUGHT_S);
            let (_, mut ts, mut ws) = board(&h);
            while Instant::now() < until {
                if ts.iter().all(|t| t.status == "ready") && !ws.iter().any(live) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
                (_, ts, ws) = board(&h);
            }
            assert!(
                td.fleetds
                    .row()
                    .is_some_and(|(pid, started)| horch_core::procid::alive(pid, Some(started))),
                "fleetd stopped during the wait"
            );
            let tasks: Vec<_> = ts.iter().map(|t| (&t.task_id, &t.status)).collect();
            let attempts: Vec<_> = ws
                .iter()
                .map(|w| (&w.worker_id, &w.status, &w.end_reason))
                .collect();
            assert!(
                ts.iter().all(|t| t.status == "ready") && !ws.iter().any(live),
                "the attempts did not end and the tasks were not queued again within \
                 {CAUGHT_S} s of the herdr restart: tasks {tasks:?}, attempts \
                 {attempts:?}, watchdog check every {} s",
                run.check_every_s
            );
        },
    );
}

#[test]
fn fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it() {
    check(
        file!(),
        "fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it",
        || {
            let h = fleet("p0-fm13-p3");
            let run = seed_run(&h, "fm13", None, "w1:p1");
            let mut td = Teardown {
                agents: Vec::new(),
                fleetds: Fleetds::new(&h),
            };
            let (_, pane, path) = held_task(&h, &mut td, &run, "t1");
            td.fleetds.start(&h, "fleetd");
            wait_tree(&h, &path);
            let head = commit_feature(&h, &path);
            std::fs::write(path.join("feature.txt"), "committed\nand changed\n").unwrap();

            restart_herdr(&h, &mut td, &[pane]);
            let rescue = the_rescue_ref(&h);
            let project = h.project.clone();
            git_in(
                &h,
                &project,
                &["merge-base", "--is-ancestor", &head, &rescue],
            );
            assert_eq!(
                file_at(&h, &rescue, "feature.txt"),
                b"committed\nand changed\n"
            );
            // The retry waits for its backoff of 30 s on the fleet clock
            // (rung 6, Phase 2).
            h.advance_fleet_clock(30);
            let next = wait_for_or(
                "the bind of the next attempt",
                || horch_e2e::bind::try_bind(&h, "opus", None).ok(),
                || evidence(&h),
            );
            let reused = match &next.worktree {
                Some(tree) => tree.path.clone(),
                None => panic!("the bind names no worktree: {next:?}"),
            };
            assert_eq!(reused, path.to_string_lossy(), "{next:?}");
        },
    );
}

#[test]
fn fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing() {
    check(
        file!(),
        "fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing",
        || {
            let h = fleet("p0-fm25");
            let run = seed_run(&h, "fm25", None, "w1:p1");
            set_stall_rounds(&h, &run, 1);
            let mut td = Teardown {
                agents: Vec::new(),
                fleetds: Fleetds::new(&h),
            };
            let (_, _, path) = held_task(&h, &mut td, &run, "t1");
            td.fleetds.start(&h, "fleetd");
            wait_tree(&h, &path);
            let tracked = b"one\ntwo\nthree, not committed\n";
            let untracked = b"a new file, never added\n";
            std::fs::write(path.join("notes.txt"), tracked).unwrap();
            std::fs::write(path.join("untracked.txt"), untracked).unwrap();

            let agent = &mut td.agents[0];
            agent.kill().unwrap();
            agent.wait().unwrap();
            // The pass ends the attempt of the dead agent (I4) before the
            // duty check, so the reap shows in the ledger, not as a verdict.
            wait_for("the reap", || {
                let (_, _, ws) = board(&h);
                (!ws.iter().any(live)).then_some(())
            });
            let rescue = the_rescue_ref(&h);
            assert_eq!(file_at(&h, &rescue, "notes.txt"), tracked);
            assert_eq!(file_at(&h, &rescue, "untracked.txt"), untracked);
            assert!(path.is_dir(), "the worktree is gone");
        },
    );
}
