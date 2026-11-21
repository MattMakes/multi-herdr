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
    call.flush();
    if scenario_has("stay") {
        hang();
    }
}

fn conversation_id(dir: &Path) -> String {
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    format!("agy_{}", hash16(&dir.to_string_lossy()))
}

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

fn usage(call: &Call) -> ! {
    call.flush();
    let left = |scenario: &str| if scenario_has(scenario) { 0.0 } else { 1.0 };
    let argon_left = left("agy_argon_exhausted");
    let neon_left = left("agy_neon_exhausted");
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
            {"name": "Gemini 4 Neon", "buckets": [
                bucket("gemini-neon-wtu-weekly", "weekly", neon_left)]},
        ]}}
    })
    .to_string());
    std::process::exit(0);
}

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
