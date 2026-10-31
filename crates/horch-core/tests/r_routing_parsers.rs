//! The quota reply parsers keep their output for the odd replies as well as
//! the real ones. The golden file was written by the code before the #153
//! refactor. The test only reads it: a test that rewrote the file could not
//! fail.

use std::fmt::Write;
use std::path::Path;

use horch_core::routing::quota::{parse_agy_usage, parse_claude_usage, parse_codex_limits};
use serde_json::{json, Value};

fn claude_cases() -> Vec<Value> {
    let usage = |rl: Value| json!({"response": {"response": {"rate_limits": rl}}});
    vec![
        json!({"response": {"subtype": "error", "error": "denied"}}),
        json!({"response": {"subtype": "error"}}),
        json!({"response": {"subtype": "success", "response": {"weird": 1}}}),
        json!({"response": {"response": {"rate_limits_available": false, "rate_limits": {}}}}),
        json!({"response": {"response": {"rate_limits": null}}}),
        usage(json!({})),
        usage(json!({
            "five_hour": {"utilization": 12.5, "resets_at": "2026-09-28T19:00:00Z"},
            "seven_day": {"utilization": 40, "resets_at": 1790000000},
            "seven_day_opus": {"utilization": 99},
            "seven_day_sonnet": {"resets_at": "2026-09-28T19:00:00Z"},
        })),
        usage(json!({"five_hour": "n/a", "seven_day": {"utilization": "x"}})),
        usage(json!({
            "seven_day": {"utilization": 10},
            "seven_day_opus": {"utilization": 20},
            "model_scoped": [
                {"display_name": "Opus", "utilization": 30},
                {"display_name": "Fable", "utilization": 50, "resets_at": "2026-10-02T14:00:00Z"},
                {"display_name": "Haiku"},
                {"utilization": 5},
            ],
        })),
        usage(json!({
            "limits": [
                {"kind": "session", "percent": 10, "resets_at": "2026-09-28T19:00:00Z"},
                {"kind": "session", "percent": 20},
                {"kind": "weekly_all", "percent": 30},
                {"kind": "weekly_all", "percent": 31},
                {"kind": "weekly_scoped", "percent": 40,
                 "scope": {"model": {"display_name": "Fable"}}},
                {"kind": "weekly_scoped", "percent": 41,
                 "scope": {"model": {"display_name": "Fable"}}},
                {"kind": "weekly_scoped", "percent": 42},
                {"kind": "other", "percent": 43},
                {"kind": "session"},
                {"percent": 44},
            ],
        })),
        usage(json!({
            "five_hour": {"utilization": 1},
            "seven_day": {"utilization": 2},
            "limits": [
                {"kind": "session", "percent": 10},
                {"kind": "weekly_all", "percent": 30},
            ],
        })),
    ]
}

fn codex_cases() -> Vec<Value> {
    let win = |mins: u64, used: f64| json!({"windowDurationMins": mins, "usedPercent": used});
    vec![
        json!({"error": {"message": "nope"}}),
        json!({"error": {}}),
        json!({}),
        json!({"result": {}}),
        json!({"result": {"rateLimits": 3}}),
        json!({"result": {"rateLimits": {
            "primary": {"windowDurationMins": 300, "usedPercent": 12.5, "resetsAt": 1790000000},
            "secondary": win(10080, 40.0),
        }}}),
        json!({"result": {"rateLimitsByLimitId": {"codex": {"primary": win(300, 5.0)}}}}),
        json!({"result": {"rateLimits": {
            "primary": "x",
            "secondary": {"windowDurationMins": 10080},
        }}}),
        json!({"result": {"rateLimits": {}, "ordinaryUsageAllowed": false}}),
        json!({"result": {"rateLimits": {"primary": win(300, 1.0)}, "ordinaryUsageAllowed": true}}),
        json!({"result": {"rateLimits": {}}}),
    ]
}

fn agy_cases() -> Vec<Value> {
    let bucket = |id: &str, window: &str, left: f64| {
        json!({"id": id, "window": window, "remaining_fraction": left,
               "reset_time": "2026-09-28T19:00:00Z"})
    };
    let groups = |buckets: Vec<Value>| json!({"status": "SUCCESS", "command": {"data": {"groups": [{"buckets": buckets}]}}});
    vec![
        json!({"status": "ERROR", "response": "\n  quota service down\nmore"}),
        json!({"status": "ERROR"}),
        json!({}),
        json!({"status": "SUCCESS"}),
        json!({"status": "SUCCESS", "command": {"data": {"groups": [{"id": "x"}]}}}),
        groups(vec![]),
        groups(vec![
            bucket("gemini-argon-pro", "weekly", 0.5),
            bucket("gemini-3-flash", "5h", 0.25),
            bucket("3p-claude", "weekly", 0.75),
            bucket("other-thing", "5h", 0.9),
        ]),
        groups(vec![
            bucket("gemini-3-flash", "weekly", 0.5),
            bucket("3p-gpt", "5h", 0.1),
        ]),
        groups(vec![
            bucket("gemini-3-flash", "monthly", 0.5),
            json!({"id": "gemini-3-flash", "window": "5h"}),
            json!({"window": "weekly", "remaining_fraction": 0.4}),
        ]),
    ]
}

fn dump() -> String {
    let mut out = String::new();
    for (name, cases, parse) in [
        (
            "claude",
            claude_cases(),
            parse_claude_usage as fn(&Value) -> _,
        ),
        ("codex", codex_cases(), parse_codex_limits),
        ("agy", agy_cases(), parse_agy_usage),
    ] {
        for (i, case) in cases.iter().enumerate() {
            writeln!(out, "{name} {i}: {:?}", parse(case)).unwrap();
        }
    }
    out
}

#[test]
fn the_parsers_match_the_golden_file() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/r_routing_parsers.txt");
    let got = dump();
    let want = std::fs::read_to_string(&golden).unwrap();
    assert!(
        want == got,
        "parser output changed; see {}",
        golden.display()
    );
}
