#![cfg(unix)]

use std::path::PathBuf;

use horch_core::harness::codex::{exec_command, read_event, ExecEvent};
use horch_core::harness::{HarnessKind, LaunchEnv, PrepareRequest};
use horch_core::roster::{PermissionMode, Roster, Teammate};
use horch_core::runtime::{MapEnv, RuntimeContext};

struct Fleet {
    root: tempfile::TempDir,
    task_dir: PathBuf,
    worktree: PathBuf,
}

fn fleet() -> Fleet {
    let root = tempfile::tempdir().unwrap();
    let task_dir = root.path().join("fleet/tasks/T-1/attempts/A1");
    let worktree = root.path().join("fleet/work/T-1");
    for dir in [&task_dir, &worktree, &root.path().join("home/.codex")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    Fleet {
        root,
        task_dir,
        worktree,
    }
}

impl Fleet {
    fn ctx(&self) -> RuntimeContext {
        let env = MapEnv::new(self.root.path())
            .with("HOME", &self.root.path().join("home").to_string_lossy())
            .with(
                "HORCH_STATE_DIR",
                &self.root.path().join("state").to_string_lossy(),
            );
        RuntimeContext::from_env(&env).unwrap()
    }

    fn exec_argv(&self, teammate: &Teammate) -> Vec<String> {
        let ctx = self.ctx();
        let prepared = HarnessKind::Codex
            .adapter()
            .prepare(
                &ctx,
                &PrepareRequest {
                    role: "codex-1",
                    exec_rules: Roster::builtin().unwrap().exec_rules(),
                    skills: None,
                    compact_window: None,
                    teammate,
                    model: "gpt-5.6-sol",
                    workdir: &self.worktree,
                    shell_env: &[],
                },
            )
            .unwrap();
        let mut teammate = teammate.clone();
        teammate.args.extend(prepared.extra_args.iter().cloned());
        let mut env = LaunchEnv::from_context(&ctx);
        env.task_dir = Some(self.task_dir.clone());
        env.worktree = Some(self.worktree.clone());
        let cmd = exec_command(&env, &teammate, "inbox/START.md").unwrap();
        prepared.finish();
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }
}

fn sol() -> Teammate {
    Roster::builtin()
        .unwrap()
        .require("codex-sol")
        .unwrap()
        .clone()
}

fn values_of<'a>(argv: &'a [String], flag: &str) -> Vec<&'a str> {
    argv.windows(2)
        .filter(|w| w[0] == flag)
        .map(|w| w[1].as_str())
        .collect()
}

fn kind(event: &ExecEvent) -> &'static str {
    match event {
        ExecEvent::Activity => "activity",
        ExecEvent::UsageLimited => "usage_limited",
        ExecEvent::Result => "result",
        ExecEvent::ContextTokens(_) => "context_tokens",
        ExecEvent::Other => "other",
    }
}

fn forms() -> Vec<(String, String, String)> {
    let text = include_str!("fixtures/streams/codex-exec.jsonl");
    text.lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            (
                v["doc"].as_str().unwrap().to_string(),
                v["event"].as_str().unwrap().to_string(),
                v["line"].to_string(),
            )
        })
        .collect()
}

#[test]
fn codex_exec_launch_has_the_flags_of_the_design() {
    let fleet = fleet();
    let argv = fleet.exec_argv(&sol());
    assert_eq!(argv[..2], ["exec", "--json"], "{argv:?}");
    assert_eq!(argv.last().map(String::as_str), Some("inbox/START.md"));
    assert!(
        values_of(&argv, "-c").contains(&"model=\"gpt-5.6-sol\""),
        "{argv:?}"
    );
    assert!(!argv.iter().any(|a| a == "resume"), "{argv:?}");
}

#[test]
fn codex_exec_never_waits_on_an_approval() {
    let fleet = fleet();
    let mut plan = sol();
    plan.permission_mode = Some(PermissionMode::Plan);
    for teammate in [sol(), plan] {
        let argv = fleet.exec_argv(&teammate);
        assert_eq!(values_of(&argv, "-a"), ["never"], "{argv:?}");
        assert!(!argv.iter().any(|a| a == "--ask-for-approval"), "{argv:?}");
    }
}

#[test]
fn codex_exec_refuses_an_approval_policy_of_the_teammate() {
    let fleet = fleet();
    let mut asks = sol();
    asks.args
        .extend(["-a".to_string(), "on-request".to_string()]);
    let ctx = fleet.ctx();
    let env = LaunchEnv::from_context(&ctx);
    let err = exec_command(&env, &asks, "inbox/START.md").unwrap_err();
    assert!(err.to_string().contains("approval"), "{err}");
}

#[test]
fn codex_exec_config_sets_the_256000_limit() {
    let fleet = fleet();
    let argv = fleet.exec_argv(&sol());
    let limits: Vec<_> = values_of(&argv, "-c")
        .into_iter()
        .filter_map(|p| p.strip_prefix("model_auto_compact_token_limit="))
        .collect();
    assert_eq!(limits, ["256000"], "{argv:?}");
    let window = HarnessKind::Codex.adapter().window_in_command(
        &exec_command(
            &LaunchEnv::from_context(&fleet.ctx()),
            &sol(),
            "inbox/START.md",
        )
        .unwrap(),
    );
    assert_eq!(window, Some(256_000));
}

#[test]
fn codex_stream_usage_limit_is_quota() {
    let forms = forms();
    let limited: Vec<_> = forms
        .iter()
        .filter(|(_, event, _)| event == "usage_limited")
        .collect();
    assert!(limited.len() >= 3, "{forms:?}");
    for (doc, event, line) in &forms {
        if event == "usage_limited" || line.contains("failed") || line.contains("error") {
            assert_eq!(kind(&read_event(line)), event, "{doc}: {line}");
        }
    }
}

#[test]
fn codex_stream_items_are_activity() {
    for (doc, event, line) in forms() {
        assert_eq!(kind(&read_event(&line)), event, "{doc}: {line}");
    }
    for line in ["", "not json", "{\"type\":\"some.future.kind\"}", "[1,2]"] {
        assert_eq!(read_event(line), ExecEvent::Other, "{line}");
    }
}
