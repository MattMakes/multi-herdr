use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::Kind;
use horch_core::fleetd::ideas::application::needs_you::{open_keys, NeedsYouList};
use horch_core::fleetd::integration::adapters::exec_bundle::{self, Build};
use horch_core::fleetd::integration::adapters::exec_gate::gate_key;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, BundleRow, NewBranch, NewBundle, NewFeature,
};
use horch_core::fleetd::integration::application::bundles::{
    self, no_tip_key, overdue_key, Bundles, NO_TIP_KIND, OVERDUE_KIND,
};
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

struct World {
    _temp: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
    tasks: u32,
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

fn open_ledger(root: &Path) -> Connection {
    let mut conn = Connection::open(root.join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn
}

fn world() -> World {
    let mut w = bare_world();
    w.gate_tree();
    w
}

fn bare_world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let root = horch_platform::path::for_child(&temp.path().canonicalize().unwrap());
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["branch", FEATURE_BRANCH]);
    let mut conn = open_ledger(&root);
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
    horch_core::taskledger::schema::run_idea(
        &tx,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    tx.commit().unwrap();
    World {
        _temp: temp,
        root,
        repo,
        conn,
        run,
        config: FleetConfig::default(),
        tasks: 0,
    }
}

impl World {
    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    fn commit_on(&self, branch: &str, file: &str, text: &str) -> String {
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
        let tip = self.tip(branch);
        git(&self.repo, &["checkout", "-q", "main"]);
        tip
    }

    fn gateway(&self) -> CliGitGateway {
        CliGitGateway::new(
            CliWorktreeGit::new("git", self.root.join("fleet")).writing(&[self.repo.join(".git")]),
        )
    }

