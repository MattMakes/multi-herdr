//! Competition mode of the fleet (docs/specs/fleet-dataset.md §7):
//! the round's report line (FDS-11) and promotion onto a branch that does
//! not exist yet (FDS-12).
//!
//! Real git touches only temp repos (NFR-07), with an empty
//! `GIT_CONFIG_GLOBAL` and pinned identities and dates. Without git on
//! `PATH` the git tests are skipped, unless `HORCH_REQUIRE_GIT=1`. The
//! fixture is a copy of the one in `tests/promotion.rs`: test files do not
//! share code.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use horch_core::competition::coordinator::RoundOutcome;
use horch_core::competition::model::RoundState;
use horch_core::competition::promotion::{
    BranchRef, FaultFired, GitPromotionEngine, PromotionEngine, PromotionPlan, PromotionResult,
    PromotionStrategy, RollbackResult, ABORT_AFTER_PROMOTION_STARTED,
};
use horch_core::competition::report::{deliver, RoundReport};
use horch_core::evaluation::validator::{ValidationReport, Validator};
use horch_core::evaluation::winner::RejectReason;
use horch_core::harness::HarnessKind;
use horch_core::ids::{ExecutionId, ExperimentId, JudgmentId, ModelId, RoundId, TeammateName};
use horch_core::measure::event::{
    CandidatePlanned, EventEnvelope, EventKind, InterventionSource, PromotionIntent,
    PromotionStarted, RoundCreated, RoundNeedsIntervention, SlotKind, WinnerSelected,
    WorktreeCreated,
};
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::projection::{
    CandidateView, JudgeView, Projection, PromotionView, RoundView,
};
use horch_core::measure::recorder::JsonlRecorder;
use horch_core::measure::store::StoreOptions;
use horch_core::runtime::fault::Faults;
use horch_core::teacher::TeacherRef;
use horch_core::vcs::git::{GitCli, GitClient, GitIdentity};
use horch_core::vcs::worktree::{FrozenCandidate, WorktreeManager, WorktreeSpec};
use horch_core::workspace::client::WorkspaceClient;
use horch_core::workspace::testing::FakeWorkspace;
use horch_marketplace::git::GitRunner;
use tempfile::TempDir;

/// The branch a competition round promotes onto. It does not exist before.
const NEW_BRANCH: &str = "compete/greeting";

/// The winner's configuration id.
const CONFIG_ID: &str = "claude/sonnet/medium";

// ─── fixture (a copy of tests/promotion.rs) ─────────────────────────────────

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

/// A validator that passes every candidate.
struct Passing;

impl Validator for Passing {
    fn validate(&self, c: &FrozenCandidate) -> anyhow::Result<ValidationReport> {
        Ok(ValidationReport {
            validation_id: "v-reval".into(),
            label: c.label.clone(),
            head_sha: c.head_sha.clone(),
            gates: Vec::new(),
            mechanical_score: 1.0,
            eligible: true,
        })
    }
}

