//! The Phase 5 keys of `fleet.toml`: the size limits of the protocol rules
//! (design, "Workspace layout and file protocol", Protocol rules, rule 7).
//! Phase 6 retired the list of orchestrator seats (`p6_authorize.rs`).

use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;

#[test]
fn phase5_keys_have_the_values_of_the_design() {
    let c = config_file::parse("").unwrap();
    assert_eq!(c.question_max_bytes, 65_536);
    assert_eq!(c.request_max_bytes, 65_536);
    assert_eq!(c.heartbeat_max_bytes, 4_096);
    assert_eq!(c.progress_max_bytes, 8_388_608);
    assert_eq!(c.progress_max_lines, 10_000);
    assert_eq!(c.progress_line_max_bytes, 4_096);
    assert_eq!(c.transcript_tail_bytes, 262_144);
    assert_eq!(c.transcript_line_max_bytes, 1_048_576);
    assert_eq!(c.job_result_max_bytes, 65_536);
    assert_eq!(c, FleetConfig::default());
}

#[test]
fn phase5_key_from_the_file_wins() {
    let c = config_file::parse(
        "question_max_bytes = 1\nrequest_max_bytes = 2\nheartbeat_max_bytes = 3\n\
         progress_max_bytes = 4\nprogress_max_lines = 5\nprogress_line_max_bytes = 6\n\
         transcript_tail_bytes = 7\ntranscript_line_max_bytes = 8\njob_result_max_bytes = 9\n",
    )
    .unwrap();
    let want = FleetConfig {
        question_max_bytes: 1,
        request_max_bytes: 2,
        heartbeat_max_bytes: 3,
        progress_max_bytes: 4,
        progress_max_lines: 5,
        progress_line_max_bytes: 6,
        transcript_tail_bytes: 7,
        transcript_line_max_bytes: 8,
        job_result_max_bytes: 9,
        ..FleetConfig::default()
    };
    assert_eq!(c, want);
}

#[test]
fn negative_or_zero_limit_is_refused_with_the_key_name() {
    for key in [
        "question_max_bytes",
        "request_max_bytes",
        "heartbeat_max_bytes",
        "progress_max_bytes",
        "progress_max_lines",
        "progress_line_max_bytes",
        "transcript_tail_bytes",
        "transcript_line_max_bytes",
        "job_result_max_bytes",
    ] {
        for value in ["0", "-1"] {
            let err = config_file::parse(&format!("{key} = {value}\n")).unwrap_err();
            let text = format!("{err:#}");
            assert!(text.contains(key), "{key} = {value}: {text}");
        }
    }
}
