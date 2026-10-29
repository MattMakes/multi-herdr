//! `multi-herdr-dataset promote <round> --to <branch>`: the operator promotes
//! a round's winner later (PRO-08, dataset design §5.2).
//!
//! A round can be promoted from COMPLETE (a default `run` collected it) or
//! from NEEDS_INTERVENTION, when it has a winner. The command records
//! `operator.promote{target}` first, which brings the round back to
//! DECIDED and on to REVALIDATING, then re-enters the experiment as
//! `resume` does: the coordinator runs the promotion engine and cleans up
//! after the receipt.
//!
//! The engine checks the frozen commit on the candidate branch alone when
//! the worktree is gone, so a round that cleanup already left without
//! worktrees still promotes. Only a round whose winner branch was pruned
//! (`--prune-branches`) is refused.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Result};
use horch_core::clock;
use horch_core::competition::model::RoundState;
use horch_core::competition::promotion::BranchRef;
use horch_core::ids::RoundId;
use horch_core::measure::event::{
    Actor, EventEnvelope, EventKind, OperatorPromote, WinnerSelected,
};
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::projection::{fold, RoundView};
use horch_core::measure::recorder::{JsonlRecorder, NewEvent, Recorder};
use horch_core::measure::store::{self, StoreOptions};
use horch_core::runtime::RuntimeContext;
use horch_core::vcs::git::{GitCli, GitClient};

use super::cli::PromoteArgs;
use super::{dataset_paths, exit, run};

pub(crate) fn promote(
    ctx: &mut RuntimeContext,
    env: &BTreeMap<String, String>,
    args: &PromoteArgs,
) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let (round, view, events) = find_round(&paths, &args.round)?;
    if let Err(refusal) = check(ctx, &paths, &round, &view, &args.to) {
        println!("REFUSED: {refusal}");
        return Ok(exit::FAILURE);
    }
    let attempt = events
        .iter()
        .filter(|e| e.round_id.as_ref() == Some(&round) && e.kind == "operator.promote")
        .count()
        + 1;
    let recorder = JsonlRecorder::open(&paths, StoreOptions::from_faults(&ctx.settings.faults))?;
    recorder.append(NewEvent {
        kind: EventKind::OperatorPromote(OperatorPromote {
            target: args.to.clone(),
        }),
        actor: Actor::Operator,
        experiment_id: view.experiment_id.clone(),
        round_id: Some(round.clone()),
        execution_id: None,
        idempotency_key: format!("operator.promote:{round}:{attempt}"),
        occurred_at: clock::now(),
    })?;
    drop(recorder);
    println!("round {round}: promoting the winner onto {}", args.to);
    run::resume(ctx, env, view.experiment_id.as_str())
}

/// The round `id`, as the events give it, and the events.
pub(super) fn find_round(
    paths: &DatasetPaths,
    id: &str,
) -> Result<(RoundId, RoundView, Vec<EventEnvelope>)> {
    let round = RoundId::new(id)?;
    let events = store::read_all(paths)?.events;
    let Some(view) = fold(&events).rounds.remove(&round) else {
        bail!("no such round {round} in {}", paths.root().display());
    };
    Ok((round, view, events))
}

/// Why the round cannot be promoted now, if it cannot. The rules run in
/// order: the round, the receipt, the frozen branch, the target.
fn check(
    ctx: &RuntimeContext,
    paths: &DatasetPaths,
    round: &RoundId,
    view: &RoundView,
    target: &str,
) -> Result<(), String> {
    let winner = check_round(round, view)?;
    check_receipt(paths, round)?;
    let project = ctx.paths.project().map_err(|e| format!("{e:#}"))?;
    let git = GitCli::new(ctx.bins.harness.git.clone());
    check_frozen(&git, &project, view, winner)?;
    check_target(&git, &project, target)
}

/// `Err(why)` unless `ok`.
fn require(ok: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(why())
    }
}

/// The round is COMPLETE or NEEDS_INTERVENTION, has a winner and did not
/// reject it. Gives the winner.
fn check_round<'a>(round: &RoundId, view: &'a RoundView) -> Result<&'a WinnerSelected, String> {
    let state = view.state;
    require(
        matches!(state, RoundState::Complete | RoundState::NeedsIntervention),
        || format!("round {round} is {state}; promote needs COMPLETE or NEEDS_INTERVENTION"),
    )?;
    let winner = view
        .winner
        .as_ref()
        .ok_or_else(|| format!("round {round} is {state} without a winner"))?;
    require(view.rejected.is_none(), || {
        format!(
            "round {round} rejected its winner: {}",
            view.rejected.map(|r| r.to_string()).unwrap_or_default()
        )
    })?;
    Ok(winner)
}

