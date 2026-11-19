//! The e2e tests of FM-38 through the binary (`docs/specs/fleetd/design.md`,
//! FM-38, "Landing a task branch on its feature branch": `conflicted` and
//! `gate_failed`; `docs/tickets/fleetd-p4/54-p4-repair-e2e.md`).
//!
//! Each test makes its own ledger, fleet root and git project in a temp
//! folder. The test writes the run, its idea, its seat and 1 queued task,
//! then runs each pass of fleetd itself with `horch ledger watch --once`,
//! so it knows the order of the passes. The fake herdr runs the pane of
//! each attempt, and the fake `claude` plays its agent in exec mode.
//!
//! The fake agent has no key that merges or edits a file. So the test does
//! the edit of a resolver or a fix attempt in the worktree between 2
//! passes, before the result of the attempt can be read, and the fake of
//! the attempt only runs `horch result`. fleetd does the rest: it binds the
//! attempt, writes its `START.md`, launches it, runs the check, and lands
//! the branch.
//!
//! The tests read no real load, limit or clock: the harness gives the
//! machine through `HORCH_MACHINE_FILE`, and the backoff and the lease
//! expiry come from moves of the fleet clock.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::fleetd::task_execution::application::repair::{repair_kind, RepairKind};
use horch_core::fleetd::task_execution::domain::repair::REPAIR_LIMIT_KIND;
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Thresholds};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, Store};
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::{Connection, OptionalExtension};

/// The persona of the task. Its fake agent is `fake-claude`.
const PERSONA: &str = "opus";

/// The 1 file that the fake agent commits (`agent.rs`). The conflict and
/// the gate failure of each test are in this file.
const FAKE_FILE: &str = "fake-claude.txt";

/// The feature branch of the run.
const FEATURE: &str = "fleet/alpha";

/// The scenario of the first attempt: it commits [`FAKE_FILE`] and runs
/// `horch result`.
const WORK: &str = "exec,commit,result";

/// The scenario of a repair attempt whose edit the test makes: it only
/// runs `horch result`.
const RESULT_ONLY: &str = "exec,result";

/// The scenario of a repair attempt that fails: it exits with no result.
const NO_RESULT: &str = "exec,exit_without_result";

/// The line that the fast gate of [`gate_failed_branch_gets_a_fix_attempt_with_the_gate_output`]
/// prints when it fails.
const GATE_LINE: &str = "gate: fake-claude.txt has no FIXED line";

/// The lease of an attempt with no heartbeat expires after 120 s on the
/// fleet clock.
const PAST_THE_LEASE_S: i64 = 121;

/// The first backoff of rung 6 on the fleet clock.
const FIRST_BACKOFF_S: i64 = 30;

/// The statuses of a live worker row.
const LIVE: [&str; 4] = ["spawning", "starting", "claiming", "working"];

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

// ─── the world of 1 test ────────────────────────────────────────────────

/// 1 fleet in workspace `w1` whose panes run their commands, with 1 run on
/// [`FEATURE`] and 1 task.
struct World {
    h: Harness,
    task_id: String,
}

impl World {
    /// A world whose project declares the fast gate `fast`, and whose run
    /// has `max_attempts`.
    fn new(name: &str, fast: &str, max_attempts: Option<i64>) -> World {
        let mut h = Harness::new(name).with_git();
        // The ledger is in the state folder and the fleet root is next to
        // it, with no FLEET_ROOT: a pane gets only HORCH_STATE_DIR and
        // HORCH_DATA_DIR, and its worker finds the same paths there.
        let fleet = h.state.join("fleet");
        std::fs::create_dir_all(&fleet).unwrap();
        // A pass each second. The load of the build host does not hold
        // dispatch.
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
        // The workspace has the name of the project folder as its label,
        // as `horch fleet` names it: a push launch splits its first pane.
        let label = project.file_name().unwrap().to_string_lossy().into_owned();
        let mut create = std::process::Command::new(h.bin.join("herdr"));
        h.seal(&mut create);
        let o = create
            .args(["workspace", "create", "--label", &label])
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", text(&o));
        // The test fleetd: it applies the spawn requests, launches each
        // attempt, and runs a pass each second. The drop of the harness
        // stops it.
        h.serve_fleetd();
        World { h, task_id }
    }

