//! Promotion (phase B5): dataset path safety, the git promotion engine,
//! restart, rollback and round cleanup.
//!
//! Real git touches only temp repos (NFR-07), with an empty
//! `GIT_CONFIG_GLOBAL` and pinned identities and dates. Without git on
//! `PATH` the git tests are skipped, unless `HORCH_REQUIRE_GIT=1`. The
//! validator is a fake whose verdict each test scripts.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use horch_core::competition::cleanup::{CleanupOptions, CleanupOutcome, RoundCleanup};
use horch_core::competition::model::RoundState;
use horch_core::competition::promotion::{
    BranchRef, FaultFired, GitPromotionEngine, PromotionEngine, PromotionPlan, PromotionReceipt,
    PromotionResult, PromotionStrategy, RollbackResult, ABORT_AFTER_PROMOTION_STARTED,
    ABORT_AFTER_RECEIPT, ABORT_AFTER_UPDATE_REF,
};
use horch_core::evaluation::validator::{ValidationReport, Validator};
use horch_core::evaluation::winner::RejectReason;
use horch_core::ids::{ExecutionId, ExperimentId, JudgmentId, RoundId};
use horch_core::measure::digest::sha256_bytes;
use horch_core::measure::digest::Digest;
use horch_core::measure::event::{
    Actor, EventEnvelope, EventKind, OperatorPromote, PromotionIntent, PromotionStarted,
    RoundCreated, WinnerSelected, WorktreeCreated, EVENT_SCHEMA_VERSION,
};
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::projection::{
    CandidateView, JudgeView, Projection, PromotionView, RoundView,
};
use horch_core::measure::recorder::JsonlRecorder;
use horch_core::measure::store::StoreOptions;
use horch_core::measure::NumstatLine;
use horch_core::runtime::fault::Faults;
use horch_core::teacher::TeacherRef;
use horch_core::vcs::git::{CheckoutLocation, CherryPick, GitCli, GitClient, GitIdentity};
use horch_core::vcs::worktree::{FrozenCandidate, WorktreeManager, WorktreeSpec};
use horch_marketplace::git::GitRunner;
use tempfile::TempDir;

// ─── dataset paths ──────────────────────────────────────────────────────────

#[test]
fn dataset_paths_reject_traversal_ids() {
    let paths = DatasetPaths::from_slug(Path::new("/s"), "p");
    let good_exp = ExperimentId::new("e").unwrap();
    let good_round = RoundId::new("r").unwrap();
    // The id types keep accepting legacy ids; the paths refuse them.
    for bad in ["../x", "a/b", ".", "..", "a\\b", "x/../y"] {
        let exp = ExperimentId::new(bad).unwrap();
        let round = RoundId::new(bad).unwrap();
        assert!(paths.experiment_dir(&exp).is_err(), "{bad:?}");
        assert!(paths.manifest(&exp).is_err(), "{bad:?}");
        assert!(paths.default_worktree_root(&exp).is_err(), "{bad:?}");
        assert!(paths.artifacts_dir(&good_exp, &round).is_err(), "{bad:?}");
        assert!(paths.judge_input_dir(&good_exp, &round).is_err(), "{bad:?}");
        assert!(paths.judgement(&round).is_err(), "{bad:?}");
        assert!(paths.promotion(&round).is_err(), "{bad:?}");
        assert!(paths.jobs_dir(&round).is_err(), "{bad:?}");
        assert!(paths.job_dir(&round, 1).is_err(), "{bad:?}");
        assert!(paths.exports_dir(bad).is_err(), "{bad:?}");
        assert!(
            paths.validation_dir(&good_exp, &good_round, bad).is_err(),
            "{bad:?}"
        );
        let err = paths.promotion(&round).unwrap_err().to_string();
        assert!(err.contains("round id"), "{err}");
    }
    // Empty: the id types refuse it, so only the string accessors see it.
    assert!(paths.exports_dir("").is_err());
    assert!(paths.validation_dir(&good_exp, &good_round, "").is_err());
    assert!(paths.exports_dir("a\0b").is_err());
    // Plain ids still work, dots inside a name too.
    let ok = RoundId::new("r.1").unwrap();
    assert_eq!(
        paths.promotion(&ok).unwrap(),
        Path::new("/s/multi-herdr/p/promotions/r.1.json")
    );
}

// ─── fixture ────────────────────────────────────────────────────────────────

const GIT_DATE: &str = "2026-09-28T12:00:00+00:00";

fn freeze_at() -> DateTime<Utc> {
    "2026-10-02T10:00:00Z".parse().unwrap()
}

fn find_git() -> Option<PathBuf> {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join("git"))
            .find(|p| p.is_absolute() && p.is_file())
    });
    if found.is_none() {
        assert!(
            std::env::var_os("HORCH_REQUIRE_GIT").is_none_or(|v| v != "1"),
            "HORCH_REQUIRE_GIT=1 is set but git is not on PATH"
        );
    }
    found
}

/// A validator whose verdict the test sets. It records what it was asked
/// to validate and the HEAD that worktree really had.
struct Scripted<'a> {
    pass: bool,
    git: &'a GitCli,
    seen: RefCell<Vec<(String, String)>>,
}

impl Validator for Scripted<'_> {
    fn validate(&self, c: &FrozenCandidate) -> anyhow::Result<ValidationReport> {
        let actual = self.git.head(&c.worktree)?;
        self.seen.borrow_mut().push((c.head_sha.clone(), actual));
        Ok(ValidationReport {
            validation_id: "v-reval".into(),
            label: c.label.clone(),
            head_sha: c.head_sha.clone(),
            gates: Vec::new(),
            mechanical_score: if self.pass { 1.0 } else { 0.0 },
            eligible: self.pass,
        })
    }
}

/// A repo on `main` with `a.txt` (3 lines) and `b.txt`, a branch `target`
/// at the base, and candidate `A` frozen with line 2 of `a.txt` changed.
struct Fixture {
    _tmp: TempDir,
    root: PathBuf,
    repo: PathBuf,
    runner: GitRunner,
    git: GitCli,
    paths: DatasetPaths,
    recorder: JsonlRecorder,
    base: String,
    candidate: FrozenCandidate,
}

