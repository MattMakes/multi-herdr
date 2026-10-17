//! The agent CLI update step (ARC-32 to ARC-35): `horch fleet` and
//! `horch orchestration` run each CLI's own update command once before the
//! first pane starts, and `horch update` does the same on demand.

use std::process::{Command, Output};
use std::time::{Duration, Instant};

use horch_e2e::harness::Harness;
use serde_json::Value;

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Each agent fake as its log names it, its program name, and the update
/// command horch must run, in the order of the result lines.
const UPDATERS: [(&str, &str, &[&str]); 6] = [
    ("claude", "claude", &["update"]),
    ("codex", "codex", &["update"]),
    ("opencode", "opencode", &["upgrade"]),
    ("antigravity", "agy", &["update"]),
    ("pi", "pi", &["update", "--self"]),
    ("prime", "prime-agent", &["update"]),
];

/// The result lines of a run where every fake updates and nothing changes.
const NO_CHANGE: [&str; 6] = [
    "update: claude: 2.1.284 (Claude Code) (no change)",
    "update: codex: codex-cli 0.158.0 (no change)",
    "update: opencode: 1.18.33 (no change)",
    "update: antigravity: 1.1.3 (no change)",
    "update: pi: 0.85.1 (no change)",
    "update: prime: 0.10.0 (no change)",
];

const HEADER: &str = "Updating agent CLIs (horch fleet --no-update skips this)...";

/// A harness that lets the fleet update, with every fake warmed: macOS scans
/// each new executable at its first exec, and that takes 14 to 35 s.
fn harness(name: &str) -> Harness {
    let mut h = Harness::new(name);
    h.allow_updates();
    warm(&h);
    h
}

fn warm(h: &Harness) {
    for (_, program, _) in UPDATERS {
        run_fake(h, program, &["--version"]);
    }
    run_fake(h, "herdr", &["--version"]);
}

fn run_fake(h: &Harness, program: &str, args: &[&str]) -> Output {
    let exe = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    let mut cmd = Command::new(h.bin.join(exe));
    cmd.args(args);
    h.seal(&mut cmd);
    cmd.output().expect("running a fake")
}

fn argv(call: &Value) -> Vec<String> {
    serde_json::from_value(call["argv"].clone()).unwrap_or_default()
}

/// The update calls `fake` recorded, 1 per process.
fn update_calls(h: &Harness, fake: &str, args: &[&str]) -> Vec<Value> {
    h.calls_of(fake)
        .into_iter()
        .filter(|c| argv(c) == args)
        .collect()
}

fn fleet(h: &Harness, extra: &[&str]) -> Output {
    let project = h.project.to_string_lossy().into_owned();
    let mut args = vec!["fleet", "--cwd", project.as_str()];
    args.extend_from_slice(extra);
    h.run(&args)
}

/// The fleet started: herdr created its workspace.
fn workspace_created(h: &Harness) -> bool {
    h.calls_of("herdr")
        .iter()
        .any(|c| argv(c).get(..2) == Some(&["workspace".to_string(), "create".to_string()][..]))
}

fn notes(o: &Output) -> Vec<String> {
    stderr(o)
        .lines()
        .filter(|l| l.starts_with("horch: NOTE: update:"))
        .map(str::to_owned)
        .collect()
}

fn result_lines(o: &Output) -> Vec<String> {
    stdout(o)
        .lines()
        .filter(|l| l.starts_with("update: "))
        .map(str::to_owned)
        .collect()
}

#[test]
fn arc_32_fleet_updates_each_cli_once_before_the_orchestrator_pane() {
    let h = harness("arc32-fleet");
    let out = fleet(&h, &[]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));

    for (fake, _, args) in UPDATERS {
        assert_eq!(update_calls(&h, fake, args).len(), 1, "{fake} {args:?}");
    }
    // Every updater ran before herdr created the workspace of the
    // orchestrator pane.
    let calls = h.calls();
    let created = calls
        .iter()
        .position(|c| {
            c["fake"] == "herdr"
                && argv(c)
                    .get(..2)
                    .is_some_and(|a| a == ["workspace", "create"])
        })
        .expect("the fleet created its workspace");
    for (fake, _, args) in UPDATERS {
        let at = calls
            .iter()
            .position(|c| c["fake"] == fake && argv(c) == args)
            .unwrap();
        assert!(at < created, "{fake} updated after the workspace");
    }

    let text = stdout(&out);
    let header = text.find(HEADER).expect("the header line");
    let first = text.find("update: ").unwrap();
    assert!(header < first, "{text}");
    assert_eq!(result_lines(&out), NO_CHANGE, "{text}");
    assert!(notes(&out).is_empty(), "{}", stderr(&out));
}

#[test]
fn arc_32_orchestration_updates_before_its_panes() {
    let h = harness("arc32-orch");
    let project = h.project.to_string_lossy().into_owned();
    let out = h.run(&["orchestration", "--cwd", &project]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    for (fake, _, args) in UPDATERS {
        assert_eq!(update_calls(&h, fake, args).len(), 1, "{fake} {args:?}");
    }
    assert_eq!(result_lines(&out), NO_CHANGE, "{}", stdout(&out));
    assert!(workspace_created(&h));
}

/// `cpx` launches `claude` and has no `update` command: with `cpx` on PATH,
/// the update runs `claude`.
#[cfg(unix)]
#[test]
fn arc_32_a_cpx_launch_updates_claude_not_cpx() {
    let mut h = harness("arc32-cpx");
    h.without("HORCH_CLAUDE_BIN");
    let cpx_log = h.root.join("cpx.log");
    h.write_bin(
        "cpx",
        format!("#!/bin/sh\necho \"$*\" >> '{}'\n", cpx_log.display()).as_bytes(),
    );
    let out = fleet(&h, &[]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));

    assert_eq!(update_calls(&h, "claude", &["update"]).len(), 1);
    let cpx_calls = std::fs::read_to_string(&cpx_log).unwrap_or_default();
    assert!(
        !cpx_calls.lines().any(|l| l.trim() == "update"),
        "cpx got an update: {cpx_calls:?}"
    );
    assert_eq!(result_lines(&out)[0], NO_CHANGE[0], "{}", stdout(&out));
}