    fn conn(&self) -> Connection {
        let conn = Connection::open(self.h.ledger_db()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        conn
    }

    /// Wait while the test fleetd runs its passes, until `f` gives a value.
    fn until<T>(&self, what: &str, mut f: impl FnMut(&World) -> Option<T>) -> T {
        wait_for_or(what, || f(self), || self.diagnostics())
    }

    /// Stop the test fleetd after its current pass, run `f` while no pass
    /// runs, then start the test fleetd again with the environment of now.
    fn paused<T>(&self, f: impl FnOnce(&World) -> T) -> T {
        self.h.without_fleetd(|| f(self))
    }

    /// The fake scenario of each pane that the test fleetd launches after
    /// its next start: a pane gets the environment of fleetd.
    fn scenario(&mut self, scenario: &str) {
        self.h.set("HORCH_FAKE_SCENARIO", scenario);
    }

    /// Move the fleet clock forward by `seconds`, in place of a wait for
    /// it. A pass adds its awake time to the stored value, so the move
    /// stays.
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

    /// Wait until the fleet clock moved `n` more seconds: at least `n - 1`
    /// more passes of 1 s ran.
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

    /// The name, the landing state and `repair_attempts` of the branch of
    /// the task, when it has a row.
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

    /// The ids of the attempts of the task, oldest first.
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

    /// The id of attempt `n` (1 is the first), when it exists.
    fn attempt(&self, n: usize) -> Option<String> {
        self.attempts().get(n - 1).cloned()
    }

    /// The worker of attempt `n` is not live.
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

    /// The kind of the repair attempt `n`.
    fn kind(&self, n: usize) -> Option<RepairKind> {
        let id = self.attempt(n)?;
        repair_kind(&self.conn(), &id).unwrap()
    }

    /// The folder of attempt `n` under the fleet root.
    fn attempt_folder(&self, n: usize) -> PathBuf {
        attempt_dir(&self.h.fleet_root(), &self.task_id, &format!("A{n}"))
    }

    /// The text of `name` in the folder of attempt `n`, when it exists.
    fn attempt_file(&self, n: usize, name: &str) -> Option<String> {
        std::fs::read_to_string(self.attempt_folder(n).join(name)).ok()
    }

    /// The worktree that `task.yaml` of attempt `n` names.
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

    /// The open needs-you items of kind `kind`: key and text.
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

    /// The moves of the task out of `done`: none while a resolver or a fix
    /// attempt runs.
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

    /// Run the first attempt to its end. The fake commits [`FAKE_FILE`] in
    /// the worktree and says "done". When its commit exists, the test
    /// fleetd stops, and `before_land` runs: it is the only time between
    /// the commit and the first bundle. The test fleetd starts again with
    /// the fake scenario `next` for each later attempt. Then the task gets
    /// to `done`.
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
            // A pass queues the check job only after it reads result.json,
            // and a later pass reads its result, so no bundle exists yet.
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

    /// What a failed wait shows: the rows of the task, its branch and its
    /// bundles, the needs-you items and the jobs.
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
             jobs: {:?}\nneeds_you: {:?}\nfeature log: {:?}\nlaunch.log:\n{}\nfleetd.stderr:\n{}",
            rows("SELECT task_id || ' ' || status FROM tasks"),
            rows("SELECT id || ' ' || via || ' ' || state FROM attempts"),
            rows("SELECT worker_id || ' ' || status FROM workers"),
            rows(
                "SELECT name || ' ' || COALESCE(landing_state, '-') || ' repairs=' || \
                 repair_attempts FROM branches"
            ),
            rows("SELECT id || ' ' || state || ' ' || COALESCE(result, '-') FROM bundles"),
            rows(
                "SELECT id || ' ' || kind || ' ' || state || ' ' || \
                 COALESCE(substr(result_json, 1, 300), '-') FROM jobs ORDER BY id"
            ),
            rows("SELECT key || ' ' || state || ' ' || text FROM needs_you"),
            git_in(&self.h, &self.h.project, &["log", "--format=%s", FEATURE]),
            tail(&self.h.fleet_root().join("run/launch.log")),
            tail(&self.h.root.join("fleetd.stderr")),
        )
    }
}

/// Write the rows that the orchestrator's commands wrote before Phase 6
/// retired them: the run on [`FEATURE`], its idea in flight (FR-12), its
/// seat in pane `w1:p1` and 1 queued task with the check `true`. Gives the
/// task id.
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
    new.land_check = Some("true".into());
    tasks::add(conn, &run, &new, Actor::Orchestrator)
        .unwrap()
        .task_id
}

/// The last 40 lines of the file at `path`.
fn tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

// ─── git ────────────────────────────────────────────────────────────────

/// Run git in `dir` with no hook; its trimmed stdout, or `None` when it
/// fails or `dir` does not exist yet.
fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = h.git_cmd(&[&["-c", "core.hooksPath=/dev/null"][..], args].concat());
    cmd.current_dir(dir);
    let o = cmd.output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// [`git_in`], and fail the test when git fails.
