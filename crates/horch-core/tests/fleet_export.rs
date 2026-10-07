//! Fleet rows in the export and the readiness report
//! (`docs/specs/fleet-dataset.md` §6, FDS-16, FDS-17).

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use horch_core::dataset::export::ExportRow;
use horch_core::dataset::fleet::{
    build_fleet_rows, fleet_readiness, fleet_rows, render_fleet_readiness, write_fleet_export,
    FleetExportRow,
};
use horch_core::dataset::readiness::{readiness, ReadinessThresholds};
use horch_core::fleet_runs::rows::{RunRow, StartRow, VerdictRow};
use horch_core::fleet_runs::store::{self, FleetFile};
use horch_core::measure::paths::DatasetPaths;
use serde_json::{json, Value};

const SECRET_PLAN: &str = "secret plan text";

/// A worker run row: `record`, `segment`, `session`, start time, duration
/// and input tokens (`None`: no transcript).
fn run(
    record: &str,
    segment: u32,
    session: Option<&str>,
    started_at: &str,
    duration_ms: i64,
    input: Option<u64>,
) -> RunRow {
    let tokens = input.map(|i| {
        json!({"input": i, "output": 10, "cache_read": 100,
               "cache_write_5m": 1, "cache_write_1h": 2})
    });
    serde_json::from_value(json!({
        "schema": "mh.fleet-run/1.0.0",
        "record_id": record,
        "session_id": session,
        "role": format!("{record}-role"),
        "segment": segment,
        "kind": "worker",
        "project": "/work/proj",
        "project_slug": "-work-proj",
        "teammate": "sonnet",
        "harness": "claude",
        "model": "sonnet",
        "effort": "high",
        "phase": "implementation",
        "routing": {"mode": "auto", "requested": "sonnet", "resolved": "sonnet"},
        "substitution_reason": null,
        "skills_expected": [],
        "task": format!("task of {record}"),
        "task_digest": format!("sha256:{record}"),
        "plan_path": "ai_docs/plans/x.md",
        "plan_digest": "sha256:plan",
        "base_sha": "a".repeat(40),
        "started_at": started_at,
        "finished_at": "2026-10-07T12:00:00Z",
        "duration_ms": duration_ms,
        "end": {"status": "done", "state": {"state": "done"}, "exit_code": 0},
        "done_summary": "did it",
        "notes": 1,
        "resumed": segment > 1,
        "tokens": tokens,
        "cost_microusd": input.map(|i| i as i64 * 2),
        "cost_source": if input.is_some() { "price_table@2026-09-28" } else { "unpriced" },
        "transcript_ref": null,
        "transcript_digest": null,
        "usage_at": "2026-10-07T12:00:00Z",
        "head_sha": null,
        "written_by": "done",
        "horch_version": "0.1.0"
    }))
    .unwrap()
}

fn verdict(record: &str, word: &str) -> VerdictRow {
    VerdictRow {
        schema: "mh.fleet-verdict/1.0.0".into(),
        record_id: record.into(),
        role: format!("{record}-role"),
        verdict: word.into(),
        note: None,
        at: "2026-10-07T12:30:00Z".into(),
        by: None,
    }
}

fn paths(state: &Path) -> DatasetPaths {
    DatasetPaths::new(&state.join("state"), &state.join("proj"))
}

fn keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
    k.sort();
    k
}

fn sorted(list: &[&str]) -> Vec<String> {
    let mut k: Vec<String> = list.iter().map(|s| s.to_string()).collect();
    k.sort();
    k
}

