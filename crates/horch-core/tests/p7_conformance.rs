//! The conformance suite (fleetd-p7 ticket 23; FR-8, FR-9, SR-1, SR-7,
//! FM-18): 9 checks of the design, each a real launch of the exec adapter
//! through the wrapper in a throwaway task folder, with a verdict for each
//! check and the text of `conformance.json`.
//!
//! Each test runs the fake agents of `crates/horch-e2e` in temporary
//! folders, never a real agent CLI (MAP.md Q6). The test builds the fakes 1
//! time, as `p7_wrapper_exec.rs` does. No test waits for a time: a failure
//! of "Launch headless" uses a fake that exits with no event.

#[path = "support/fakes.rs"]
mod fakes;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use horch_core::fleetd::worker_profiles::adapters::conformance_runner::ConformanceRunner;
use horch_core::fleetd::worker_profiles::application::conformance::{
    conformance_json, read_conformance_json, run_check, run_suite, SuiteSubject,
};
use horch_core::fleetd::worker_profiles::domain::conformance::{
    Check, CheckResult, Verdict, CHECKS, CONTEXT_LIMIT, FIRST_EVENT_LIMIT, MARKER_SKILL,
    TEST_MCP_SERVER,
};
use horch_core::fleetd::worker_profiles::domain::operator_boundary::OperatorPaths;
use horch_core::harness::HarnessKind;
use horch_core::roster::Teammate;
use horch_core::runtime::{MapEnv, RuntimeContext};
use serde_json::{json, Value};

/// The fakes that the tests launch.
const FAKES: [&str; 4] = ["fake-claude", "fake-codex", "fake-antigravity", "fake-pi"];

fn fake(name: &str) -> PathBuf {
    fakes::fake(&FAKES, name)
}

/// The variables of the host that a CLI reads as its compaction setting.
/// A test machine has none of them.
const HOST_WINDOW_VARS: [&str; 2] = [
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
];

/// A script in `dir/bin` that starts the fake `name` without
/// [`HOST_WINDOW_VARS`]: the CLI of the test machine does not see the
/// settings of the host that runs the test.
fn sealed(dir: &Path, name: &str) -> PathBuf {
    let script = dir.join("bin").join(name);
    let text = format!(
        "#!/bin/sh\nunset {}\nexec '{}' \"$@\"\n",
        HOST_WINDOW_VARS.join(" "),
        fake(name).display()
    );
    std::fs::write(&script, text).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    script
}

/// A test machine in `/tmp`: a home, the horch folders, a fleet root and
/// the scratch folder of the throwaway task folders. Each CLI binary is a
/// fake.
struct Machine {
    tmp: tempfile::TempDir,
    bins: BTreeMap<&'static str, PathBuf>,
}

