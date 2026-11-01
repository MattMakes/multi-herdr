//! `multi-herdr-dataset rollback <round>`: move a promoted round's target
//! branch back to `dest_before` by compare-and-swap (dataset design §5.2).
//!
//! The round must hold a receipt. A target that moved since the promotion,
//! or that a worktree has checked out, is left alone: the command prints
//! why and exits 5. A second rollback changes nothing.

use anyhow::Result;
use horch_core::competition::model::RoundState;
use horch_core::competition::promotion::{BranchRef, GitPromotionEngine, RollbackResult};
use horch_core::evaluation::validator::CommandValidator;
use horch_core::ids::RoundId;
use horch_core::measure::event::PromotionCompleted;
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::projection::RoundView;
use horch_core::measure::recorder::JsonlRecorder;
use horch_core::measure::store::StoreOptions;
use horch_core::runtime::RuntimeContext;
use horch_core::vcs::git::GitCli;

use super::cli::RollbackArgs;
use super::promote::find_round;
use super::{dataset_paths, exit};

pub(crate) fn rollback(ctx: &RuntimeContext, args: &RollbackArgs) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let (round, view, _) = find_round(&paths, &args.round)?;
    receipt(&round, &view).map_or_else(refused, |completed| {
        run_rollback(ctx, &paths, &round, &view, completed)
    })
}

/// The receipt of the promotion of `round`, or the line that refuses its
/// rollback: it was never promoted, or it is not PROMOTED or COMPLETE. The
/// first of the 2 reasons wins.
fn receipt<'a>(round: &RoundId, view: &'a RoundView) -> Result<&'a PromotionCompleted, String> {
    let Some(completed) = &view.promotion.completed else {
        return Err(format!(
            "REFUSED: round {round} is {} and was never promoted",
            view.state
        ));
    };
    if matches!(view.state, RoundState::Promoted | RoundState::Complete) {
        Ok(completed)
    } else {
        Err(format!(
            "REFUSED: round {round} is {}; rollback needs PROMOTED or COMPLETE",
            view.state
        ))
    }
}

/// Print a refusal line. The command fails and changes nothing.
fn refused(line: String) -> Result<u8> {
    println!("{line}");
    Ok(exit::FAILURE)
}

/// Move the target of `round` back to where it was before the promotion
/// that `completed` records.
fn run_rollback(
    ctx: &RuntimeContext,
    paths: &DatasetPaths,
    round: &RoundId,
    view: &RoundView,
    completed: &PromotionCompleted,
) -> Result<u8> {
    let started = view
        .promotion
        .started
        .as_ref()
        .map_or_else(String::new, |s| s.target.clone());
    let project = ctx.paths.project()?;
    let target = BranchRef {
        repo: project,
        name: started,
    };
    let git = GitCli::new(ctx.bins.harness.git.clone());
    let recorder = JsonlRecorder::open(paths, StoreOptions::from_faults(&ctx.settings.faults))?;
    // Rollback validates nothing; the engine only needs a validator type.
    let validator =
        CommandValidator::new(Vec::new(), paths.root().to_path_buf(), Default::default());
    let engine = GitPromotionEngine {
        git: &git,
        validator: &validator,
        recorder: &recorder,
        paths,
        faults: &ctx.settings.faults,
    };
    match engine.rollback(&view.experiment_id, round, &target)? {
        RollbackResult::RolledBack { restored } => {
            println!(
                "round {round}: {} is back at {restored} (was {})",
                target.refname(),
                completed.dest_after
            );
            Ok(exit::SUCCESS)
        }
        RollbackResult::NeedsIntervention { reason } => {
            println!("REFUSED: {reason}. Nothing changed.");
            Ok(exit::NEEDS_INTERVENTION)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use horch_core::ids::ExperimentId;
    use horch_core::measure::digest::sha256_bytes;
    use horch_core::measure::event::RoundCreated;
    use horch_core::measure::projection::{JudgeView, PromotionView};
    use horch_core::teacher::TeacherRef;

    use super::*;

    const UUID: &str = "0199a5b0-0000-7000-8000-00000000000";

    fn round() -> RoundId {
        RoundId::new(format!("{UUID}1")).unwrap()
    }

    fn completed() -> PromotionCompleted {
        PromotionCompleted {
            receipt_digest: sha256_bytes(b"receipt"),
            dest_after: "after".into(),
        }
    }

    /// A round in `state` that holds a receipt, or none.
    fn view(state: RoundState, receipt: Option<PromotionCompleted>) -> RoundView {
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
            candidates: BTreeMap::new(),
            judge: JudgeView::default(),
            winner: None,
            rejected: None,
            promotion: PromotionView {
                completed: receipt,
                ..PromotionView::default()
            },
            needs_intervention: None,
            cleaned_from: None,
            final_outcome: None,
            cleanup_failures: Vec::new(),
            outcomes: Vec::new(),
        }
    }

    /// A round with no receipt has nothing to roll back, in any state, and
    /// that reason comes before the one about the state.
    #[test]
    fn a_round_with_no_receipt_was_never_promoted() {
        for (state, name) in [
            (RoundState::Complete, "COMPLETE"),
            (RoundState::Promoted, "PROMOTED"),
            (RoundState::Cleanup, "CLEANUP"),
        ] {
            let v = view(state, None);
            assert_eq!(
                receipt(&round(), &v),
                Err(format!(
                    "REFUSED: round {} is {name} and was never promoted",
                    round()
                ))
            );
        }
    }

    /// The 2 states a rollback is for give the receipt back.
    #[test]
    fn a_receipt_in_promoted_or_complete_is_given_back() {
        for state in [RoundState::Promoted, RoundState::Complete] {
            let v = view(state, Some(completed()));
            assert_eq!(receipt(&round(), &v), Ok(&completed()));
        }
    }

    /// A receipt in any other state is refused, with the name of the state.
    #[test]
    fn a_receipt_in_another_state_is_refused() {
        for (state, name) in [
            (RoundState::Decided, "DECIDED"),
            (RoundState::Promoting, "PROMOTING"),
            (RoundState::NeedsIntervention, "NEEDS_INTERVENTION"),
            (RoundState::Cleanup, "CLEANUP"),
        ] {
            let v = view(state, Some(completed()));
            assert_eq!(
                receipt(&round(), &v),
                Err(format!(
                    "REFUSED: round {} is {name}; rollback needs PROMOTED or COMPLETE",
                    round()
                ))
            );
        }
    }
}
