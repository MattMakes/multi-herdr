//! A recording stand-in for the Antigravity CLI, `agy`.
//!
//! Three kinds of call:
//! - `--version` prints a fixed version.
//! - `-p /usage ...` is the google pool probe: it prints a `/usage` reply
//!   with every bucket full, or with the Argon bucket empty under the
//!   `agy_argon_exhausted` scenario. It is not a launch.
//! - Any other argv is a launch (`--model m [--conversation id]
//!   --prompt-interactive p`). The fake records it with its cwd, then writes
//!   the conversation id into `$HOME/.gemini/antigravity-cli/cache/
//!   last_conversations.json` under the cwd, as `agy` does. A fresh launch's
//!   id is `agy_` and `hash16` of the canonical cwd; a resume keeps the id it
//!   was given.
//!
//! A launch records a violation for every key `agy` must never receive:
//! `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `GOOGLE_API_KEY` and
//! `GOOGLE_GEMINI_BASE_URL`.
//!
//! Scenarios (a comma-separated list is allowed):
//! - `default`: a launch exits 0 at once.
//! - `stay`: a launch sleeps until killed, like a TUI in a pane.

use std::path::{Path, PathBuf};

use horch_e2e::{hang, hash16, say, scenario_has, Call};
use serde_json::{json, Value};

const FORBIDDEN: [&str; 4] = [
    "ANTHROPIC_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_GEMINI_BASE_URL",
];

fn main() {
    let mut call = Call::start("antigravity");
    let args: Vec<String> = call.argv.clone();
    if args == ["--version"] {
        say("1.1.3");
        call.flush();
        return;
    }
    if args == ["update"] {
        update(&call);
    }
    if args.first().map(String::as_str) == Some("-p")
        && args.get(1).map(String::as_str) == Some("/usage")
    {
        usage(&call);
    }
    for key in FORBIDDEN {
        if std::env::var_os(key).is_some() {
            call.violate(format!("{key} present in the environment"));
        }
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let resumed = flag(&args, "--conversation");
    let id = resumed.clone().unwrap_or_else(|| conversation_id(&cwd));
    call.extra.insert("session_id".into(), json!(id));
    call.extra
        .insert("cwd".into(), json!(cwd.to_string_lossy()));
    if let Some(model) = flag(&args, "--model") {
        call.extra.insert("model".into(), json!(model));
    }
    if let Some(resumed) = resumed {
        call.extra.insert("resumed".into(), json!(resumed));
    }
    if flag(&args, "--prompt-interactive").is_none() {
        call.violate("no --prompt-interactive");
    }
    if let Some(home) = std::env::var_os("HOME") {
        record_conversation(&PathBuf::from(home), &cwd, &id);
    }
    // Eager: a `stay` launch is killed, so nothing is written after it.
    call.flush();
    if scenario_has("stay") {
        hang();
    }
}

/// The id a fresh launch in `dir` gets.
fn conversation_id(dir: &Path) -> String {
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    format!("agy_{}", hash16(&dir.to_string_lossy()))
}

/// Add `dir -> id` to the session cache, keeping the other directories.
fn record_conversation(home: &Path, dir: &Path, id: &str) {
    let cache = home.join(".gemini/antigravity-cli/cache/last_conversations.json");
    let mut map: Value = std::fs::read_to_string(&cache)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    map[dir.to_string_lossy().as_ref()] = json!(id);
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache, map.to_string());
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// `agy -p /usage --output-format json`: the agy 1.4.3 reply shape, no turn.
fn usage(call: &Call) -> ! {
    call.flush();
    let argon_left = if scenario_has("agy_argon_exhausted") {
        0.0
    } else {
        1.0
    };
    let bucket = |id: &str, window: &str, left: f64| {
        json!({"id": id, "window": window, "remaining_fraction": left,
               "reset_time": "2026-10-27T18:48:25Z"})
    };
    say(&json!({
        "conversation_id": "", "status": "SUCCESS", "num_turns": 0,
        "command": {"name": "usage", "data": {"groups": [
            {"name": "Gemini Models", "buckets": [
                bucket("gemini-weekly", "weekly", 1.0), bucket("gemini-5h", "5h", 1.0)]},
            {"name": "Claude and GPT models", "buckets": [
                bucket("3p-weekly", "weekly", 1.0), bucket("3p-5h", "5h", 1.0)]},
            {"name": "Gemini 4 Argon", "buckets": [
                bucket("gemini-argon-wtu-weekly", "weekly", argon_left)]},
        ]}}
    })
    .to_string());
    std::process::exit(0);
}

/// `agy update`: record the call, then answer as the scenario says.
/// `update_fail:antigravity` exits 1 with a coloured error line, and
/// `update_hang:antigravity` sleeps 60 s first. Otherwise it prints 1 line and
/// exits 0.
fn update(call: &Call) -> ! {
    call.flush();
    if horch_e2e::scenario_has("update_fail:antigravity") {
        eprintln!("\u{1b}[31merror: antigravity update failed: network unreachable\u{1b}[0m");
        std::process::exit(1);
    }
    if horch_e2e::scenario_has("update_hang:antigravity") {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    horch_e2e::say("agy 1.1.3 is up to date");
    std::process::exit(0);
}