impl Fixture {
    fn new() -> Option<Self> {
        let bin = find_git()?;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let temp_root = std::env::temp_dir().canonicalize().unwrap();
        assert!(root.starts_with(&temp_root), "real git only on temp repos");
        let config = root.join("gitconfig");
        std::fs::write(&config, "").unwrap();
        let config = config.to_str().unwrap().to_owned();
        let pins = [
            ("GIT_CONFIG_GLOBAL", config.as_str()),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Horch Fixture"),
            ("GIT_AUTHOR_EMAIL", "fixture@horch.invalid"),
            ("GIT_COMMITTER_NAME", "Horch Fixture"),
            ("GIT_COMMITTER_EMAIL", "fixture@horch.invalid"),
            ("GIT_AUTHOR_DATE", GIT_DATE),
            ("GIT_COMMITTER_DATE", GIT_DATE),
        ];
        let mut runner = GitRunner::new(&bin);
        let mut git = GitCli::new(bin);
        for (k, v) in pins {
            runner = runner.with_env(k, v);
            git = git.with_env(k, v);
        }
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        run(&runner, &repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(repo.join("b.txt"), "bee\n").unwrap();
        let base = commit(&runner, &repo, "base");
        run(&runner, &repo, &["branch", "target"]);

        let mgr = WorktreeManager { git: &git };
        let spec = spec(&root, &repo, "A", &base);
        let wt = mgr.create(&spec).unwrap();
        std::fs::write(wt.join("a.txt"), "one\ntwo by A\nthree\n").unwrap();
        std::fs::write(wt.join("new.txt"), "new\n").unwrap();
        let candidate = mgr.freeze(&spec, &execution(), freeze_at()).unwrap();

        let paths = DatasetPaths::from_slug(&root.join("state"), "p");
        let recorder = JsonlRecorder::open(&paths, StoreOptions::default()).unwrap();
        Some(Fixture {
            _tmp: tmp,
            root,
            repo,
            runner,
            git,
            paths,
            recorder,
            base,
            candidate,
        })
    }

    fn sh(&self, dir: &Path, args: &[&str]) -> String {
        run(&self.runner, dir, args)
    }

    fn rev(&self, rev: &str) -> String {
        self.git.rev_parse(&self.repo, rev).unwrap().unwrap()
    }

    fn target(&self, name: &str) -> BranchRef {
        BranchRef {
            repo: self.repo.clone(),
            name: name.into(),
        }
    }

    fn plan(&self) -> PromotionPlan {
        PromotionPlan {
            experiment: ExperimentId::new("exp-1").unwrap(),
            round: RoundId::new("r-1").unwrap(),
            judgment_id: JudgmentId::new("j-1").unwrap(),
            identity: GitIdentity {
                name: "multi-herdr-dataset".into(),
                email: "dataset@multi-herdr.invalid".into(),
                date: "2026-10-02T11:00:00Z".into(),
            },
            integration_root: self.root.join("integrate"),
            attempt: 1,
        }
    }

    fn validator(&self, pass: bool) -> Scripted<'_> {
        Scripted {
            pass,
            git: &self.git,
            seen: RefCell::new(Vec::new()),
        }
    }

    fn engine<'a>(
        &'a self,
        validator: &'a Scripted<'a>,
        faults: &'a Faults,
    ) -> GitPromotionEngine<'a, GitCli, Scripted<'a>, JsonlRecorder> {
        GitPromotionEngine {
            git: &self.git,
            validator,
            recorder: &self.recorder,
            paths: &self.paths,
            faults,
        }
    }

    fn cleanup<'a>(&'a self, faults: &'a Faults) -> RoundCleanup<'a, GitCli, JsonlRecorder> {
        RoundCleanup {
            git: &self.git,
            recorder: &self.recorder,
            paths: &self.paths,
            faults,
            repo: self.repo.clone(),
        }
    }

    fn events(&self) -> Vec<EventEnvelope> {
        self.recorder.read_all().unwrap().events
    }

    fn kinds(&self) -> Vec<String> {
        self.events().into_iter().map(|e| e.kind).collect()
    }

    /// Branches under `mh/promote/`: the engine's temp branches.
    fn temp_branches(&self) -> String {
        self.sh(
            &self.repo,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads/mh/promote",
            ],
        )
    }

    /// Freeze another candidate from the base.
    fn freeze(&self, label: &str, file: &str) -> FrozenCandidate {
        let mgr = WorktreeManager { git: &self.git };
        let spec = spec(&self.root, &self.repo, label, &self.base);
        let wt = mgr.create(&spec).unwrap();
        std::fs::write(wt.join(file), format!("{label}\n")).unwrap();
        mgr.freeze(&spec, &execution(), freeze_at()).unwrap()
    }
}

fn run(runner: &GitRunner, dir: &Path, args: &[&str]) -> String {
    runner.run(dir, args).unwrap().stdout_text()
}

fn commit(runner: &GitRunner, dir: &Path, message: &str) -> String {
    run(runner, dir, &["add", "-A"]);
    run(runner, dir, &["commit", "-q", "-m", message]);
    run(runner, dir, &["rev-parse", "HEAD"])
}

fn spec(root: &Path, repo: &Path, label: &str, base: &str) -> WorktreeSpec {
    WorktreeSpec {
        repo: repo.to_path_buf(),
        root: root.join("wt"),
        exp8: "0a1b2c3d".into(),
        round_index: 1,
        label: label.into(),
        base_sha: base.into(),
    }
}

fn execution() -> ExecutionId {
    ExecutionId::mint(freeze_at())
}

fn no_faults() -> Faults {
    Faults::default()
}

fn started_of(events: &[EventEnvelope]) -> PromotionStarted {
    events
        .iter()
        .rev()
        .find_map(|e| match e.event().unwrap() {
            EventKind::PromotionStarted(s) => Some(s),
            _ => None,
        })
        .expect("a promotion.started event")
}

/// A round of `candidates` in `state`, its winner `A`, as the projection
/// would show it.
fn round_view(state: RoundState, candidates: &[&FrozenCandidate]) -> RoundView {
    let mut views = BTreeMap::new();
    for c in candidates {
        views.insert(
            c.label.clone(),
            CandidateView {
                execution_id: Some(c.execution_id.clone()),
                worktree: Some(WorktreeCreated {
                    label: c.label.clone(),
                    path: c.worktree.clone(),
                    branch: c.branch.clone(),
                    base_sha: c.base_sha.clone(),
                }),
                ..CandidateView::default()
            },
        );
    }
    let winner = candidates[0];
    RoundView {
        experiment_id: ExperimentId::new("exp-1").unwrap(),
        state,
        created: RoundCreated {
            index: 1,
            base_sha: winner.base_sha.clone(),
            labels: views.keys().cloned().collect(),
            eligible_set: Vec::new(),
            propensities: BTreeMap::new(),
            teacher: TeacherRef::none(),
            seed: 1,
            label_policy_version: "slot-order-1".into(),
        },
        candidates: views,
        judge: JudgeView::default(),
        winner: Some(WinnerSelected {
            label: winner.label.clone(),
            execution_id: winner.execution_id.clone(),
            head_sha: winner.head_sha.clone(),
            judgment_id: JudgmentId::new("j-1").unwrap(),
            promotion: PromotionIntent::NotRequested,
        }),
        rejected: None,
        promotion: PromotionView::default(),
        needs_intervention: None,
        cleaned_from: None,
        final_outcome: None,
        cleanup_failures: Vec::new(),
        outcomes: Vec::new(),
    }
}

