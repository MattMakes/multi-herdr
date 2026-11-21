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

const FAKES: [&str; 4] = ["fake-claude", "fake-codex", "fake-antigravity", "fake-pi"];

fn fake(name: &str) -> PathBuf {
    fakes::fake(&FAKES, name)
}

const HOST_WINDOW_VARS: [&str; 2] = [
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
];

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

fn machine_root() -> PathBuf {
    if cfg!(target_os = "linux") {
        PathBuf::from("/var/tmp")
    } else if cfg!(unix) {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    }
}

struct Machine {
    tmp: tempfile::TempDir,
    bins: BTreeMap<&'static str, PathBuf>,
}

impl Machine {
    fn new() -> Machine {
        Machine::new_in(machine_root())
    }

    fn new_in(root: impl AsRef<std::path::Path>) -> Machine {
        let tmp = tempfile::tempdir_in(root).unwrap();
        for d in ["home", "state", "data", "ledger/fleet", "scratch", "bin"] {
            std::fs::create_dir_all(tmp.path().join(d)).unwrap();
        }
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

    fn operator_paths(&self) -> OperatorPaths {
        OperatorPaths::of(&self.path("ledger/fleet"))
    }

    fn log(&self) -> PathBuf {
        self.path("fake.log")
    }

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
            fleet_window: Some((CONTEXT_LIMIT, "context-windows.md".into())),
            env_rule: vec![
                ("HORCH_FAKE_SCENARIO".into(), scenario.into()),
                ("HORCH_FAKE_LOG".into(), s(self.log())),
            ],
            scratch: self.path("scratch"),
            first_event_limit: FIRST_EVENT_LIMIT,
        }
    }

    fn calls(&self) -> Vec<Value> {
        std::fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

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

    fn assert_scratch_is_empty(&self) {
        let left: Vec<_> = std::fs::read_dir(self.path("scratch"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert!(left.is_empty(), "throwaway folders are left: {left:?}");
    }
}

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

#[test]
fn launch_headless_passes_when_the_cli_streams_and_exits_0() {
    let m = Machine::new();
    let r = m.check(&claude(), "default", Check::LaunchHeadless);
    assert_verdict(&r, Check::LaunchHeadless, Verdict::Pass);
    m.assert_no_violation();
    m.assert_scratch_is_empty();
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
    m.bins.insert("HORCH_CLAUDE_BIN", fake("fake-pi"));
    let r = m.check(&claude(), "default", Check::LaunchHeadless);
    assert_verdict(&r, Check::LaunchHeadless, Verdict::Fail);
    assert!(r.evidence.contains("no stream event"), "{r:?}");
    m.assert_scratch_is_empty();
}

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
    assert!(!m.home().join(".codex/skills").join(MARKER_SKILL).exists());
}

#[test]
fn private_home_fails_when_the_launch_uses_the_operator_home() {
    let m = Machine::new();
    let r = m.check(
        &profile(HarnessKind::Antigravity),
        "conformance",
        Check::PrivateHome,
    );
    assert_verdict(&r, Check::PrivateHome, Verdict::Fail);
    assert!(r.evidence.contains("home of the operator"), "{r:?}");
    assert!(!m
        .home()
        .join(".gemini/antigravity-cli/skills")
        .join(MARKER_SKILL)
        .exists());
}

#[test]
fn global_only_mcp_server_is_not_listed() {
    let m = Machine::new();
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
    let r = m.check(
        &profile(HarnessKind::Antigravity),
        "conformance",
        Check::ExactMcpSet,
    );
    assert_verdict(&r, Check::ExactMcpSet, Verdict::Fail);
    assert!(r.evidence.contains("is not listed"), "{r:?}");
}

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

#[test]
fn sandbox_write_outside_is_refused() {
    let m = Machine::new();
    let r = m.check(&claude(), "conformance", Check::SandboxBoundary);
    assert_verdict(&r, Check::SandboxBoundary, Verdict::Pass);
    assert!(r.evidence.contains("refused"), "{r:?}");
}

#[test]
fn sandbox_boundary_fails_when_the_sandbox_does_not_see_the_real_folder() {
    let not_seen = |r: &CheckResult| {
        assert_verdict(r, Check::SandboxBoundary, Verdict::Fail);
        assert!(
            r.evidence
                .starts_with("the outside write: not run: the sandbox does not see the real "),
            "{r:?}"
        );
    };
    let m = Machine::new();
    let settings = m.home().join("override-settings.json");
    let rules =
        json!({"sandbox": {"enabled": true, "filesystem": {"denyRead": [m.path("scratch")]}}});
    std::fs::write(&settings, rules.to_string()).unwrap();
    let mut t = claude();
    t.args = vec!["--settings".into(), settings.to_string_lossy().into_owned()];
    not_seen(&m.check(&t, "conformance", Check::SandboxBoundary));

    if cfg!(target_os = "linux") {
        let old = Machine::new_in("/tmp");
        not_seen(&old.check(&claude(), "conformance", Check::SandboxBoundary));
    }
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
    runner.fleet_window = Some((200_000, "context-windows.md".into()));
    let r = run_check(&mut runner, Check::ContextLimit);
    assert_verdict(&r, Check::ContextLimit, Verdict::Fail);
    assert!(r.evidence.contains("reads back 200000"), "{r:?}");
}

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

#[test]
fn no_trust_prompt_passes_when_the_stream_has_no_dialog() {
    let m = Machine::new();
    let r = m.check(&claude(), "default", Check::NoTrustPrompt);
    assert_verdict(&r, Check::NoTrustPrompt, Verdict::Pass);
}

#[test]
fn trust_prompt_fails_the_check() {
    let m = Machine::new();
    let r = m.check(&claude(), "trust_prompt", Check::NoTrustPrompt);
    assert_verdict(&r, Check::NoTrustPrompt, Verdict::Fail);
    assert!(r.evidence.contains("trust"), "{r:?}");
    m.assert_scratch_is_empty();
}

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
    let r = m.check(&claude(), "conformance,ignore_stop_gate", Check::StopGate);
    assert_verdict(&r, Check::StopGate, Verdict::Fail);
    assert!(r.evidence.contains("result"), "{r:?}");
}

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