    fn ready_branch(&mut self, name: &str, file: &str) {
        self.commit_on(name, file, &format!("{name}\n"));
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

    fn pass(&mut self, clock: i64) {
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
        bundles::advance(&b, &tx, FleetSeconds(clock)).unwrap();
        tx.commit().unwrap();
    }

    fn pass_then_crash_before_the_commit(&mut self, clock: i64) {
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
        bundles::advance(&b, &tx, FleetSeconds(clock)).unwrap();
        drop(tx);
    }

    fn restart(&mut self) {
        self.conn = open_ledger(&self.root);
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

    fn only_bundle(&self) -> BundleRow {
        let ids = self.bundle_ids();
        assert_eq!(ids.len(), 1, "bundles: {ids:?}");
        self.bundle(ids[0])
    }

    fn landing_state(&self, branch: &str) -> String {
        rows::branch(&self.conn, branch)
            .unwrap()
            .unwrap()
            .landing_state
            .unwrap()
    }

    fn jobs_of(&self, kind: Kind) -> Vec<JobRow> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM jobs WHERE kind = ?1 ORDER BY id")
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([kind.as_str()], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        ids.into_iter()
            .map(|id| jobs_store::get(&self.conn, id).unwrap().unwrap())
            .collect()
    }

    fn run_build(&self, id: i64) -> Option<Build> {
        let row = jobs_store::get(&self.conn, id).unwrap().unwrap();
        let ran = exec_bundle::run(&self.conn, &row).unwrap();
        let state = if ran.outcome.passed {
            "passed"
        } else {
            "failed"
        };
        let json = serde_json::json!({
            "passed": ran.outcome.passed,
            "verdict": ran.outcome.verdict,
            "text": ran.outcome.text,
            "data": ran.data,
        });
        self.end_job(id, state, Some(&json.to_string()));
        ran.data.map(|d| serde_json::from_value(d).unwrap())
    }

    fn gate_tree(&mut self) {
        let job = self.reserve_gate_tree();
        self.make_gate_tree(job);
    }

    fn make_gate_tree(&mut self, job: i64) {
        let path = gate_tree::path(&self.repo, FEATURE);
        let tip = self.tip(FEATURE_BRANCH);
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
        self.end_job(job, "passed", Some(json));
        let ended = jobs_store::get(&self.conn, job).unwrap().unwrap();
        let config = self.config.clone();
        let tx = self.conn.transaction().unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let seen = create::observe(&ctx, &tx, &ended).unwrap();
        tx.commit().unwrap();
        assert!(matches!(seen, Observed::Created(_)), "{seen:?}");
    }

    fn reserve_gate_tree(&mut self) -> i64 {
        let repo = self.repo.clone();
        let tip = self.tip(FEATURE_BRANCH);
        let run_id = self.run.run_id.clone();
        let req = GateTreeRequest {
            feature_id: FEATURE,
            run_id: &run_id,
            repo: &repo,
            base: FEATURE_BRANCH,
            tip: &tip,
        };
        let config = self.config.clone();
        let tx = self.conn.transaction().unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let reserved = gate_tree::reserve(&ctx, &tx, &req, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        match reserved {
            GateTree::Reserved { job, .. } => job,
            other => panic!("expected a reserved gate tree, got {other:?}"),
        }
    }

    fn end_job(&self, id: i64, state: &str, result_json: Option<&str>) {
        self.conn
            .execute(
                "UPDATE jobs SET state = ?2, result_json = ?3 WHERE id = ?1",
                params![id, state, result_json],
            )
            .unwrap();
    }

    fn overdue_items(&self) -> Vec<String> {
        open_keys(&self.conn, OVERDUE_KIND).unwrap()
    }

    fn build_s(&self) -> i64 {
        (self.config.job_limit_s.bundle_build + self.config.landing_job_margin_s) as i64
    }

    fn gate_s(&self) -> i64 {
        (self.config.job_limit_s.fast_gate + self.config.landing_job_margin_s) as i64
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn three_ready() -> World {
    let mut w = world();
    w.ready_branch("fleet/r1-a", "a1.txt");
    w.ready_branch("fleet/r1-b", "b1.txt");
    w.ready_branch("fleet/r1-c", "c1.txt");
    w
}

const ABC: [&str; 3] = ["fleet/r1-a", "fleet/r1-b", "fleet/r1-c"];

#[test]
fn given_3_ready_branches_1_bundle_row_and_3_members_in_1_transaction() {
    let mut w = three_ready();
    let base = w.tip(FEATURE_BRANCH);

    w.pass_then_crash_before_the_commit(100);
    assert!(w.bundle_ids().is_empty());
    assert!(w.jobs_of(Kind::BundleBuild).is_empty());
    for b in ABC {
        assert_eq!(w.landing_state(b), "ready");
    }

    w.pass(100);
    let bundle = w.only_bundle();
    assert_eq!(bundle.state, "building");
    assert_eq!(bundle.feature_id, FEATURE);
    assert_eq!(bundle.base_tip, base);
    assert_eq!(bundle.parent_id, None);
    assert_eq!(bundle.members, names(&ABC));
    assert_eq!(bundle.due_at, 100 + w.build_s());
    for b in ABC {
        assert_eq!(w.landing_state(b), "in_bundle");
    }
    let builds = w.jobs_of(Kind::BundleBuild);
    assert_eq!(builds.len(), 1);
    let job = &builds[0];
    assert_eq!(job.subject, bundle.id.to_string());
    assert_eq!(job.repo.as_deref(), Some(w.repo.to_str().unwrap()));
    let tips: Vec<String> = ABC.iter().map(|b| w.tip(b)).collect();
    let tips: Vec<&str> = tips.iter().map(String::as_str).collect();
    assert_eq!(job.input_key, exec_bundle::input_key(&base, &tips));
    assert_eq!(bundle.job_id, Some(job.id));
}

#[test]
fn passed_build_queues_1_fast_gate() {
    let mut w = three_ready();
    let base = w.tip(FEATURE_BRANCH);
    w.pass(100);
    let id = w.only_bundle().id;
    let build = w.jobs_of(Kind::BundleBuild)[0].id;
    let Some(Build::Candidate {
        candidate_ref,
        candidate_tip,
    }) = w.run_build(build)
    else {
        panic!("the build of 3 clean members gives a candidate");
    };

    w.pass(200);
    let bundle = w.bundle(id);
    assert_eq!(bundle.state, "gating");
    assert_eq!(
        bundle.candidate_ref.as_deref(),
        Some(candidate_ref.as_str())
    );
    assert_eq!(
        bundle.candidate_tip.as_deref(),
        Some(candidate_tip.as_str())
    );
    assert_eq!(bundle.due_at, 200 + w.gate_s());
    let gates = w.jobs_of(Kind::FastGate);
    assert_eq!(gates.len(), 1);
    assert_eq!(gates[0].subject, id.to_string());
    assert_eq!(gates[0].input_key, gate_key(&candidate_tip, &base));
    assert_eq!(bundle.job_id, Some(gates[0].id));
    for b in ABC {
        assert_eq!(w.landing_state(b), "in_bundle");
    }

    w.pass(300);
    w.pass(400);
    assert_eq!(w.jobs_of(Kind::FastGate).len(), 1);
    assert_eq!(w.jobs_of(Kind::BundleBuild).len(), 1);
    assert_eq!(w.only_bundle().state, "gating");
}

#[test]
fn fast_gate_waits_for_the_gate_tree_of_the_feature() {
    let mut w = bare_world();
    w.ready_branch("fleet/r1-a", "a1.txt");
    w.pass(100);
    let id = w.only_bundle().id;
    let build = w.jobs_of(Kind::BundleBuild)[0].id;
    assert!(matches!(w.run_build(build), Some(Build::Candidate { .. })));

    w.pass(200);
    assert_eq!(w.bundle(id).state, "gating");
    assert!(
        w.jobs_of(Kind::FastGate).is_empty(),
        "no gate tree, no gate"
    );
    let adds = w.jobs_of(Kind::WorktreeAdd);
    assert_eq!(adds.len(), 1, "the gate tree is reserved: {adds:?}");
    let tree = gate_tree::of_feature(&w.conn, &w.repo, FEATURE)
        .unwrap()
        .expect("the reserved row of the gate tree");
    assert_eq!(adds[0].subject, tree.worktree_id);

    w.pass(300);
    assert_eq!(w.jobs_of(Kind::WorktreeAdd).len(), 1);
    assert!(w.jobs_of(Kind::FastGate).is_empty());

    w.make_gate_tree(adds[0].id);
    w.pass(400);
    let gates = w.jobs_of(Kind::FastGate);
    assert_eq!(gates.len(), 1);
    assert_eq!(w.bundle(id).job_id, Some(gates[0].id));
}

#[test]
fn conflict_sends_the_member_to_conflicted_and_the_others_to_ready() {
    let mut w = world();
    w.ready_branch("fleet/r1-a", "same.txt");
    w.ready_branch("fleet/r1-b", "same.txt");
    w.ready_branch("fleet/r1-c", "c1.txt");
    w.pass(100);
    let id = w.only_bundle().id;
    let build = w.jobs_of(Kind::BundleBuild)[0].id;
    let Some(Build::Conflict { member, .. }) = w.run_build(build) else {
        panic!("a and b change 1 file in 2 ways");
    };
    assert_eq!(member, "fleet/r1-b");

    w.pass(200);
    assert_eq!(w.bundle(id).state, "dropped");
    assert_eq!(w.landing_state("fleet/r1-a"), "ready");
    assert_eq!(w.landing_state("fleet/r1-b"), "conflicted");
    assert_eq!(w.landing_state("fleet/r1-c"), "ready");
    assert!(w.jobs_of(Kind::FastGate).is_empty());

    w.pass(300);
    let ids = w.bundle_ids();
    assert_eq!(ids.len(), 2);
    let next = w.bundle(ids[1]);
    assert_eq!(next.state, "building");
    assert_eq!(next.members, names(&["fleet/r1-a", "fleet/r1-c"]));
}

#[test]
fn moved_feature_tip_drops_a_building_bundle() {
    let mut w = three_ready();
    w.pass(100);
    let id = w.only_bundle().id;
    let moved = w.commit_on(FEATURE_BRANCH, "f.txt", "moved\n");

    w.pass(200);
    assert_eq!(w.bundle(id).state, "dropped");
    for b in ABC {
        assert_eq!(w.landing_state(b), "ready");
    }

    w.pass(300);
    let ids = w.bundle_ids();
    assert_eq!(ids.len(), 2);
    let next = w.bundle(ids[1]);
    assert_eq!(next.base_tip, moved);
    assert_eq!(next.members, names(&ABC));
}

#[test]
fn candidate_written_after_the_drop_is_recorded_for_its_deletion() {
    let mut w = three_ready();
    w.pass(100);
    let id = w.only_bundle().id;
    let build = w.jobs_of(Kind::BundleBuild)[0].id;
    let row = jobs_store::get(&w.conn, build).unwrap().unwrap();
    let ran = exec_bundle::run(&w.conn, &row).unwrap();
    let Some(Build::Candidate {
        candidate_ref,
        candidate_tip,
    }) = ran.data.clone().map(|d| serde_json::from_value(d).unwrap())
    else {
        panic!("the build of 3 clean members gives a candidate");
    };
    w.commit_on(FEATURE_BRANCH, "f.txt", "moved\n");
    w.pass(200);
    assert_eq!(w.bundle(id).state, "dropped");
    assert_eq!(w.bundle(id).candidate_ref, None);
    let json = serde_json::json!({
        "passed": true,
        "verdict": true,
        "text": ran.outcome.text,
        "data": ran.data,
    });
    w.end_job(build, "passed", Some(&json.to_string()));

    w.pass(300);
    let bundle = w.bundle(id);
    assert_eq!(bundle.state, "dropped");
    assert_eq!(
        bundle.candidate_ref.as_deref(),
        Some(candidate_ref.as_str())
    );
    assert_eq!(
        bundle.candidate_tip.as_deref(),
        Some(candidate_tip.as_str())
    );
    assert_eq!(w.tip(&candidate_ref), candidate_tip);
}

#[test]
fn moved_feature_tip_drops_a_gating_bundle() {
    let mut w = three_ready();
    w.pass(100);
    let id = w.only_bundle().id;
    let build = w.jobs_of(Kind::BundleBuild)[0].id;
    w.run_build(build);
    w.pass(200);
    assert_eq!(w.bundle(id).state, "gating");
    w.commit_on(FEATURE_BRANCH, "f.txt", "moved\n");

    w.pass(300);
    assert_eq!(w.bundle(id).state, "dropped");
    for b in ABC {
        assert_eq!(w.landing_state(b), "ready");
    }
}

#[test]
fn fm36_restart_continues_the_bundle_from_its_row() {
    let mut w = three_ready();
    w.pass(100);
    let id = w.only_bundle().id;

    w.restart();
    w.pass(200);
    assert_eq!(w.only_bundle().id, id);
    assert_eq!(w.jobs_of(Kind::BundleBuild).len(), 1);

    let build = w.jobs_of(Kind::BundleBuild)[0].id;
    w.run_build(build);
    w.restart();
    w.pass(300);
    w.restart();
    w.pass(400);
    let bundle = w.only_bundle();
    assert_eq!(bundle.id, id);
    assert_eq!(bundle.state, "gating");
    assert_eq!(w.jobs_of(Kind::BundleBuild).len(), 1);
    assert_eq!(w.jobs_of(Kind::FastGate).len(), 1);
    for b in ABC {
        assert_eq!(w.landing_state(b), "in_bundle");
    }
}

#[test]
fn branch_in_an_open_bundle_is_not_put_in_a_second() {
    let mut w = world();
    w.ready_branch("fleet/r1-a", "a1.txt");
    w.pass(100);
    let first = w.only_bundle();
    w.ready_branch("fleet/r1-d", "d1.txt");

    w.pass(200);
    assert_eq!(w.only_bundle().id, first.id);
    assert_eq!(w.landing_state("fleet/r1-d"), "ready");

    let tx = w.conn.transaction().unwrap();
    let members = ["fleet/r1-a", "fleet/r1-d"];
    let new = NewBundle {
        feature_id: FEATURE,
        base_tip: &first.base_tip,
        parent_id: None,
        members: &members,
        due_at: 0,
    };
    let err = rows::insert_bundle(&tx, None, &new, cause("a second bundle")).unwrap_err();
    assert!(format!("{err:#}").contains("I12"), "{err:#}");
    drop(tx);

    w.commit_on(FEATURE_BRANCH, "f.txt", "moved\n");
    w.pass(300);
    w.pass(400);
    let ids = w.bundle_ids();
    assert_eq!(ids.len(), 2);
    assert_eq!(
        w.bundle(ids[1]).members,
        names(&["fleet/r1-a", "fleet/r1-d"])
    );
}

#[test]
fn bundle_past_its_limit_opens_1_needs_you_item() {
    let mut w = three_ready();
    w.pass(100);
    let id = w.only_bundle().id;
    let due = 100 + w.build_s();

    w.pass(due - 1);
    assert!(w.overdue_items().is_empty());
    w.pass(due);
    w.pass(due + 1);
    assert_eq!(w.overdue_items(), [overdue_key(id)]);

    let build = w.jobs_of(Kind::BundleBuild)[0].id;
    w.run_build(build);
    w.pass(due + 2);
    assert_eq!(w.bundle(id).state, "gating");
    assert!(w.overdue_items().is_empty());

    let gate_due = due + 2 + w.gate_s();
    w.pass(gate_due);
    assert_eq!(w.overdue_items(), [overdue_key(id)]);

    w.commit_on(FEATURE_BRANCH, "f.txt", "moved\n");
    w.pass(gate_due + 1);
    assert_eq!(w.bundle(id).state, "dropped");
    assert!(w.overdue_items().is_empty());
}

#[test]
fn crash_after_the_commit_builds_the_same_bundle_1_time() {
    let mut w = three_ready();
    let base = w.tip(FEATURE_BRANCH);
    w.pass(100);
    let id = w.only_bundle().id;
    w.restart();
    w.pass(110);
    let builds = w.jobs_of(Kind::BundleBuild);
    assert_eq!(builds.len(), 1);

    let first = builds[0].id;
    let row = jobs_store::get(&w.conn, first).unwrap().unwrap();
    exec_bundle::run(&w.conn, &row).unwrap();
    let candidate = w.tip(&exec_bundle::candidate_ref(id));
    w.end_job(first, "abandoned", None);
    w.restart();

    w.pass(120);
    let builds = w.jobs_of(Kind::BundleBuild);
    assert_eq!(builds.len(), 2, "1 new run after the abandoned run");
    assert_eq!(builds[1].subject, id.to_string());
    assert_eq!(builds[1].run, 2);
    assert_eq!(builds[1].input_key, builds[0].input_key);
    assert_eq!(w.only_bundle().state, "building");
    for b in ABC {
        assert_eq!(w.landing_state(b), "in_bundle");
    }

    let Some(Build::Candidate { candidate_tip, .. }) = w.run_build(builds[1].id) else {
        panic!("the second run passes on the ref of the first");
    };
    assert_eq!(candidate_tip, candidate);
    w.pass(130);
    let bundle = w.only_bundle();
    assert_eq!(bundle.state, "gating");
    assert_eq!(bundle.candidate_tip.as_deref(), Some(candidate.as_str()));
    assert_eq!(w.jobs_of(Kind::BundleBuild).len(), 2);
    assert_eq!(w.jobs_of(Kind::FastGate).len(), 1);
    assert_eq!(w.tip(FEATURE_BRANCH), base, "no tip moved");
}

#[test]
fn failed_build_drops_the_bundle_and_its_members_go_back_to_ready() {
    let mut w = three_ready();
    w.pass(100);
    let id = w.only_bundle().id;
    let build = w.jobs_of(Kind::BundleBuild)[0].id;
    let json = r#"{"passed":false,"verdict":true,"text":"git failed"}"#;
    w.end_job(build, "failed", Some(json));

    w.pass(200);
    assert_eq!(w.bundle(id).state, "dropped");
    for b in ABC {
        assert_eq!(w.landing_state(b), "ready");
    }
    assert!(w.jobs_of(Kind::FastGate).is_empty());
}

impl World {
    fn built_candidate(&mut self) -> String {
        self.pass(100);
        let build = self.jobs_of(Kind::BundleBuild)[0].id;
        let Some(Build::Candidate { candidate_tip, .. }) = self.run_build(build) else {
            panic!("the build gives a candidate");
        };
        candidate_tip
    }

    fn merges_past(&self, base: &str, rev: &str) -> Vec<String> {
        let out = git(
            &self.repo,
            &["rev-list", "--merges", &format!("{base}..{rev}")],
        );
        out.lines().map(str::to_string).collect()
    }
}

#[test]
fn member_already_in_the_feature_adds_no_candidate_commit() {
    let mut w = world();
    w.ready_branch("fleet/r1-a", "a1.txt");
    let tip = w.tip("fleet/r1-a");
    git(&w.repo, &["branch", "-f", FEATURE_BRANCH, &tip]);

    let candidate = w.built_candidate();
    assert_eq!(candidate, tip, "the candidate is the feature tip");
    assert_eq!(w.only_bundle().base_tip, tip);
    assert_eq!(w.landing_state("fleet/r1-a"), "in_bundle");
}

#[test]
fn lost_branch_lands_on_its_recorded_tip_else_opens_1_item() {
    let mut w = world();
    w.ready_branch("fleet/r1-a", "a1.txt");
    w.ready_branch("fleet/r1-b", "b1.txt");
    let tip = w.tip("fleet/r1-a");
    git(&w.repo, &["branch", "-f", FEATURE_BRANCH, &tip]);
    let tx = w.conn.transaction().unwrap();
    let fact = BranchFact::SeenTip(&tip);
    rows::set_branch_fact(&tx, None, "fleet/r1-a", fact, cause("fleetd read the tip")).unwrap();
    tx.commit().unwrap();
    git(&w.repo, &["branch", "-D", "fleet/r1-a", "fleet/r1-b"]);

    let candidate = w.built_candidate();
    assert_eq!(candidate, tip);
    assert_eq!(w.landing_state("fleet/r1-a"), "in_bundle");
    assert_eq!(w.landing_state("fleet/r1-b"), "ready");
    let items = open_keys(&w.conn, NO_TIP_KIND).unwrap();
    assert_eq!(items, [no_tip_key("fleet/r1-b")]);

    w.pass(200);
    let items = open_keys(&w.conn, NO_TIP_KIND).unwrap();
    assert_eq!(items, [no_tip_key("fleet/r1-b")]);
}

#[test]
fn normal_member_still_gets_its_candidate_commit() {
    let mut w = world();
    w.ready_branch("fleet/r1-a", "a1.txt");
    w.ready_branch("fleet/r1-b", "b1.txt");
    let a = w.tip("fleet/r1-a");
    git(&w.repo, &["branch", "-f", FEATURE_BRANCH, &a]);

    let candidate = w.built_candidate();
    assert_ne!(candidate, a);
    assert_eq!(w.merges_past(&a, &candidate).len(), 1);
    let b = w.tip("fleet/r1-b");
    assert_eq!(
        git(&w.repo, &["merge-base", "--is-ancestor", &b, &candidate]),
        ""
    );
}
