//! A recording stand-in for the Antigravity CLI, `agy`.
//!
//! Two kinds of call:
//! - `--version` prints a fixed version.
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
