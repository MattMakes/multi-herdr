//! `multi-herdr-dataset rebuild <exp>`: re-derive the projections from the
//! events, and compare them byte for byte with the live, incremental fold.

use anyhow::{bail, Result};
use horch_core::ids::ExperimentId;
use horch_core::measure::digest::canonical_json;
use horch_core::measure::event::EventEnvelope;
use horch_core::measure::projection::{fold, Projection};
use horch_core::measure::store;
use horch_core::runtime::RuntimeContext;

use super::{dataset_paths, exit};

pub(crate) fn rebuild(ctx: &RuntimeContext, experiment: &str) -> Result<u8> {
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

    let (rebuilt, rebuilt_json, identical) = fold_both(&events)?;
    print_report(&exp, &events, read.torn_lines, &rebuilt)?;
    if identical {
        println!(
            "projection: identical to the live fold ({} bytes)",
            rebuilt_json.len()
        );
        Ok(exit::SUCCESS)
    } else {
        println!("projection: DIFFERS from the live fold");
        Ok(exit::FAILURE)
    }
}

/// Fold the events in one pass (batch), and one event at a time (live, as a
/// running coordinator applies them). Gives the batch projection, its
/// canonical JSON, and whether the live fold's canonical JSON is the same.
fn fold_both(events: &[EventEnvelope]) -> Result<(Projection, String, bool)> {
    let rebuilt = fold(events);
    let mut live = Projection::default();
    for env in events {
        live.apply(env);
    }
    let rebuilt_json = canonical_json(&serde_json::to_value(&rebuilt)?);
    let live_json = canonical_json(&serde_json::to_value(&live)?);
    let identical = rebuilt_json == live_json;
    Ok((rebuilt, rebuilt_json, identical))
}

/// Print the experiment state, the counts, the rounds and the anomalies.
fn print_report(
    exp: &ExperimentId,
    events: &[EventEnvelope],
    torn_lines: u32,
    rebuilt: &Projection,
) -> Result<()> {
    let Some(view) = rebuilt.experiments.get(exp) else {
        bail!("experiment {exp} has events but no experiment.created");
    };
    println!("experiment {exp} {}", view.state.as_str());
    println!("events: {}", events.len());
    println!("torn lines skipped: {torn_lines}");
    for round_id in &view.rounds {
        if let Some(r) = rebuilt.rounds.get(round_id) {
            println!("round {round_id} {}", r.state.as_str());
        }
    }
    println!("anomalies: {}", rebuilt.anomalies.len());
    for a in &rebuilt.anomalies {
        println!("  {}: {}", a.event_id, a.reason);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use horch_core::competition::model::RoundState;
    use horch_core::ids::TaskId;
    use horch_core::measure::digest::sha256_bytes;
    use horch_core::measure::event::{Actor, EventKind, ExperimentCreated};
    use horch_core::measure::paths::DatasetPaths;
    use horch_core::measure::recorder::{JsonlRecorder, NewEvent, Recorder};
    use horch_core::measure::store::StoreOptions;

    fn exp() -> ExperimentId {
        ExperimentId::new("0199a5b0-0000-7000-8000-000000000e01").unwrap()
    }

    /// The events of an experiment that was created and nothing else.
    fn created_events() -> Vec<EventEnvelope> {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DatasetPaths::from_slug(tmp.path(), "fixture");
        let rec = JsonlRecorder::open(&paths, StoreOptions::default()).unwrap();
        rec.append(NewEvent {
            kind: EventKind::ExperimentCreated(ExperimentCreated {
                task_id: TaskId::new("task-1").unwrap(),
                task_digest: sha256_bytes(b"task"),
                config_digest: sha256_bytes(b"config"),
                base_sha: "a".repeat(40),
                repo_digest: sha256_bytes(b"repo"),
                environment_digest: sha256_bytes(b"env"),
                candidates: 2,
                strategy: "diverse".into(),
                budget_usd_micro: 5_000_000,
                promote_to: None,
            }),
            actor: Actor::Coordinator,
            experiment_id: exp(),
            round_id: None,
            execution_id: None,
            idempotency_key: "k1".into(),
            occurred_at: Utc.with_ymd_and_hms(2026, 10, 2, 12, 0, 0).unwrap(),
        })
        .unwrap();
        store::read_all(&paths).unwrap().events
    }

    #[test]
    fn the_batch_and_the_live_fold_agree_on_a_real_event() {
        let events = created_events();
        assert_eq!(events.len(), 1);
        let (rebuilt, json, identical) = fold_both(&events).unwrap();
        assert!(identical);
        assert_eq!(rebuilt.experiments[&exp()].state, RoundState::Preflight);
        assert!(json.contains("0199a5b0-0000-7000-8000-000000000e01"));
    }

    #[test]
    fn no_events_fold_to_an_empty_projection_and_the_report_refuses_it() {
        let (rebuilt, _, identical) = fold_both(&[]).unwrap();
        assert!(identical);
        let err = print_report(&exp(), &[], 0, &rebuilt).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("experiment {} has events but no experiment.created", exp())
        );
    }

    #[test]
    fn the_report_prints_for_a_created_experiment() {
        let events = created_events();
        let (rebuilt, _, _) = fold_both(&events).unwrap();
        print_report(&exp(), &events, 0, &rebuilt).unwrap();
    }
}