/// A projection that holds only `view` as round `r-1`.
fn projection_of(view: RoundView) -> Projection {
    let mut p = Projection::default();
    p.rounds.insert(RoundId::new("r-1").unwrap(), view);
    p
}

/// Every file under `dir` (without `.git`), with its bytes.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let p = entry.unwrap().path();
            if p.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p.clone(), std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

// ─── PRO-01 ─────────────────────────────────────────────────────────────────

#[test]
fn pro_01_stale_judgment_cannot_promote() {
    let Some(f) = Fixture::new() else { return };
    // A commit lands in the candidate's worktree after the freeze.
    std::fs::write(f.candidate.worktree.join("late.txt"), "late\n").unwrap();
    commit(&f.runner, &f.candidate.worktree, "after the judgment");
    let v = f.validator(true);
    let faults = no_faults();
    let got = f
        .engine(&v, &faults)
        .promote(&f.candidate, &f.target("target"), &f.plan())
        .unwrap();
    assert_eq!(
        got,
        PromotionResult::Rejected {
            reason: RejectReason::StaleJudgment
        }
    );
    assert_eq!(f.rev("refs/heads/target"), f.base, "target untouched");
    assert!(v.seen.borrow().is_empty(), "no revalidation");
    let events = f.events();
    assert_eq!(f.kinds(), ["winner.rejected"]);
    assert_eq!(events[0].payload["reason"], "stale_judgment");

    // The branch alone moved (the worktree is gone): stale too.
    let Some(f) = Fixture::new() else { return };
    std::fs::remove_dir_all(&f.candidate.worktree).unwrap();
    f.sh(&f.repo, &["worktree", "prune"]);
    f.sh(&f.repo, &["branch", "-f", &f.candidate.branch, &f.base]);
    let got = f
        .engine(&v, &faults)
        .promote(&f.candidate, &f.target("target"), &f.plan())
        .unwrap();
    assert!(matches!(
        got,
        PromotionResult::Rejected {
            reason: RejectReason::StaleJudgment
        }
    ));
}

// ─── PRO-02 ─────────────────────────────────────────────────────────────────

#[test]
fn pro_02_revalidation_failure_rejected() {
    let Some(f) = Fixture::new() else { return };
    let v = f.validator(false);
    let faults = no_faults();
    let got = f
        .engine(&v, &faults)
        .promote(&f.candidate, &f.target("target"), &f.plan())
        .unwrap();
    assert_eq!(
        got,
        PromotionResult::Rejected {
            reason: RejectReason::RevalidationFailed
        }
    );
    // The gates ran on the integrated commit, checked out for real.
    assert_eq!(
        *v.seen.borrow(),
        [(f.candidate.head_sha.clone(), f.candidate.head_sha.clone())]
    );
    assert_eq!(f.rev("refs/heads/target"), f.base, "target untouched");
    assert!(!f.paths.promotion(&f.plan().round).unwrap().exists());
    assert_eq!(f.kinds(), ["winner.rejected"]);
    assert_eq!(f.events()[0].payload["reason"], "revalidation_failed");
    // The temp worktree and branch are gone; the candidate is kept.
    assert_eq!(f.temp_branches(), "");
    assert!(!f.plan().integration_root.join("r-1-a1").exists());
    assert_eq!(
        f.git.head(&f.candidate.worktree).unwrap(),
        f.candidate.head_sha
    );
}

// ─── PRO-03 ─────────────────────────────────────────────────────────────────

#[test]
fn pro_03_ff() {
    // Not checked out: compare-and-swap update-ref.
    let Some(f) = Fixture::new() else { return };
    let v = f.validator(true);
    let faults = no_faults();
    let PromotionResult::Promoted(receipt) = f
        .engine(&v, &faults)
        .promote(&f.candidate, &f.target("target"), &f.plan())
        .unwrap()
    else {
        panic!("not promoted")
    };
    assert_eq!(receipt.strategy, PromotionStrategy::FastForward);
    assert_eq!(receipt.publish, "update_ref_cas");
    assert_eq!(f.rev("refs/heads/target"), f.candidate.head_sha);
    assert_eq!(f.kinds(), ["promotion.started", "promotion.completed"]);
    assert_eq!(f.temp_branches(), "");
    // main and its checkout are untouched.
    assert_eq!(f.rev("refs/heads/main"), f.base);
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), "");

    // The events take a REVALIDATING round to PROMOTED without anomalies.
    let mut p = projection_of(round_view(RoundState::Revalidating, &[&f.candidate]));
    for e in f.events() {
        p.apply(&e);
    }
    assert_eq!(p.anomalies, []);
    assert_eq!(
        p.rounds.values().next().unwrap().state,
        RoundState::Promoted
    );

    // Checked out and clean: a ref swap, then read-tree in that checkout.
    let Some(f) = Fixture::new() else { return };
    let PromotionResult::Promoted(receipt) = f
        .engine(&v, &faults)
        .promote(&f.candidate, &f.target("main"), &f.plan())
        .unwrap()
    else {
        panic!("not promoted")
    };
    assert_eq!(receipt.strategy, PromotionStrategy::FastForward);
    assert_eq!(receipt.publish, "update_ref_cas_read_tree");
    assert_eq!(f.git.head(&f.repo).unwrap(), f.candidate.head_sha);
    assert_eq!(
        std::fs::read_to_string(f.repo.join("a.txt")).unwrap(),
        "one\ntwo by A\nthree\n"
    );
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), "");
}

