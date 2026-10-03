//! `multi-herdr-dataset rebuild <exp>`: re-derive the projections from the
//! events, and compare them byte for byte with the live, incremental fold.

use anyhow::{bail, Result};
use horch_core::ids::ExperimentId;
use horch_core::measure::digest::canonical_json;
use horch_core::measure::projection::{fold, Projection};
use horch_core::measure::store;
use horch_core::runtime::RuntimeContext;

use super::{dataset_paths, exit};

pub fn rebuild(ctx: &RuntimeContext, experiment: &str) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let exp = ExperimentId::new(experiment)?;
    let read = store::read_all(&paths)?;
    let events: Vec<_> = read
        .events
        .into_iter()
        .filter(|e| e.experiment_id == exp)
        .collect();
    if events.is_empty() {
        bail!(
            "no events for experiment {exp} in {}",
            paths.root().display()
        );
    }

    let rebuilt = fold(&events);
    // The live fold: one event at a time, as a running coordinator applies them.
    let mut live = Projection::default();
    for env in &events {
        live.apply(env);
    }
    let rebuilt_json = canonical_json(&serde_json::to_value(&rebuilt)?);
    let live_json = canonical_json(&serde_json::to_value(&live)?);
    let identical = rebuilt_json == live_json;

    let Some(view) = rebuilt.experiments.get(&exp) else {
        bail!("experiment {exp} has events but no experiment.created");
    };
    println!("experiment {exp} {}", view.state.as_str());
    println!("events: {}", events.len());
    println!("torn lines skipped: {}", read.torn_lines);
    for round_id in &view.rounds {
        if let Some(r) = rebuilt.rounds.get(round_id) {
            println!("round {round_id} {}", r.state.as_str());
        }
    }
    println!("anomalies: {}", rebuilt.anomalies.len());
    for a in &rebuilt.anomalies {
        println!("  {}: {}", a.event_id, a.reason);
    }
    if !identical {
        println!("projection: DIFFERS from the live fold");
        return Ok(exit::FAILURE);
    }
    println!(
        "projection: identical to the live fold ({} bytes)",
        rebuilt_json.len()
    );
    Ok(exit::SUCCESS)
}