/// FDS-16: a fleet export row has every key of §6, `api` is
/// `systemone/v1`, and no `plan_text` reaches the file. An orchestrator
/// record gets no row.
#[test]
fn fds_16_export_fleet_rows_shape() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    store::append(
        &p,
        FleetFile::Starts,
        &StartRow {
            schema: "mh.fleet-start/1.0.0".into(),
            record_id: "r-1".into(),
            at: "2026-10-07T10:00:01Z".into(),
            base_sha: None,
            plan_path: Some("ai_docs/plans/x.md".into()),
            plan_digest: Some("sha256:plan".into()),
            plan_text: Some(SECRET_PLAN.into()),
            plan_truncated: false,
        },
    )
    .unwrap();
    let mut orch = run("r-orch", 1, None, "2026-10-07T09:00:00Z", 5, None);
    orch.kind = "orchestrator".into();
    for row in [
        run(
            "r-1",
            1,
            Some("s-1"),
            "2026-10-07T10:00:00Z",
            60_000,
            Some(1000),
        ),
        orch,
    ] {
        store::append(&p, FleetFile::Runs, &row).unwrap();
    }

    let rows = fleet_rows(&p).unwrap();
    assert_eq!(rows.len(), 1, "{rows:?}");
    let ts = horch_core::clock::parse("2026-10-07T13:00:00Z").unwrap();
    let file = write_fleet_export(&p, &rows, ts).unwrap();
    assert_eq!(
        file,
        p.exports_root()
            .join("fleet-observed-1/20261007T130000.000Z.jsonl")
    );
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(!text.contains("plan_text"), "{text}");
    assert!(!text.contains(SECRET_PLAN), "{text}");
    assert_eq!(text.lines().count(), 1);
    let v: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();

    assert_eq!(
        keys(&v),
        sorted(&[
            "api",
            "schema",
            "label_policy_version",
            "task_id",
            "state",
            "questions",
            "answers",
            "config",
            "facts",
            "teacher",
        ])
    );
    assert_eq!(v["api"], "systemone/v1");
    assert_eq!(v["schema"], "mh.export-fleet/1.0.0");
    assert_eq!(v["label_policy_version"], "fleet-observed-1");
    assert_eq!(v["task_id"], "sha256:r-1");
    assert_eq!(
        v["state"],
        json!({"task_digest": "sha256:r-1", "plan_digest": "sha256:plan",
               "task_features": {"phase": "implementation", "routing_mode": "auto",
                                 "resumed": false}})
    );
    assert_eq!(
        v["questions"],
        json!({"verdict": {"type": "choice",
                           "instructions": "Did the orchestrator accept this worker's result?",
                           "options": ["accepted", "rework", "rejected"]}})
    );
    assert_eq!(v["answers"], json!({"verdict": null}));
    assert_eq!(
        v["config"],
        json!({"config_id": "sonnet|claude|sonnet|high", "teammate": "sonnet",
               "harness": "claude", "model": "sonnet", "effort": "high"})
    );
    assert_eq!(v["teacher"], json!({"id": "none", "probabilities": null}));
    // `facts` is the run row plus `segments` and `active_ms`.
    let mut want = serde_json::to_value(run(
        "r-1",
        1,
        Some("s-1"),
        "2026-10-07T10:00:00Z",
        60_000,
        Some(1000),
    ))
    .unwrap();
    want["segments"] = json!(1);
    want["active_ms"] = json!(60_000);
    assert_eq!(v["facts"], want);

    // Effort null gives `-` in the config id.
    let mut no_effort = run("r-2", 1, None, "2026-10-07T11:00:00Z", 1, None);
    no_effort.effort = None;
    let rows = build_fleet_rows(&[no_effort], &[]);
    assert_eq!(rows[0].config.config_id, "sonnet|claude|sonnet|-");
}

/// FDS-16: the last verdict row of a record is its answer.
#[test]
fn fds_16_export_fleet_latest_verdict_wins() {
    let runs = [
        run("r-1", 1, Some("s-1"), "2026-10-07T10:00:00Z", 1, None),
        run("r-2", 1, Some("s-2"), "2026-10-07T11:00:00Z", 1, None),
    ];
    let verdicts = [
        verdict("r-1", "accepted"),
        verdict("r-2", "rejected"),
        verdict("r-1", "rework"),
    ];
    let rows = build_fleet_rows(&runs, &verdicts);
    let answer = |row: &FleetExportRow| serde_json::to_value(&row.answers["verdict"]).unwrap();
    assert_eq!(
        answer(&rows[0]),
        json!({"choice": "rework", "confidence": null, "probabilities": {"rework": 1.0}})
    );
    assert_eq!(answer(&rows[1])["choice"], "rejected");
}