#[test]
fn pro_03_cherry_pick_when_target_moved() {
    let Some(f) = Fixture::new() else { return };
    // The target gains an unrelated commit after the round's base.
    std::fs::write(f.repo.join("c.txt"), "sea\n").unwrap();
    let moved = commit(&f.runner, &f.repo, "unrelated");
    f.sh(&f.repo, &["branch", "-f", "target", &moved]);
    let v = f.validator(true);
    let faults = no_faults();
    let PromotionResult::Promoted(receipt) = f
        .engine(&v, &faults)
        .promote(&f.candidate, &f.target("target"), &f.plan())
        .unwrap()
    else {
        panic!("not promoted")
    };
    assert_eq!(receipt.strategy, PromotionStrategy::CherryPick);
    assert_eq!(receipt.publish, "update_ref_cas");
    assert_eq!(receipt.dest_before, moved);
    let after = f.rev("refs/heads/target");
    assert_eq!(receipt.dest_after, after);
    assert_ne!(after, f.candidate.head_sha, "a new commit");
    assert_eq!(f.rev(&format!("{after}^")), moved, "on top of the target");
    // The integrated tree has both changes; the committer is the plan's.
    assert_eq!(f.sh(&f.repo, &["show", &format!("{after}:c.txt")]), "sea");
    assert_eq!(
        f.sh(&f.repo, &["show", &format!("{after}:a.txt")]),
        "one\ntwo by A\nthree"
    );
    assert_eq!(
        f.sh(&f.repo, &["log", "-1", "--format=%cn <%ce> %cI", &after]),
        "multi-herdr-dataset <dataset@multi-herdr.invalid> 2026-10-02T11:00:00Z"
            .replace("Z", "+00:00")
    );
    // Revalidation saw exactly that commit.
    assert_eq!(*v.seen.borrow(), [(after.clone(), after.clone())]);
    assert_eq!(f.temp_branches(), "");

    // Deterministic: the same promotion in a fresh repo gives the same hash.
    let Some(g) = Fixture::new() else { return };
    std::fs::write(g.repo.join("c.txt"), "sea\n").unwrap();
    let moved = commit(&g.runner, &g.repo, "unrelated");
    g.sh(&g.repo, &["branch", "-f", "target", &moved]);
    let v = g.validator(true);
    g.engine(&v, &faults)
        .promote(&g.candidate, &g.target("target"), &g.plan())
        .unwrap();
    assert_eq!(g.rev("refs/heads/target"), after);
}

#[test]
fn pro_03_checked_out_dirty_needs_intervention() {
    let Some(f) = Fixture::new() else { return };
    std::fs::write(f.repo.join("a.txt"), "local edit\n").unwrap();
    std::fs::write(f.repo.join("untracked.txt"), "mine\n").unwrap();
    let files = snapshot(&f.repo);
    let head = f.git.head(&f.repo).unwrap();
    let status = f.git.status_porcelain(&f.repo).unwrap();
    let index = std::fs::read(f.repo.join(".git/index")).unwrap();

    let v = f.validator(true);
    let faults = no_faults();
    let got = f
        .engine(&v, &faults)
        .promote(&f.candidate, &f.target("main"), &f.plan())
        .unwrap();
    let PromotionResult::NeedsIntervention { reason } = got else {
        panic!("{got:?}")
    };
    assert!(reason.contains("local changes"), "{reason}");
    assert_eq!(snapshot(&f.repo), files, "files byte-identical");
    assert_eq!(f.git.head(&f.repo).unwrap(), head);
    assert_eq!(f.rev("refs/heads/main"), head);
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), status);
    assert_eq!(std::fs::read(f.repo.join(".git/index")).unwrap(), index);
    assert_eq!(f.kinds(), ["round.needs_intervention"]);
    assert_eq!(f.events()[0].payload["source"], "promotion");
    assert!(!f.paths.promotion(&f.plan().round).unwrap().exists());
    assert_eq!(f.temp_branches(), "");
}

/// The real client, with one hook: `race` runs once, just before the first
/// compare-and-swap on `refs/heads/main`. That is the window between the
/// publish's check of the checkout and its move.
struct Racy<'a> {
    git: &'a GitCli,
    race: RefCell<Option<Box<dyn FnOnce() + 'a>>>,
}

impl GitClient for Racy<'_> {
    fn toplevel(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        self.git.toplevel(dir)
    }
    fn head(&self, dir: &Path) -> anyhow::Result<String> {
        self.git.head(dir)
    }
    fn current_branch(&self, dir: &Path) -> anyhow::Result<Option<String>> {
        self.git.current_branch(dir)
    }
    fn status_porcelain(&self, dir: &Path) -> anyhow::Result<String> {
        self.git.status_porcelain(dir)
    }
    fn version(&self) -> anyhow::Result<String> {
        self.git.version()
    }
    fn worktree_add(
        &self,
        repo: &Path,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> anyhow::Result<()> {
        self.git.worktree_add(repo, path, branch, base)
    }
    fn worktree_remove(&self, repo: &Path, path: &Path, force: bool) -> anyhow::Result<()> {
        self.git.worktree_remove(repo, path, force)
    }
    fn worktree_list(&self, repo: &Path) -> anyhow::Result<Vec<(PathBuf, Option<String>)>> {
        self.git.worktree_list(repo)
    }
    fn commit_all(
        &self,
        dir: &Path,
        message: &str,
        id: &GitIdentity,
    ) -> anyhow::Result<Option<String>> {
        self.git.commit_all(dir, message, id)
    }
    fn rev_parse(&self, dir: &Path, rev: &str) -> anyhow::Result<Option<String>> {
        self.git.rev_parse(dir, rev)
    }
    fn rev_list(&self, dir: &Path, range: &str) -> anyhow::Result<Vec<String>> {
        self.git.rev_list(dir, range)
    }
    fn diff_numstat(&self, dir: &Path, base: &str, head: &str) -> anyhow::Result<Vec<NumstatLine>> {
        self.git.diff_numstat(dir, base, head)
    }
    fn diff_patch(
        &self,
        dir: &Path,
        base: &str,
        head: &str,
        cap_bytes: usize,
    ) -> anyhow::Result<(String, bool)> {
        self.git.diff_patch(dir, base, head, cap_bytes)
    }
    fn diff_digest(&self, dir: &Path, base: &str, head: &str) -> anyhow::Result<Digest> {
        self.git.diff_digest(dir, base, head)
    }
    fn is_ancestor(&self, dir: &Path, a: &str, b: &str) -> anyhow::Result<bool> {
        self.git.is_ancestor(dir, a, b)
    }
    fn update_ref_cas(
        &self,
        repo: &Path,
        refname: &str,
        new: &str,
        expected_old: &str,
    ) -> anyhow::Result<bool> {
        if refname == "refs/heads/main" {
            if let Some(race) = self.race.borrow_mut().take() {
                race();
            }
        }
        self.git.update_ref_cas(repo, refname, new, expected_old)
    }
    fn cherry_pick(&self, dir: &Path, range: &str, id: &GitIdentity) -> anyhow::Result<CherryPick> {
        self.git.cherry_pick(dir, range, id)
    }
    fn read_tree_update(&self, dir: &Path, old: &str, new: &str) -> anyhow::Result<()> {
        self.git.read_tree_update(dir, old, new)
    }
    fn branch_checkout_location(
        &self,
        repo: &Path,
        branch: &str,
    ) -> anyhow::Result<CheckoutLocation> {
        self.git.branch_checkout_location(repo, branch)
    }
}

/// Promote into `main`, checked out clean in `f.repo`, with `race` run in
/// the window between the publish's check and its move.
fn promote_racing<'a>(f: &'a Fixture, race: impl FnOnce() + 'a) -> PromotionResult {
    let racy = Racy {
        git: &f.git,
        race: RefCell::new(Some(Box::new(race))),
    };
    let v = f.validator(true);
    let faults = no_faults();
    let engine = GitPromotionEngine {
        git: &racy,
        validator: &v,
        recorder: &f.recorder,
        paths: &f.paths,
        faults: &faults,
    };
    let got = engine
        .promote(&f.candidate, &f.target("main"), &f.plan())
        .unwrap();
    assert!(racy.race.borrow().is_none(), "the race ran");
    got
}

