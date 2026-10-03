//! A recording stand-in for the `prime-agent` CLI.
//!
//! Three kinds of call:
//! - `--version` prints a fixed version.
//! - `status --json` prints the array `horch_core::harness::prime` parses
//!   (`[{"socketPath":..,"pid":..,"sessionCount":..}]`): one entry for each
//!   launch whose process is still alive. `Daemon::finish` reads it to find
//!   the pid to stop.
//! - Any other argv is a launch
//!   (`--model m [--thinking l] --daemon-socket S --session-dir D [--resume F] -- prompt`).
//!   The fake records it and creates `S` as a plain file (horch only removes
//!   it). It writes the session file `D/<id>.jsonl`, where the id is
//!   `horch_e2e::prime_session_id(D)`: `prime_` and 16 hex digits. `--resume F`
//!   reuses `F` and writes nothing new. Then it behaves by scenario.
//!
//! Scenarios (a comma-separated list is allowed):
//! - `default`: a launch exits 0 at once. No daemon stays registered.
//! - `stay`: a launch stays alive until killed, and is the registered daemon.
//! - `inspect_skills`: a launch also writes its `--skill` dirs to
//!   `$HORCH_FAKE_LOG.skills.json` (`horch_e2e::write_skills_report`).
//!
//! `HORCH_FAKE_TRANSCRIPTS=1`: the session file also gets one assistant
//! message line that horch's telemetry reader counts.
//!
//! The registry of live launches is `$HORCH_FAKE_LOG.prime-daemons.json`.

use std::io::Write;
use std::path::{Path, PathBuf};

use horch_e2e::{hang, prime_session_id, say, scenario_has, write_skills_report, Call};
use serde_json::{json, Value};

fn main() {
    let mut call = Call::start("prime");
    let argv = call.argv.clone();
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["--version"] => say("0.9.4"),
        ["status", ..] => say(&Value::Array(live_daemons()).to_string()),
        _ => {
            launch(&mut call, &args);
            return;
        }
    }
    call.flush();
}

fn launch(call: &mut Call, args: &[&str]) {
    // Options end at `--`: what follows is the prompt.
    let options = &args[..args.iter().position(|a| *a == "--").unwrap_or(args.len())];
    let socket = flag(options, "--daemon-socket");
    let sessions = flag(options, "--session-dir");
    let resume = flag(options, "--resume");
    for (key, value) in [
        ("socket", socket),
        ("session_dir", sessions),
        ("resumed", resume),
        ("model", flag(options, "--model")),
    ] {
        if let Some(value) = value {
            call.extra.insert(key.into(), json!(value));
        }
    }
    if let Some(socket) = socket {
        let socket = Path::new(socket);
        if let Some(dir) = socket.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(socket, "");
    }
    let session_file = match (resume, sessions) {
        (Some(file), _) => Some(PathBuf::from(file)),
        (None, Some(dir)) => {
            let id = prime_session_id(dir);
            call.extra.insert("session_id".into(), json!(id));
            Some(Path::new(dir).join(format!("{id}.jsonl")))
        }
        (None, None) => None,
    };
    if let Some(file) = &session_file {
        call.extra
            .insert("session_file".into(), json!(file.to_string_lossy()));
        write_session(file, resume.is_some());
    }
    if scenario_has("inspect_skills") {
        let (dirs, flags) = skill_flags(options);
        write_skills_report("prime", &dirs, json!({ "flags": flags }));
    }
    // Eager: a `stay` launch is killed, so nothing is written after it.
    call.flush();
    if scenario_has("stay") {
        if let Some(socket) = socket {
            register(socket);
        }
        hang();
    }
}

/// Every `--skill <dir>` among the options: the dirs, and the flag pairs.
fn skill_flags(options: &[&str]) -> (Vec<PathBuf>, Vec<String>) {
    let mut dirs = Vec::new();
    let mut flags = Vec::new();
    for pair in options.windows(2).filter(|w| w[0] == "--skill") {
        dirs.push(PathBuf::from(pair[1]));
        flags.extend([pair[0].to_string(), pair[1].to_string()]);
    }
    (dirs, flags)
}

fn flag<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| *a == name)
        .and_then(|i| args.get(i + 1).copied())
}

/// The session file in the shape of the telemetry fixtures. A resumed file is
/// only appended to, and only when transcripts are on.
fn write_session(file: &Path, resumed: bool) {
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut lines = Vec::new();
    if !resumed {
        let id = file.file_stem().unwrap_or_default().to_string_lossy();
        let cwd = std::env::current_dir().unwrap_or_default();
        lines.push(json!({"type": "session", "version": 3, "id": id,
            "timestamp": "2026-09-28T12:59:00.000Z", "cwd": cwd.to_string_lossy()}));
    }
    if std::env::var("HORCH_FAKE_TRANSCRIPTS").is_ok_and(|v| v == "1") {
        lines.push(json!({"type": "message", "id": "p1",
            "timestamp": "2026-09-28T13:00:00.000Z",
            "message": {"role": "assistant", "provider": "anthropic",
                "model": "claude-opus-5-5",
                "usage": {"input": 50, "output": 40, "cacheRead": 1000, "cacheWrite": 200,
                          "totalTokens": 1290,
                          "cost": {"input": 0, "output": 0, "cacheRead": 0,
                                   "cacheWrite": 0, "total": 0}},
                "stopReason": "stop"}}));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
    {
        for line in lines {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn registry() -> Option<PathBuf> {
    std::env::var_os("HORCH_FAKE_LOG").map(|p| {
        let mut p = PathBuf::from(p).into_os_string();
        p.push(".prime-daemons.json");
        PathBuf::from(p)
    })
}

fn load() -> Vec<Value> {
    registry()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn register(socket: &str) {
    let mut all = load();
    all.push(json!({"socketPath": socket, "pid": std::process::id(), "sessionCount": 1}));
    if let Some(path) = registry() {
        let _ = std::fs::write(path, Value::Array(all).to_string());
    }
}

/// Registered launches whose process is alive: a killed `stay` leaves its
/// entry behind, and horch must not be told to stop a pid that is gone.
fn live_daemons() -> Vec<Value> {
    load()
        .into_iter()
        .filter(|d| d["pid"].as_u64().is_some_and(alive))
        .collect()
}

fn alive(pid: u64) -> bool {
    if cfg!(windows) {
        return true;
    }
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("kill -0 {pid} 2>/dev/null"))
        .status()
        .is_ok_and(|s| s.success())
}