/// The round has no promotion receipt yet.
fn check_receipt(paths: &DatasetPaths, round: &RoundId) -> Result<(), String> {
    let receipt = paths.promotion(round).map_err(|e| e.to_string())?;
    require(!receipt.exists(), || {
        format!(
            "round {round} is already promoted: {} exists",
            receipt.display()
        )
    })
}

/// The frozen commit is still reachable from its branch: the winner was
/// frozen, its branch exists and it did not move.
fn check_frozen(
    git: &GitCli,
    project: &Path,
    view: &RoundView,
    winner: &WinnerSelected,
) -> Result<(), String> {
    let candidate = &view.candidates[&winner.label];
    let (wt, frozen) = candidate
        .worktree
        .as_ref()
        .zip(candidate.frozen.as_ref())
        .ok_or_else(|| format!("the winner {} was never frozen", winner.label))?;
    let tip = git
        .rev_parse(project, &format!("refs/heads/{}", wt.branch))
        .map_err(|e| format!("{e:#}"))?;
    require(tip.is_some(), || {
        format!(
            "the round was cleaned up and the winner's branch {} is gone; \
             promotion needs the frozen worktree or its branch",
            wt.branch
        )
    })?;
    require(tip.as_deref() == Some(frozen.head_sha.as_str()), || {
        format!(
            "the winner's branch {} moved from the judged commit {}",
            wt.branch, frozen.head_sha
        )
    })
}

