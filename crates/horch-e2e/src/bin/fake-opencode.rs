//! A recording stand-in for the `opencode` CLI.
//!
//! Three kinds of call:
//! - `--version` prints a fixed version.
//! - `session list [--format json]` prints the JSON array that
//!   `horch_core::opencode` parses: one session whose `directory` is the
//!   process cwd and whose `created` is now. Its id is
//!   `horch_e2e::opencode_session_id(cwd)`: `ses_` and 16 hex digits.
//! - Any other argv is a launch (`--model m [--session id] --prompt p`). The
//!   fake records it, then behaves by scenario.
//!
//! Scenarios (a comma-separated list is allowed):
//! - `default`: a launch exits 0 at once.
//! - `stay`: a launch sleeps until killed, like a TUI in a pane.
//!
//! `HORCH_FAKE_TRANSCRIPTS=1`: a launch also writes the rows horch's telemetry
//! reader needs into `opencode.db` (`$HORCH_OPENCODE_DB`, else
//! `$XDG_DATA_HOME/opencode/opencode.db`, else `$HOME/.local/share/opencode/
//! opencode.db`) through the `sqlite3` CLI (`$HORCH_SQLITE3_BIN`, else
//! `sqlite3`). The rows are 1 session and 1 completed assistant message. The
//! fake skips this without a message when `sqlite3` is absent or fails.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use horch_e2e::{hang, opencode_session_id, say, scenario_has, Call};
use serde_json::json;

fn main() {
    let mut call = Call::start("opencode");
    let argv = call.argv.clone();
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let id = opencode_session_id(&cwd);
    match args.as_slice() {
        ["--version"] => say("1.18.33"),
        ["session", "list", ..] => {
            call.extra.insert("session_id".into(), json!(id));
            say(&json!([{
                "id": id,
                "title": "fake session",
                "updated": now_ms(),
                "created": now_ms(),
                "projectId": "global",
                "directory": cwd.to_string_lossy(),
            }])
            .to_string());
        }
        _ => {
            call.extra.insert("session_id".into(), json!(id));
            call.extra
                .insert("cwd".into(), json!(cwd.to_string_lossy()));
            if let Some(model) = flag(&args, "--model") {
                call.extra.insert("model".into(), json!(model));
            }
            if let Some(resumed) = flag(&args, "--session") {
                call.extra.insert("resumed".into(), json!(resumed));
            }
            // Eager: a `stay` launch is killed, so nothing is written after it.
            call.flush();
            if std::env::var("HORCH_FAKE_TRANSCRIPTS").is_ok_and(|v| v == "1") {
                write_transcript(&id, &cwd);
            }
            if scenario_has("stay") {
                hang();
            }
            return;
        }
    }
    call.flush();
}

fn flag<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| *a == name)
        .and_then(|i| args.get(i + 1).copied())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn db_path() -> Option<PathBuf> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    var("HORCH_OPENCODE_DB").or_else(|| {
        let data = var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local/share")))?;
        Some(data.join("opencode/opencode.db"))
    })
}

/// The schema subset the reader queries, as in
/// `crates/horch-core/tests/fixtures/telemetry/opencode/opencode.sql`.
fn write_transcript(session: &str, cwd: &std::path::Path) {
    let Some(db) = db_path() else { return };
    if let Some(dir) = db.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let now = now_ms();
    let data = json!({
        "role": "assistant", "modelID": "nemotron-3-ultra-free", "providerID": "opencode",
        "mode": "build", "cost": 0,
        "tokens": {"total": 150, "input": 100, "output": 50, "reasoning": 0,
                   "cache": {"read": 0, "write": 0}},
        "time": {"created": now, "completed": now},
    })
    .to_string()
    .replace('\'', "''");
    let directory = cwd.to_string_lossy().replace('\'', "''");
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS session (id TEXT PRIMARY KEY, project_id TEXT, directory TEXT, title TEXT, time_created INTEGER, time_updated INTEGER);\n\
         CREATE TABLE IF NOT EXISTS message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES session(id), time_created INTEGER, time_updated INTEGER, data TEXT NOT NULL);\n\
         INSERT OR REPLACE INTO session VALUES ('{session}','prj_fake','{directory}','fake session',{now},{now});\n\
         INSERT OR REPLACE INTO message VALUES ('msg_{session}','{session}',{now},{now},'{data}');\n"
    );
    let bin = std::env::var_os("HORCH_SQLITE3_BIN").unwrap_or_else(|| "sqlite3".into());
    // The SQL goes in on stdin: an argument that starts with `--` is an option.
    let Ok(mut child) = Command::new(bin)
        .arg(&db)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(sql.as_bytes());
    }
    let _ = child.wait();
}
