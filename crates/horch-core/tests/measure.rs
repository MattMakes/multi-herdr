//! B1 measurement layer: dataset paths, the event log, the projection fold
//! and WorkerRun 1.0.0 (dataset design §2 and §4.1 to §4.3).

use std::path::{Path, PathBuf};

use horch_core::ids::ExperimentId;
use horch_core::measure::paths::DatasetPaths;
use horch_core::telemetry::collect::read_ledgers;
use serde_json::json;

fn ledger_json(record_id: &str) -> String {
    json!([{
        "record_id": record_id, "session_id": "s-1", "agent": "claude", "tier": "sonnet",
        "model": "sonnet", "role": "sonnet-1", "status": "done", "task": "t",
        "history": [], "created_at": "2026-10-02T12:00:00Z",
        "updated_at": "2026-10-02T12:00:00Z"
    }])
    .to_string()
}

#[test]
fn mea_08_dataset_dir_not_read_as_ledger() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path();
    std::fs::write(state.join("proj.json"), ledger_json("rec-root")).unwrap();

    let paths = DatasetPaths::new(state, Path::new("/work/my proj"));
    paths.ensure().unwrap();
    assert_eq!(paths.root(), state.join("multi-herdr/-work-my-proj"));
    let exp = ExperimentId::new("exp-1").unwrap();
    std::fs::create_dir_all(paths.experiment_dir(&exp)).unwrap();
    // Valid-looking ledgers at every dataset level, plus an event file.
    for p in [
        paths.root().join("ledger.json"),
        paths.root().parent().unwrap().join("ledger.json"),
        paths.manifest(&exp),
        paths.events_dir().join("x.json"),
    ] {
        std::fs::write(&p, ledger_json("rec-dataset")).unwrap();
    }
    std::fs::write(
        paths.events_file(chrono::NaiveDate::from_ymd_opt(2026, 10, 2).unwrap()),
        "{}\n",
    )
    .unwrap();

    let ids: Vec<String> = read_ledgers(state)
        .into_iter()
        .map(|r| r.record_id)
        .collect();
    assert_eq!(ids, vec!["rec-root".to_string()]);
}

#[test]
fn dataset_paths_layout() {
    let paths = DatasetPaths::from_slug(Path::new("/s"), "p");
    let exp = ExperimentId::new("e").unwrap();
    let round = horch_core::ids::RoundId::new("r").unwrap();
    let day = chrono::NaiveDate::from_ymd_opt(2026, 1, 9).unwrap();
    let want: Vec<(PathBuf, &str)> = vec![
        (
            paths.events_file(day),
            "/s/multi-herdr/p/events/2026-01-09.jsonl",
        ),
        (paths.events_lock_dir(), "/s/multi-herdr/p/events.lock"),
        (
            paths.manifest(&exp),
            "/s/multi-herdr/p/experiments/e/manifest.json",
        ),
        (
            paths.round_file(&exp, &round),
            "/s/multi-herdr/p/experiments/e/rounds/r.json",
        ),
        (
            paths.judge_input_dir(&exp, &round),
            "/s/multi-herdr/p/experiments/e/artifacts/r/judge-input",
        ),
        (
            paths.validation_dir(&exp, &round, "A"),
            "/s/multi-herdr/p/experiments/e/artifacts/r/validation/A",
        ),
        (
            paths.judgement(&round),
            "/s/multi-herdr/p/judgements/r.json",
        ),
        (
            paths.promotion(&round),
            "/s/multi-herdr/p/promotions/r.json",
        ),
        (paths.exports_dir("lp-1"), "/s/multi-herdr/p/exports/lp-1"),
        (paths.job_dir(&round, 2), "/s/multi-herdr/p/jobs/r/judge-2"),
        (
            paths.default_worktree_root(&exp),
            "/s/multi-herdr/p/worktrees/e",
        ),
    ];
    for (got, want) in want {
        assert_eq!(got, PathBuf::from(want));
    }
}
