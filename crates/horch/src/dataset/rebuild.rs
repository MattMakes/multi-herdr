//! `multi-herdr-dataset rebuild <exp>`: re-derive the projections from the
//! events, and compare them byte for byte with the live, incremental fold.

use anyhow::{ensure, Context, Result};
use horch_core::ids::ExperimentId;
use horch_core::measure::digest::canonical_json;
use horch_core::measure::event::EventEnvelope;
use horch_core::measure::projection::{fold, ExperimentView, Projection};
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
    ensure!(
        !events.is_empty(),
        "no events for experiment {exp} in {}",
        paths.root().display()
    );

    let (rebuilt, rebuilt_json, identical) = fold_both(&events)?;
    let view = view_of(&rebuilt, &exp)?;
    for line in report_lines(&exp, view, events.len(), read.torn_lines, &rebuilt) {
        println!("{line}");
    }
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
    let rebuilt_json = canonical(&rebuilt)?;
    let identical = rebuilt_json == canonical(&live)?;
    Ok((rebuilt, rebuilt_json, identical))
}

/// The canonical JSON of a projection.
fn canonical(projection: &Projection) -> Result<String> {
    serde_json::to_value(projection)
        .map(|value| canonical_json(&value))
        .map_err(Into::into)
}

/// The experiment in the rebuilt projection: it needs its
/// `experiment.created`.
fn view_of<'a>(rebuilt: &'a Projection, exp: &ExperimentId) -> Result<&'a ExperimentView> {
    rebuilt
        .experiments
        .get(exp)
        .with_context(|| format!("experiment {exp} has events but no experiment.created"))
}

/// The report lines: the experiment state, the counts, the rounds and the
/// anomalies.
fn report_lines(
    exp: &ExperimentId,
    view: &ExperimentView,
    event_count: usize,
    torn_lines: u32,
    rebuilt: &Projection,
) -> Vec<String> {
    let rounds = view
        .rounds
        .iter()
        .flat_map(|id| rebuilt.rounds.get(id).map(|r| (id, r)))
        .map(|(id, r)| format!("round {id} {}", r.state.as_str()));
    let anomalies = rebuilt
        .anomalies
        .iter()
        .map(|a| format!("  {}: {}", a.event_id, a.reason));
    [
        format!("experiment {exp} {}", view.state.as_str()),
        format!("events: {event_count}"),
        format!("torn lines skipped: {torn_lines}"),
    ]
    .into_iter()
    .chain(rounds)
    .chain([format!("anomalies: {}", rebuilt.anomalies.len())])
    .chain(anomalies)
    .collect()
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
        let err = view_of(&rebuilt, &exp()).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("experiment {} has events but no experiment.created", exp())
        );
    }

    #[test]
    fn the_report_lists_the_state_the_counts_and_no_anomalies() {
        let events = created_events();
        let (rebuilt, _, _) = fold_both(&events).unwrap();
        let view = view_of(&rebuilt, &exp()).unwrap();
        assert_eq!(
            report_lines(&exp(), view, events.len(), 3, &rebuilt),
            [
                format!("experiment {} PREFLIGHT", exp()),
                "events: 1".to_string(),
                "torn lines skipped: 3".to_string(),
                "anomalies: 0".to_string(),
            ]
        );
    }
}