/// A repo on `main` with `a.txt`, and candidate `A` frozen with `a.txt`
/// changed and `new.txt` added.
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
        let base = commit(&runner, &repo, "base");

        let mgr = WorktreeManager { git: &git };
        let spec = WorktreeSpec {
            repo: repo.clone(),
            root: root.join("wt"),
            exp8: "0a1b2c3d".into(),
            round_index: 1,
            label: "A".into(),
            base_sha: base.clone(),
        };
        let wt = mgr.create(&spec).unwrap();
        std::fs::write(wt.join("a.txt"), "one\ntwo by A\nthree\n").unwrap();
        std::fs::write(wt.join("new.txt"), "new\n").unwrap();
        let candidate = mgr
            .freeze(&spec, &ExecutionId::mint(freeze_at()), freeze_at())
            .unwrap();

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

    fn rev(&self, rev: &str) -> Option<String> {
        self.git.rev_parse(&self.repo, rev).unwrap()
    }

    fn target(&self) -> BranchRef {
        BranchRef {
            repo: self.repo.clone(),
            name: NEW_BRANCH.into(),
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

    fn engine<'a>(
        &'a self,
        faults: &'a Faults,
    ) -> GitPromotionEngine<'a, GitCli, Passing, JsonlRecorder> {
        GitPromotionEngine {
            git: &self.git,
            validator: &Passing,
            recorder: &self.recorder,
            paths: &self.paths,
            faults,
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
        run(
            &self.runner,
            &self.repo,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads/mh/promote",
            ],
        )
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

/// A decided round whose winner `c` goes to [`NEW_BRANCH`], in REVALIDATING
/// as the projection shows it after `winner.selected{requested}`.
fn decided_round(c: &FrozenCandidate) -> Projection {
    let view = RoundView {
        experiment_id: ExperimentId::new("exp-1").unwrap(),
        state: RoundState::Revalidating,
        created: RoundCreated {
            index: 1,
            base_sha: c.base_sha.clone(),
            labels: vec![c.label.clone()],
            eligible_set: Vec::new(),
            propensities: BTreeMap::new(),
            teacher: TeacherRef::none(),
            seed: 1,
            label_policy_version: "slot-order-1".into(),
        },
        candidates: BTreeMap::from([(
            c.label.clone(),
            CandidateView {
                execution_id: Some(c.execution_id.clone()),
                planned: Some(CandidatePlanned {
                    label: c.label.clone(),
                    teammate: TeammateName::new("sonnet").unwrap(),
                    harness: HarnessKind::Claude,
                    model: ModelId::new("sonnet").unwrap(),
                    effort: Some("medium".into()),
                    slot: SlotKind::Baseline,
                    propensity: 1.0,
                    config_id: CONFIG_ID.into(),
                }),
                worktree: Some(WorktreeCreated {
                    label: c.label.clone(),
                    path: c.worktree.clone(),
                    branch: c.branch.clone(),
                    base_sha: c.base_sha.clone(),
                }),
                ..CandidateView::default()
            },
        )]),
        judge: JudgeView::default(),
        winner: Some(WinnerSelected {
            label: c.label.clone(),
            execution_id: c.execution_id.clone(),
            head_sha: c.head_sha.clone(),
            judgment_id: JudgmentId::new("j-1").unwrap(),
            promotion: PromotionIntent::Requested {
                target: NEW_BRANCH.into(),
            },
        }),
        rejected: None,
        promotion: PromotionView::default(),
        needs_intervention: None,
        cleaned_from: None,
        final_outcome: None,
        cleanup_failures: Vec::new(),
        outcomes: Vec::new(),
    };
    let mut p = Projection::default();
    p.rounds.insert(RoundId::new("r-1").unwrap(), view);
    p
}

// ─── FDS-12 ─────────────────────────────────────────────────────────────────

/// `--promote-to compete/<slug>` names a branch that does not exist: the
/// promotion creates it at the winner's commit by a compare-and-swap
/// create, writes the receipt, and takes the round to PROMOTED. No checkout
/// moves. A rollback deletes the created branch again.
#[test]
fn fds_12_promote_creates_new_branch() {
    let Some(f) = Fixture::new() else { return };
    let refname = format!("refs/heads/{NEW_BRANCH}");
    assert_eq!(f.rev(&refname), None, "the branch is absent before");
    let zero = "0".repeat(f.base.len());

    let faults = Faults::default();
    let PromotionResult::Promoted(receipt) = f
        .engine(&faults)
        .promote(&f.candidate, &f.target(), &f.plan())
        .unwrap()
    else {
        panic!("not promoted")
    };
    assert_eq!(f.rev(&refname), Some(f.candidate.head_sha.clone()));
    assert_eq!(receipt.dest_ref, refname);
    assert_eq!(receipt.dest_before, zero);
    assert_eq!(receipt.dest_after, f.candidate.head_sha);
    assert_eq!(receipt.strategy, PromotionStrategy::FastForward);
    assert_eq!(receipt.publish, "update_ref_cas");
    assert_eq!(
        receipt.source_shas,
        std::slice::from_ref(&f.candidate.head_sha)
    );
    // The receipt is on disk, as `promotions/<round>.json`.
    let on_disk = std::fs::read(f.paths.promotion(&f.plan().round).unwrap()).unwrap();
    let on_disk: serde_json::Value = serde_json::from_slice(&on_disk).unwrap();
    assert_eq!(on_disk["dest_after"], f.candidate.head_sha);
    assert_eq!(f.kinds(), ["promotion.started", "promotion.completed"]);
    assert_eq!(f.temp_branches(), "");
    // main and its checkout are untouched.
    assert_eq!(f.rev("refs/heads/main"), Some(f.base.clone()));
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), "");

    // The events take the decided round to PROMOTED without anomalies.
    let mut p = decided_round(&f.candidate);
    for e in f.events() {
        p.apply(&e);
    }
    assert_eq!(p.anomalies, []);
    assert_eq!(
        p.rounds.values().next().unwrap().state,
        RoundState::Promoted
    );

    // A rollback deletes the created branch, once.
    let plan = f.plan();
    for _ in 0..2 {
        let got = f
            .engine(&faults)
            .rollback(&plan.experiment, &plan.round, &f.target())
            .unwrap();
        assert_eq!(
            got,
            RollbackResult::RolledBack {
                restored: zero.clone()
            }
        );
        assert_eq!(f.rev(&refname), None);
    }
}