/// The target branch exists.
fn check_target(git: &GitCli, project: &Path, target: &str) -> Result<(), String> {
    let dest = BranchRef {
        repo: project.to_path_buf(),
        name: target.to_string(),
    };
    let tip = git
        .rev_parse(project, &dest.refname())
        .map_err(|e| format!("{e:#}"))?;
    require(tip.is_some(), || {
        format!("the target {} does not exist", dest.refname())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::competition::model::RoundState;
    use horch_core::evaluation::winner::RejectReason;
    use horch_core::ids::{ExecutionId, ExperimentId, JudgmentId};
    use horch_core::measure::digest::sha256_bytes;
    use horch_core::measure::event::{
        CandidateFrozen, PromotionIntent, RoundCreated, WorktreeCreated,
    };
    use horch_core::measure::projection::{CandidateView, JudgeView, PromotionView};
    use horch_core::teacher::TeacherRef;
    use std::path::PathBuf;
    use std::process::Command;

    const UUID: &str = "0199a5b0-0000-7000-8000-00000000000";

    fn round() -> RoundId {
        RoundId::new(format!("{UUID}1")).unwrap()
    }

    fn winner() -> WinnerSelected {
        WinnerSelected {
            label: "A".into(),
            execution_id: ExecutionId::new(format!("{UUID}2")).unwrap(),
            head_sha: "abc".into(),
            judgment_id: JudgmentId::new(format!("{UUID}3")).unwrap(),
            promotion: PromotionIntent::NotRequested,
        }
    }

    /// A round in `state` whose winner `A` was frozen at `head_sha` on `branch`.
    fn view(state: RoundState, head_sha: &str, branch: &str) -> RoundView {
        let candidate = CandidateView {
            worktree: Some(WorktreeCreated {
                label: "A".into(),
                path: PathBuf::from("/nowhere"),
                branch: branch.into(),
                base_sha: "base".into(),
            }),
            frozen: Some(CandidateFrozen {
                label: "A".into(),
                head_sha: head_sha.into(),
                numstat: Vec::new(),
                diff_digest: sha256_bytes(b"diff"),
            }),
            ..CandidateView::default()
        };
        RoundView {
            experiment_id: ExperimentId::new(format!("{UUID}4")).unwrap(),
            state,
            created: RoundCreated {
                index: 0,
                base_sha: "base".into(),
                labels: vec!["A".into()],
                eligible_set: Vec::new(),
                propensities: [("A".to_string(), 1.0)].into(),
                teacher: TeacherRef::none(),
                seed: 1,
                label_policy_version: "slot-order-1".into(),
            },
            candidates: [("A".to_string(), candidate)].into(),
            judge: JudgeView::default(),
            winner: Some(winner()),
            rejected: None,
            promotion: PromotionView::default(),
            needs_intervention: None,
            cleaned_from: None,
            final_outcome: None,
            cleanup_failures: Vec::new(),
            outcomes: Vec::new(),
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    }

    /// A repo with 1 commit on `main` and a branch `compete/a` on it.
    /// Gives the repo dir, its git client and the commit.
    fn repo() -> (tempfile::TempDir, GitCli, String) {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["commit", "-q", "--allow-empty", "-m", "one"]);
        git(dir.path(), &["branch", "compete/a"]);
        let sha = git(dir.path(), &["rev-parse", "HEAD"]);
        (dir, GitCli::new(PathBuf::from("git")), sha)
    }

    #[test]
    fn require_gives_the_reason_only_when_the_rule_fails() {
        assert_eq!(require(true, || unreachable!()), Ok(()));
        assert_eq!(require(false, || "why".into()), Err("why".to_string()));
    }

    #[test]
    fn refuses_a_round_in_another_state() {
        let v = view(RoundState::Running, "x", "b");
        assert_eq!(
            check_round(&round(), &v).unwrap_err(),
            format!(
                "round {} is RUNNING; promote needs COMPLETE or NEEDS_INTERVENTION",
                round()
            )
        );
    }

    #[test]
    fn refuses_a_round_without_a_winner() {
        let mut v = view(RoundState::NeedsIntervention, "x", "b");
        v.winner = None;
        assert_eq!(
            check_round(&round(), &v).unwrap_err(),
            format!("round {} is NEEDS_INTERVENTION without a winner", round())
        );
    }

    #[test]
    fn refuses_a_round_that_rejected_its_winner() {
        let mut v = view(RoundState::Complete, "x", "b");
        v.rejected = Some(RejectReason::Tie);
        assert_eq!(
            check_round(&round(), &v).unwrap_err(),
            format!("round {} rejected its winner: tie", round())
        );
    }

    #[test]
    fn accepts_a_complete_round_and_gives_its_winner() {
        let v = view(RoundState::Complete, "x", "b");
        assert_eq!(check_round(&round(), &v).unwrap().label, "A");
    }

    #[test]
    fn refuses_a_round_with_a_receipt() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DatasetPaths::from_slug(tmp.path(), "p");
        assert_eq!(check_receipt(&paths, &round()), Ok(()));
        let receipt = paths.promotion(&round()).unwrap();
        std::fs::create_dir_all(receipt.parent().unwrap()).unwrap();
        std::fs::write(&receipt, "{}").unwrap();
        assert_eq!(
            check_receipt(&paths, &round()).unwrap_err(),
            format!(
                "round {} is already promoted: {} exists",
                round(),
                receipt.display()
            )
        );
    }

    #[test]
    fn refuses_a_winner_that_was_never_frozen() {
        let (dir, git, _) = repo();
        let mut v = view(RoundState::Complete, "x", "compete/a");
        v.candidates.get_mut("A").unwrap().frozen = None;
        assert_eq!(
            check_frozen(&git, dir.path(), &v, &winner()).unwrap_err(),
            "the winner A was never frozen"
        );
    }

    #[test]
    fn refuses_a_winner_whose_branch_is_gone() {
        let (dir, git, sha) = repo();
        let v = view(RoundState::Complete, &sha, "compete/pruned");
        assert_eq!(
            check_frozen(&git, dir.path(), &v, &winner()).unwrap_err(),
            "the round was cleaned up and the winner's branch compete/pruned is gone; \
             promotion needs the frozen worktree or its branch"
        );
    }

    #[test]
    fn refuses_a_winner_whose_branch_moved() {
        let (dir, git, sha) = repo();
        git_commit(dir.path());
        git_on_branch(dir.path(), "compete/a");
        let v = view(RoundState::Complete, &sha, "compete/a");
        assert_eq!(
            check_frozen(&git, dir.path(), &v, &winner()).unwrap_err(),
            format!("the winner's branch compete/a moved from the judged commit {sha}")
        );
    }

    /// Move `compete/a` to a new commit.
    fn git_commit(dir: &Path) {
        git(dir, &["checkout", "-q", "compete/a"]);
        git(dir, &["commit", "-q", "--allow-empty", "-m", "two"]);
        git(dir, &["checkout", "-q", "main"]);
    }

    fn git_on_branch(dir: &Path, branch: &str) {
        assert_ne!(
            git(dir, &["rev-parse", "main"]),
            git(dir, &["rev-parse", branch])
        );
    }

    #[test]
    fn accepts_a_winner_whose_branch_sits_on_the_frozen_commit() {
        let (dir, git, sha) = repo();
        let v = view(RoundState::Complete, &sha, "compete/a");
        assert_eq!(check_frozen(&git, dir.path(), &v, &winner()), Ok(()));
    }

    #[test]
    fn refuses_a_target_that_does_not_exist() {
        let (dir, git, _) = repo();
        assert_eq!(
            check_target(&git, dir.path(), "nope").unwrap_err(),
            "the target refs/heads/nope does not exist"
        );
        assert_eq!(check_target(&git, dir.path(), "main"), Ok(()));
    }
}
