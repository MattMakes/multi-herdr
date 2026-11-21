use std::io::BufRead;
use std::path::Path;

use horch_e2e::{hang, say, scenario, scenario_has, write_skills_report, Call};
use serde_json::{json, Value};

fn main() {
    let mut call = Call::start("claude");
    if std::env::var_os("ANTHROPIC_API_KEY").is_some() {
        call.violate("ANTHROPIC_API_KEY present in the environment");
    }
    if call.argv.iter().any(|a| a == "--version") {
        say("2.1.284 (Claude Code)");
        call.flush();
        return;
    }
    if call.argv == ["update"] {
        update(&call);
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

fn update(call: &Call) -> ! {
    call.flush();
    if horch_e2e::scenario_has("update_fail:claude") {
        eprintln!("\u{1b}[31merror: claude update failed: network unreachable\u{1b}[0m");
        std::process::exit(1);
    }
    if horch_e2e::scenario_has("update_hang:claude") {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    horch_e2e::say("Claude Code is up to date (2.1.284)");
    std::process::exit(0);
}