/// FDS-16: a record with 3 segments gives 1 row: the latest segment's
/// facts, `segments` 3, `active_ms` the sum, and tokens summed over the
/// distinct sessions (the latest row of each).
#[test]
fn fds_16_export_segments_one_row() {
    let runs = [
        run(
            "r-1",
            1,
            Some("s-a"),
            "2026-10-07T10:00:00Z",
            1_000,
            Some(100),
        ),
        run("r-2", 1, Some("s-x"), "2026-10-07T10:05:00Z", 7, Some(1)),
        // The same session resumed: its transcript holds segment 1 too.
        run(
            "r-1",
            2,
            Some("s-a"),
            "2026-10-07T10:40:00Z",
            2_000,
            Some(250),
        ),
        // A new session.
        run(
            "r-1",
            3,
            Some("s-b"),
            "2026-10-07T11:00:00Z",
            3_000,
            Some(50),
        ),
    ];
    let rows = build_fleet_rows(&runs, &[]);
    assert_eq!(rows.len(), 2);
    let r = &rows[0];
    assert_eq!(r.facts.run.record_id, "r-1");
    assert_eq!(r.facts.segments, 3);
    assert_eq!(r.facts.active_ms, 6_000);
    assert_eq!(r.facts.run.segment, 3);
    assert_eq!(r.facts.run.started_at, "2026-10-07T11:00:00Z");
    assert_eq!(r.facts.run.session_id.as_deref(), Some("s-b"));
    let tokens = r.facts.run.tokens.unwrap();
    assert_eq!(tokens.input, 300);
    assert_eq!(tokens.output, 20);
    assert_eq!(tokens.cache_read, 200);
    assert_eq!(r.facts.run.cost_microusd, Some(600));
    assert!(r.state.task_features.resumed);
    assert_eq!((rows[1].facts.segments, rows[1].facts.active_ms), (1, 7));
}

/// FDS-17: the `fleet observed:` section counts rows, verdicts, tasks, the
/// verdict mix, and per config the runs and the median duration.
#[test]
fn fds_17_readiness_fleet_section() {
    let mut opus = run("r-3", 1, None, "2026-10-07T10:03:00Z", 900, None);
    opus.teammate = "opus".into();
    opus.model = Some("opus".into());
    opus.task_digest = "sha256:r-1".into();
    let runs = [
        run("r-1", 1, None, "2026-10-07T10:00:00Z", 100, None),
        run("r-2", 1, None, "2026-10-07T10:01:00Z", 300, None),
        run("r-4", 1, None, "2026-10-07T10:04:00Z", 200, None),
        run("r-5", 1, None, "2026-10-07T10:05:00Z", 50, None),
        run("r-5", 2, None, "2026-10-07T10:06:00Z", 450, None),
        opus,
    ];
    let verdicts = [
        verdict("r-1", "accepted"),
        verdict("r-2", "rework"),
        verdict("r-2", "accepted"),
        verdict("r-3", "rejected"),
    ];
    let report = fleet_readiness(&build_fleet_rows(&runs, &verdicts));
    assert_eq!(
        render_fleet_readiness(&report),
        "fleet observed:\n\
         \x20 rows: 5\n\
         \x20 rows with a verdict: 3\n\
         \x20 distinct tasks: 4\n\
         \x20 verdicts: accepted 2, rework 0, rejected 1\n\
         \x20 configs: 2\n\
         \x20   opus|claude|opus|high runs 1, median duration_ms 900\n\
         \x20   sonnet|claude|sonnet|high runs 4, median duration_ms 250\n"
    );
    let empty = fleet_readiness(&[]);
    assert_eq!(
        render_fleet_readiness(&empty),
        "fleet observed:\n  rows: 0\n  rows with a verdict: 0\n  distinct tasks: 0\n  \
         verdicts: accepted 0, rework 0, rejected 0\n  configs: 0\n"
    );
}

/// FDS-17: the readiness verdict and the Clef gaps count judged rounds
/// only: fleet rows in the store change neither.
#[test]
fn fds_17_readiness_verdict_unchanged() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/export-1.0.0.jsonl");
    let round_rows: Vec<ExportRow> = std::fs::read_to_string(golden)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let t = ReadinessThresholds::builtin();
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(tmp.path());
    let before = readiness(&round_rows, &t);
    assert!(fleet_rows(&p).unwrap().is_empty());

    for i in 0..60 {
        let id = format!("r-{i}");
        store::append(
            &p,
            FleetFile::Runs,
            &run(&id, 1, None, "2026-10-07T10:00:00Z", 1, None),
        )
        .unwrap();
        store::append(&p, FleetFile::Verdicts, &verdict(&id, "accepted")).unwrap();
    }
    let fleet = fleet_rows(&p).unwrap();
    assert_eq!(fleet_readiness(&fleet).with_verdict, 60);
    let after = readiness(&round_rows, &t);
    assert_eq!(after, before);
    assert_eq!(after.verdict, before.verdict);
    assert_eq!(after.gaps, before.gaps);
    assert!(
        after
            .gaps
            .iter()
            .any(|g| g.starts_with("clef: judged rounds")),
        "{:?}",
        after.gaps
    );
}
