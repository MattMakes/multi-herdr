//! The bisect of a failed bundle: each half is a child bundle, the failing
//! branch goes to `gate_failed`, and the others go on
//! (docs/specs/fleetd/design.md, "The bundle", "The bundle state machine"
//! (`failed`); docs/tickets/fleetd-p4/30-bisect.md).
//!
//! Each test has its own temporary ledger, fleet root and git repository.
//! No test reads the load, the limits or the clock of the machine: the fleet
//! clock is a number that the test gives, and the config is the default.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::exec_bundle;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, BundleRow, NewBranch, NewFeature,
};
use horch_core::fleetd::integration::application::bundles::{self, Bundles, FailedGate};
use horch_core::fleetd::integration::application::landing;
use horch_core::fleetd::integration::domain::task_branch::LandingState;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::fleetd::worktree_custody::application::create::{self, Observed};
use horch_core::fleetd::worktree_custody::application::gate_tree::{
    self, GateTree, GateTreeRequest,
};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

const FEATURE: &str = "F-0001";
const FEATURE_BRANCH: &str = "fleet/r1";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _temp: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
    tasks: u32,
    clock: i64,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn git_out(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_out(dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let root = horch_platform::path::for_child(&temp.path().canonicalize().unwrap());
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["branch", FEATURE_BRANCH]);
    let mut conn = Connection::open(root.join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        run_branch: Some(FEATURE_BRANCH.into()),
        ..Default::default()
    };
    let run = runs::create(&conn, &repo, &new_run).unwrap();
    let new = NewFeature {
        id: FEATURE,
        name: Some("r1"),
        branch: Some(FEATURE_BRANCH),
        primary_branch: "main",
    };
    let tx = conn.transaction().unwrap();
    rows::insert_feature(&tx, None, &new, cause("feature accepted")).unwrap();
    // FR-12: a task needs an idea in flight of the feature of its run.
    horch_core::taskledger::schema::run_idea(
        &tx,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    tx.commit().unwrap();
    let w = World {
        _temp: temp,
        root,
        repo,
        conn,
        run,
        config: FleetConfig::default(),
        tasks: 0,
        clock: 0,
    };
    w.gate_tree();
    w
}

impl World {
    /// The gate tree of [`FEATURE`], so a fast gate can run (Q18): its
    /// reserved row, the folder that its `worktree_add` job makes, and the
    /// observation of the passed job, as a pass observes it.
    fn gate_tree(&self) {
        let tip = git(&self.repo, &["rev-parse", FEATURE_BRANCH]);
        let req = GateTreeRequest {
            feature_id: FEATURE,
            run_id: &self.run.run_id,
            repo: &self.repo,
            base: FEATURE_BRANCH,
            tip: &tip,
        };
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        let job = match gate_tree::reserve(&ctx, &tx, &req, FleetSeconds(0)).unwrap() {
            GateTree::Reserved { job, .. } => job,
            other => panic!("expected a reserved gate tree, got {other:?}"),
        };
        tx.commit().unwrap();
        let path = gate_tree::path(&self.repo, FEATURE);
        git(
            &self.repo,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                &path.to_string_lossy(),
                &tip,
            ],
        );
        let json = r#"{"passed":true,"verdict":true,"text":"made"}"#;
        self.end_job(job, "passed", json);
        let ended = jobs_store::get(&self.conn, job).unwrap().unwrap();
        let tx = self.conn.unchecked_transaction().unwrap();
        let seen = create::observe(&ctx, &tx, &ended).unwrap();
        tx.commit().unwrap();
        assert!(matches!(seen, Observed::Created(_)), "{seen:?}");
    }

    /// Commit `file` with `text` on `branch`, made from the feature branch
    /// when it is new.
    fn commit_on(&self, branch: &str, file: &str, text: &str) {
        let exists = git_out(&self.repo, &["rev-parse", "--verify", "-q", branch])
            .status
            .success();
        if exists {
            git(&self.repo, &["checkout", "-q", branch]);
        } else {
            git(
                &self.repo,
                &["checkout", "-q", "-b", branch, FEATURE_BRANCH],
            );
        }
        std::fs::write(self.repo.join(file), text).unwrap();
        git(&self.repo, &["add", file]);
        git(&self.repo, &["commit", "-q", "-m", branch]);
        git(&self.repo, &["checkout", "-q", "main"]);
    }

    fn gateway(&self) -> CliGitGateway {
        CliGitGateway::new(
            CliWorktreeGit::new("git", self.root.join("fleet")).writing(&[self.repo.join(".git")]),
        )
    }

    /// A task, its git branch `name` with 1 commit of its own file, and its
    /// task branch row moved to `ready`, as the landing moves it.
    fn ready_branch(&mut self, name: &str) {
        self.commit_on(name, &format!("branch-{name}.txt"), &format!("{name}\n"));
        self.tasks += 1;
        let n = self.tasks;
        let new = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), "dev");
        let run = self.run.clone();
        let config = self.config.clone();
        let tx = self.conn.transaction().unwrap();
        let task = tasks::add(&tx, &run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        let row = NewBranch {
            name,
            feature_id: FEATURE,
            task_id: Some(&task),
            owner: "worker",
            due_at: 0,
        };
        rows::insert_branch(&tx, None, &row, cause("task bound")).unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        for to in [LandingState::Verifying, LandingState::Ready] {
            landing::enter(&ctx, &tx, name, to, FleetSeconds(0), "its task is done").unwrap();
        }
        tx.commit().unwrap();
    }

    /// 1 pass of bundles at the next second of the fleet clock, committed.
    fn pass(&mut self) {
        self.clock += 1;
        let config = self.config.clone();
        let gw = self.gateway();
        let tx = self.conn.transaction().unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let b = Bundles {
            jobs: &ctx,
            git: &gw,
        };
        bundles::advance(&b, &tx, FleetSeconds(self.clock)).unwrap();
        tx.commit().unwrap();
    }

    fn bundle_ids(&self) -> Vec<i64> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM bundles ORDER BY id")
            .unwrap();
        let ids = stmt.query_map([], |r| r.get(0)).unwrap();
        ids.collect::<rusqlite::Result<_>>().unwrap()
    }

    fn bundle(&self, id: i64) -> BundleRow {
        rows::bundle(&self.conn, id).unwrap().unwrap()
    }

    fn children(&self, parent: i64) -> Vec<BundleRow> {
        self.bundle_ids()
            .into_iter()
            .map(|id| self.bundle(id))
            .filter(|b| b.parent_id == Some(parent))
            .collect()
    }

    fn landing_state(&self, branch: &str) -> String {
        rows::branch(&self.conn, branch)
            .unwrap()
            .unwrap()
            .landing_state
            .unwrap()
    }

    /// Run the `bundle_build` job of the `building` bundle `id` as its job
    /// process runs it, and end its row with the result.
    fn build(&self, id: i64) {
        let bundle = self.bundle(id);
        assert_eq!(bundle.state, "building", "bundle {id}");
        let job = bundle.job_id.unwrap();
        let row = jobs_store::get(&self.conn, job).unwrap().unwrap();
        let ran = exec_bundle::run(&self.conn, &row).unwrap();
        assert!(ran.outcome.passed, "the build of bundle {id}");
        let json = serde_json::json!({
            "passed": true,
            "verdict": true,
            "text": ran.outcome.text,
            "data": ran.data,
        });
        self.end_job(job, "passed", &json.to_string());
    }

    /// The fast gate of the `gating` bundle `id` ends with the verdict
    /// `passed`, as its job process records it.
    fn gate(&self, id: i64, passed: bool) {
        let bundle = self.bundle(id);
        assert_eq!(bundle.state, "gating", "bundle {id}");
        let job = bundle.job_id.unwrap();
        let tip = bundle.candidate_tip.as_deref().unwrap();
        let run = rows::gate_run_of(&self.conn, FEATURE, "fast", tip, &bundle.base_tip)
            .unwrap()
            .unwrap();
        let result = if passed { "pass" } else { "fail" };
        let tx = self.conn.unchecked_transaction().unwrap();
        rows::set_gate_result(&tx, None, run.id, job, result, cause("the gate ran")).unwrap();
        tx.commit().unwrap();
        let json = serde_json::json!({
            "passed": passed,
            "verdict": true,
            "text": format!("gate of bundle {id}: {result}; output in gate-output/{job}.log"),
        });
        let state = if passed { "passed" } else { "failed" };
        self.end_job(job, state, &json.to_string());
    }

    fn end_job(&self, id: i64, state: &str, result_json: &str) {
        self.conn
            .execute(
                "UPDATE jobs SET state = ?2, result_json = ?3 WHERE id = ?1",
                params![id, state, result_json],
            )
            .unwrap();
    }

    /// Build and gate the `building` bundle `id` to the `gating` state.
    fn build_and_gate(&mut self, id: i64) {
        self.build(id);
        self.pass();
        assert_eq!(self.bundle(id).state, "gating");
    }

    /// The open bundles (`building`, `gating`, `passed`) that hold `branch`.
    fn open_bundles_of(&self, branch: &str) -> Vec<i64> {
        self.bundle_ids()
            .into_iter()
            .map(|id| self.bundle(id))
            .filter(|b| matches!(b.state.as_str(), "building" | "gating" | "passed"))
            .filter(|b| b.members.iter().any(|m| m == branch))
            .map(|b| b.id)
            .collect()
    }

    /// Each of `members` is in exactly 1 place: 1 open bundle while it is
    /// `in_bundle`, else `gate_failed` or `ready` and in no open bundle.
    fn assert_each_member_in_1_place(&self, members: &[&str]) {
        for m in members {
            let open = self.open_bundles_of(m);
            let state = self.landing_state(m);
            match state.as_str() {
                "in_bundle" => assert_eq!(open.len(), 1, "{m} is in bundles {open:?}"),
                "gate_failed" | "ready" => {
                    assert!(open.is_empty(), "{m} is {state} and in bundles {open:?}")
                }
                other => panic!("{m} is {other}"),
            }
        }
    }
}

