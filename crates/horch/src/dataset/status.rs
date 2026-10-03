//! `multi-herdr-dataset status [<exp>]`: the experiments and rounds, folded
//! from the events, with their state.

use anyhow::{bail, Result};
use horch_core::ids::ExperimentId;
use horch_core::measure::projection::{fold, Projection};
use horch_core::measure::store;
use horch_core::runtime::RuntimeContext;

use super::{dataset_paths, exit};
use crate::output;

pub fn status(ctx: &RuntimeContext, experiment: Option<&str>) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let read = store::read_all(&paths)?;
    let projection = fold(&read.events);
    let only = experiment.map(ExperimentId::new).transpose()?;
    if let Some(exp) = &only {
        if !projection.experiments.contains_key(exp) {
            bail!("no experiment {exp} in {}", paths.root().display());
        }
    }
    output::print(&render(&projection, only.as_ref(), read.torn_lines));
    Ok(exit::SUCCESS)
}

/// The text `status` prints.
pub fn render(projection: &Projection, only: Option<&ExperimentId>, torn_lines: u32) -> String {
    let mut out = String::new();
    if projection.experiments.is_empty() {
        out.push_str("no experiments\n");
    }
    for (id, exp) in &projection.experiments {
        if only.is_some_and(|o| o != id) {
            continue;
        }
        out.push_str(&format!(
            "experiment {id} {} task {} candidates {} base {}\n",
            exp.state.as_str(),
            exp.created.task_id,
            exp.created.candidates,
            short(&exp.created.base_sha),
        ));
        if let Some(aborted) = &exp.aborted {
            out.push_str(&format!(
                "  aborted: {} ({})\n",
                aborted.reason,
                aborted.failed_checks.join(", ")
            ));
        }
        for round_id in &exp.rounds {
            let Some(r) = projection.rounds.get(round_id) else {
                continue;
            };
            let winner = r
                .winner
                .as_ref()
                .map(|w| format!(" winner {}", w.label))
                .unwrap_or_default();
            out.push_str(&format!(
                "  round {round_id} #{} {} candidates {}{winner}\n",
                r.created.index,
                r.state.as_str(),
                r.candidates.len(),
            ));
        }
    }
    if !projection.anomalies.is_empty() {
        out.push_str(&format!(
            "{} anomalies (events not applied)\n",
            projection.anomalies.len()
        ));
    }
    if torn_lines > 0 {
        out.push_str(&format!("{torn_lines} torn lines skipped\n"));
    }
    out
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}