#[test]
fn pro_03_checked_out_publish_is_cas() {
    // A commit lands on the checked-out target after the check: the swap
    // loses, and the target, HEAD and files are the racer's.
    let Some(f) = Fixture::new() else { return };
    let racer = RefCell::new(String::new());
    let got = promote_racing(&f, || {
        std::fs::write(f.repo.join("b.txt"), "racer\n").unwrap();
        *racer.borrow_mut() = commit(&f.runner, &f.repo, "racer");
    });
    let PromotionResult::NeedsIntervention { reason } = got else {
        panic!("{got:?}")
    };
    let racer = racer.into_inner();
    assert!(reason.contains("moved from"), "{reason}");
    assert_eq!(f.rev("refs/heads/main"), racer);
    assert_eq!(f.git.head(&f.repo).unwrap(), racer);
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), "");
    assert_eq!(
        std::fs::read_to_string(f.repo.join("a.txt")).unwrap(),
        "one\ntwo\nthree\n"
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("b.txt")).unwrap(),
        "racer\n"
    );
    assert!(!f.repo.join("new.txt").exists());
    assert_eq!(f.kinds(), ["promotion.started", "round.needs_intervention"]);
    assert!(!f.paths.promotion(&f.plan().round).unwrap().exists());
    assert_eq!(f.temp_branches(), "");

    // A local edit lands after the check, in a path the promotion changes:
    // the ref swap wins, git refuses the tree update, and the ref goes back.
    let Some(f) = Fixture::new() else { return };
    let got = promote_racing(&f, || {
        std::fs::write(f.repo.join("a.txt"), "local edit\n").unwrap();
    });
    let PromotionResult::NeedsIntervention { reason } = got else {
        panic!("{got:?}")
    };
    assert!(reason.contains("is back at"), "{reason}");
    assert_eq!(f.rev("refs/heads/main"), f.base);
    assert_eq!(f.git.head(&f.repo).unwrap(), f.base);
    assert_eq!(
        std::fs::read_to_string(f.repo.join("a.txt")).unwrap(),
        "local edit\n"
    );
    assert!(!f.repo.join("new.txt").exists());
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), " M a.txt");
    assert_eq!(f.kinds(), ["promotion.started", "round.needs_intervention"]);
    assert!(!f.paths.promotion(&f.plan().round).unwrap().exists());

    // A local edit in a path the promotion does not change is kept, and
    // the promotion goes through.
    let Some(f) = Fixture::new() else { return };
    let got = promote_racing(&f, || {
        std::fs::write(f.repo.join("b.txt"), "local edit\n").unwrap();
    });
    let PromotionResult::Promoted(receipt) = got else {
        panic!("{got:?}")
    };
    assert_eq!(receipt.publish, "update_ref_cas_read_tree");
    assert_eq!(f.git.head(&f.repo).unwrap(), f.candidate.head_sha);
    assert_eq!(f.rev("refs/heads/main"), f.candidate.head_sha);
    assert_eq!(
        std::fs::read_to_string(f.repo.join("a.txt")).unwrap(),
        "one\ntwo by A\nthree\n"
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("new.txt")).unwrap(),
        "new\n"
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("b.txt")).unwrap(),
        "local edit\n"
    );
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), " M b.txt");

    // A crash between the ref swap and the tree update leaves `main` at
    // the candidate and the files at the base. The restart moves the files
    // and leaves the ref alone.
    let Some(f) = Fixture::new() else { return };
    let v = f.validator(true);
    let crash = Faults::parse(Some(ABORT_AFTER_PROMOTION_STARTED));
    let target = f.target("main");
    f.engine(&v, &crash)
        .promote(&f.candidate, &target, &f.plan())
        .unwrap_err();
    f.sh(
        &f.repo,
        &[
            "update-ref",
            "refs/heads/main",
            &f.candidate.head_sha,
            &f.base,
        ],
    );
    assert_ne!(f.git.status_porcelain(&f.repo).unwrap(), "");
    let before = ref_moves(&f, "refs/heads/main");
    let started = started_of(&f.events());
    let faults = no_faults();
    for _ in 0..2 {
        let got = f
            .engine(&v, &faults)
            .resume_promotion(&f.candidate, &target, &f.plan(), &started)
            .unwrap();
        assert!(matches!(got, PromotionResult::Promoted(_)), "{got:?}");
    }
    assert_eq!(f.git.head(&f.repo).unwrap(), f.candidate.head_sha);
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), "");
    assert_eq!(
        std::fs::read_to_string(f.repo.join("new.txt")).unwrap(),
        "new\n"
    );
    assert_eq!(ref_moves(&f, "refs/heads/main"), before);
}

/// The checked-out publish is recorded as what it does: a ref swap, then
/// `read-tree`. A `promotion.started` or a receipt written before D17 says
/// `merge_ff_only` for the same mode: the receipt still parses, and a
/// restart still moves the checkout's files.
#[test]
fn pro_03_checked_out_publish_label() {
    let Some(f) = Fixture::new() else { return };
    let v = f.validator(true);
    let crash = Faults::parse(Some(ABORT_AFTER_PROMOTION_STARTED));
    let target = f.target("main");
    f.engine(&v, &crash)
        .promote(&f.candidate, &target, &f.plan())
        .unwrap_err();
    let mut started = started_of(&f.events());
    assert_eq!(started.publish, "update_ref_cas_read_tree");

    // A crash between the swap and the tree update, under an old label.
    f.sh(
        &f.repo,
        &[
            "update-ref",
            "refs/heads/main",
            &f.candidate.head_sha,
            &f.base,
        ],
    );
    started.publish = "merge_ff_only".into();
    let PromotionResult::Promoted(receipt) = f
        .engine(&v, &no_faults())
        .resume_promotion(&f.candidate, &target, &f.plan(), &started)
        .unwrap()
    else {
        panic!("not promoted")
    };
    assert_eq!(receipt.publish, "merge_ff_only");
    assert_eq!(f.git.head(&f.repo).unwrap(), f.candidate.head_sha);
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), "");
    assert_eq!(
        std::fs::read_to_string(f.repo.join("new.txt")).unwrap(),
        "new\n"
    );

    // The receipt on disk round-trips with the old label.
    let file = f.paths.promotion(&f.plan().round).unwrap();
    let back: PromotionReceipt = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(back, receipt);
}

// ─── PRO-04 ─────────────────────────────────────────────────────────────────

