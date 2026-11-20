//! The judge branch in a fix round (docs/tickets/fleetd-p6/42-judge-branch.md):
//! when the fix attempt of the judged task passes its structural check,
//! fleetd writes a rescue ref of the judge branch (I10), then moves the
//! judge branch by compare-and-set on `seen_tip` (I9) onto 1 new commit:
//! its verdict file on top of the new judged tip. So the next judge reads
//! the change (RR-3), and no tip is lost (docs/specs/fleetd/design.md,
//! "The judge pair", step 6).
//!
//! Each test makes its own ledger and git repository in a temporary folder.
//! No test touches this repository, and no test reads the clock, the load or
//! the limits of the machine. A pass is [`judge_branch::follow`] in 1
//! transaction; each effect then runs after the commit, and its observation
//! is the next transaction. A crash is a test that skips a step.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewBranch, NewFeature};
use horch_core::fleetd::integration::application::judge_branch::{
    self, Done, Effect, GitJudgeBranchRefs, JudgeBranches,
};
use horch_core::fleetd::task_execution::adapters::verdict_file;
use horch_core::fleetd::task_execution::domain::structural;
use horch_core::fleetd::task_execution::domain::verification::{Decision, Verdict};
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::schema;
use horch_core::taskledger::tasks::{self, NewTask};
use rusqlite::{params, Connection};

const FEATURE: &str = "F-1";
const FEATURE_BRANCH: &str = "fleet/r1";
const JUDGED_BRANCH: &str = "fleet/r1-T1-judged";
const JUDGE_BRANCH: &str = "fleet/r1-T2-judge";
const DOC: &str = "docs/sync.md";
const VERDICT: &str = "docs/sync.review.md";
const VERDICT_TEXT: &str = "## Verdict\n\ncritical_open\n\n## Findings\n\n- [CRITICAL] x\n";

/// The folder of every rescue ref of a branch.
const RESCUES: &str = "refs/horch/rescue/branch/";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    root: PathBuf,
    repo: PathBuf,
    judged: String,
    /// The tip of the judged branch that the first judge judged.
    first_judged_tip: String,
    /// The tip of the judge branch with the verdict of the first judge.
    first_judge_tip: String,
    /// The tip of the judged branch after its fix attempt.
    fixed_tip: String,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// A judge pair in its first fix round. In git: the feature branch, the
/// judged branch with its document, the judge branch with its verdict file
/// on the first judged tip, and the fix commit on the judged branch. In the
/// ledger: the judged task in `verifying` with 2 passed structural checks
/// (the first tip, then the fixed tip), and the judge task in `pending`.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut conn = Connection::open(root.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    std::fs::create_dir_all(root.join("fleet")).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    commit(&repo, "a.txt", "a\n");
    git(&repo, &["checkout", "-q", "-b", FEATURE_BRANCH]);
    git(&repo, &["checkout", "-q", "-b", JUDGED_BRANCH]);
    let first_judged_tip = commit(&repo, DOC, "# Sync\n");
    git(&repo, &["checkout", "-q", "-b", JUDGE_BRANCH]);
    let first_judge_tip = commit(&repo, VERDICT, VERDICT_TEXT);
    git(&repo, &["checkout", "-q", JUDGED_BRANCH]);
    let fixed_tip = commit(&repo, DOC, "# Sync\n\nThe fix.\n");
    git(&repo, &["checkout", "-q", "main"]);

    let tx = conn.transaction().unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        run_branch: Some(FEATURE_BRANCH.into()),
        ..Default::default()
    };
    let run = runs::create(&tx, &repo, &new_run).unwrap();
    let feature = NewFeature {
        id: FEATURE,
        name: Some("r1"),
        branch: Some(FEATURE_BRANCH),
        primary_branch: "main",
    };
    rows::insert_feature(&tx, None, &feature, cause("the run starts")).unwrap();
    // FR-12: a task needs an idea in flight of the feature of its run.
    horch_core::taskledger::schema::run_idea(
        &tx,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    let judged = tasks::add(
        &tx,
        &run,
        &NewTask::work("judged", "t.md", "opus"),
        Actor::Orchestrator,
    )
    .unwrap()
    .task_id;
    let judge = tasks::add(
        &tx,
        &run,
        &NewTask::work("judge", "j.md", "codex"),
        Actor::Orchestrator,
    )
    .unwrap()
    .task_id;
    tx.execute(
        "UPDATE tasks SET status = 'verifying', fix_rounds = 1 WHERE task_id = ?1",
        params![judged],
    )
    .unwrap();
    tx.execute(
        "UPDATE tasks SET status = 'pending', parent_task_id = ?2,
             spec_json = '{\"kind\":\"judge\"}'
         WHERE task_id = ?1",
        params![judge, judged],
    )
    .unwrap();
    for (name, task) in [(JUDGED_BRANCH, &judged), (JUDGE_BRANCH, &judge)] {
        let branch = NewBranch {
            name,
            feature_id: FEATURE,
            task_id: Some(task),
            owner: "worker",
            due_at: 0,
        };
        rows::insert_branch(&tx, None, &branch, cause("the attempt is bound")).unwrap();
    }
    for tip in [&first_judged_tip, &fixed_tip] {
        passed(&tx, &judged, tip);
    }
    tx.commit().unwrap();
    World {
        _dir: dir,
        conn,
        config: FleetConfig::default(),
        root,
        repo,
        judged,
        first_judged_tip,
        first_judge_tip,
        fixed_tip,
    }
}

