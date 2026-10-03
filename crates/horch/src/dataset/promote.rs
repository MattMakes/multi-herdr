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

use anyhow::{bail, Result};
use horch_core::clock;
use horch_core::competition::model::RoundState;
use horch_core::competition::promotion::BranchRef;
use horch_core::ids::RoundId;
use horch_core::measure::event::{Actor, EventEnvelope, EventKind, OperatorPromote};
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::projection::{fold, RoundView};
use horch_core::measure::recorder::{JsonlRecorder, NewEvent, Recorder};
use horch_core::measure::store::{self, StoreOptions};
use horch_core::runtime::RuntimeContext;
use horch_core::vcs::git::{GitCli, GitClient};

use super::cli::PromoteArgs;
use super::{dataset_paths, exit, run};

pub fn promote(
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

/// Why the round cannot be promoted now, if it cannot.
fn check(
    ctx: &RuntimeContext,
    paths: &DatasetPaths,
    round: &RoundId,
    view: &RoundView,
    target: &str,
) -> Result<(), String> {
    let state = view.state;
    if !matches!(state, RoundState::Complete | RoundState::NeedsIntervention) {
        return Err(format!(
            "round {round} is {state}; promote needs COMPLETE or NEEDS_INTERVENTION"
        ));
    }
    let Some(winner) = &view.winner else {
        return Err(format!("round {round} is {state} without a winner"));
    };
    if let Some(reason) = view.rejected {
        return Err(format!("round {round} rejected its winner: {reason}"));
    }
    let receipt = paths.promotion(round).map_err(|e| e.to_string())?;
    if receipt.exists() {
        return Err(format!(
            "round {round} is already promoted: {} exists",
            receipt.display()
        ));
    }
    // The frozen commit must still be reachable from its branch.
    let candidate = &view.candidates[&winner.label];
    let (Some(wt), Some(frozen)) = (&candidate.worktree, &candidate.frozen) else {
        return Err(format!("the winner {} was never frozen", winner.label));
    };
    let project = ctx.paths.project().map_err(|e| format!("{e:#}"))?;
    let git = GitCli::new(ctx.bins.harness.git.clone());
    let tip = git
        .rev_parse(&project, &format!("refs/heads/{}", wt.branch))
        .map_err(|e| format!("{e:#}"))?;
    if tip.is_none() {
        return Err(format!(
            "the round was cleaned up and the winner's branch {} is gone; \
             promotion needs the frozen worktree or its branch",
            wt.branch
        ));
    }
    if tip.as_deref() != Some(frozen.head_sha.as_str()) {
        return Err(format!(
            "the winner's branch {} moved from the judged commit {}",
            wt.branch, frozen.head_sha
        ));
    }
    let dest = BranchRef {
        repo: project.clone(),
        name: target.to_string(),
    };
    match git.rev_parse(&project, &dest.refname()) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(format!("the target {} does not exist", dest.refname())),
        Err(e) => Err(format!("{e:#}")),
    }
}
