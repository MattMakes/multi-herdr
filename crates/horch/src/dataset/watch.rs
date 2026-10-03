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
use horch_core::measure::projection::{fold, Projection};
use horch_core::measure::store;
use horch_core::runtime::RuntimeContext;

use super::cli::WatchArgs;
use super::{exit, status};

/// How often the status is printed.
const EVERY: Duration = Duration::from_secs(3);

pub fn watch(ctx: &RuntimeContext, args: &WatchArgs) -> Result<u8> {
    let experiment = ExperimentId::new(args.experiment.as_str())?;
    let state_root = args
        .state_dir
        .clone()
        .unwrap_or_else(|| ctx.paths.state_root.clone());
    let project = match &args.project {
        Some(p) => p.clone(),
        None => ctx.paths.project()?,
    };
    let paths = DatasetPaths::new(&state_root, &project);
    // Discard stdin: nothing typed here goes anywhere.
    std::thread::spawn(|| {
        let mut sink = [0u8; 1024];
        let mut stdin = std::io::stdin();
        while matches!(stdin.read(&mut sink), Ok(n) if n > 0) {}
    });
    loop {
        let Ok(read) = store::read_all(&paths) else {
            return Ok(exit::SUCCESS);
        };
        let projection = fold(&read.events);
        if !projection.experiments.contains_key(&experiment) {
            println!("no experiment {experiment} (yet)");
        } else {
            print!(
                "{}",
                status::render(&projection, Some(&experiment), read.torn_lines)
            );
            if settled(&projection, &experiment) {
                println!("no candidate runs any more.");
                return Ok(exit::SUCCESS);
            }
        }
        if !paths.events_dir().is_dir() {
            return Ok(exit::SUCCESS);
        }
        std::thread::sleep(EVERY);
    }
}

/// Whether no candidate of `exp` can still run.
pub fn settled(projection: &Projection, exp: &ExperimentId) -> bool {
    let Some(x) = projection.experiments.get(exp) else {
        return false;
    };
    if x.state == RoundState::Aborted {
        return true;
    }
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