/// A world with the ready branches `names`, put in 1 bundle and taken to a
/// failed fast gate. Gives the id of that bundle.
fn failed_bundle_of(names: &[&str]) -> (World, i64) {
    let mut w = world();
    for name in names {
        w.ready_branch(name);
    }
    w.pass();
    let ids = w.bundle_ids();
    assert_eq!(ids.len(), 1);
    let id = ids[0];
    w.build_and_gate(id);
    w.gate(id, false);
    w.pass();
    (w, id)
}

fn members_of(b: &BundleRow) -> Vec<&str> {
    b.members.iter().map(String::as_str).collect()
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn failed_bundle_of_4_makes_2_children_of_2() {
    let (w, id) = failed_bundle_of(&["a", "b", "c", "d"]);
    let parent = w.bundle(id);
    assert_eq!(parent.state, "failed");
    let children = w.children(id);
    assert_eq!(children.len(), 2, "children: {children:?}");
    assert_eq!(members_of(&children[0]), ["a", "b"]);
    assert_eq!(members_of(&children[1]), ["c", "d"]);
    for child in &children {
        assert_eq!(child.state, "building");
        assert_eq!(child.base_tip, parent.base_tip);
        assert!(child.job_id.is_some(), "child {} has a build job", child.id);
        assert_ne!(child.job_id, parent.job_id);
    }
    for m in ["a", "b", "c", "d"] {
        assert_eq!(w.landing_state(m), "in_bundle", "{m}");
    }
}

#[test]
fn child_of_1_that_fails_sends_its_branch_to_gate_failed() {
    let (mut w, id) = failed_bundle_of(&["a", "b"]);
    let children = w.children(id);
    assert_eq!(children.len(), 2);
    let (left, right) = (children[0].id, children[1].id);
    assert_eq!(members_of(&children[0]), ["a"]);
    w.build(left);
    w.build(right);
    w.pass();
    let gate_job = w.bundle(left).job_id.unwrap();
    w.gate(left, false);
    w.pass();
    assert_eq!(w.bundle(left).state, "failed");
    assert!(w.children(left).is_empty(), "a bundle of 1 is not split");
    assert_eq!(w.landing_state("a"), "gate_failed");
    assert_eq!(w.landing_state("b"), "in_bundle");
    // The gate output of the failed run goes with the branch.
    assert_eq!(
        bundles::failed_gate(&w.conn, "a").unwrap(),
        Some(FailedGate {
            bundle_id: left,
            job_id: gate_job,
        })
    );
    assert_eq!(bundles::failed_gate(&w.conn, "b").unwrap(), None);
}

#[test]
fn clean_child_lands_its_members() {
    let (mut w, id) = failed_bundle_of(&["a", "b", "c"]);
    let children = w.children(id);
    let (left, right) = (children[0].id, children[1].id);
    assert_eq!(members_of(&children[1]), ["b", "c"]);
    w.build(left);
    w.build(right);
    w.pass();
    w.gate(right, true);
    w.pass();
    // A clean child is not split again: it goes to `passed`, and the ref
    // move of ticket 31 lands its members.
    assert_eq!(w.bundle(right).state, "passed");
    let limit = i64::try_from(w.config.bundle_passed_limit_s).unwrap();
    assert_eq!(
        w.bundle(right).due_at,
        w.clock + limit,
        "the limit of passed"
    );
    assert!(w.children(right).is_empty());
    for m in ["b", "c"] {
        assert_eq!(w.landing_state(m), "in_bundle", "{m}");
        assert_eq!(w.open_bundles_of(m), [right]);
    }
    assert_eq!(bundles::failed_gate(&w.conn, "b").unwrap(), None);
}

#[test]
fn each_member_ends_in_1_place() {
    let names = ["a", "b", "c", "d", "e"];
    let (mut w, id) = failed_bundle_of(&names);
    w.assert_each_member_in_1_place(&names);
    // [a b] fails and splits; [c d e] passes.
    let children = w.children(id);
    let (left, right) = (children[0].id, children[1].id);
    w.build(left);
    w.build(right);
    w.pass();
    w.gate(left, false);
    w.gate(right, true);
    w.pass();
    w.assert_each_member_in_1_place(&names);
    // [a] fails and is found; [b] is dropped by a moved feature tip and
    // goes back to `ready`.
    let grand = w.children(left);
    assert_eq!(grand.len(), 2);
    let (ga, gb) = (grand[0].id, grand[1].id);
    w.build(ga);
    w.build(gb);
    w.pass();
    w.gate(ga, false);
    w.pass();
    w.assert_each_member_in_1_place(&names);
    w.commit_on(FEATURE_BRANCH, "moved.txt", "moved\n");
    w.pass();
    w.assert_each_member_in_1_place(&names);
    assert_eq!(w.landing_state("a"), "gate_failed");
    assert_eq!(w.landing_state("b"), "ready");
    assert_eq!(w.bundle(gb).state, "dropped");
}
