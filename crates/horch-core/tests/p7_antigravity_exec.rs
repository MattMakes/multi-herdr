//! The Antigravity adapter in exec mode (fleetd-p7 ticket 15): the launch of
//! 1 exec attempt, and the events of its `stream-json` output.
//!
//! The stream lines are the fixture `fixtures/streams/agy-exec.jsonl`. Each
//! line names its doc page in `_doc`. The soft-denial line and the
//! usage-limit line are not in the docs; the conformance job confirms them.

use std::path::Path;

use horch_core::harness::antigravity::{
    exec_command, read_event, seed_stop_hook, Event, ExecLaunch,
};

const FIXTURE: &str = include_str!("fixtures/streams/agy-exec.jsonl");

/// The fixture line whose text has each of `parts`. Exactly 1 line must.
fn line(parts: &[&str]) -> &'static str {
    let found: Vec<&str> = FIXTURE
        .lines()
        .filter(|l| parts.iter().all(|p| l.contains(p)))
        .collect();
    assert_eq!(found.len(), 1, "fixture lines with {parts:?}: {found:?}");
    found[0]
}

#[test]
fn agy_exec_launch_has_the_flags_of_the_design() {
    let launch = ExecLaunch {
        bin: Path::new("/opt/agy"),
        model: "gemini-3.8-flash",
        effort: Some("high"),
        home: Path::new("/fleet/attempts/A1/home"),
        workdir: Path::new("/fleet/worktrees/T1"),
        prompt: "/fleet/tasks/T1/inbox/START.md",
    };
    let cmd = exec_command(&launch);

    let argv: Vec<String> = std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        argv,
        [
            "/opt/agy",
            "-p",
            "/fleet/tasks/T1/inbox/START.md",
            "--output-format",
            "stream-json",
            "--mode",
            "auto",
            "--model",
            "gemini-3.8-flash",
            "--effort",
            "high",
        ]
    );
    assert_eq!(
        cmd.get_current_dir(),
        Some(Path::new("/fleet/worktrees/T1"))
    );

    let envs: Vec<(String, Option<String>)> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    assert!(
        envs.contains(&("HOME".into(), Some("/fleet/attempts/A1/home".into()))),
        "the private HOME of the attempt: {envs:?}"
    );
    for key in ["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_GEMINI_BASE_URL"] {
        assert!(
            envs.contains(&(key.into(), None)),
            "{key} removed: {envs:?}"
        );
    }

    let bare = exec_command(&ExecLaunch {
        effort: None,
        ..launch
    });
    assert!(!bare.get_args().any(|a| a == "--effort"));
}

#[test]
fn agy_stream_soft_denial_is_a_signal() {
    let event = read_event(line(&["\"step_type\":\"tool\"", "\"error\""]));
    assert_eq!(
        event,
        Event::SoftDenied {
            tool: "run_command".into(),
            message: "run_command needs approval, which headless mode cannot obtain".into(),
        }
    );
    // The run goes on after a soft denial: it is activity, not a wait.
    assert!(event.is_activity());

    // A tool step that did not fail is plain activity.
    let ok = read_event(line(&["hello_headless_demo"]));
    assert_eq!(ok, Event::Activity);
}

#[test]
fn agy_stream_gives_the_context_token_count() {
    // input_tokens 278 + cache_read_tokens 30214: the context of the turn.
    let event = read_event(line(&["\"step_type\":\"agent_response\"", "\"usage\""]));
    assert_eq!(event, Event::ContextTokens(30492));
    assert!(event.is_activity());

    // The result totals the session, so it is not the context.
    let result = read_event(line(&["\"status\":\"SUCCESS\""]));
    assert_eq!(
        result,
        Event::Result {
            status: "SUCCESS".into()
        }
    );
    // A checkpoint step has its own small call, not the context.
    assert_eq!(
        read_event(line(&["\"step_type\":\"checkpoint\""])),
        Event::Activity
    );
}

#[test]
fn agy_stream_usage_limit_is_quota() {
    let event = read_event(line(&["RESOURCE_EXHAUSTED"]));
    assert_eq!(
        event,
        Event::UsageLimited {
            message: "RESOURCE_EXHAUSTED: quota exceeded for model gemini-3.8-flash".into(),
        }
    );

    // An ERROR that does not name a limit is a plain result, not quota.
    let other = read_event(line(&["invalid model selection"]));
    assert_eq!(
        other,
        Event::Result {
            status: "ERROR".into()
        }
    );
    assert_eq!(read_event("not json"), Event::Other);
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

/// R3-F2 (FM-6): an exec attempt seeds the same `Stop` hook as a push
/// launch, so `agy -p` cannot end its turn without `result.json`.
#[test]
fn agy_exec_launch_seeds_stop_hook_in_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let worktree = dir.path().join("wt");
    std::fs::create_dir_all(worktree.join(".agents")).unwrap();
    git(&worktree, &["init", "-q"]);
    git(&worktree, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let mine = serde_json::json!({"mine": {"Stop": [{"type": "command", "command": "mine"}]}});
    std::fs::write(worktree.join(".agents/hooks.json"), mine.to_string()).unwrap();

    let horch = Path::new("/opt/horch/bin/horch");
    seed_stop_hook(&worktree, horch).unwrap();

    let text = std::fs::read_to_string(worktree.join(".agents/hooks.json")).unwrap();
    let hooks: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(hooks["mine"], mine["mine"], "{text}");
    let gate = &hooks["horch-result-gate"]["Stop"][0];
    assert_eq!(gate["type"], "command", "{text}");
    let command = gate["command"].as_str().unwrap();
    assert!(
        command.starts_with("'/opt/horch/bin/horch' result --check"),
        "{command}"
    );
    assert!(command.contains("if [ $? -eq 2 ]"), "{command}");
    assert!(command.contains(r#""decision":"continue""#), "{command}");

    let status = git(
        &worktree,
        &["status", "--porcelain", "--untracked-files=all"],
    );
    assert!(status.is_empty(), "{status}");
}
