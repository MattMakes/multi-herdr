//! `horch agent-list`: found and missing binaries, versions, efforts, the
//! models the roster runs on each harness, the `--json` shape, and that
//! `--no-probe` runs no binary.
//!
//! Hermetic: the harness binaries are shell scripts in a temp dir, the
//! environment is cleared (so PATH finds nothing), and the roster is the
//! built-ins plus one fixture teammate.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for dir in ["home", "state", "data", "teammates", "bin"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        // A fixture teammate on the codex harness with its own model.
        let luna = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates/codex-luna.md");
        let text = std::fs::read_to_string(luna)
            .unwrap()
            .replace("name: codex-luna", "name: fixture-codex")
            .replace("model: gpt-5.6-luna", "model: fixture-model")
            .replace("effort: low", "effort: high");
        std::fs::write(root.join("teammates/fixture-codex.md"), text).unwrap();
        World { _tmp: tmp, root }
    }

    fn log(&self) -> PathBuf {
        self.root.join("probe.log")
    }

    /// A fake CLI that records its call and prints `version` for `--version`.
    fn fake(&self, name: &str, version: &str) -> PathBuf {
        let path = self.root.join("bin").join(name);
        let script = format!(
            "#!/bin/sh\necho \"{name} $*\" >> '{}'\necho '{version}'\n",
            self.log().display()
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn run(&self, args: &[&str], envs: &[(&str, &Path)]) -> String {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.arg("agent-list")
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("HOME", self.root.join("home"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("HORCH_STATE_DIR", self.root.join("state"))
            .env("HORCH_PROJECT_DIR", "/oracle/project")
            .env("HORCH_TEAMMATES_DIR", self.root.join("teammates"));
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// Claude and codex are fakes, pi is an absolute path that is not there,
    /// and opencode and prime are bare names with no PATH to find them.
    fn envs(&self) -> Vec<(&'static str, PathBuf)> {
        vec![
            (
                "HORCH_CLAUDE_BIN",
                self.fake("claude", "9.9.9 (Fake Claude)"),
            ),
            ("HORCH_CODEX_BIN", self.fake("codex", "codex-cli 1.2.3")),
            ("HORCH_PI_BIN", self.root.join("bin/absent-pi")),
        ]
    }

    fn json(&self, extra: &[&str]) -> Vec<Value> {
        let envs = self.envs();
        let envs: Vec<(&str, &Path)> = envs.iter().map(|(k, v)| (*k, v.as_path())).collect();
        let mut args = vec!["--json"];
        args.extend_from_slice(extra);
        serde_json::from_str(&self.run(&args, &envs)).unwrap()
    }
}

fn row<'a>(rows: &'a [Value], agent: &str) -> &'a Value {
    rows.iter().find(|r| r["agent"] == agent).unwrap()
}

#[test]
fn agent_list_json_reports_found_and_missing_binaries() {
    let w = World::new();
    let rows = w.json(&[]);

    let claude = row(&rows, "claude");
    assert_eq!(claude["found"], true);
    assert_eq!(claude["version"], "9.9.9 (Fake Claude)");
    assert_eq!(claude["available"], true);
    assert_eq!(
        claude["efforts"],
        serde_json::json!(["low", "medium", "high", "xhigh", "max"])
    );
    assert_eq!(claude["capabilities"]["resume"], true);
    assert_eq!(claude["capabilities"]["skills"], "plugin-dir");

    let codex = row(&rows, "codex");
    assert_eq!(codex["version"], "codex-cli 1.2.3");
    let fixture = codex["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["model"] == "fixture-model")
        .expect("the fixture model is listed under codex");
    assert_eq!(
        fixture["teammates"],
        serde_json::json!([{"name": "fixture-codex", "effort": "high"}])
    );

    for missing in ["pi", "opencode", "prime"] {
        let r = row(&rows, missing);
        assert_eq!(r["found"], false, "{missing}");
        assert_eq!(r["available"], false, "{missing}");
        assert_eq!(r["path"], Value::Null, "{missing}");
        assert_eq!(r["version"], Value::Null, "{missing}");
    }
    assert!(rows.iter().all(|r| r["agent"] != "none"));
    for key in [
        "agent",
        "found",
        "path",
        "version",
        "efforts",
        "capabilities",
        "models",
        "pools",
        "pool_state",
        "available",
    ] {
        assert!(claude.get(key).is_some(), "json key {key}");
    }
}

#[test]
fn agent_list_no_probe_runs_no_binary() {
    let w = World::new();
    let rows = w.json(&["--no-probe"]);
    assert_eq!(row(&rows, "claude")["found"], true);
    assert_eq!(row(&rows, "claude")["version"], Value::Null);
    assert!(!w.log().exists(), "no harness binary may run");

    w.json(&[]);
    let log = std::fs::read_to_string(w.log()).unwrap();
    assert!(log.contains("claude --version") && log.contains("codex --version"));
}

#[test]
fn agent_list_text_table_shows_a_row_per_harness() {
    let w = World::new();
    let envs = w.envs();
    let envs: Vec<(&str, &Path)> = envs.iter().map(|(k, v)| (*k, v.as_path())).collect();
    let text = w.run(&[], &envs);
    for agent in ["claude", "codex", "opencode", "pi", "prime"] {
        assert!(
            text.lines().any(|l| l.starts_with(agent)),
            "no row for {agent}:\n{text}"
        );
    }
    assert!(text.contains("9.9.9 (Fake Claude)"));
    assert!(text.contains("(not found)"));
    assert!(text.contains("fixture-codex (high)"));
}
