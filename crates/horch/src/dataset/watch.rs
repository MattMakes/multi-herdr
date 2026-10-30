//! `multi-herdr-dataset watch <exp>`: what the dataset workspace's root pane
//! runs. It prints the experiment's status every few seconds and discards
//! anything typed into the pane, so a stray keystroke reaches no agent.
//!
//! It stops once no candidate of the experiment can still run (every round
//! is past VALIDATING, or preflight aborted), or when the experiment's
//! events are gone.

use std::io::Read;
use std::time::Duration;

use anyhow::Result;
use horch_core::competition::model::RoundState;
use horch_core::ids::ExperimentId;
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::projection::{fold, ExperimentView, Projection};
use horch_core::measure::store::{self, ReadEvents};
use horch_core::runtime::RuntimeContext;

use super::cli::WatchArgs;
use super::{exit, status};

/// How often the status is printed.
const EVERY: Duration = Duration::from_secs(3);

pub(crate) fn watch(ctx: &RuntimeContext, args: &WatchArgs) -> Result<u8> {
    let experiment = ExperimentId::new(args.experiment.as_str())?;
    let state_root = args
        .state_dir
        .clone()
        .unwrap_or_else(|| ctx.paths.state_root.clone());
    // The global `--project` flag is already in the context.
    let paths = DatasetPaths::new(&state_root, &ctx.paths.project()?);
    // Discard stdin: nothing typed here goes anywhere.
    std::thread::spawn(|| {
        let mut sink = [0u8; 1024];
        let mut stdin = std::io::stdin();
        while matches!(stdin.read(&mut sink), Ok(n) if n > 0) {}
    });
    while tick(&paths, &experiment) {
        std::thread::sleep(EVERY);
    }
    Ok(exit::SUCCESS)
}

/// One pass of the poll loop: print the view. Gives `true` while the view
/// must go on: the events can be read, the experiment is not settled and the
/// events directory is still there.
fn tick(paths: &DatasetPaths, experiment: &ExperimentId) -> bool {
    store::read_all(paths).is_ok_and(|read| show(&read, experiment)) && paths.events_dir().is_dir()
}

/// Print the status of `experiment`. Gives `false` once it is settled.
fn show(read: &ReadEvents, experiment: &ExperimentId) -> bool {
    let projection = fold(&read.events);
    if !projection.experiments.contains_key(experiment) {
        println!("no experiment {experiment} (yet)");
        return true;
    }
    print!(
        "{}",
        status::render(&projection, Some(experiment), read.torn_lines)
    );
    let settled = settled(&projection, experiment);
    if settled {
        println!("no candidate runs any more.");
    }
    !settled
}

/// Whether no candidate of `exp` can still run.
pub(crate) fn settled(projection: &Projection, exp: &ExperimentId) -> bool {
    projection
        .experiments
        .get(exp)
        .is_some_and(|x| x.state == RoundState::Aborted || all_rounds_ended(projection, x))
}

/// The experiment has a round, and every round is past VALIDATING.
fn all_rounds_ended(projection: &Projection, x: &ExperimentView) -> bool {
    !x.rounds.is_empty()
        && x.rounds.iter().all(|r| {
            projection.rounds.get(r).is_some_and(|v| {
                !matches!(
                    v.state,
                    RoundState::Planned
                        | RoundState::Provisioning
                        | RoundState::Running
                        | RoundState::Validating
                )
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::test_fixtures::{exp, projection};

    #[test]
    fn a_missing_experiment_is_not_settled() {
        assert!(!settled(&Projection::default(), &exp()));
    }

    #[test]
    fn an_aborted_experiment_is_settled() {
        assert!(settled(&projection(RoundState::Aborted, &[]), &exp()));
    }

    #[test]
    fn an_experiment_with_no_rounds_is_not_settled() {
        assert!(!settled(&projection(RoundState::Planned, &[]), &exp()));
    }

    #[test]
    fn a_running_round_is_not_settled_even_beside_a_decided_one() {
        let p = projection(
            RoundState::Planned,
            &[RoundState::Decided, RoundState::Running],
        );
        assert!(!settled(&p, &exp()));
    }

    #[test]
    fn each_round_before_judging_keeps_the_experiment_unsettled() {
        for state in [
            RoundState::Planned,
            RoundState::Provisioning,
            RoundState::Running,
            RoundState::Validating,
        ] {
            assert!(
                !settled(&projection(RoundState::Planned, &[state]), &exp()),
                "{state}"
            );
        }
    }

    #[test]
    fn rounds_past_validating_settle_the_experiment() {
        let p = projection(
            RoundState::Planned,
            &[RoundState::JudgingBackground, RoundState::Complete],
        );
        assert!(settled(&p, &exp()));
    }

    #[test]
    fn a_round_without_a_view_is_not_settled() {
        let mut p = projection(RoundState::Planned, &[RoundState::Complete]);
        p.rounds.clear();
        assert!(!settled(&p, &exp()));
    }

    #[test]
    fn tick_goes_on_while_the_events_dir_is_there_and_the_experiment_is_unknown() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DatasetPaths::from_slug(tmp.path(), "p");
        std::fs::create_dir_all(paths.events_dir()).unwrap();
        assert!(tick(&paths, &exp()));
    }

    #[test]
    fn tick_stops_when_the_events_dir_is_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DatasetPaths::from_slug(tmp.path(), "p");
        std::fs::create_dir_all(paths.events_dir()).unwrap();
        std::fs::remove_dir_all(paths.events_dir()).unwrap();
        assert!(!tick(&paths, &exp()));
    }

    #[test]
    fn show_goes_on_when_the_experiment_has_no_events_yet() {
        assert!(show(&ReadEvents::default(), &exp()));
    }
}