#[test]
fn arc_33_a_failed_updater_gives_one_note_and_the_fleet_starts() {
    let mut h = harness("arc33-fail");
    h.set("HORCH_FAKE_SCENARIO", "update_fail:codex");
    let out = fleet(&h, &[]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(
        notes(&out),
        ["horch: NOTE: update: codex: error: codex update failed: network unreachable"],
        "{}",
        stderr(&out)
    );
    // The other 5 still report.
    let lines = result_lines(&out);
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert!(!lines.iter().any(|l| l.starts_with("update: codex:")));
    assert!(workspace_created(&h));
}

#[test]
fn arc_33_a_hung_updater_is_killed_at_the_timeout() {
    let mut h = harness("arc33-hang");
    h.set("HORCH_FAKE_SCENARIO", "update_hang:opencode");
    h.set("HORCH_UPDATE_TIMEOUT_MS", "5000");
    let started = Instant::now();
    let out = fleet(&h, &[]);
    let took = started.elapsed();
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(
        notes(&out),
        ["horch: NOTE: update: opencode: timed out after 5 s"],
        "{}",
        stderr(&out)
    );
    // The fake sleeps 60 s; the fleet did not wait for it.
    assert!(took < Duration::from_secs(50), "the fleet took {took:?}");
    assert!(workspace_created(&h));

    // The killed updater is gone.
    #[cfg(unix)]
    {
        let call = update_calls(&h, "opencode", &["upgrade"]);
        let pid = call[0]["pid"].as_u64().unwrap().to_string();
        let alive = Command::new("kill")
            .args(["-0", &pid])
            .output()
            .is_ok_and(|o| o.status.success());
        assert!(!alive, "updater {pid} outlived its timeout");
    }
}

#[test]
fn arc_33_no_updater_gets_anthropic_api_key() {
    let mut h = harness("arc33-key");
    h.set("ANTHROPIC_API_KEY", "SENTINEL");
    let out = fleet(&h, &[]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    for (fake, _, args) in UPDATERS {
        let calls = update_calls(&h, fake, args);
        assert_eq!(calls.len(), 1, "{fake}");
        let keys: Vec<String> = serde_json::from_value(calls[0]["env_keys"].clone()).unwrap();
        assert!(
            !keys.iter().any(|k| k == "ANTHROPIC_API_KEY"),
            "{fake}: {keys:?}"
        );
        assert_eq!(calls[0]["violations"], serde_json::json!([]), "{fake}");
    }
}

#[test]
fn arc_34_no_update_and_horch_no_update_run_no_updater() {
    let h = harness("arc34-flag");
    let out = fleet(&h, &["--no-update"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    let project = h.project.to_string_lossy().into_owned();
    let orch = h.run(&["orchestration", "--cwd", &project, "--no-update"]);
    assert!(
        orch.status.success(),
        "{}\n{}",
        stdout(&orch),
        stderr(&orch)
    );

    let mut env = harness("arc34-env");
    env.set("HORCH_NO_UPDATE", "1");
    let env_out = fleet(&env, &[]);
    assert!(
        env_out.status.success(),
        "{}\n{}",
        stdout(&env_out),
        stderr(&env_out)
    );

    for (h, outs) in [(&h, vec![&out, &orch]), (&env, vec![&env_out])] {
        for (fake, _, args) in UPDATERS {
            assert!(update_calls(h, fake, args).is_empty(), "{fake} updated");
        }
        for o in outs {
            assert!(!stdout(o).contains(HEADER), "{}", stdout(o));
        }
        assert!(workspace_created(h));
    }
}

#[test]
fn arc_35_horch_update_runs_herdr_only_on_request() {
    let h = harness("arc35");
    let herdr_updates = |h: &Harness| update_calls(h, "herdr", &["update"]).len();

    let out = h.run(&["update"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(result_lines(&out), NO_CHANGE, "{}", stdout(&out));
    assert_eq!(herdr_updates(&h), 0, "horch update ran herdr update");

    let out = h.run(&["update", "herdr"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(herdr_updates(&h), 1);
    assert!(
        stderr(&out).contains(
            "horch: NOTE: herdr update can stop the herdr server. \
             That closes every pane, a running fleet included."
        ),
        "{}",
        stderr(&out)
    );
    // Only herdr: no agent CLI updated again.
    assert_eq!(update_calls(&h, "claude", &["update"]).len(), 1);
    assert!(result_lines(&out).is_empty(), "{}", stdout(&out));

    let out = h.run(&["update", "pi", "codex"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(
        result_lines(&out),
        [NO_CHANGE[1], NO_CHANGE[4]],
        "{}",
        stdout(&out)
    );

    let mut failing = harness("arc35-fail");
    failing.set("HORCH_FAKE_SCENARIO", "update_fail:pi");
    let out = failing.run(&["update"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(notes(&out).len(), 1, "{}", stderr(&out));

    let out = h.run(&["update", "vim"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}