/// Record a passed structural check of `task` at `tip`, as the verifier
/// records it.
fn passed(tx: &Connection, task: &str, tip: &str) {
    let v = Verdict {
        task_id: task.into(),
        attempt: format!("{task}-a"),
        n: 0,
        decision: Decision::Passed,
        tip: tip.into(),
        check_hash: String::new(),
        job: None,
        reason: format!("{}: 1 output", structural::PASSED),
    };
    assert!(structural::is_pass(&v));
    verdict_file::record(tx, None, v).unwrap();
}

/// Run git in `dir` for the test itself, with no config of the user.
fn git_out(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

/// [`git_out`], trimmed. A failed call fails the test.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_out(dir, args);
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Commit `file` with `text` on the branch of `repo`, and give the tip.
fn commit(repo: &Path, file: &str, text: &str) -> String {
    let path = repo.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    git(repo, &["add", file]);
    git(repo, &["commit", "-q", "-m", file]);
    git(repo, &["rev-parse", "HEAD"])
}

impl World {
    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    fn fleet_git(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.root.join("fleet"))
            .reading(std::slice::from_ref(&self.repo))
            .writing(&[self.repo.join(".git")])
    }

    fn gateway(&self) -> CliGitGateway {
        CliGitGateway::new(self.fleet_git())
    }

    fn refs(&self) -> GitJudgeBranchRefs {
        GitJudgeBranchRefs::new(self.fleet_git(), self.root.join("fleet/tmp/judge-branch"))
    }

    /// 1 pass: [`judge_branch::follow`] in 1 transaction.
    fn pass(&mut self) -> Vec<Effect> {
        let git = self.gateway();
        let refs = self.refs();
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let j = JudgeBranches {
            jobs: &ctx,
            git: &git,
            refs: &refs,
        };
        let tx = self.conn.transaction().unwrap();
        let effects = judge_branch::follow(&j, &tx).unwrap();
        tx.commit().unwrap();
        effects
    }

    /// Run `effect` after the commit of its pass.
    fn run(&self, effect: &Effect) -> Done {
        judge_branch::run(effect, &self.gateway(), &self.refs()).unwrap()
    }

    /// Record `done` in 1 transaction.
    fn observe(&mut self, done: &Done) {
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let tx = self.conn.transaction().unwrap();
        judge_branch::observe(&ctx, &tx, done).unwrap();
        tx.commit().unwrap();
    }

    /// Passes until a pass gives no effect, each effect run and observed.
    fn settle(&mut self) {
        for _ in 0..5 {
            let effects = self.pass();
            if effects.is_empty() {
                return;
            }
            for effect in &effects {
                let done = self.run(effect);
                self.observe(&done);
            }
        }
        panic!("the judge branch does not settle in 5 passes");
    }

    /// Each rescue ref of a branch, as (name, tip).
    fn rescues(&self) -> Vec<(String, String)> {
        let out = git(
            &self.repo,
            &["for-each-ref", "--format=%(refname) %(objectname)", RESCUES],
        );
        out.lines()
            .filter_map(|l| l.split_once(' '))
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect()
    }

    fn row(&self, branch: &str) -> rows::BranchRow {
        rows::branch(&self.conn, branch).unwrap().unwrap()
    }

    fn is_ancestor(&self, a: &str, b: &str) -> bool {
        git_out(&self.repo, &["merge-base", "--is-ancestor", a, b])
            .status
            .success()
    }

    fn show(&self, rev: &str, file: &str) -> String {
        git(&self.repo, &["show", &format!("{rev}:{file}")])
    }
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn judge_branch_has_a_rescue_ref_before_its_move() {
    let mut w = world();
    let old = w.first_judge_tip.clone();

    // The first pass gives only the rescue: the branch does not move.
    let effects = w.pass();
    assert_eq!(
        effects,
        vec![Effect::Rescue {
            repo: w.repo.clone(),
            branch: JUDGE_BRANCH.into(),
            tip: old.clone(),
        }]
    );
    assert_eq!(w.row(JUDGE_BRANCH).seen_tip.as_deref(), Some(old.as_str()));
    let done = w.run(&effects[0]);
    assert_eq!(w.tip(JUDGE_BRANCH), old, "a rescue moves no branch");
    let rescues = w.rescues();
    assert_eq!(rescues.len(), 1, "{rescues:?}");
    assert_eq!(rescues[0].1, old);
    w.observe(&done);
    assert_eq!(
        w.row(JUDGE_BRANCH).rescue_ref.as_deref(),
        Some(rescues[0].0.as_str())
    );

    // With the rescue ref recorded, the next pass gives the move.
    let effects = w.pass();
    assert_eq!(
        effects,
        vec![Effect::Move {
            repo: w.repo.clone(),
            branch: JUDGE_BRANCH.into(),
            from: old.clone(),
            base: w.first_judged_tip.clone(),
            onto: w.fixed_tip.clone(),
        }]
    );
    let done = w.run(&effects[0]);
    assert!(matches!(done, Done::Moved { .. }), "{done:?}");
    // The rescue ref still holds the old tip after the move.
    assert_eq!(w.rescues(), rescues);
}

#[test]
fn judge_branch_moves_onto_the_new_judged_tip() {
    let mut w = world();
    w.settle();

    let tip = w.tip(JUDGE_BRANCH);
    // 1 commit on top of the new judged tip.
    assert_eq!(w.tip(&format!("{tip}^")), w.fixed_tip);
    assert_eq!(
        git(
            &w.repo,
            &["rev-list", "--count", &format!("{}..{tip}", w.fixed_tip)]
        ),
        "1"
    );
    // It holds the fixed document and the verdict file, and nothing else
    // changes on top of the judged tip.
    assert_eq!(w.show(&tip, DOC), "# Sync\n\nThe fix.");
    assert_eq!(w.show(&tip, VERDICT), VERDICT_TEXT.trim_end());
    assert_eq!(
        git(&w.repo, &["diff", "--name-only", &w.fixed_tip, &tip]),
        VERDICT
    );
    // fleetd saw the new tip, and a pass finds nothing more to do.
    assert_eq!(w.row(JUDGE_BRANCH).seen_tip.as_deref(), Some(tip.as_str()));
    assert!(w.pass().is_empty());
    // The judged branch did not move.
    assert_eq!(w.tip(JUDGED_BRANCH), w.fixed_tip);
    assert_eq!(w.judged, w.row(JUDGED_BRANCH).task_id.unwrap());
}

#[test]
fn crash_between_rescue_and_move_loses_no_tip() {
    let mut w = world();
    let old = w.first_judge_tip.clone();

    // A crash after the rescue ref is written and before it is recorded.
    let effects = w.pass();
    assert!(
        matches!(effects[..], [Effect::Rescue { .. }]),
        "{effects:?}"
    );
    let first = w.run(&effects[0]);
    assert_eq!(w.tip(JUDGE_BRANCH), old);

    // The next pass gives the rescue again, and git keeps the same 1 ref.
    let effects = w.pass();
    assert!(
        matches!(effects[..], [Effect::Rescue { .. }]),
        "{effects:?}"
    );
    let again = w.run(&effects[0]);
    assert_eq!(again, first);
    assert_eq!(w.rescues().len(), 1);
    w.observe(&again);

    // A crash after the move and before it is recorded.
    let effects = w.pass();
    assert!(matches!(effects[..], [Effect::Move { .. }]), "{effects:?}");
    let moved = w.run(&effects[0]);
    let Done::Moved { tip, .. } = &moved else {
        panic!("{moved:?}");
    };
    assert_eq!(&w.tip(JUDGE_BRANCH), tip);

    // The next pass sees the move done: it records the tip and moves
    // nothing more.
    assert!(w.pass().is_empty());
    assert_eq!(w.row(JUDGE_BRANCH).seen_tip.as_deref(), Some(tip.as_str()));
    assert_eq!(&w.tip(JUDGE_BRANCH), tip);

    // No tip is lost: the old tip is on its rescue ref, and the new tip
    // holds the fixed judged tip.
    let rescues = w.rescues();
    assert_eq!(rescues.len(), 1);
    assert_eq!(rescues[0].1, old);
    assert!(w.is_ancestor(&w.fixed_tip, tip));
}

#[test]
fn move_with_a_stale_seen_tip_moves_nothing() {
    let mut w = world();
    let effects = w.pass();
    let done = w.run(&effects[0]);
    w.observe(&done);
    let effects = w.pass();
    assert!(matches!(effects[..], [Effect::Move { .. }]), "{effects:?}");

    // After the pass, the judge branch gets a commit that fleetd did not see.
    git(&w.repo, &["checkout", "-q", JUDGE_BRANCH]);
    let other = commit(&w.repo, "late.txt", "late\n");
    git(&w.repo, &["checkout", "-q", "main"]);

    let done = w.run(&effects[0]);
    assert_eq!(
        done,
        Done::Mismatch {
            branch: JUDGE_BRANCH.into(),
            actual: Some(other.clone()),
        }
    );
    assert_eq!(w.tip(JUDGE_BRANCH), other, "the branch did not move");
    w.observe(&done);
    assert_eq!(
        w.row(JUDGE_BRANCH).seen_tip.as_deref(),
        Some(other.as_str())
    );

    // The next pass decides again: first a rescue ref of the new tip.
    let effects = w.pass();
    assert_eq!(
        effects,
        vec![Effect::Rescue {
            repo: w.repo.clone(),
            branch: JUDGE_BRANCH.into(),
            tip: other.clone(),
        }]
    );
    assert_eq!(w.tip(JUDGE_BRANCH), other);
}

#[test]
fn judge_branch_holds_the_new_judged_tip_only_after_its_move() {
    let mut w = world();
    let judge_id = w.row(JUDGE_BRANCH).task_id.unwrap();
    let holds = |w: &World| {
        let judge = tasks::get(&w.conn, &judge_id).unwrap();
        judge_branch::holds_judged_tip(&w.gateway(), &w.refs(), &w.conn, &judge).unwrap()
    };
    assert!(!holds(&w), "the judge branch is on the first judged tip");
    w.settle();
    assert!(holds(&w), "the judge branch moved onto the fixed tip");
}