/// A crash after `promotion.started` leaves the branch absent; the restart
/// creates it once. A branch that someone else created in between is never
/// moved: the round needs the operator.
#[test]
fn fds_12_promote_new_branch_restart() {
    let Some(f) = Fixture::new() else { return };
    let refname = format!("refs/heads/{NEW_BRANCH}");
    let crash = Faults::parse(Some(ABORT_AFTER_PROMOTION_STARTED));
    let err = f
        .engine(&crash)
        .promote(&f.candidate, &f.target(), &f.plan())
        .unwrap_err();
    assert!(err.downcast_ref::<FaultFired>().is_some(), "{err:#}");
    assert_eq!(f.rev(&refname), None);
    let faults = Faults::default();
    let started = started_of(&f.events());
    let got = f
        .engine(&faults)
        .resume_promotion(&f.candidate, &f.target(), &f.plan(), &started)
        .unwrap();
    assert!(matches!(got, PromotionResult::Promoted(_)), "{got:?}");
    assert_eq!(f.rev(&refname), Some(f.candidate.head_sha.clone()));
    assert_eq!(f.kinds(), ["promotion.started", "promotion.completed"]);

    let Some(g) = Fixture::new() else { return };
    g.engine(&crash)
        .promote(&g.candidate, &g.target(), &g.plan())
        .unwrap_err();
    run(&g.runner, &g.repo, &["branch", NEW_BRANCH, &g.base]);
    let started = started_of(&g.events());
    let got = g
        .engine(&faults)
        .resume_promotion(&g.candidate, &g.target(), &g.plan(), &started)
        .unwrap();
    assert!(
        matches!(got, PromotionResult::NeedsIntervention { .. }),
        "{got:?}"
    );
    assert_eq!(g.rev(&refname), Some(g.base.clone()), "not moved");
    assert!(!g.paths.promotion(&g.plan().round).unwrap().exists());
}

// ─── FDS-11 ─────────────────────────────────────────────────────────────────

