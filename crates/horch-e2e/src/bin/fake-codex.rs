use std::io::BufRead;
use std::path::{Path, PathBuf};

use horch_e2e::{hang, hash16, say, scenario, scenario_has, write_skills_report, Call};
use serde_json::{json, Value};

const ALLOWED: [&str; 3] = ["initialize", "initialized", "account/rateLimits/read"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("debug")
        && args.get(1).map(String::as_str) == Some("models")
    {
        say(r#"{"models":[{"slug":"gpt-5.6-sol","multi_agent_version":"v2"}]}"#);
        return;
    }
    let mut call = Call::start("codex");
    if call.argv.first().map(String::as_str) == Some("sandbox") {
        sandbox(&mut call);
    }
    if call.argv.iter().any(|a| a == "--version") {
        say("codex-cli 0.158.0");
        call.flush();
        return;
    }
    if call.argv == ["update"] {
        update(&call);
    }
    if call.argv.first().map(String::as_str) != Some("app-server") {
        if scenario_has("inspect_skills") {
            inspect_skills();
        }
        if scenario_has("stay") {
            stay(&mut call);
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

fn result(scenario: &str) -> Value {
    let (primary, secondary, allowed) = match scenario {
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

fn stay(call: &mut Call) {
    if call.argv.first().map(String::as_str) == Some("resume") {
        if let Some(id) = call.argv.iter().skip(1).find(|arg| {
            arg.len() == 36
                && arg.split('-').map(str::len).eq([8, 4, 4, 4, 12])
                && arg.bytes().all(|b| b == b'-' || b.is_ascii_hexdigit())
        }) {
            call.extra.insert("resumed".into(), json!(id));
        }
    } else {
        let id = write_rollout();
        call.extra.insert("session_id".into(), json!(id));
    }
    call.flush();
    hang();
}

fn write_rollout() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let cwd = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    let prompt = std::env::args().skip(1).last().unwrap_or_default();
    let launch = format!("{}:{}:{}", cwd.display(), std::process::id(), prompt);
    let h = format!("{}{}", hash16(&launch), hash16("codex"));
    let id = format!(
        "{}-{}-4{}-8{}-{}",
        &h[0..8],
        &h[8..12],
        &h[13..16],
        &h[17..20],
        &h[20..32]
    );
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let dir = home.join(".codex/sessions/2026/10/02");
    let _ = std::fs::create_dir_all(&dir);
    let line = json!({"type": "session_meta", "payload": {"id": id, "cwd": cwd.to_string_lossy()}});
    let _ = std::fs::write(
        dir.join(format!("rollout-2026-10-02T00-00-00-{id}.jsonl")),
        format!(
            "{line}\n{}\n",
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":prompt}]}})
        ),
    );
    id
}

fn update(call: &Call) -> ! {
    call.flush();
    if horch_e2e::scenario_has("update_fail:codex") {
        eprintln!("\u{1b}[31merror: codex update failed: network unreachable\u{1b}[0m");
        std::process::exit(1);
    }
    if horch_e2e::scenario_has("update_hang:codex") {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    horch_e2e::say("codex-cli 0.158.0 is the latest version");
    std::process::exit(0);
}

fn sandbox(call: &mut Call) -> ! {
    let args = call.argv[1..].to_vec();
    let Some(split) = args.iter().position(|a| a == "--") else {
        call.violate("codex sandbox without -- before the command");
        call.flush();
        std::process::exit(2);
    };
    let (opts, command) = (&args[..split], &args[split + 1..]);
    let mut writable: Vec<String> = Vec::new();
    let mut opts = opts.iter();
    while let Some(opt) = opts.next() {
        let value = opts.next();
        match opt.as_str() {
            "-C" => writable.extend(value.cloned()),
            "-c" => writable.extend(
                value
                    .and_then(|pair| pair.split_once(".workspace_roots="))
                    .and_then(|(_, roots)| roots.split('"').nth(1))
                    .map(str::to_string),
            ),
            _ => {}
        }
    }
    call.extra
        .insert("sandbox_writable".into(), json!(writable.clone()));
    call.flush();
    let code = match confined(command, &writable).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(_) => 127,
    };
    std::process::exit(code);
}

#[cfg(target_os = "macos")]
fn confined(command: &[String], writable: &[String]) -> std::process::Command {
    let allow: String = writable
        .iter()
        .map(|p| {
            let real = std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
            format!(" (subpath {:?})", real.to_string_lossy())
        })
        .collect();
    let profile = format!(
        "(version 1)(allow default)(deny file-write*)(allow file-write*{allow} (subpath \"/dev\"))"
    );
    let mut cmd = std::process::Command::new("/usr/bin/sandbox-exec");
    cmd.arg("-p").arg(profile).args(command);
    cmd
}

#[cfg(not(target_os = "macos"))]
fn confined(command: &[String], _writable: &[String]) -> std::process::Command {
    let mut cmd = std::process::Command::new(&command[0]);
    cmd.args(&command[1..]);
    cmd
}