#[test]
fn pro_04_conflict_needs_intervention_preserves_worktrees() {
    let Some(f) = Fixture::new() else { return };
    let b = f.freeze("B", "b-only.txt");
    // The target changes the line candidate A changed.
    std::fs::write(f.repo.join("a.txt"), "one\ntwo by target\nthree\n").unwrap();
    let moved = commit(&f.runner, &f.repo, "conflicting");
    f.sh(&f.repo, &["branch", "-f", "target", &moved]);

    let v = f.validator(true);
    let faults = no_faults();
    let got = f
        .engine(&v, &faults)
        .promote(&f.candidate, &f.target("target"), &f.plan())
        .unwrap();
    assert!(
        matches!(got, PromotionResult::NeedsIntervention { .. }),
        "{got:?}"
    );
    let events = f.events();
    assert_eq!(f.kinds(), ["promotion.conflicted"]);
    assert_eq!(events[0].payload["paths"], serde_json::json!(["a.txt"]));
    assert_eq!(f.rev("refs/heads/target"), moved, "target untouched");
    assert!(v.seen.borrow().is_empty(), "no revalidation");
    // Every candidate worktree is kept, at its frozen commit.
    for c in [&f.candidate, &b] {
        assert_eq!(f.git.head(&c.worktree).unwrap(), c.head_sha);
        assert_eq!(f.git.status_porcelain(&c.worktree).unwrap(), "");
    }
    // Only the temp integration worktree and branch are gone.
    assert_eq!(f.temp_branches(), "");
    let listed = f.git.worktree_list(&f.repo).unwrap();
    assert_eq!(listed.len(), 3, "{listed:?}");

    // The projection: REVALIDATING → NEEDS_INTERVENTION.
    let mut p = projection_of(round_view(RoundState::Revalidating, &[&f.candidate, &b]));
    for e in &events {
        p.apply(e);
    }
    assert_eq!(p.anomalies, []);
    let r = p.rounds.values().next().unwrap();
    assert_eq!(r.state, RoundState::NeedsIntervention);
    // A NEEDS_INTERVENTION round keeps its worktrees on a plain cleanup.
    let faults = no_faults();
    let out = f
        .cleanup(&faults)
        .run(&f.plan().round, r, CleanupOptions::default())
        .unwrap();
    assert!(matches!(out, CleanupOutcome::Skipped { .. }), "{out:?}");
    assert!(f.candidate.worktree.exists() && b.worktree.exists());
}

// ─── PRO-05 ─────────────────────────────────────────────────────────────────

#[test]
fn pro_05_receipt_fields() {
    let Some(f) = Fixture::new() else { return };
    let v = f.validator(true);
    let faults = no_faults();
    let PromotionResult::Promoted(receipt) = f
        .engine(&v, &faults)
        .promote(&f.candidate, &f.target("target"), &f.plan())
        .unwrap()
    else {
        panic!("not promoted")
    };
    let path = f.paths.promotion(&f.plan().round).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let on_disk: PromotionReceipt = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(on_disk, receipt);
    assert_eq!(receipt.schema_version, "1.0.0");
    assert_eq!(receipt.round_id.as_str(), "r-1");
    assert_eq!(receipt.label, "A");
    assert_eq!(
        receipt.source_shas,
        std::slice::from_ref(&f.candidate.head_sha)
    );
    assert_eq!(receipt.dest_ref, "refs/heads/target");
    assert_eq!(receipt.dest_before, f.base);
    assert_eq!(receipt.dest_after, f.candidate.head_sha);
    assert_eq!(receipt.strategy, PromotionStrategy::FastForward);
    assert_eq!(receipt.publish, "update_ref_cas");
    assert_eq!(receipt.validation_ids, ["v-reval"]);
    assert_eq!(receipt.judgment_id.as_str(), "j-1");
    DateTime::parse_from_rfc3339(&receipt.promoted_at).unwrap();
    // Exactly these keys (the map sorts them).
    let keys: Vec<String> =
        serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&bytes)
            .unwrap()
            .keys()
            .cloned()
            .collect();
    let mut want = [
        "schema_version",
        "round_id",
        "label",
        "source_shas",
        "dest_ref",
        "dest_before",
        "dest_after",
        "strategy",
        "publish",
        "validation_ids",
        "judgment_id",
        "promoted_at",
    ];
    want.sort();
    assert_eq!(keys, want);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // promotion.completed cites the receipt's bytes and the new head.
    let events = f.events();
    let completed = &events[1];
    assert_eq!(completed.kind, "promotion.completed");
    assert_eq!(
        completed.payload["receipt_digest"],
        serde_json::to_value(sha256_bytes(&bytes)).unwrap()
    );
    assert_eq!(completed.payload["dest_after"], f.candidate.head_sha);
    // promotion.started was recorded before the publish, with the plan.
    let started = started_of(&events);
    assert_eq!(started.dest_before, f.base);
    assert_eq!(started.planned_after, f.candidate.head_sha);
    assert_eq!(started.strategy, PromotionStrategy::FastForward);
    assert_eq!(events[0].actor, Actor::Coordinator);
    assert_eq!(
        events[0].execution_id,
        Some(f.candidate.execution_id.clone())
    );

    // A second promotion of the round is refused: one receipt per round.
    assert!(f
        .engine(&v, &faults)
        .promote(&f.candidate, &f.target("target"), &f.plan())
        .is_err());
}

#[test]
fn pro_05_cleanup_after_receipt_only() {
    let Some(f) = Fixture::new() else { return };
    let round = f.plan().round;
    let faults = no_faults();
    // PROMOTED but no receipt yet (a crash before it): nothing is removed.
    let view = round_view(RoundState::Promoted, &[&f.candidate]);
    let out = f
        .cleanup(&faults)
        .run(&round, &view, CleanupOptions::default())
        .unwrap();
    assert!(matches!(out, CleanupOutcome::Skipped { .. }), "{out:?}");
    assert!(f.candidate.worktree.exists());
    assert_eq!(f.kinds(), Vec::<String>::new());

    // Any other unfinished state waits too.
    for state in [RoundState::Promoting, RoundState::Revalidating] {
        let out = f
            .cleanup(&faults)
            .run(
                &round,
                &round_view(state, &[&f.candidate]),
                CleanupOptions::default(),
            )
            .unwrap();
        assert!(matches!(out, CleanupOutcome::Skipped { .. }), "{state}");
    }

    let v = f.validator(true);
    let got = f
        .engine(&v, &faults)
        .promote(&f.candidate, &f.target("target"), &f.plan())
        .unwrap();
    assert!(matches!(got, PromotionResult::Promoted(_)));
    assert!(f.candidate.worktree.exists(), "promotion removes nothing");

    let out = f
        .cleanup(&faults)
        .run(&round, &view, CleanupOptions::default())
        .unwrap();
    assert_eq!(
        out,
        CleanupOutcome::Done {
            removed: vec!["A".into()],
            failed: vec![],
        }
    );
    assert!(!f.candidate.worktree.exists());
    assert_eq!(
        f.rev(&format!("refs/heads/{}", f.candidate.branch)),
        f.candidate.head_sha,
        "the branch is kept"
    );
    assert_eq!(
        &f.kinds()[2..],
        ["round.cleanup_started", "round.completed"]
    );
    assert_eq!(f.events()[3].payload["final_outcome"], "promoted");
}