/// At the terminal state the coordinator types exactly 1 STE line into the
/// report pane, the way `horch tell` does: the state, the winner's config
/// id, the promoted branch and commit, and the reason. A pane that does not
/// take the line gives `false` and nothing else.
#[test]
fn fds_11_report_line_on_terminal_state() {
    let Some(f) = Fixture::new() else { return };
    let faults = Faults::default();
    f.engine(&faults)
        .promote(&f.candidate, &f.target(), &f.plan())
        .unwrap();
    let mut p = decided_round(&f.candidate);
    for e in f.events() {
        p.apply(&e);
    }
    let promoted = p.rounds.values().next().unwrap().clone();
    let plan = f.plan();
    let exp8 = plan.experiment.short();
    let report = |outcome: &RoundOutcome, view: &RoundView, reason: Option<&str>| {
        RoundReport::from_view(&plan.experiment, &plan.round, outcome, view, reason)
    };

    // DECIDED and promoted: 1 `agent prompt` call carries the whole line.
    let ws = FakeWorkspace::new();
    let pane = ws
        .workspace_create("orchestrator", None, false)
        .unwrap()
        .root_pane_id;
    let decided = report(&RoundOutcome::Decided, &promoted, None);
    assert!(deliver(&ws, &pane, &decided));
    let want = format!(
        "[compete-{exp8}] DONE: round r-1 is DECIDED. Winner: {CONFIG_ID}. \
         Promoted: {NEW_BRANCH}@{}. Reason: none.",
        f.candidate.head_sha
    );
    let typed: Vec<_> = ws
        .calls()
        .into_iter()
        .filter(|c| c.method == "agent_prompt")
        .collect();
    assert_eq!(typed.len(), 1, "{typed:?}");
    assert_eq!(typed[0].args, [pane.clone(), want]);
    assert!(!ws.calls().iter().any(|c| c.method == "pane_send_text"));

    // DECIDED without a promotion.
    let mut plain = promoted.clone();
    plain.promotion = PromotionView::default();
    assert_eq!(
        report(&RoundOutcome::Decided, &plain, None).line(),
        format!(
            "[compete-{exp8}] DONE: round r-1 is DECIDED. Winner: {CONFIG_ID}. \
             Promoted: none. Reason: none."
        )
    );

    // NEEDS_INTERVENTION: the latest reason wins over the projection's,
    // on 1 line, without a doubled full stop.
    let mut stuck = plain.clone();
    stuck.needs_intervention = Some(RoundNeedsIntervention {
        reason: "an older reason".into(),
        source: InterventionSource::Promotion,
    });
    assert_eq!(
        report(
            &RoundOutcome::NeedsIntervention,
            &stuck,
            Some("the winner conflicts with\ncompete/greeting in a.txt.")
        )
        .line(),
        format!(
            "[compete-{exp8}] DONE: round r-1 is NEEDS_INTERVENTION. Winner: {CONFIG_ID}. \
             Promoted: none. Reason: the winner conflicts with compete/greeting in a.txt."
        )
    );
    assert!(report(&RoundOutcome::NeedsIntervention, &stuck, None)
        .line()
        .ends_with("Reason: an older reason."));

    // REJECTED: no winner, the reject reason or the budget.
    let mut rejected = plain.clone();
    rejected.rejected = Some(RejectReason::NoEligible);
    let line = report(&RoundOutcome::Rejected { budget: false }, &rejected, None).line();
    assert_eq!(
        line,
        format!(
            "[compete-{exp8}] DONE: round r-1 is REJECTED. Winner: none. Promoted: none. \
             Reason: {}.",
            RejectReason::NoEligible
        )
    );
    assert!(
        report(&RoundOutcome::Rejected { budget: true }, &rejected, None)
            .line()
            .ends_with("Reason: the budget stopped the candidates.")
    );

    // A coordinator that stopped on an error says how to go on.
    let stopped = RoundReport::stopped(&plan.experiment, &plan.round, "git failed").line();
    assert!(
        stopped.starts_with(&format!("[compete-{exp8}] DONE: round r-1 is STOPPED.")),
        "{stopped}"
    );
    assert!(
        stopped.contains("multi-herdr-dataset resume exp-1"),
        "{stopped}"
    );

    // A pane that is gone: false, and the line is not typed anywhere else.
    let gone = FakeWorkspace::new();
    assert!(!deliver(&gone, "w9:p9", &decided));
    assert!(gone
        .calls()
        .iter()
        .filter(|c| c.args.len() > 1)
        .all(|c| c.args[0] == "w9:p9"));
}
