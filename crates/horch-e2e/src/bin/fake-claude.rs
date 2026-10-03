//! A recording stand-in for the `claude` CLI.
//!
//! Two jobs:
//! - As a pane agent (`horch pane-launch` / `horch worker`): record argv and
//!   exit 0.
//! - As the quota probe (`-p --input-format stream-json ...`): read control
//!   requests from stdin and answer `get_usage` in the shape Claude Code
//!   2.1.284 ships (`ai_docs/reports/quota-signals.md` section 1a).
//!
//! Violations: any stdin line of `"type":"user"` (the probe must never start
//! a model turn), and `ANTHROPIC_API_KEY` in the environment.
//!
//! Scenarios: `usage_ok`, `usage_exhausted` (the default for a probe),
//! `usage_scoped`, `usage_hang` (or `hang`), `usage_garbage` (or `error`).
//! `inspect_skills`: a pane launch also writes the `skills/` dir of each
//! `--plugin-dir`, the plugin manifest names and the `--settings` value to
//! `$HORCH_FAKE_LOG.skills.json` (`horch_e2e::write_skills_report`).

use std::io::BufRead;
use std::path::Path;

use horch_e2e::{hang, say, scenario, scenario_has, write_skills_report, Call};
use serde_json::{json, Value};

fn main() {
    let mut call = Call::start("claude");
    // The operator's rule (CLAUDE.md): nothing horch starts carries the key.
    if std::env::var_os("ANTHROPIC_API_KEY").is_some() {
        call.violate("ANTHROPIC_API_KEY present in the environment");
    }
    if call.argv.iter().any(|a| a == "--version") {
        say("2.1.284 (Claude Code)");
        call.flush();
        return;
    }
    let probe = call
        .argv
        .windows(2)
        .any(|w| w[0] == "--input-format" && w[1] == "stream-json");
    if !probe {
        if scenario_has("inspect_skills") {
            inspect_skills(&call.argv);
        }
        call.flush();
        return;
    }
    // Written now, and again per line: a probe that times out kills us, so
    // the log may hold several lines for one pid. The last one is complete.
    call.flush();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        call.stdin.push(line.clone());
        let msg: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        if msg["type"] == "user" {
            call.violate("a user message on stdin: the probe started a model turn");
        }
        call.flush();
        if msg["type"] == "control_request" && msg["request"]["subtype"] == "get_usage" {
            let id = msg["request_id"].as_str().unwrap_or_default().to_string();
            match scenario().as_str() {
                "usage_hang" | "hang" => {
                    call.flush();
                    hang();
                }
                "usage_garbage" | "error" => {
                    say("this is not json");
                    say(&json!({"type":"control_response","response":{
                        "subtype":"success","request_id":id,
                        "response":{"weird":{"shape":true}}}})
                    .to_string());
                }
                s => {
                    // Noise first: a real session prints its init line.
                    say(
                        &json!({"type":"system","subtype":"init","session_id":"probe"}).to_string(),
                    );
                    say(&json!({"type":"control_response","response":{
                        "subtype":"success","request_id":id,"response":payload(s)}})
                    .to_string());
                }
            }
        }
    }
    call.flush();
}

/// The `get_usage` success payload. Identity fields are included on purpose:
/// TEL-11 proves horch drops them.
fn payload(scenario: &str) -> Value {
    let (five, seven, scoped) = match scenario {
        "usage_ok" => (12, 40, 5),
        "usage_scoped" => (10, 60, 99),
        _ => (3, 100, 3),
    };
    json!({
        "session": {"total_cost_usd": 0.0, "total_api_duration_ms": 0},
        "subscription_type": "max",
        "rate_limits_available": true,
        "rate_limits": {
            "five_hour": {"utilization": five, "resets_at": "2026-09-28T19:19:59Z"},
            "seven_day": {"utilization": seven, "resets_at": "2026-10-02T13:59:59Z"},
            "seven_day_oauth_apps": null,
            "seven_day_opus": null,
            "seven_day_sonnet": null,
            "model_scoped": [
                {"display_name": "Fable", "utilization": scoped, "resets_at": "2026-10-02T13:59:59Z"}
            ],
            "limits": [{
                "kind": "weekly_scoped", "group": "weekly", "percent": scoped,
                "severity": if scoped >= 90 { "critical" } else { "normal" },
                "resets_at": "2026-10-02T13:59:59Z",
                "scope": {"model": {"display_name": "Fable"}}, "is_active": false
            }],
            "extra_usage": {"is_enabled": true, "monthly_limit": 50000, "used_credits": 3750,
                            "utilization": 7.5, "currency": "USD"}
        },
        "behaviors": null,
        "account_uuid": "acct_SENTINEL",
        "email": "sentinel@example.invalid"
    })
}

fn inspect_skills(argv: &[String]) {
    let mut dirs = Vec::new();
    let mut flags = Vec::new();
    let mut plugins = Vec::new();
    for pair in argv.windows(2).filter(|w| w[0] == "--plugin-dir") {
        let dir = Path::new(&pair[1]);
        dirs.push(dir.join("skills"));
        flags.extend(pair.iter().cloned());
        let manifest = std::fs::read_to_string(dir.join(".claude-plugin/plugin.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        plugins.push(manifest.map_or(Value::Null, |m| m["name"].clone()));
    }
    let settings = argv
        .windows(2)
        .find(|w| w[0] == "--settings")
        .map(|w| w[1].clone());
    write_skills_report(
        "claude",
        &dirs,
        json!({"flags": flags, "plugins": plugins, "settings": settings}),
    );
}
