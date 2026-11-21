//! The Claude Code adapter in exec mode (fleetd-p7 ticket 13; FR-9,
//! RR-11, FM-18): the run-to-completion launch of 1 attempt, no dialog,
//! 256000 as the compaction window, and the events of its stream.
//!
//! The real `claude` never runs here. The tests read the built command, and
//! the stream tests read the documented forms in
//! `tests/fixtures/streams/claude-exec.jsonl`. Each fixture line names its
//! doc page in `_doc` and the event that it gives in `_expect`.

use std::path::{Path, PathBuf};

use horch_core::harness::claude::{exec_command, read_event, Event};
use horch_core::harness::{LaunchEnv, Session};
use horch_core::roster::{PermissionMode, Teammate};
use horch_core::runtime::{MapEnv, RuntimeContext};
use serde_json::{json, Value};

/// A test fleet in `/tmp` (a socket path has a short limit): a home, a
/// state root, the ledger folder with the fleet root in it, a task folder
/// with its `inbox/START.md`, and a worktree.
struct Fleet {
    root: tempfile::TempDir,
    fleet_root: PathBuf,
    ledger_db: PathBuf,
    task_dir: PathBuf,
    worktree: PathBuf,
}

fn fleet() -> Fleet {
    let base = if cfg!(unix) {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let root = tempfile::tempdir_in(base).unwrap();
    let ledger_dir = root.path().join("ledger");
    let fleet_root = ledger_dir.join("fleet");
    let task_dir = fleet_root.join("tasks/T-1/attempts/A1");
    let worktree = fleet_root.join("work/T-1");
    for dir in [
        &task_dir.join("inbox"),
        &worktree,
        &root.path().join("bin"),
        &root.path().join("home"),
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(task_dir.join("inbox/START.md"), "# T-1\n").unwrap();
    Fleet {
        ledger_db: ledger_dir.join("ledger.sqlite3"),
        root,
        fleet_root,
        task_dir,
        worktree,
    }
}

impl Fleet {
    /// The launch environment of a worker of this fleet. The `PATH` has
    /// stub Linux sandbox tools, so the builder runs on Linux too.
    fn env(&self) -> LaunchEnv {
        let home = self.root.path().join("home");
        let bin = self.root.path().join("bin");
        for tool in ["bwrap", "socat"] {
            let path = bin.join(tool);
            std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
        }
        let s = |p: &Path| p.to_string_lossy().into_owned();
        let env = MapEnv::new(&home)
            .with("HOME", &s(&home))
            .with("PATH", &s(&bin))
            .with("HORCH_STATE_DIR", &s(&self.root.path().join("state")))
            .with("HORCH_LEDGER_DB", &s(&self.ledger_db))
            .with("FLEET_ROOT", &s(&self.fleet_root))
            .with("HERDR_SOCKET_PATH", &s(&home.join("herdr.sock")))
            .with(
                "HORCH_CLAUDE_MANAGED_SETTINGS",
                &s(&home.join("no-managed-settings.json")),
            );
        let mut env = LaunchEnv::from_context(&RuntimeContext::from_env(&env).unwrap());
        env.task_dir = Some(self.task_dir.clone());
        env.worktree = Some(self.worktree.clone());
        env
    }

    fn start_md(&self) -> PathBuf {
        self.task_dir.join("inbox/START.md")
    }

    /// The argv (program excluded) of the exec launch of `t`.
    fn argv(&self, t: &Teammate) -> Vec<String> {
        let cmd = exec_command(&self.env(), t, Session::Fresh("sid-1"), &self.start_md()).unwrap();
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// The `--settings` object of the exec launch of `t`.
    fn settings(&self, t: &Teammate) -> Value {
        let args = self.argv(t);
        let i = args.iter().position(|a| a == "--settings").unwrap();
        serde_json::from_str(&args[i + 1]).unwrap()
    }
}

/// A sandboxed fleet worker that today runs in `auto` mode.
fn worker() -> Teammate {
    Teammate {
        name: "boxed-worker".into(),
        model: Some("opus".into()),
        permission_mode: Some(PermissionMode::Auto),
        mcp_servers: Some(Default::default()),
        sandbox: Some(
            json!({"filesystem": {"allowWrite": []}})
                .as_object()
                .unwrap()
                .clone(),
        ),
        ..Teammate::default()
    }
}

fn strings(list: &Value) -> Vec<String> {
    list.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Each line of the fixture with the event that it names in `_expect`.
fn fixture() -> Vec<(String, String)> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/streams/claude-exec.jsonl");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            assert!(
                v["_doc"].as_str().is_some(),
                "a line names no doc page: {l}"
            );
            (l.to_owned(), v["_expect"].as_str().unwrap().to_owned())
        })
        .collect()
}