// ─── PRO-06 ─────────────────────────────────────────────────────────────────

/// How many times the reflog saw `refname` change (creation included).
fn ref_moves(f: &Fixture, refname: &str) -> usize {
    f.sh(&f.repo, &["reflog", "show", "--format=%H", refname])
        .lines()
        .count()
}

#[test]
fn pro_06_crash_after_update_ref_resumes_once() {
    let Some(f) = Fixture::new() else { return };
    let target = f.target("target");
    let v = f.validator(true);
    let before = ref_moves(&f, "refs/heads/target");

    let crash = Faults::parse(Some(ABORT_AFTER_UPDATE_REF));
    let err = f
        .engine(&v, &crash)
        .promote(&f.candidate, &target, &f.plan())
        .unwrap_err();
    assert!(err.downcast_ref::<FaultFired>().is_some(), "{err:#}");
    assert_eq!(f.rev("refs/heads/target"), f.candidate.head_sha);
    assert!(!f.paths.promotion(&f.plan().round).unwrap().exists());
    assert_eq!(f.kinds(), ["promotion.started"]);

    // The restart reads the last promotion.started and finishes.
    let faults = no_faults();
    let started = started_of(&f.events());
    for _ in 0..2 {
        let got = f
            .engine(&v, &faults)
            .resume_promotion(&f.candidate, &target, &f.plan(), &started)
            .unwrap();
        assert!(matches!(got, PromotionResult::Promoted(_)), "{got:?}");
    }
    let receipts = std::fs::read_dir(f.paths.promotions_dir()).unwrap().count();
    assert_eq!(receipts, 1, "exactly 1 receipt");
    assert_eq!(f.kinds(), ["promotion.started", "promotion.completed"]);
    assert_eq!(f.rev("refs/heads/target"), f.candidate.head_sha);
    assert_eq!(
        ref_moves(&f, "refs/heads/target"),
        before + 1,
        "the target moved once"
    );
    assert_eq!(f.temp_branches(), "");
}

#[test]
fn pro_06_restart_after_each_fault_point() {
    for point in [ABORT_AFTER_PROMOTION_STARTED, ABORT_AFTER_RECEIPT] {
        let Some(f) = Fixture::new() else { return };
        let target = f.target("target");
        let v = f.validator(true);
        let before = ref_moves(&f, "refs/heads/target");
        let crash = Faults::parse(Some(point));
        let err = f
            .engine(&v, &crash)
            .promote(&f.candidate, &target, &f.plan())
            .unwrap_err();
        assert!(
            err.downcast_ref::<FaultFired>().is_some(),
            "{point}: {err:#}"
        );
        let receipt = f.paths.promotion(&f.plan().round).unwrap();
        let written = if point == ABORT_AFTER_RECEIPT {
            Some(std::fs::read(&receipt).unwrap())
        } else {
            assert_eq!(f.rev("refs/heads/target"), f.base, "{point}");
            None
        };

        let faults = no_faults();
        let started = started_of(&f.events());
        let got = f
            .engine(&v, &faults)
            .resume_promotion(&f.candidate, &target, &f.plan(), &started)
            .unwrap();
        assert!(matches!(got, PromotionResult::Promoted(_)), "{point}");
        if let Some(bytes) = written {
            assert_eq!(std::fs::read(&receipt).unwrap(), bytes, "receipt kept");
        }
        assert_eq!(f.rev("refs/heads/target"), f.candidate.head_sha);
        assert_eq!(ref_moves(&f, "refs/heads/target"), before + 1, "{point}");
        assert_eq!(f.kinds(), ["promotion.started", "promotion.completed"]);
    }

    // The target moved elsewhere after promotion.started: the operator.
    let Some(f) = Fixture::new() else { return };
    let target = f.target("target");
    let v = f.validator(true);
    let crash = Faults::parse(Some(ABORT_AFTER_PROMOTION_STARTED));
    f.engine(&v, &crash)
        .promote(&f.candidate, &target, &f.plan())
        .unwrap_err();
    let other = f.freeze("B", "other.txt");
    f.sh(&f.repo, &["branch", "-f", "target", &other.head_sha]);
    let faults = no_faults();
    let started = started_of(&f.events());
    let got = f
        .engine(&v, &faults)
        .resume_promotion(&f.candidate, &target, &f.plan(), &started)
        .unwrap();
    assert!(
        matches!(got, PromotionResult::NeedsIntervention { .. }),
        "{got:?}"
    );
    assert_eq!(f.rev("refs/heads/target"), other.head_sha, "not moved");
    assert!(!f.paths.promotion(&f.plan().round).unwrap().exists());
    assert_eq!(f.kinds(), ["promotion.started", "round.needs_intervention"]);
}

// ─── PRO-07 ─────────────────────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn pro_07_cleanup_failure_recorded_history_intact() {
    use std::os::unix::fs::PermissionsExt;

    let Some(f) = Fixture::new() else { return };
    let b = f.freeze("B", "b.txt");
    let c = f.freeze("C", "c.txt");
    // B's worktree cannot lose its files.
    let locked = b.worktree.clone();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();

    let faults = no_faults();
    let view = round_view(RoundState::Decided, &[&f.candidate, &b, &c]);
    let out = f
        .cleanup(&faults)
        .run(&f.plan().round, &view, CleanupOptions::default());
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = out.unwrap();
    assert_eq!(
        out,
        CleanupOutcome::Done {
            removed: vec!["A".into(), "C".into()],
            failed: vec!["B".into()],
        }
    );
    assert!(!f.candidate.worktree.exists() && !c.worktree.exists());
    let events = f.events();
    assert_eq!(
        f.kinds(),
        [
            "round.cleanup_started",
            "worktree.cleanup_failed",
            "round.completed"
        ]
    );
    assert_eq!(events[1].payload["label"], "B");
    assert_eq!(events[2].payload["final_outcome"], "winner");
    // History is intact: every branch still holds its frozen commit.
    for cand in [&f.candidate, &b, &c] {
        assert_eq!(f.rev(&format!("refs/heads/{}", cand.branch)), cand.head_sha);
    }
    // The events fold from DECIDED to COMPLETE with the failure recorded.
    let mut p = projection_of(view);
    for e in &events {
        p.apply(e);
    }
    assert_eq!(p.anomalies, []);
    let r = p.rounds.values().next().unwrap();
    assert_eq!(r.state, RoundState::Complete);
    assert_eq!(r.cleanup_failures.len(), 1);
}