fn git_ok(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut cmd = h.git_cmd(&[&["-c", "core.hooksPath=/dev/null"][..], args].concat());
    cmd.current_dir(dir);
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Commit `body` as `file` on [`FEATURE`] with no checkout, by
/// compare-and-set from its tip, as a landing of another task does.
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

/// `ancestor` is in the history of `rev`.
fn is_ancestor(h: &Harness, ancestor: &str, rev: &str) -> bool {
    git_in(
        h,
        &h.project,
        &["merge-base", "--is-ancestor", ancestor, rev],
    )
    .is_some()
}

// ─── the tests ──────────────────────────────────────────────────────────

/// FM-38, `conflicted`: the task branch conflicts with its feature branch
/// after its worker exited. fleetd starts 1 resolver attempt of the `done`
/// task in its worktree, with the conflict files in `START.md`. Its result
/// passes the check, and the branch lands. The task stays `done`.
#[test]
fn fm38_conflicted_branch_gets_a_resolver_attempt_and_lands() {
    let mut w = World::new("p4-repair-resolve", "true", None);
    let mut feature_commit = String::new();
    // The resolver attempt only says "done". The test makes its merge.
    let worktree = w.first_attempt_done(RESULT_ONLY, |w, _| {
        // Another landing adds the same file to the feature with other
        // text, after the worktree left the feature tip.
        feature_commit = commit_on_feature(&w.h, FAKE_FILE, "the feature side\n");
    });
    let work_commit = git_ok(&w.h, &worktree, &["rev-parse", "HEAD"]);

    let (branch, _, _) = w.until("the branch of the task to be conflicted", |w| {
        w.branch().filter(|b| b.1 == "conflicted")
    });
    // The resolution of the agent of the resolver attempt, while no pass
    // runs: merge the feature, keep both sides, commit. The fake of the
    // attempt does not write to the worktree.
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

    let start = w.until("the START.md of the resolver attempt A2", |w| {
        w.attempt_file(2, "inbox/START.md")
    });
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

/// FM-38 and the test of the gate of Phase 4: a failed resolver attempt
/// adds 1 to `repair_attempts` and starts the next one after the backoff.
/// At `max_attempts` of the task no attempt starts, and 1 needs-you item
/// opens. The task stays `done`.
#[test]
fn failed_resolver_attempt_starts_the_next_and_at_max_attempts_opens_1_item() {
    let mut w = World::new("p4-repair-limit", "true", Some(2));
    // Each resolver attempt exits with no result.
    w.first_attempt_done(NO_RESULT, |w, _| {
        commit_on_feature(&w.h, FAKE_FILE, "the feature side\n");
    });
    let (branch, _, _) = w.until("the branch of the task to be conflicted", |w| {
        w.branch().filter(|b| b.1 == "conflicted")
    });

    // A2 fails: its agent exits with no result, and its lease expires.
    w.until("the resolver attempt A2", |w| w.kind(2));
    w.until("the worker of A2 to end", |w| {
        w.worker_ended(2).then_some(())
    });
    w.advance_clock(PAST_THE_LEASE_S);
    w.until("repair_attempts 1 after the failure of A2", |w| {
        w.branch().filter(|b| b.2 == 1)
    });
    // The backoff of 30 s on the fleet clock holds the next attempt.
    assert_eq!(
        w.attempts().len(),
        2,
        "the next attempt starts in the backoff: {}",
        w.diagnostics()
    );
    w.advance_clock(FIRST_BACKOFF_S);

    // A3 is the next resolver attempt, and it fails also.
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

    // More passes and a long time start no attempt and no second item.
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

/// FM-38, `gate_failed`: the fast gate fails on the task branch after its
/// worker exited. fleetd starts 1 fix attempt of the `done` task in its
/// worktree, with the output of the gate in `START.md`. Its result passes
/// the check, and the branch lands. The task stays `done`.
#[test]
fn gate_failed_branch_gets_a_fix_attempt_with_the_gate_output() {
    let fast = format!("grep -q FIXED {FAKE_FILE} || {{ echo '{GATE_LINE}'; exit 1; }}");
    let mut w = World::new("p4-repair-fix", &fast, None);
    // The fix attempt only says "done". The test makes its fix.
    let worktree = w.first_attempt_done(RESULT_ONLY, |_, _| {});
    w.until("the branch of the task to fail the gate", |w| {
        (w.landing_state() == "gate_failed").then_some(())
    });

    // The fix of the agent of the fix attempt, while no pass runs.
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

    let start = w.until("the START.md of the fix attempt A2", |w| {
        w.attempt_file(2, "inbox/START.md")
    });
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