fn kind(e: &Event) -> String {
    match e {
        Event::Activity => "activity".into(),
        Event::UsageLimited => "usage_limited".into(),
        Event::Result => "result".into(),
        Event::ContextTokens(n) => format!("context_tokens:{n}"),
        Event::Other => "other".into(),
    }
}

#[test]
fn claude_exec_launch_has_the_flags_of_the_design() {
    let f = fleet();
    let t = worker();
    let cmd = exec_command(&f.env(), &t, Session::Fresh("sid-1"), &f.start_md()).unwrap();
    let args = f.argv(&t);
    // The headless launch of the design, first, so no variadic flag eats it.
    assert_eq!(
        args[..5],
        [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-hook-events"
        ],
        "{args:?}"
    );
    // The prompt is the path of START.md, last.
    assert_eq!(args.last().unwrap(), &f.start_md().to_string_lossy());
    // Today's config isolation: the `--settings` overlay and
    // `--strict-mcp-config`, no private config home.
    assert!(args.contains(&"--settings".to_string()), "{args:?}");
    assert!(
        args.contains(&"--strict-mcp-config".to_string()),
        "{args:?}"
    );
    assert!(
        !cmd.get_envs().any(|(k, _)| k == "CLAUDE_CONFIG_DIR"),
        "an exec launch sets no CLAUDE_CONFIG_DIR"
    );
    assert!(
        cmd.get_envs()
            .any(|(k, v)| k == "ANTHROPIC_API_KEY" && v.is_none()),
        "an exec launch removes the API key variable"
    );
    let i = args.iter().position(|a| a == "--session-id").unwrap();
    assert_eq!(args[i + 1], "sid-1");
}

#[test]
fn claude_exec_never_waits_on_a_dialog() {
    let f = fleet();
    let args = f.argv(&worker());
    let modes: Vec<_> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--permission-mode")
        .map(|(i, _)| args[i + 1].as_str())
        .collect();
    // The teammate's `auto` gives way to `dontAsk` in exec.
    assert_eq!(modes, ["dontAsk"], "{args:?}");
    let i = args
        .iter()
        .position(|a| a == "--permission-prompts")
        .unwrap();
    assert_eq!(args[i + 1], "none");

    // The explicit allow rules: Bash in the sandbox, Skill, and Edit and
    // Write in the writable paths only.
    let allow = strings(&f.settings(&worker())["permissions"]["allow"]);
    for dir in [&f.worktree, &f.task_dir] {
        for tool in ["Edit", "Write"] {
            let rule = format!("{tool}(/{}/**)", dir.to_string_lossy());
            assert!(allow.contains(&rule), "{rule} not in {allow:?}");
        }
    }
    for rule in ["Bash", "Skill"] {
        assert!(allow.contains(&rule.to_string()), "{rule} not in {allow:?}");
    }
    assert!(
        !allow.iter().any(|r| r == "Edit" || r == "Write"),
        "no Edit or Write rule without a path: {allow:?}"
    );
}

#[test]
fn claude_exec_settings_set_the_256000_window() {
    let f = fleet();
    let env = &f.settings(&worker())["env"];
    assert_eq!(env["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "256000", "{env}");

    // A teammate value gives way: 256000 is the window of each exec seat.
    let mut t = worker();
    t.settings = Some(json!({"env": {"CLAUDE_CODE_AUTO_COMPACT_WINDOW": "180000"}}).to_string());
    let env = &f.settings(&t)["env"];
    assert_eq!(env["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "256000", "{env}");
}

#[test]
fn claude_stream_usage_limit_is_quota() {
    let lines = fixture();
    let limited: Vec<_> = lines
        .iter()
        .filter(|(_, want)| want == "usage_limited")
        .collect();
    assert!(
        limited.len() >= 2,
        "the fixture has a usage limit and an outage"
    );
    for (line, want) in &lines {
        assert_eq!(&kind(&read_event(line)), want, "{line}");
    }
    // A warning is not a limit.
    let warning = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning"}}"#;
    assert_eq!(read_event(warning), Event::Other);
}

#[test]
fn claude_stream_tool_events_are_activity() {
    for (line, want) in fixture() {
        let event = read_event(&line);
        let tool = line.contains("\"hook_event\":\"PreToolUse\"")
            || line.contains("\"tool_use\"")
            || line.contains("\"tool_result\"");
        if tool {
            assert!(event.is_activity(), "{line}");
        }
        assert_eq!(
            event.is_activity(),
            want == "activity" || want.starts_with("context_tokens:"),
            "{line}"
        );
    }
    // A line that is not JSON, or JSON of an unknown type, is no signal.
    assert_eq!(
        read_event("Waiting for background work to finish"),
        Event::Other
    );
    assert_eq!(read_event(r#"{"type":"prompt_suggestion"}"#), Event::Other);
}
