#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::fleetd::task_execution::application::repair::{repair_kind, RepairKind};
use horch_core::fleetd::task_execution::domain::repair::REPAIR_LIMIT_KIND;
use horch_core::taskledger::model::{Actor, VerifyProfile, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Thresholds};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, Store};
use horch_core::vcs::network::OTHER_BOUND;
use horch_e2e::harness::{fixtures, Harness, DEADLINE};
use rusqlite::{Connection, OptionalExtension};

const PERSONA: &str = "opus";

const FAKE_FILE: &str = "fake-claude.txt";

const FEATURE: &str = "fleet/alpha";

const WORK: &str = "exec,commit,result";

const RESULT_ONLY: &str = "exec,result";

const NO_RESULT: &str = "exec,exit_without_result";

const GATE_LINE: &str = "gate: fake-claude.txt has no FIXED line";

const PAST_THE_LEASE_S: i64 = 121;

const FIRST_BACKOFF_S: i64 = 30;

const LIVE: [&str; 4] = ["spawning", "starting", "claiming", "working"];

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
    task_id: String,
}

impl World {
    fn new(name: &str, fast: &str, max_attempts: Option<i64>) -> World {
        let mut h = Harness::new(name).with_git();
        let fleet = h.state.join("fleet");
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::write(
            fleet.join("fleet.toml"),
            "pass_period_s = 1\nhealth_max_load_per_cpu = 1000000.0\n",
        )
        .unwrap();
        h.set(
            "HORCH_LEDGER_DB",
            h.state.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set(
            "HORCH_QUOTA_FILE",
            fixtures().join("quota/all-ok.json").to_string_lossy(),
        );
        h.set("HORCH_WORKSPACE_ID", "w1");
        h.set("HORCH_FAKE_SCENARIO", WORK);
        let project = h.project.clone();
        std::fs::write(
            project.join("fleet.toml"),
            format!(
                "[gates]\nfast = {}\nfull = \"true\"\n",
                serde_json::Value::from(fast)
            ),
        )
        .unwrap();
        git_ok(&h, &project, &["add", "fleet.toml"]);
        git_ok(&h, &project, &["commit", "--quiet", "-m", "gates"]);
        git_ok(&h, &project, &["branch", FEATURE, "main"]);
        let task_id = seed(&h, max_attempts);
        let ws = h.fleet_workspace();
        h.set("HORCH_WORKSPACE_ID", ws);
        h.serve_fleetd();
        World { h, task_id }
    }

    fn conn(&self) -> Connection {
        let conn = Connection::open(self.h.ledger_db()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        conn
    }

    fn until<T>(&self, what: &str, mut f: impl FnMut(&World) -> Option<T>) -> T {
        let bound = OTHER_BOUND + Duration::from_secs(FleetConfig::default().stop_grace_s);
        let wait = DEADLINE + bound;
        let end = Instant::now() + wait;
        while Instant::now() < end {
            if let Some(v) = f(self) {
                return v;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!(
            "timed out after {} s waiting for {what}\n{}",
            wait.as_secs(),
            self.diagnostics()
        );
    }

    fn paused<T>(&self, f: impl FnOnce(&World) -> T) -> T {
        self.h.without_fleetd(|| f(self))
    }

    fn scenario(&mut self, scenario: &str) {
        self.h.set("HORCH_FAKE_SCENARIO", scenario);
    }

    fn advance_clock(&self, seconds: i64) {
        let moved = self
            .conn()
            .execute(
                "UPDATE fleetd SET fleet_clock_s = fleet_clock_s + ?1 WHERE id = 1",
                [seconds],
            )
            .unwrap();
        assert_eq!(moved, 1, "the ledger has a fleetd row");
    }

    fn fleet_clock(&self) -> i64 {
        self.conn()
            .query_row("SELECT fleet_clock_s FROM fleetd WHERE id = 1", [], |r| {
                r.get(0)
            })
            .unwrap()
    }

    fn passes(&self, n: i64) {
        let until = self.fleet_clock() + n;
        self.until(&format!("{n} more seconds of the fleet clock"), |w| {
            (w.fleet_clock() >= until).then_some(())
        });
    }

    fn task_status(&self) -> String {
        self.conn()
            .query_row(
                "SELECT status FROM tasks WHERE task_id = ?1",
                [&self.task_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn branch(&self) -> Option<(String, String, i64)> {
        self.conn()
            .query_row(
                "SELECT name, COALESCE(landing_state, ''), repair_attempts FROM branches
                 WHERE task_id = ?1",
                [&self.task_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .unwrap()
    }

    fn landing_state(&self) -> String {
        self.branch().map(|b| b.1).unwrap_or_default()
    }

    fn attempts(&self) -> Vec<String> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id FROM attempts WHERE task_id = ?1 ORDER BY rowid")
            .unwrap();
        stmt.query_map([&self.task_id], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    fn attempt(&self, n: usize) -> Option<String> {
        self.attempts().get(n - 1).cloned()
    }

    fn worker_ended(&self, n: usize) -> bool {
        let Some(id) = self.attempt(n) else {
            return false;
        };
        let status: String = self
            .conn()
            .query_row(
                "SELECT w.status FROM workers w JOIN attempts a ON a.worker_id = w.worker_id
                 WHERE a.id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        !LIVE.contains(&status.as_str())
    }

    fn kind(&self, n: usize) -> Option<RepairKind> {
        let id = self.attempt(n)?;
        repair_kind(&self.conn(), &id).unwrap()
    }

    fn attempt_folder(&self, n: usize) -> PathBuf {
        attempt_dir(&self.h.fleet_root(), &self.task_id, &format!("A{n}"))
    }

    fn attempt_file(&self, n: usize, name: &str) -> Option<String> {
        std::fs::read_to_string(self.attempt_folder(n).join(name)).ok()
    }

    fn worktree(&self, n: usize) -> PathBuf {
        let yaml = self
            .attempt_file(n, "task.yaml")
            .unwrap_or_else(|| panic!("no task.yaml of A{n}\n{}", self.diagnostics()));
        let value = yaml
            .lines()
            .find_map(|l| l.strip_prefix("worktree: "))
            .unwrap_or_else(|| panic!("no worktree in the task.yaml of A{n}: {yaml}"));
        PathBuf::from(value.trim().trim_matches('"'))
    }

    fn open_items(&self, kind: &str) -> Vec<(String, String)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT key, text FROM needs_you WHERE kind = ?1 AND state = 'open'
                 ORDER BY key",
            )
            .unwrap();
        stmt.query_map([kind], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    fn moves_out_of_done(&self) -> i64 {
        self.conn()
            .query_row(
                "SELECT COUNT(*) FROM events
                 WHERE entity = 'task' AND entity_id = ?1 AND from_state = 'done'",
                [&self.task_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn first_attempt_done(
        &mut self,
        next: &str,
        before_land: impl FnOnce(&World, &Path),
    ) -> PathBuf {
        let worktree = self.until("the commit of the fake in the worktree of A1", |w| {
            w.attempt_file(1, "task.yaml")?;
            let tree = w.worktree(1);
            let subject = git_in(&w.h, &tree, &["log", "-1", "--format=%s"])?;
            subject.starts_with("fake ").then_some(tree)
        });
        self.scenario(next);
        self.paused(|w| {
            assert!(
                !["ready", "in_bundle", "landed"].contains(&w.landing_state().as_str()),
                "the branch moved on before the test: {}",
                w.diagnostics()
            );
            before_land(w, &worktree);
        });
        self.until("the task of A1 to be done", |w| {
            (w.task_status() == "done").then_some(())
        });
        worktree
    }

    fn jobs(&self, conn: &Connection) -> String {
        let Ok(mut stmt) = conn.prepare(
            "SELECT id, kind, state, COALESCE(started_at, '-'), pid, deadline, run, \
             COALESCE(substr(result_json, 1, 300), '-') FROM jobs ORDER BY id",
        ) else {
            return "bad jobs query".into();
        };
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, String>(7)?,
                ))
            })
            .map(|it| it.flatten().collect::<Vec<_>>())
            .unwrap_or_default();
        let dir = self.h.fleet_root().join("run/jobs");
        let mut out = String::new();
        for (id, kind, state, started, pid, deadline, run, result) in rows {
            let file = dir.join(format!("{id}-{run}.json"));
            let process = match (state.as_str(), pid) {
                ("running", Some(pid)) => {
                    let ps = std::process::Command::new("ps")
                        .args(["-o", "stat=,command=", "-p", &pid.to_string()])
                        .output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .unwrap_or_default();
                    if ps.is_empty() {
                        " process gone".to_string()
                    } else {
                        format!(" process alive: {ps}")
                    }
                }
                _ => String::new(),
            };
            out.push_str(&format!(
                "  {id} {kind} {state} started {started} pid {pid:?} deadline {deadline:?} \
                 run {run}; result file {} exists={}{process}; {result}\n",
                file.display(),
                file.exists(),
            ));
        }
        out
    }

    fn diagnostics(&self) -> String {
        let conn = self.conn();
        let rows = |sql: &str| -> Vec<String> {
            let Ok(mut stmt) = conn.prepare(sql) else {
                return vec![format!("bad query: {sql}")];
            };
            stmt.query_map([], |r| r.get::<_, String>(0))
                .map(|it| it.flatten().collect())
                .unwrap_or_default()
        };
        format!(
            "tasks: {:?}\nattempts: {:?}\nworkers: {:?}\nbranches: {:?}\nbundles: {:?}\n\
             fleet clock: {}\njobs:\n{}\nneeds_you: {:?}\nfeature log: {:?}\nlaunch.log:\n{}\nfleetd.stderr:\n{}",
            rows("SELECT task_id || ' ' || status FROM tasks"),
            rows("SELECT id || ' ' || via || ' ' || state FROM attempts"),
            rows("SELECT worker_id || ' ' || status FROM workers"),
            rows(
                "SELECT name || ' ' || COALESCE(landing_state, '-') || ' repairs=' || \
                 repair_attempts FROM branches"
            ),
            rows("SELECT id || ' ' || state || ' ' || COALESCE(result, '-') FROM bundles"),
            self.fleet_clock(),
            self.jobs(&conn),
            rows("SELECT key || ' ' || state || ' ' || text FROM needs_you"),
            git_in(&self.h, &self.h.project, &["log", "--format=%s", FEATURE]),
            tail(&self.h.fleet_root().join("run/launch.log")),
            tail(&self.h.root.join("fleetd.stderr")),
        )
    }
}

fn seed(h: &Harness, max_attempts: Option<i64>) -> String {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t0.md"), "# t0\n").unwrap();
    let store = Store::open(&h.ledger_db()).unwrap();
    let conn = store.conn();
    let run = runs::create(
        conn,
        &h.project,
        &NewRun {
            slug: "alpha".into(),
            workspace_id: Some("w1".into()),
            base_branch: Some("main".into()),
            run_branch: Some(FEATURE.into()),
            finish_steps: VerifyProfile::Minimal.steps(),
            thresholds: Thresholds {
                max_attempts,
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    schema::run_idea(conn, &run.run_id, &horch_core::clock::now_stamp(), true)
        .unwrap()
        .expect("the idea of alpha");
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
    new.land_check = Some("true".into());
    tasks::add(conn, &run, &new, Actor::Orchestrator)
        .unwrap()
        .task_id
}

fn tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = h.git_cmd(&[&["-c", "core.hooksPath=/dev/null"][..], args].concat());
    cmd.current_dir(dir);
    let o = cmd.output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn git_ok(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut cmd = h.git_cmd(&[&["-c", "core.hooksPath=/dev/null"][..], args].concat());
    cmd.current_dir(dir);
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn commit_on_feature(h: &Harness, file: &str, body: &str) -> String {
    let project = h.project.clone();
    let index = h.root.join("feature-index");
    let blob = h.root.join("feature-blob");
    std::fs::write(&blob, body).unwrap();
    let git = |args: &[&str]| {
        let mut cmd = h.git_cmd(args);
        cmd.current_dir(&project).env("GIT_INDEX_FILE", &index);
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    let old = git(&["rev-parse", FEATURE]);
    git(&["read-tree", &old]);
    let sha = git(&["hash-object", "-w", &blob.to_string_lossy()]);
    git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{sha},{file}"),
    ]);
    let tree = git(&["write-tree"]);
    let new = git(&[
        "commit-tree",
        &tree,
        "-p",
        &old,
        "-m",
        "feature: another task",
    ]);
    git(&["update-ref", &format!("refs/heads/{FEATURE}"), &new, &old]);
    new
}

fn is_ancestor(h: &Harness, ancestor: &str, rev: &str) -> bool {
    git_in(
        h,
        &h.project,
        &["merge-base", "--is-ancestor", ancestor, rev],
    )
    .is_some()
}

#[test]
fn fm38_conflicted_branch_gets_a_resolver_attempt_and_lands() {
    let mut w = World::new("p4-repair-resolve", "true", None);
    let mut feature_commit = String::new();
    let worktree = w.first_attempt_done(RESULT_ONLY, |w, _| {
        feature_commit = commit_on_feature(&w.h, FAKE_FILE, "the feature side\n");
    });
    let work_commit = git_ok(&w.h, &worktree, &["rev-parse", "HEAD"]);

    let (branch, _, _) = w.until("the branch of the task to be conflicted", |w| {
        w.branch().filter(|b| b.1 == "conflicted")
    });
    w.paused(|w| {
        assert_eq!(w.task_status(), "done", "{}", w.diagnostics());
        assert!(
            git_in(&w.h, &worktree, &["merge", "--no-edit", FEATURE]).is_none(),
            "the merge of {FEATURE} into the worktree does not conflict"
        );
        let ours = git_ok(&w.h, &worktree, &["show", &format!("HEAD:{FAKE_FILE}")]);
        std::fs::write(
            worktree.join(FAKE_FILE),
            format!("the feature side\n{ours}\n"),
        )
        .unwrap();
        git_ok(&w.h, &worktree, &["add", FAKE_FILE]);
        git_ok(&w.h, &worktree, &["commit", "--quiet", "--no-edit"]);
    });

    w.until("the task files of the resolver attempt A2", |w| {
        w.attempt_file(2, "task.yaml")
    });
    let start = w.attempt_file(2, "inbox/START.md").unwrap();
    assert_eq!(
        w.kind(2),
        Some(RepairKind::Resolver),
        "A2 is not a resolver attempt: {}",
        w.diagnostics()
    );
    assert_eq!(
        w.worktree(2),
        worktree,
        "the resolver attempt runs in another worktree"
    );
    for want in [
        "## Resolver attempt",
        &format!("`{FAKE_FILE}`"),
        "zdiff3",
        &branch,
    ] {
        assert!(
            start.contains(want),
            "the START.md of A2 has no {want:?}:\n{start}"
        );
    }

    w.until("the branch of the task to land", |w| {
        (w.landing_state() == "landed").then_some(())
    });
    assert_eq!(w.task_status(), "done", "{}", w.diagnostics());
    assert_eq!(w.moves_out_of_done(), 0, "{}", w.diagnostics());
    assert_eq!(
        w.attempts().len(),
        2,
        "1 resolver attempt resolves the conflict: {}",
        w.diagnostics()
    );
    for (what, commit) in [("work", &work_commit), ("feature", &feature_commit)] {
        assert!(
            is_ancestor(&w.h, commit, FEATURE),
            "the {what} commit {commit} is not on {FEATURE}: {}",
            w.diagnostics()
        );
    }
}

#[test]
fn failed_resolver_attempt_starts_the_next_and_at_max_attempts_opens_1_item() {
    let mut w = World::new("p4-repair-limit", "true", Some(2));
    w.first_attempt_done(NO_RESULT, |w, _| {
        commit_on_feature(&w.h, FAKE_FILE, "the feature side\n");
    });
    let (branch, _, _) = w.until("the branch of the task to be conflicted", |w| {
        w.branch().filter(|b| b.1 == "conflicted")
    });

    w.until("the resolver attempt A2", |w| w.kind(2));
    w.until("the worker of A2 to end", |w| {
        w.worker_ended(2).then_some(())
    });
    w.advance_clock(PAST_THE_LEASE_S);
    w.until("repair_attempts 1 after the failure of A2", |w| {
        w.branch().filter(|b| b.2 == 1)
    });
    assert_eq!(
        w.attempts().len(),
        2,
        "the next attempt starts in the backoff: {}",
        w.diagnostics()
    );
    w.advance_clock(FIRST_BACKOFF_S);

    assert_eq!(
        w.until("the resolver attempt A3", |w| w.kind(3)),
        RepairKind::Resolver
    );
    w.until("the worker of A3 to end", |w| {
        w.worker_ended(3).then_some(())
    });
    w.advance_clock(PAST_THE_LEASE_S);
    let key = format!("{REPAIR_LIMIT_KIND}:{branch}");
    w.until("1 needs-you item at max_attempts", |w| {
        w.open_items(REPAIR_LIMIT_KIND)
            .iter()
            .any(|(k, _)| *k == key)
            .then_some(())
    });

    w.advance_clock(3600);
    w.passes(3);
    let items = w.open_items(REPAIR_LIMIT_KIND);
    assert_eq!(items.len(), 1, "{items:?}\n{}", w.diagnostics());
    assert_eq!(items[0].0, key);
    assert!(
        items[0].1.contains("horch task requeue") && items[0].1.contains(&branch),
        "the item does not name its branch and its exit: {}",
        items[0].1
    );
    assert_eq!(
        w.attempts().len(),
        3,
        "an attempt starts at max_attempts: {}",
        w.diagnostics()
    );
    assert_eq!(
        w.branch().map(|b| (b.1, b.2)),
        Some(("conflicted".into(), 2))
    );
    assert_eq!(w.task_status(), "done", "{}", w.diagnostics());
    assert_eq!(w.moves_out_of_done(), 0, "{}", w.diagnostics());
}

#[test]
fn gate_failed_branch_gets_a_fix_attempt_with_the_gate_output() {
    let fast = format!(
        r#"while read -r l; do [ "$l" = FIXED ] && exit 0; done < {FAKE_FILE}; echo '{GATE_LINE}'; exit 1"#
    );
    let mut w = World::new("p4-repair-fix", &fast, None);
    let worktree = w.first_attempt_done(RESULT_ONLY, |_, _| {});
    w.until("the branch of the task to fail the gate", |w| {
        (w.landing_state() == "gate_failed").then_some(())
    });

    let fix_commit = w.paused(|w| {
        assert_eq!(w.task_status(), "done", "{}", w.diagnostics());
        let mut body = std::fs::read_to_string(worktree.join(FAKE_FILE)).unwrap();
        body.push_str("FIXED\n");
        std::fs::write(worktree.join(FAKE_FILE), body).unwrap();
        git_ok(&w.h, &worktree, &["add", FAKE_FILE]);
        git_ok(
            &w.h,
            &worktree,
            &["commit", "--quiet", "-m", "fix the gate"],
        );
        git_ok(&w.h, &worktree, &["rev-parse", "HEAD"])
    });

    w.until("the task files of the fix attempt A2", |w| {
        w.attempt_file(2, "task.yaml")
    });
    let start = w.attempt_file(2, "inbox/START.md").unwrap();
    assert_eq!(
        w.kind(2),
        Some(RepairKind::Fix),
        "A2 is not a fix attempt: {}",
        w.diagnostics()
    );
    assert_eq!(
        w.worktree(2),
        worktree,
        "the fix attempt runs in another worktree"
    );
    for want in ["## Fix attempt", GATE_LINE] {
        assert!(
            start.contains(want),
            "the START.md of A2 has no {want:?}:\n{start}"
        );
    }

    w.until("the branch of the task to land", |w| {
        (w.landing_state() == "landed").then_some(())
    });
    assert_eq!(w.task_status(), "done", "{}", w.diagnostics());
    assert_eq!(w.moves_out_of_done(), 0, "{}", w.diagnostics());
    assert!(
        is_ancestor(&w.h, &fix_commit, FEATURE),
        "the fix {fix_commit} is not on {FEATURE}: {}",
        w.diagnostics()
    );
}
