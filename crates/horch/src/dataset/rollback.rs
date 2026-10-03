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
use horch_core::measure::recorder::JsonlRecorder;
use horch_core::measure::store::StoreOptions;
use horch_core::runtime::RuntimeContext;
use horch_core::vcs::git::GitCli;

use super::cli::RollbackArgs;
use super::promote::find_round;
use super::{dataset_paths, exit};

pub fn rollback(ctx: &RuntimeContext, args: &RollbackArgs) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let (round, view, _) = find_round(&paths, &args.round)?;
    let Some(completed) = &view.promotion.completed else {
        println!(
            "REFUSED: round {round} is {} and was never promoted",
            view.state
        );
        return Ok(exit::FAILURE);
    };
    if !matches!(view.state, RoundState::Promoted | RoundState::Complete) {
        println!(
            "REFUSED: round {round} is {}; rollback needs PROMOTED or COMPLETE",
            view.state
        );
        return Ok(exit::FAILURE);
    }
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
    let recorder = JsonlRecorder::open(&paths, StoreOptions::from_faults(&ctx.settings.faults))?;
    // Rollback validates nothing; the engine only needs a validator type.
    let validator =
        CommandValidator::new(Vec::new(), paths.root().to_path_buf(), Default::default());
    let engine = GitPromotionEngine {
        git: &git,
        validator: &validator,
        recorder: &recorder,
        paths: &paths,
        faults: &ctx.settings.faults,
    };
    match engine.rollback(&view.experiment_id, &round, &target)? {
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
