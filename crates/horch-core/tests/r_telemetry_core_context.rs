//! Branches of the context readers that `context_readers.rs` leaves to the
//! fixture home: the lines a tail scan skips, the Codex model look-back, the
//! marker kinds of each agent, and marker lines longer than a read step.

use std::path::{Path, PathBuf};

use horch_core::runtime::Inherited;
use horch_core::telemetry::context::{read_current, scan_tail};
use horch_core::usage::Locations;

fn claude(id: &str, extra: &str, input: u64) -> String {
    format!(
        r#"{{"type":"assistant"{extra},"message":{{"id":"{id}","model":"claude-opus-5-5","usage":{{"input_tokens":{input},"output_tokens":1}}}}}}"#
    )
}

fn write(dir: &Path, name: &str, lines: &[String]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    path
}

#[test]
fn claude_tail_skips_sidechain_synthetic_and_zero_input_lines() {
    let dir = tempfile::tempdir().unwrap();
    let synthetic = claude("s", "", 900).replace("claude-opus-5-5", "<synthetic>");
    let path = write(
        dir.path(),
        "c.jsonl",
        &[
            claude("a", "", 100),
            claude("side", r#","isSidechain":true"#, 200),
            synthetic,
            claude("zero", "", 0),
            "not json".into(),
        ],
    );
    let tail = scan_tail(&path, "claude").unwrap();
    let u = tail.usable.expect("the first line is usable");
    assert_eq!((u.tokens, u.offset), (101, 0));
    assert_eq!(u.model.as_deref(), Some("claude-opus-5-5"));
}

fn token_count(total: u64) -> String {
    format!(
        r#"{{"type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"total_tokens":{total}}},"model_context_window":1000}}}}}}"#
    )
}

#[test]
fn codex_model_comes_from_the_turn_context_before_the_usage() {
    let dir = tempfile::tempdir().unwrap();
    let turn = |m: &str| format!(r#"{{"type":"turn_context","payload":{{"model":"{m}"}}}}"#);
    let path = write(
        dir.path(),
        "x.jsonl",
        &[
            turn("old-model"),
            turn("gpt-new"),
            r#"{"type":"event_msg","payload":{"type":"agent_message"}}"#.into(),
            token_count(77),
        ],
    );
    let u = scan_tail(&path, "codex").unwrap().usable.unwrap();
    assert_eq!((u.tokens, u.window), (77, Some(1000)));
    assert_eq!(
        u.model.as_deref(),
        Some("gpt-new"),
        "the nearest turn context"
    );

    let path = write(dir.path(), "y.jsonl", &[token_count(5)]);
    let u = scan_tail(&path, "codex").unwrap().usable.unwrap();
    assert_eq!((u.tokens, u.model), (5, None));
}

fn pi_message(role: &str, stop: Option<&str>, total: u64) -> String {
    let stop = stop.map_or(String::new(), |s| format!(r#","stopReason":"{s}""#));
    format!(
        r#"{{"type":"message","message":{{"role":"{role}"{stop},"model":"m1","usage":{{"totalTokens":{total}}}}}}}"#
    )
}

#[test]
fn pi_tail_takes_the_newest_assistant_response_that_did_not_fail() {
    let dir = tempfile::tempdir().unwrap();
    let path = write(
        dir.path(),
        "p.jsonl",
        &[
            pi_message("assistant", None, 40),
            pi_message("assistant", Some("error"), 99),
            pi_message("assistant", Some("aborted"), 98),
            pi_message("toolResult", None, 97),
        ],
    );
    for agent in ["pi", "prime"] {
        let u = scan_tail(&path, agent).unwrap().usable.unwrap();
        assert_eq!((u.tokens, u.model.as_deref()), (40, Some("m1")));
    }
}

fn read_marks(loc: &Locations, agent: &str, sid: &str) -> horch_core::telemetry::context::Reading {
    let cache = tempfile::tempdir().unwrap();
    read_current(loc, cache.path(), agent, Some(sid)).unwrap()
}

fn loc_in(home: &Path) -> Locations {
    Locations::under_home(home, &Inherited::default())
}

#[test]
fn each_agent_reads_its_own_marker_fields() {
    let home = tempfile::tempdir().unwrap();
    let sessions = home.path().join(".pi/agent/sessions/--t--");
    std::fs::create_dir_all(&sessions).unwrap();
    write(
        &sessions,
        "2026-09-20T12-00-00-000Z_aaaaaaaa-0000-4000-8000-0000000000aa.jsonl",
        &[
            pi_message("assistant", None, 40),
            r#"{"type":"compaction","timestamp":"T1","tokensBefore":1234}"#.into(),
        ],
    );
    let r = read_marks(
        &loc_in(home.path()),
        "pi",
        "aaaaaaaa-0000-4000-8000-0000000000aa",
    );
    let m = r.last_compaction.unwrap();
    assert_eq!(
        (m.at.as_str(), m.pre_tokens, m.trigger),
        ("T1", Some(1234), None)
    );
    assert!(r.pending);
    assert_eq!(r.tokens, None);

    let projects = home.path().join(".claude/projects/-t");
    std::fs::create_dir_all(&projects).unwrap();
    write(
        &projects,
        "cccccccc-0000-4000-8000-0000000000cc.jsonl",
        &[
            claude("a", "", 10),
            // Nested objects and other types that hold the needle do not count.
            r#"{"type":"user","subtype":"compact_boundary"}"#.into(),
            r#"{"type":"system","subtype":"compact_boundary","timestamp":"T2","compactMetadata":{"trigger":"auto","preTokens":500,"postTokens":50}}"#.into(),
        ],
    );
    let r = read_marks(
        &loc_in(home.path()),
        "claude",
        "cccccccc-0000-4000-8000-0000000000cc",
    );
    assert_eq!(r.marks, 1);
    let m = r.last_compaction.unwrap();
    assert_eq!(m.trigger.as_deref(), Some("auto"));
    assert_eq!((m.pre_tokens, m.post_tokens), (Some(500), Some(50)));
    assert_eq!((r.tokens, r.provisional), (Some(50), true));
}

#[test]
fn a_marker_line_longer_than_a_read_step_is_found_whole() {
    let home = tempfile::tempdir().unwrap();
    let projects = home.path().join(".claude/projects/-t");
    std::fs::create_dir_all(&projects).unwrap();
    let pad = "x".repeat(150 << 10);
    let long_marker = format!(
        r#"{{"type":"system","subtype":"compact_boundary","timestamp":"T3","pad":"{pad}","compactMetadata":{{"trigger":"manual"}}}}"#
    );
    let first = claude("a", &format!(r#","pad":"{pad}""#), 10);
    let path = write(
        &projects,
        "dddddddd-0000-4000-8000-0000000000dd.jsonl",
        &[first.clone(), long_marker, claude("b", "", 20)],
    );
    let r = read_marks(
        &loc_in(home.path()),
        "claude",
        "dddddddd-0000-4000-8000-0000000000dd",
    );
    assert_eq!(r.marks, 1);
    let m = r.last_compaction.unwrap();
    assert_eq!(m.offset, first.len() as u64 + 1, "the start of the line");
    assert_eq!(m.trigger.as_deref(), Some("manual"));
    assert_eq!(r.tokens, Some(21));
    assert_eq!(r.transcript, path);
}