#[test]
fn cleanup_fault_stops_after_the_nth_removal_and_resumes() {
    let Some(f) = Fixture::new() else { return };
    let b = f.freeze("B", "b.txt");
    let view = round_view(RoundState::Rejected, &[&f.candidate, &b]);
    let crash = Faults::parse(Some("abort-during-cleanup:1"));
    let err = f
        .cleanup(&crash)
        .run(&f.plan().round, &view, CleanupOptions::default())
        .unwrap_err();
    assert!(err.downcast_ref::<FaultFired>().is_some(), "{err:#}");
    assert!(!f.candidate.worktree.exists() && b.worktree.exists());
    assert_eq!(f.kinds(), ["round.cleanup_started"]);

    // The restart sees the round in CLEANUP and finishes it.
    let mut p = projection_of(view);
    for e in f.events() {
        p.apply(&e);
    }
    let view = p.rounds.values().next().unwrap().clone();
    assert_eq!(view.state, RoundState::Cleanup);
    let faults = no_faults();
    let out = f
        .cleanup(&faults)
        .run(
            &f.plan().round,
            &view,
            CleanupOptions {
                prune_branches: true,
                force: false,
            },
        )
        .unwrap();
    assert_eq!(
        out,
        CleanupOutcome::Done {
            removed: vec!["B".into()],
            failed: vec![],
        }
    );
    assert_eq!(f.kinds(), ["round.cleanup_started", "round.completed"]);
    // --prune-branches removed only the branch whose worktree it removed.
    assert!(f
        .git
        .rev_parse(&f.repo, &format!("refs/heads/{}", b.branch))
        .unwrap()
        .is_none());
    assert_eq!(
        f.rev(&format!("refs/heads/{}", f.candidate.branch)),
        f.candidate.head_sha
    );
}

// ─── rollback ───────────────────────────────────────────────────────────────

#[test]
fn pro_rollback_cas() {
    let Some(f) = Fixture::new() else { return };
    let target = f.target("target");
    let exp = f.plan().experiment;
    let round = f.plan().round;
    let v = f.validator(true);
    let faults = no_faults();
    let engine = f.engine(&v, &faults);

    // No receipt, no rollback.
    assert!(engine.rollback(&exp, &round, &target).is_err());

    engine.promote(&f.candidate, &target, &f.plan()).unwrap();
    // Someone moved the target after the promotion: nothing changes.
    let other = f.freeze("B", "other.txt");
    f.sh(&f.repo, &["branch", "-f", "target", &other.head_sha]);
    let got = engine.rollback(&exp, &round, &target).unwrap();
    assert!(
        matches!(got, RollbackResult::NeedsIntervention { .. }),
        "{got:?}"
    );
    assert_eq!(f.rev("refs/heads/target"), other.head_sha);

    // Back at dest_after: the swap restores dest_before.
    f.sh(&f.repo, &["branch", "-f", "target", &f.candidate.head_sha]);
    for _ in 0..2 {
        let got = engine.rollback(&exp, &round, &target).unwrap();
        assert_eq!(
            got,
            RollbackResult::RolledBack {
                restored: f.base.clone()
            }
        );
        assert_eq!(f.rev("refs/heads/target"), f.base);
    }
    let events = f.events();
    assert_eq!(
        f.kinds(),
        [
            "promotion.started",
            "promotion.completed",
            "promotion.rolled_back"
        ]
    );
    assert_eq!(events[2].payload["restored"], f.base);
    assert_eq!(events[2].actor, Actor::Operator);
    // A rolled-back PROMOTED round stays PROMOTED.
    let mut p = projection_of(round_view(RoundState::Revalidating, &[&f.candidate]));
    for e in &events {
        p.apply(e);
    }
    assert_eq!(p.anomalies, []);
    assert_eq!(
        p.rounds.values().next().unwrap().state,
        RoundState::Promoted
    );

    // A checked-out target is not rolled back under its checkout.
    let Some(g) = Fixture::new() else { return };
    let v = g.validator(true);
    let engine = g.engine(&v, &faults);
    engine
        .promote(&g.candidate, &g.target("main"), &g.plan())
        .unwrap();
    let got = engine.rollback(&exp, &round, &g.target("main")).unwrap();
    assert!(
        matches!(got, RollbackResult::NeedsIntervention { .. }),
        "{got:?}"
    );
    assert_eq!(g.rev("refs/heads/main"), g.candidate.head_sha);
}

// ─── operator.promote ───────────────────────────────────────────────────────

#[test]
fn operator_promote_reenters_revalidation() {
    let envelope = |n: u32| EventEnvelope {
        schema_version: EVENT_SCHEMA_VERSION.into(),
        event_id: horch_core::ids::EventId::mint(freeze_at()),
        kind: "operator.promote".into(),
        occurred_at: "2026-10-02T10:00:00.000Z".into(),
        actor: Actor::Operator,
        experiment_id: ExperimentId::new("exp-1").unwrap(),
        round_id: Some(RoundId::new("r-1").unwrap()),
        execution_id: None,
        idempotency_key: format!("operator.promote:r-1:{n}"),
        payload: EventKind::OperatorPromote(OperatorPromote {
            target: "main".into(),
        })
        .payload(),
    };
    let frozen = FrozenCandidate {
        label: "A".into(),
        execution_id: execution(),
        worktree: "/w/A".into(),
        branch: "mh/exp/0a1b2c3d/r1/A".into(),
        base_sha: "a".repeat(40),
        head_sha: "b".repeat(40),
        numstat: Vec::new(),
        diff_digest: sha256_bytes(b""),
        frozen_at: "2026-10-02T10:00:00Z".into(),
    };
    for from in [RoundState::Complete, RoundState::NeedsIntervention] {
        let mut p = projection_of(round_view(from, &[&frozen]));
        p.apply(&envelope(1));
        assert_eq!(p.anomalies, [], "{from}");
        let r = p.rounds.values().next().unwrap();
        assert_eq!(r.state, RoundState::Revalidating, "{from}");
    }
    // Not from a live state, and not without a winner.
    let mut p = projection_of(round_view(RoundState::Running, &[&frozen]));
    p.apply(&envelope(1));
    assert_eq!(p.anomalies.len(), 1);
    let mut view = round_view(RoundState::Complete, &[&frozen]);
    view.winner = None;
    let mut p = projection_of(view);
    p.apply(&envelope(2));
    assert_eq!(p.anomalies.len(), 1);
    assert_eq!(
        p.rounds.values().next().unwrap().state,
        RoundState::Complete
    );
}
