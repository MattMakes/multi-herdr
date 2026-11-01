//! Pins for `store::read_rows` and `facts::record_run`: the cases a refactor
//! of either function could break.

use std::path::PathBuf;

use horch_core::competition::budget::UsageMeter;
use horch_core::competition::observe::TelemetryUsage;
use horch_core::execution::legacy::LedgerRecordV1;
use horch_core::fleet_runs::facts::{record_run, RunSources, RunWrite};
use horch_core::fleet_runs::rows::WrittenBy;
use horch_core::fleet_runs::store::{self, read_rows, FleetFile};
use horch_core::measure::paths::DatasetPaths;
use horch_core::runtime::Inherited;
use horch_core::usage::Locations;
use horch_core::vcs::git::GitCli;
use serde_json::{json, Value};

fn paths() -> (tempfile::TempDir, DatasetPaths) {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let project = tmp.path().join("proj");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let paths = DatasetPaths::new(&state, &project);
    (tmp, paths)
}

/// Write `bytes` as `starts.jsonl`.
fn put(paths: &DatasetPaths, bytes: &[u8]) {
    store::ensure_fleet_dir(paths).unwrap();
    std::fs::write(FleetFile::Starts.path(paths), bytes).unwrap();
}

fn read(paths: &DatasetPaths) -> (Vec<Value>, u32) {
    let r = read_rows::<Value>(paths, FleetFile::Starts).unwrap();
    (r.rows, r.torn_lines)
}

#[test]
fn read_rows_missing_file_has_no_rows() {
    let (_tmp, paths) = paths();
    assert_eq!(read(&paths), (vec![], 0));
}

#[test]
fn read_rows_skips_blank_lines_without_counting_them() {
    let (_tmp, paths) = paths();
    put(&paths, b"{\"n\":1}\n\n   \n\t\n{\"n\":2}\n");
    assert_eq!(read(&paths), (vec![json!({"n": 1}), json!({"n": 2})], 0));
}

#[test]
fn read_rows_counts_a_line_that_does_not_parse() {
    let (_tmp, paths) = paths();
    put(&paths, b"{\"n\":1}\nnot json\n{\"n\":3}\n");
    assert_eq!(read(&paths), (vec![json!({"n": 1}), json!({"n": 3})], 1));
}

#[test]
fn read_rows_counts_an_unterminated_last_line_even_when_it_parses() {
    let (_tmp, paths) = paths();
    put(&paths, b"{\"n\":1}\n{\"n\":2}");
    assert_eq!(read(&paths), (vec![json!({"n": 1})], 1));
}

#[test]
fn read_rows_accepts_crlf_and_trailing_space_before_the_newline() {
    let (_tmp, paths) = paths();
    put(&paths, b"{\"n\":1}\r\n{\"n\":2}  \n");
    assert_eq!(read(&paths), (vec![json!({"n": 1}), json!({"n": 2})], 0));
}

#[test]
fn read_rows_counts_each_bad_line_once() {
    let (_tmp, paths) = paths();
    put(&paths, b"{\"n\":1\n[1,\n{\"n\":2}\n{\"n\":3");
    assert_eq!(read(&paths), (vec![json!({"n": 2})], 3));
}

#[test]
fn record_run_skips_a_record_that_gets_no_row() {
    let (tmp, paths) = paths();
    let project = tmp.path().join("proj");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let usage = TelemetryUsage {
        locations: Locations::under_home(&home, &Inherited::default()),
    };
    let meter = UsageMeter::default();
    let git = GitCli::new(PathBuf::from("git"));
    let sources = RunSources {
        usage: &usage,
        meter: &meter,
        git: &git,
    };
    let record: LedgerRecordV1 = serde_json::from_value(json!({
        "record_id": "r-1", "agent": "claude", "tier": "backend-developer", "model": "claude-opus-5-5", "effort": "high", "phase": "implementation",
        "role": "w", "status": "working", "task": "t", "history": [],
        "created_at": "2026-10-07T10:00:00Z", "updated_at": "2026-10-07T10:00:00Z",
        "state": {"state": "running"}
    }))
    .unwrap();
    let now = horch_core::clock::parse("2026-10-07T12:00:00Z").unwrap();
    let out = record_run(&paths, &project, &record, &sources, now, WrittenBy::Done);
    assert_eq!(out.unwrap(), RunWrite::Skipped("not finished"));
    assert!(!FleetFile::Runs.path(&paths).exists());
}
