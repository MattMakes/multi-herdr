//! `multi-herdr-dataset outcome <round> --kind …`: what happened to a
//! decided round's change after the merge (EXP-06).
//!
//! The fold records an outcome for a round without a winner as an anomaly,
//! and it never reaches the export. So the command checks the round first
//! and refuses.

use anyhow::{bail, Result};
use horch_core::clock;
use horch_core::competition::model::RoundState;
use horch_core::dataset::outcome::record_outcome;
use horch_core::ids::RoundId;
use horch_core::measure::event::OutcomeKind;
use horch_core::measure::projection::{fold, RoundView};
use horch_core::measure::recorder::{Appended, JsonlRecorder};
use horch_core::measure::store::{self, StoreOptions};
use horch_core::runtime::RuntimeContext;

use super::{dataset_paths, exit};

/// The states in which a round's winner can have an outcome.
const OUTCOME_STATES: [RoundState; 3] = [
    RoundState::Decided,
    RoundState::Promoted,
    RoundState::Complete,
];

pub fn outcome(
    ctx: &RuntimeContext,
    round: &str,
    kind: OutcomeKind,
    score: f64,
    note: Option<String>,
) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let round_id = RoundId::new(round)?;
    let projection = fold(&store::read_all(&paths)?.events);
    let Some(view) = projection.rounds.get(&round_id) else {
        bail!("no round {round_id} in {}", paths.root().display());
    };
    check_round(&round_id, view)?;
    let recorder = JsonlRecorder::open(&paths, StoreOptions::from_faults(&ctx.settings.faults))?;
    let appended = record_outcome(
        &recorder,
        &view.experiment_id,
        &round_id,
        kind,
        score,
        note,
        clock::now(),
    )?;
    match appended {
        Appended::Recorded(_) => println!("recorded the outcome of round {round_id}"),
        Appended::Duplicate(_) => println!("the outcome of round {round_id} was already recorded"),
    }
    Ok(exit::SUCCESS)
}

/// Refuse unless the round is DECIDED, PROMOTED or COMPLETE with a winner.
pub fn check_round(round_id: &RoundId, view: &RoundView) -> Result<()> {
    if !OUTCOME_STATES.contains(&view.state) {
        bail!(
            "round {round_id} is {}; an outcome needs DECIDED, PROMOTED or COMPLETE",
            view.state.as_str()
        );
    }
    if view.winner.is_none() || view.rejected.is_some() {
        bail!("round {round_id} has no winner; an outcome needs one");
    }
    Ok(())
}
