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
//!
//! - As the headless judge (`-p --output-format json`): read the prompt from
//!   stdin, record argv, cwd and env keys, then act per
//!   `$HORCH_FAKE_LOG.judge.json`, `{"attempts": ["crash", "valid"]}`: the
//!   n-th judge call takes the n-th entry (the last one repeats). `valid`
//!   answers a winner over the bundle labels in `./manifest.json`;
//!   `invalid` answers a fenced object; `crash` exits 3; `hang` blocks;
//!   `oversize` prints a 2 MiB answer.
//! `inspect_skills`: a pane launch also writes the `skills/` dir of each
//! `--plugin-dir`, the plugin manifest names and the `--settings` value to
//! `$HORCH_FAKE_LOG.skills.json` (`horch_e2e::write_skills_report`).

use std::io::BufRead;
use std::path::Path;

use horch_e2e::{
    candidate_spec, hang, run_candidate, say, scenario, scenario_has, write_skills_report, Call,
};
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
    let judge = call.argv.first().is_some_and(|a| a == "-p")
        && call
            .argv
            .windows(2)
            .any(|w| w[0] == "--output-format" && w[1] == "json");
    if judge {
        judge_mode(call);
        return;
    }
    let probe = call
        .argv
        .windows(2)
        .any(|w| w[0] == "--input-format" && w[1] == "stream-json");
    if !probe {
        if let Some((label, spec)) = candidate_spec() {
            let argv = call.argv.clone();
            run_candidate(&mut call, &label, &spec, |u| write_transcript(&argv, u));
        }
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

/// The headless judge: one prompt on stdin, one json envelope on stdout.
fn judge_mode(mut call: Call) {
    let mut prompt = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut prompt);
    let cwd = std::env::current_dir().unwrap_or_default();
    call.extra
        .insert("cwd".into(), json!(cwd.to_string_lossy()));
    call.extra.insert("mode".into(), json!("judge"));
    call.extra.insert(
        "prompt_has_rubric".into(),
        json!(!prompt.contains("{rubric}")),
    );
    call.stdin.push(format!("<prompt: {} bytes>", prompt.len()));
    let log = std::env::var("HORCH_FAKE_LOG").unwrap_or_default();
    let counter = format!("{log}.judge.count");
    let n: usize = std::fs::read_to_string(&counter)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let _ = std::fs::write(&counter, (n + 1).to_string());
    let script: Value = std::fs::read_to_string(format!("{log}.judge.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let attempts: Vec<String> = script["attempts"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let mode = attempts
        .get(n)
        .or(attempts.last())
        .cloned()
        .unwrap_or_else(|| "valid".into());
    call.extra.insert("judge_scenario".into(), json!(mode));
    call.flush();
    let session = call
        .argv
        .windows(2)
        .find(|w| w[0] == "--session-id")
        .map(|w| w[1].clone())
        .unwrap_or_default();
    let envelope = |result: String| {
        json!({"type": "result", "subtype": "success", "is_error": false,
               "result": result, "session_id": session, "total_cost_usd": 0.0})
        .to_string()
    };
    match mode.as_str() {
        "crash" => {
            eprintln!("fake-claude: judge crash");
            std::process::exit(3);
        }
        "hang" => hang(),
        "invalid" => say(&envelope("```json\n{}\n```".into())),
        "oversize" => say(&envelope("x".repeat(2 * 1024 * 1024))),
        _ => say(&envelope(judgment(&labels(&cwd)))),
    }
}

/// The bundle labels from `manifest.json` in the judge's cwd.
fn labels(cwd: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(cwd.join("manifest.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|m| {
            m["labels"].as_array().map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// A valid judgment: the first label wins with confidence 0.9.
fn judgment(labels: &[String]) -> String {
    let scores = json!({"correctness": 8, "tests": 7, "scope": 9, "maintainability": 8, "risk": 8});
    let candidates: serde_json::Map<String, Value> = labels
        .iter()
        .map(|l| {
            (
                l.clone(),
                json!({"scores": scores, "acceptable": true, "notes": "ok"}),
            )
        })
        .collect();
    json!({
        "schema_version": "1.0.0",
        "verdict": "winner",
        "winner": labels.first(),
        "ranking": labels,
        "candidates": candidates,
        "confidence": 0.9,
        "rationale": "The first candidate is best.",
    })
    .to_string()
}

/// A transcript at `$HOME/.claude/projects/<cwd slug>/<session>.jsonl` with
/// one assistant message using `usage` (`model`, `input`, `output`), so
/// horch's telemetry readers price the session.
fn write_transcript(argv: &[String], usage: &Value) {
    let Some(sid) = argv
        .windows(2)
        .find(|w| w[0] == "--session-id")
        .map(|w| w[1].clone())
    else {
        return;
    };
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    let slug: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let dir = home.join(".claude/projects").join(slug);
    let _ = std::fs::create_dir_all(&dir);
    let line = json!({
        "type": "assistant",
        "sessionId": sid,
        "timestamp": "2026-10-02T12:00:00.000Z",
        "message": {"id": "msg_candidate", "model": usage["model"], "role": "assistant",
            "content": [{"type": "text", "text": "working"}],
            "usage": {"input_tokens": usage["input"], "output_tokens": usage["output"],
                      "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0}}
    });
    let _ = std::fs::write(dir.join(format!("{sid}.jsonl")), format!("{line}\n"));
}
