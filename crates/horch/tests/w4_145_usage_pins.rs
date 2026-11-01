//! `horch usage`: how it reads its flags and which stored events it keeps.
//!
//! The command prints to stdout, so these tests run the real binary in a
//! sealed world: a temp home and state root, a pinned clock and a Claude
//! transcript of 3 calls (17:00, 17:01 and 17:02 on 2026-09-28) for the 1
//! ledger record of the project `/p`.

use std::path::PathBuf;
use std::process::{Command, Output};

use horch_core::telemetry::store::GROUPS;
use serde_json::Value;

/// `HORCH_NOW` of every run, so a `--window` has 1 start.
const NOW: &str = "2026-09-29T12:00:00Z";

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

fn call(id: &str, at: &str) -> String {
    serde_json::json!({
        "type": "assistant", "sessionId": "sid-1", "timestamp": at,
        "uuid": format!("u-{id}"),
        "message": {
            "id": id, "type": "message", "role": "assistant",
            "model": "claude-sonnet-5-5", "content": [],
            "usage": {
                "input_tokens": 10, "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 100, "output_tokens": 5,
            },
        },
    })
    .to_string()
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let transcripts = root.join("home/.claude/projects/-p");
        std::fs::create_dir_all(&transcripts).unwrap();
        std::fs::create_dir_all(root.join("state")).unwrap();
        let lines = [
            call("m1", "2026-09-28T17:00:00.000Z"),
            call("m2", "2026-09-28T17:01:00.000Z"),
            call("m3", "2026-09-28T17:02:00.000Z"),
        ];
        std::fs::write(transcripts.join("sid-1.jsonl"), lines.join("\n") + "\n").unwrap();
        let ledger = serde_json::json!([{
            "record_id": "r1", "session_id": "sid-1", "agent": "claude", "tier": "sonnet",
            "model": "claude-sonnet-5-5", "role": "sonnet-1", "status": "working",
            "task": "t", "history": [], "project": "/p",
            "created_at": "2026-09-28T16:00:00Z", "updated_at": "2026-09-28T17:00:00Z"
        }]);
        std::fs::write(root.join("state/-p.json"), ledger.to_string()).unwrap();
        World { _tmp: tmp, root }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_horch"))
            .arg("usage")
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("HORCH_STATE_DIR", self.root.join("state"))
            .env("HORCH_PROJECT_DIR", "/oracle/project")
            .env("HORCH_GIT_BIN", "/nonexistent")
            .env("HORCH_NOW", NOW)
            .output()
            .unwrap()
    }

    /// The `--json` report of a run that must succeed.
    fn report(&self, args: &[&str]) -> Value {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let out = self.run(&all);
        assert!(out.status.success(), "usage {all:?}: {}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out)))
    }
}

fn text(o: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// The events a report counts.
fn events(report: &Value) -> u64 {
    report["total"]["events"].as_u64().unwrap()
}

#[test]
fn usage_names_the_flag_that_is_wrong() {
    let w = World::new();
    for (args, message) in [
        (
            vec!["--by", "nonsense"],
            format!("--by takes one of: {}", GROUPS.join(", ")),
        ),
        (
            vec!["--since", "yesterday"],
            "--since: 'yesterday' is not a date or an RFC 3339 time".to_string(),
        ),
        (
            vec!["--window", "3w"],
            "--window takes 5h, today or 7d".to_string(),
        ),
        // A `--since` that does not parse is not rescued by a good `--window`.
        (
            vec!["--since", "yesterday", "--window", "7d"],
            "--since: 'yesterday' is not a date or an RFC 3339 time".to_string(),
        ),
    ] {
        let out = w.run(&args);
        assert!(!out.status.success(), "{args:?}: {}", text(&out));
        assert!(out.stdout.is_empty(), "{args:?}: {}", text(&out));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains(&message), "{args:?}: {}", text(&out));
    }
}

#[test]
fn usage_since_wins_over_window_and_a_window_is_its_start() {
    let w = World::new();
    // `--since` is used as it is, and the `--window` beside it is not read.
    let both = w.report(&["--since", "2026-09-28", "--window", "3w"]);
    assert_eq!(both["since"], "2026-09-28T00:00:00Z");
    assert_eq!(both["window"], "3w");

    for (window, start) in [
        ("5h", "2026-09-29T07:00:00Z"),
        ("today", "2026-09-29T00:00:00Z"),
        ("7d", "2026-09-22T12:00:00Z"),
    ] {
        let report = w.report(&["--window", window]);
        assert_eq!(report["since"], start, "{window}");
        assert_eq!(report["window"], window);
    }

    let none = w.report(&[]);
    assert!(
        none["since"].is_null() && none["window"].is_null(),
        "{none}"
    );
}

#[test]
fn usage_keeps_the_events_since_the_start_and_in_the_project() {
    let w = World::new();
    assert_eq!(events(&w.report(&[])), 3);
    assert_eq!(events(&w.report(&["--since", "2026-09-28T17:00:30Z"])), 2);
    assert_eq!(events(&w.report(&["--since", "2026-09-28T17:01:30Z"])), 1);
    assert_eq!(events(&w.report(&["--since", "2026-09-28T17:02:30Z"])), 0);
    assert_eq!(events(&w.report(&["--window", "5h"])), 0, "all are older");
    assert_eq!(events(&w.report(&["--window", "7d"])), 3);

    assert_eq!(events(&w.report(&["--project", "/p"])), 3);
    assert_eq!(events(&w.report(&["--project", "/other"])), 0);
    // Both filters apply.
    let both = ["--since", "2026-09-28T17:00:30Z", "--project"];
    assert_eq!(events(&w.report(&[both[0], both[1], both[2], "/p"])), 2);
    assert_eq!(events(&w.report(&[both[0], both[1], both[2], "/other"])), 0);
}