impl Machine {
    fn new() -> Machine {
        let tmp = tempfile::tempdir_in("/tmp").unwrap();
        for d in ["home", "state", "data", "ledger/fleet", "scratch", "bin"] {
            std::fs::create_dir_all(tmp.path().join(d)).unwrap();
        }
        // Stub Linux sandbox tools, so the Claude sandbox check passes on
        // Linux too.
        #[cfg(unix)]
        for tool in ["bwrap", "socat"] {
            let path = tmp.path().join("bin").join(tool);
            std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
        }
        let bins = BTreeMap::from([
            ("HORCH_CLAUDE_BIN", sealed(tmp.path(), "fake-claude")),
            ("HORCH_CODEX_BIN", fake("fake-codex")),
            ("HORCH_ANTIGRAVITY_BIN", fake("fake-antigravity")),
        ]);
        Machine { tmp, bins }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    fn home(&self) -> PathBuf {
        self.path("home")
    }

    /// The operator paths of the fleet root of [`Machine::ctx`].
    fn operator_paths(&self) -> OperatorPaths {
        OperatorPaths::of(&self.path("ledger/fleet"))
    }

    fn log(&self) -> PathBuf {
        self.path("fake.log")
    }

    /// The context of the process that runs the suite.
    fn ctx(&self) -> RuntimeContext {
        let s = |p: PathBuf| p.to_string_lossy().into_owned();
        let mut env = MapEnv::new(self.home())
            .with("HOME", &s(self.home()))
            .with("PATH", &s(self.path("bin")))
            .with("HORCH_STATE_DIR", &s(self.path("state")))
            .with("HORCH_DATA_DIR", &s(self.path("data")))
            .with("HORCH_LEDGER_DB", &s(self.path("ledger/ledger.sqlite3")))
            .with("FLEET_ROOT", &s(self.path("ledger/fleet")))
            .with(
                "HORCH_CLAUDE_MANAGED_SETTINGS",
                &s(self.home().join("no-managed-settings.json")),
            );
        for (key, bin) in &self.bins {
            env = env.with(key, &s(bin.clone()));
        }
        RuntimeContext::from_env(&env).unwrap()
    }

    /// The verdict of `check` for `teammate`, with the fake scenario
    /// `scenario`.
    fn check(&self, teammate: &Teammate, scenario: &str, check: Check) -> CheckResult {
        let ctx = self.ctx();
        let mut runner = self.runner(&ctx, teammate, scenario);
        run_check(&mut runner, check)
    }

    fn runner<'a>(
        &self,
        ctx: &'a RuntimeContext,
        teammate: &'a Teammate,
        scenario: &str,
    ) -> ConformanceRunner<'a> {
        let s = |p: PathBuf| p.to_string_lossy().into_owned();
        ConformanceRunner {
            ctx,
            teammate,
            // The window of each seat (fleetd-p7 ticket 21, MAP.md Q16).
            fleet_window: Some((CONTEXT_LIMIT, "context-windows.md".into())),
            env_rule: vec![
                ("HORCH_FAKE_SCENARIO".into(), scenario.into()),
                ("HORCH_FAKE_LOG".into(), s(self.log())),
            ],
            scratch: self.path("scratch"),
            first_event_limit: FIRST_EVENT_LIMIT,
        }
    }

    /// Each call of the fakes, from the log.
    fn calls(&self) -> Vec<Value> {
        std::fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// No fake saw a violation, such as the forbidden name in its
    /// environment (SR-7).
    fn assert_no_violation(&self) {
        let calls = self.calls();
        assert!(!calls.is_empty(), "no fake ran");
        for call in calls {
            let v = &call["violations"];
            assert!(
                v.is_null() || v.as_array().is_some_and(Vec::is_empty),
                "the fake saw a violation: {call}"
            );
        }
    }

    /// The scratch folder holds no throwaway folder after a launch.
    fn assert_scratch_is_empty(&self) {
        let left: Vec<_> = std::fs::read_dir(self.path("scratch"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert!(left.is_empty(), "throwaway folders are left: {left:?}");
    }
}

/// The reference profile of a CLI: a sandboxed worker in exec mode.
fn profile(agent: HarnessKind) -> Teammate {
    let (name, model) = match agent {
        HarnessKind::Claude => ("conformance-claude", "claude-opus-5-5"),
        HarnessKind::Codex => ("conformance-codex", "gpt-5.5"),
        _ => ("conformance-agy", "gemini-3-pro"),
    };
    let mut t = Teammate {
        name: name.into(),
        agent,
        model: Some(model.into()),
        ..Teammate::default()
    };
    if agent == HarnessKind::Claude {
        t.sandbox = Some(serde_json::Map::new());
    }
    // `mode: exec`, as a teammate file writes it.
    let mut v = serde_json::to_value(&t).unwrap();
    v["mode"] = json!("exec");
    serde_json::from_value(v).unwrap()
}

fn claude() -> Teammate {
    profile(HarnessKind::Claude)
}

#[track_caller]
fn assert_verdict(r: &CheckResult, check: Check, verdict: Verdict) {
    assert_eq!(r.check, check, "{r:?}");
    assert_eq!(r.verdict, verdict, "{r:?}");
    assert!(!r.evidence.is_empty(), "{r:?}");
    assert!(!r.evidence.contains('\n'), "{r:?}");
}

// Launch headless

#[test]
fn launch_headless_passes_when_the_cli_streams_and_exits_0() {
    let m = Machine::new();
    let r = m.check(&claude(), "default", Check::LaunchHeadless);
    assert_verdict(&r, Check::LaunchHeadless, Verdict::Pass);
    m.assert_no_violation();
    m.assert_scratch_is_empty();
    // The CLI read the fixed prompt of the check.
    assert_eq!(
        m.calls()[0]["exec_prompt"],
        Check::LaunchHeadless.prompt(&m.operator_paths()),
        "{:?}",
        m.calls()
    );
}

#[test]
fn launch_headless_fails_when_the_cli_emits_no_event() {
    let mut m = Machine::new();
    // fake-pi has no exec mode: it exits 0 and prints nothing.
    m.bins.insert("HORCH_CLAUDE_BIN", fake("fake-pi"));
    let r = m.check(&claude(), "default", Check::LaunchHeadless);
    assert_verdict(&r, Check::LaunchHeadless, Verdict::Fail);
    assert!(r.evidence.contains("no stream event"), "{r:?}");
    m.assert_scratch_is_empty();
}

// Private home honoured

#[test]
fn private_home_marker_is_visible() {
    let m = Machine::new();
    let r = m.check(
        &profile(HarnessKind::Codex),
        "conformance",
        Check::PrivateHome,
    );
    assert_verdict(&r, Check::PrivateHome, Verdict::Pass);
    assert!(r.evidence.contains(MARKER_SKILL), "{r:?}");
    let call = &m.calls()[0];
    let skills = call["conformance"]["skills"].as_array().unwrap();
    assert!(skills.iter().any(|s| s == MARKER_SKILL), "{call}");
    // The marker is not left behind.
    assert!(!m.home().join(".codex/skills").join(MARKER_SKILL).exists());
}

#[test]
fn private_home_fails_when_the_launch_uses_the_operator_home() {
    let m = Machine::new();
    // The Antigravity launch runs with the HOME of this process.
    let r = m.check(
        &profile(HarnessKind::Antigravity),
        "conformance",
        Check::PrivateHome,
    );
    assert_verdict(&r, Check::PrivateHome, Verdict::Fail);
    assert!(r.evidence.contains("home of the operator"), "{r:?}");
    // The runner puts nothing in the home of the operator.
    assert!(!m
        .home()
        .join(".gemini/antigravity-cli/skills")
        .join(MARKER_SKILL)
        .exists());
}

// Exact MCP set

#[test]
fn global_only_mcp_server_is_not_listed() {
    let m = Machine::new();
    // A server only in the global config of the user.
    let global = json!({"mcpServers": {"global-only": {"command": "true"}}});
    std::fs::write(m.home().join(".claude.json"), global.to_string()).unwrap();
    let r = m.check(&claude(), "conformance", Check::ExactMcpSet);
    assert_verdict(&r, Check::ExactMcpSet, Verdict::Pass);
    assert!(r.evidence.contains(TEST_MCP_SERVER), "{r:?}");
    assert!(!r.evidence.contains("global-only"), "{r:?}");
}

#[test]
fn exact_mcp_set_fails_when_the_profile_server_is_not_listed() {
    let m = Machine::new();
    // The Antigravity launch renders no MCP server of the profile.
    let r = m.check(
        &profile(HarnessKind::Antigravity),
        "conformance",
        Check::ExactMcpSet,
    );
    assert_verdict(&r, Check::ExactMcpSet, Verdict::Fail);
    assert!(r.evidence.contains("is not listed"), "{r:?}");
}

// Shared write

#[test]
fn shared_write_reaches_outbox_result_json() {
    let m = Machine::new();
    let r = m.check(&claude(), "conformance", Check::SharedWrite);
    assert_verdict(&r, Check::SharedWrite, Verdict::Pass);
    let call = &m.calls()[0];
    assert_eq!(call["conformance"]["result_write"], "allowed", "{call}");
}

#[test]
fn shared_write_fails_when_the_agent_ends_without_the_write() {
    let m = Machine::new();
    let r = m.check(
        &claude(),
        "conformance,exit_without_result",
        Check::SharedWrite,
    );
    assert_verdict(&r, Check::SharedWrite, Verdict::Fail);
    assert!(r.evidence.contains("outbox/result.json"), "{r:?}");
}

// Sandbox boundary

#[test]
fn sandbox_write_outside_is_refused() {
    let m = Machine::new();
    let r = m.check(&claude(), "conformance", Check::SandboxBoundary);
    assert_verdict(&r, Check::SandboxBoundary, Verdict::Pass);
    assert!(r.evidence.contains("refused"), "{r:?}");
}

#[test]
fn sandbox_boundary_fails_when_the_profile_has_no_sandbox() {
    let m = Machine::new();
    let mut t = claude();
    t.sandbox = None;
    let r = m.check(&t, "conformance", Check::SandboxBoundary);
    assert_verdict(&r, Check::SandboxBoundary, Verdict::Fail);
    assert!(r.evidence.contains("allowed"), "{r:?}");
}

// Context limit

#[test]
fn context_limit_reads_back_256000() {
    let m = Machine::new();
    for agent in [HarnessKind::Claude, HarnessKind::Codex] {
        let r = m.check(&profile(agent), "conformance", Check::ContextLimit);
        assert_verdict(&r, Check::ContextLimit, Verdict::Pass);
        assert!(r.evidence.contains("reads back"), "{agent:?}: {r:?}");
        assert!(r.evidence.contains(&CONTEXT_LIMIT.to_string()), "{r:?}");
    }
}

#[test]
fn context_limit_fails_when_the_setting_reads_back_another_value() {
    let m = Machine::new();
    let ctx = m.ctx();
    let t = profile(HarnessKind::Codex);
    let mut runner = m.runner(&ctx, &t, "conformance");
    // A seat with the window of before ticket 21.
    runner.fleet_window = Some((200_000, "context-windows.md".into()));
    let r = run_check(&mut runner, Check::ContextLimit);
    assert_verdict(&r, Check::ContextLimit, Verdict::Fail);
    assert!(r.evidence.contains("reads back 200000"), "{r:?}");
}

// Hooks fire

#[test]
fn hooks_reach_the_wrapper() {
    let m = Machine::new();
    let r = m.check(&claude(), "default", Check::HooksFire);
    assert_verdict(&r, Check::HooksFire, Verdict::Pass);
}

#[test]
fn hooks_fire_fails_when_no_stop_reaches_the_wrapper() {
    let m = Machine::new();
    let r = m.check(
        &claude(),
        "conformance,exit_without_result",
        Check::HooksFire,
    );
    assert_verdict(&r, Check::HooksFire, Verdict::Fail);
    assert!(r.evidence.contains("no stop"), "{r:?}");
}

// No trust prompt

#[test]
fn no_trust_prompt_passes_when_the_stream_has_no_dialog() {
    let m = Machine::new();
    let r = m.check(&claude(), "default", Check::NoTrustPrompt);
    assert_verdict(&r, Check::NoTrustPrompt, Verdict::Pass);
}

#[test]
fn trust_prompt_fails_the_check() {
    let m = Machine::new();
    // The fake prints a dialog and waits: the runner ends it at once.
    let r = m.check(&claude(), "trust_prompt", Check::NoTrustPrompt);
    assert_verdict(&r, Check::NoTrustPrompt, Verdict::Fail);
    assert!(r.evidence.contains("trust"), "{r:?}");
    m.assert_scratch_is_empty();
}

// Stop gate

#[test]
fn exit_without_result_triggers_the_stop_gate() {
    let m = Machine::new();
    let r = m.check(&claude(), "exit_without_result", Check::StopGate);
    assert_verdict(&r, Check::StopGate, Verdict::Pass);
    assert!(r.evidence.contains("repair pass"), "{r:?}");
    assert_eq!(
        m.calls()[0]["exec_prompt"],
        Check::StopGate.prompt(&m.operator_paths())
    );
}

#[test]
fn stop_gate_fails_when_the_agent_leaves_a_result() {
    let m = Machine::new();
    // The agent does not obey the prompt of the check.
    let r = m.check(&claude(), "conformance,ignore_stop_gate", Check::StopGate);
    assert_verdict(&r, Check::StopGate, Verdict::Fail);
    assert!(r.evidence.contains("result"), "{r:?}");
}

// conformance.json

#[test]
fn conformance_json_lists_each_check() {
    let m = Machine::new();
    for agent in [
        HarnessKind::Claude,
        HarnessKind::Codex,
        HarnessKind::Antigravity,
    ] {
        let ctx = m.ctx();
        let t = profile(agent);
        let mut runner = m.runner(&ctx, &t, "conformance");
        let subject = SuiteSubject {
            cli: format!("{agent:?}").to_lowercase(),
            version: "9.8.7-probe".into(),
            adapter_hash: "sha256:0123".into(),
        };
        let report = run_suite(subject, &mut runner);
        let text = conformance_json(&report);
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["cli"], format!("{agent:?}").to_lowercase());
        assert_eq!(v["version"], "9.8.7-probe");
        assert_eq!(v["adapter_hash"], "sha256:0123");
        let names: Vec<&str> = v["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap())
            .collect();
        let want: Vec<&str> = CHECKS.iter().map(|c| c.name()).collect();
        assert_eq!(names, want, "{text}");
        for c in v["checks"].as_array().unwrap() {
            assert!(
                ["pass", "fail", "not supported"].contains(&c["verdict"].as_str().unwrap()),
                "{c}"
            );
            assert!(c["evidence"].as_str().is_some_and(|e| !e.is_empty()), "{c}");
        }
        assert_eq!(read_conformance_json(&text).unwrap(), report);
        // The contract names the CLI, the version and each failed check.
        if let Some(failure) = report.failure() {
            let line = failure.to_string();
            assert!(line.starts_with("adapter-conformance-failed: "), "{line}");
            for check in report.failed() {
                assert!(line.contains(check.name()), "{line}");
            }
        }
    }
    m.assert_no_violation();
    m.assert_scratch_is_empty();
}

#[test]
fn the_first_event_limit_is_a_value_of_the_runner() {
    let m = Machine::new();
    let ctx = m.ctx();
    let t = claude();
    let mut runner = m.runner(&ctx, &t, "default");
    assert_eq!(runner.first_event_limit, Duration::from_secs(60));
    runner.first_event_limit = Duration::from_secs(600);
    let r = run_check(&mut runner, Check::LaunchHeadless);
    assert_verdict(&r, Check::LaunchHeadless, Verdict::Pass);
}
