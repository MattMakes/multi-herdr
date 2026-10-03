//! A recording stand-in for the `codex` CLI.
//!
//! As a pane agent it records argv and exits 0. As `codex app-server` it
//! speaks the JSON-RPC lines the quota probe uses, in the shape Codex 0.158.0
//! ships (`ai_docs/reports/quota-signals.md` section 2).
//!
//! Violation: any method other than `initialize`, `initialized` and
//! `account/rateLimits/read`. A thread or a turn is never allowed.
//!
//! Scenarios: `limits_weekly` (the default), `limits_5h_weekly`,
//! `limits_not_allowed`, `hang`, `error`. `inspect_skills`: a pane launch
//! also writes `$CODEX_HOME/skills` and the files under it to
//! `$HORCH_FAKE_LOG.skills.json` (`horch_e2e::write_skills_report`).

use std::io::BufRead;
use std::path::{Path, PathBuf};

use horch_e2e::{hang, say, scenario, scenario_has, write_skills_report, Call};
use serde_json::{json, Value};

const ALLOWED: [&str; 3] = ["initialize", "initialized", "account/rateLimits/read"];

fn main() {
    let mut call = Call::start("codex");
    if call.argv.iter().any(|a| a == "--version") {
        say("codex-cli 0.158.0");
        call.flush();
        return;
    }
    if call.argv.first().map(String::as_str) != Some("app-server") {
        if scenario_has("inspect_skills") {
            inspect_skills();
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
        let method = msg["method"].as_str().unwrap_or_default().to_string();
        if !ALLOWED.contains(&method.as_str()) {
            call.violate(format!("method not allowed for a probe: '{method}'"));
        }
        call.flush();
        let id = msg["id"].clone();
        match method.as_str() {
            "initialize" => say(&json!({"id": id, "result": {
                "userAgent": "codex_cli_rs/0.158.0 (fake)"}})
            .to_string()),
            "account/rateLimits/read" => match scenario().as_str() {
                "hang" => {
                    call.flush();
                    hang();
                }
                "error" => say(&json!({"id": id, "error": {
                    "code": -32603, "message": "not signed in"}})
                .to_string()),
                s => {
                    // A notification first: a real server may send them any time.
                    say(&json!({"method": "account/rateLimits/updated",
                                "params": {"rateLimits": {"primary": null}}})
                    .to_string());
                    say(&json!({"id": id, "result": result(s)}).to_string());
                }
            },
            _ => {}
        }
    }
    call.flush();
}

fn window(used: i64, mins: i64, resets: i64) -> Value {
    json!({"usedPercent": used, "windowDurationMins": mins, "resetsAt": resets})
}

/// The `account/rateLimits/read` result. `accountId` is there on purpose:
/// TEL-11 proves horch drops it.
fn result(scenario: &str) -> Value {
    let (primary, secondary, allowed) = match scenario {
        // An older plan shape: 5h in the first slot, the week in the second.
        "limits_5h_weekly" => (
            window(20, 300, 1_790_620_000),
            window(40, 10_080, 1_791_054_949),
            json!(true),
        ),
        "limits_not_allowed" => (window(50, 10_080, 1_791_054_949), Value::Null, json!(false)),
        _ => (window(99, 10_080, 1_791_054_949), Value::Null, json!(true)),
    };
    let snapshot = json!({
        "limitId": "codex", "limitName": null, "primary": primary, "secondary": secondary,
        "credits": {"hasCredits": false, "unlimited": false, "balance": null},
        "planType": "prolite", "rateLimitReachedType": null,
    });
    json!({
        "ordinaryUsageAllowed": allowed,
        "rateLimits": snapshot,
        "rateLimitsByLimitId": {"codex": snapshot},
        "rateLimitResetCredits": null,
        "accountId": "acct_SENTINEL",
    })
}

/// The private home's `skills` entry, and every file under it, relative.
fn inspect_skills() {
    let home = std::env::var_os("CODEX_HOME").map(PathBuf::from);
    let skills = home.as_ref().map(|h| h.join("skills"));
    let mut files = Vec::new();
    if let Some(dir) = &skills {
        list_files(dir, dir, &mut files);
    }
    files.sort();
    write_skills_report(
        "codex",
        &skills.into_iter().collect::<Vec<_>>(),
        json!({"env": {"CODEX_HOME": home.map(|h| h.to_string_lossy().into_owned())},
               "files": files}),
    );
}

fn list_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            list_files(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}
