//! The check "Operator boundary" of the conformance suite (fleetd-p7
//! ticket 24; SR-8): for each adapter, 1 real launch tries the 3 operator
//! acts from inside its sandbox, and the check passes only when each act is
//! refused. An adapter with no sandbox on this OS gives `not supported`.
//!
//! The fleet root of each test is a temporary folder with a real token file
//! (mode 0600), a real channel socket and a request folder. Each launch
//! runs a fake agent of `crates/horch-e2e`, never a real agent CLI (MAP.md
//! Q6). The fakes try each act in a child under the platform sandbox with
//! the rules of their rendered config.
//!
//! Unix only: the test channel is a Unix socket, and on Windows no adapter
//! has an operator boundary.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use horch_core::fleetd::worker_profiles::adapters::conformance_runner::ConformanceRunner;
use horch_core::fleetd::worker_profiles::application::conformance::{
    conformance_json, read_conformance_json, run_check, SuiteSubject,
};
use horch_core::fleetd::worker_profiles::domain::conformance::{
    Check, CheckResult, ConformanceReport, Verdict, CONTEXT_LIMIT, FIRST_EVENT_LIMIT,
};
use horch_core::fleetd::worker_profiles::domain::operator_boundary::{Act, OperatorPaths};
use horch_core::harness::HarnessKind;
use horch_core::roster::{PermissionMode, Teammate};
use horch_core::runtime::{MapEnv, RuntimeContext};
use serde_json::{json, Value};

/// The fakes that the tests launch.
const FAKES: [&str; 2] = ["fake-claude", "fake-codex"];

/// Each adapter of the suite (MAP.md Q5).
const ADAPTERS: [HarnessKind; 6] = [
    HarnessKind::Claude,
    HarnessKind::Codex,
    HarnessKind::Antigravity,
    HarnessKind::OpenCode,
    HarnessKind::Pi,
    HarnessKind::Prime,
];

/// The folder of the built fakes: `target/<profile>`, beside this test.
fn bin_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let mut dir = exe.parent().unwrap().to_path_buf();
    if dir.file_name().and_then(|n| n.to_str()) == Some("deps") {
        dir.pop();
    }
    dir
}

/// The variables cargo sets for a test process. A nested build that sees
/// them has another fingerprint, and builds each crate again.
const CARGO_RUN_VARS: &[&str] = &[
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_CRATE_NAME",
    "CARGO_BIN_NAME",
    "CARGO_PRIMARY_PACKAGE",
    "CARGO_TARGET_TMPDIR",
    "CARGO_RUSTC_CURRENT_DIR",
    "OUT_DIR",
];

/// Build the fakes 1 time in this test process, into the target folder
/// of this test. `cargo test -p horch-core` does not build them.
fn fake(name: &str) -> PathBuf {
    static BUILT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    let dir = bin_dir();
    BUILT.get_or_init(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut cmd = StdCommand::new(cargo);
        for (key, _) in std::env::vars_os() {
            let key = key.to_string_lossy();
            if key.starts_with("CARGO_PKG_") || CARGO_RUN_VARS.contains(&key.as_ref()) {
                cmd.env_remove(key.as_ref());
            }
        }
        cmd.current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .args(["build", "--locked", "-p", "horch-e2e"]);
        for bin in FAKES {
            cmd.args(["--bin", bin]);
        }
        match dir.file_name().and_then(|n| n.to_str()) {
            Some("debug") | None => {}
            Some("release") => {
                cmd.arg("--release");
            }
            Some(profile) => {
                cmd.args(["--profile", profile]);
            }
        }
        cmd.arg("--target-dir").arg(dir.parent().unwrap());
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "the build of the fakes failed: {cmd:?}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    });
    dir.join(name)
}

fn executable(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, PermissionsExt::from_mode(0o755)).unwrap();
}

/// The Codex of a test machine: fake-codex with the version and the
/// features that the operator boundary of Codex reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Codex {
    /// Codex 0.167.0 with the `network_proxy` feature.
    WithNetworkProxy,
    /// Codex 0.167.0 with no `network_proxy` feature.
    WithoutNetworkProxy,
}

/// A test machine in `/tmp`: a home, the horch folders, the scratch folder
/// and a fleet root with a real token file, a real channel socket and a
/// request folder.
struct Machine {
    tmp: tempfile::TempDir,
    bins: BTreeMap<&'static str, PathBuf>,
    /// The listener of the channel. It accepts no connection: a connect
    /// to it completes in the backlog.
    _channel: UnixListener,
}

impl Machine {
    fn new(codex: Codex) -> Machine {
        let tmp = tempfile::tempdir_in("/tmp").unwrap();
        for d in [
            "home",
            "state",
            "data",
            "ledger/fleet/run/requests",
            "scratch",
            "bin",
        ] {
            std::fs::create_dir_all(tmp.path().join(d)).unwrap();
        }
        // Stub Linux sandbox tools, so the Claude sandbox check passes on
        // Linux too.
        for tool in ["bwrap", "socat"] {
            executable(&tmp.path().join("bin").join(tool), "#!/bin/sh\nexit 0\n");
        }
        let paths = OperatorPaths::of(&tmp.path().join("ledger/fleet"));
        std::fs::write(&paths.token, "test-operator-token\n").unwrap();
        std::fs::set_permissions(&paths.token, PermissionsExt::from_mode(0o600)).unwrap();
        let channel = UnixListener::bind(&paths.channel).unwrap();

        let claude = tmp.path().join("bin/claude");
        executable(
            &claude,
            &format!(
                "#!/bin/sh\nunset CLAUDE_CODE_AUTO_COMPACT_WINDOW CLAUDE_AUTOCOMPACT_PCT_OVERRIDE\nexec '{}' \"$@\"\n",
                fake("fake-claude").display()
            ),
        );
        let features = match codex {
            Codex::WithNetworkProxy => "network_proxy  experimental  true",
            Codex::WithoutNetworkProxy => "shell_tool  stable  true",
        };
        let codex_bin = tmp.path().join("bin/codex");
        executable(
            &codex_bin,
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n  --version) echo 'codex-cli 0.167.0'; exit 0 ;;\n  features) echo '{features}'; exit 0 ;;\nesac\nexec '{}' \"$@\"\n",
                fake("fake-codex").display()
            ),
        );
        let bins = BTreeMap::from([("HORCH_CLAUDE_BIN", claude), ("HORCH_CODEX_BIN", codex_bin)]);
        Machine {
            tmp,
            bins,
            _channel: channel,
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.tmp.path().join(rel)
    }

    fn operator_paths(&self) -> OperatorPaths {
        OperatorPaths::of(&self.path("ledger/fleet"))
    }

    fn log(&self) -> PathBuf {
        self.path("fake.log")
    }

    /// The context of the process that runs the suite.
    fn ctx(&self) -> RuntimeContext {
        let s = |p: PathBuf| p.to_string_lossy().into_owned();
        let mut env = MapEnv::new(self.path("home"))
            .with("HOME", &s(self.path("home")))
            .with("PATH", &s(self.path("bin")))
            .with("HORCH_STATE_DIR", &s(self.path("state")))
            .with("HORCH_DATA_DIR", &s(self.path("data")))
            .with("HORCH_LEDGER_DB", &s(self.path("ledger/ledger.sqlite3")))
            .with("FLEET_ROOT", &s(self.path("ledger/fleet")))
            .with(
                "HORCH_CLAUDE_MANAGED_SETTINGS",
                &s(self.path("home/no-managed-settings.json")),
            );
        for (key, bin) in &self.bins {
            env = env.with(key, &s(bin.clone()));
        }
        RuntimeContext::from_env(&env).unwrap()
    }

    /// The verdict of "Operator boundary" for `teammate`.
    fn check(&self, teammate: &Teammate) -> CheckResult {
        self.check_in(teammate, "conformance")
    }

    fn check_in(&self, teammate: &Teammate, scenario: &str) -> CheckResult {
        let ctx = self.ctx();
        let mut runner = ConformanceRunner {
            ctx: &ctx,
            teammate,
            fleet_window: Some((CONTEXT_LIMIT, "context-windows.md".into())),
            // The fakes read the operator paths from FLEET_ROOT, not from
            // the prompt. Without it they probe the fleet root of the
            // host.
            env_rule: vec![
                ("HORCH_FAKE_SCENARIO".into(), scenario.into()),
                (
                    "HORCH_FAKE_LOG".into(),
                    self.log().to_string_lossy().into_owned(),
                ),
                (
                    "FLEET_ROOT".into(),
                    self.path("ledger/fleet").to_string_lossy().into_owned(),
                ),
            ],
            scratch: self.path("scratch"),
            first_event_limit: FIRST_EVENT_LIMIT,
        };
        run_check(&mut runner, Check::OperatorBoundary)
    }

    /// No fake saw a violation, such as the forbidden name in its
    /// environment (SR-7).
    fn assert_no_violation(&self) {
        let text = std::fs::read_to_string(self.log()).unwrap_or_default();
        assert!(!text.is_empty(), "no fake ran");
        for line in text.lines() {
            let call: Value = serde_json::from_str(line).unwrap();
            let v = &call["violations"];
            assert!(
                v.is_null() || v.as_array().is_some_and(Vec::is_empty),
                "the fake saw a violation: {call}"
            );
        }
    }

    /// The launch left no throwaway folder and no request file, and the
    /// token is still the operator's.
    fn assert_clean(&self) {
        let left: Vec<_> = std::fs::read_dir(self.path("scratch"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert!(left.is_empty(), "throwaway folders are left: {left:?}");
        let requests: Vec<_> = std::fs::read_dir(&self.operator_paths().requests)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert!(requests.is_empty(), "request files are left: {requests:?}");
        let log = std::fs::read_to_string(self.log()).unwrap_or_default();
        assert!(
            !log.contains("test-operator-token"),
            "a fake printed the token"
        );
    }

    /// A Claude profile whose last `--settings` gives these sandbox rules,
    /// so the rendered config of the launch is `filesystem`.
    fn claude_with_rules(&self, filesystem: Value) -> Teammate {
        let file = self.path("home/override-settings.json");
        let settings = json!({"sandbox": {"enabled": true, "filesystem": filesystem}});
        std::fs::write(&file, settings.to_string()).unwrap();
        let mut t = profile(HarnessKind::Claude);
        t.args = vec!["--settings".into(), file.to_string_lossy().into_owned()];
        t
    }
}

/// The reference profile of an adapter: a sandboxed worker in exec mode
/// where the adapter has exec mode.
fn profile(agent: HarnessKind) -> Teammate {
    let name = format!("conformance-{}", agent.as_str());
    let model = match agent {
        HarnessKind::Claude => "claude-opus-5-5",
        HarnessKind::Codex => "gpt-5.5",
        HarnessKind::Antigravity => "gemini-3-pro",
        _ => "test-model",
    };
    let mut t = Teammate {
        name,
        agent,
        model: Some(model.into()),
        ..Teammate::default()
    };
    match agent {
        HarnessKind::Claude => t.sandbox = Some(serde_json::Map::new()),
        HarnessKind::Codex => t.permission_mode = Some(PermissionMode::Auto),
        _ => {}
    }
    if !matches!(
        agent,
        HarnessKind::Claude | HarnessKind::Codex | HarnessKind::Antigravity
    ) {
        return t;
    }
    // `mode: exec`, as a teammate file writes it.
    let mut v = serde_json::to_value(&t).unwrap();
    v["mode"] = json!("exec");
    serde_json::from_value(v).unwrap()
}

#[track_caller]
fn assert_verdict(r: &CheckResult, verdict: Verdict) {
    assert_eq!(r.check, Check::OperatorBoundary, "{r:?}");
    assert_eq!(r.verdict, verdict, "{r:?}");
    assert!(!r.evidence.is_empty(), "{r:?}");
}

#[track_caller]
fn assert_only_allowed(r: &CheckResult, act: Act) {
    assert_verdict(r, Verdict::Fail);
    assert!(
        r.evidence
            .starts_with(&format!("allowed: {act}, orphan: {act};")),
        "{r:?}"
    );
    for other in Act::ALL.into_iter().filter(|a| *a != act) {
        let refused = r.evidence.split_once("refused: ").map(|(_, r)| r);
        assert!(
            refused.is_some_and(|r| r.contains(&other.to_string())),
            "{other} is not refused: {r:?}"
        );
    }
}

#[test]
fn conformance_operator_boundary_check_runs_for_each_adapter() {
    let m = Machine::new(Codex::WithNetworkProxy);
    for agent in ADAPTERS {
        let r = m.check(&profile(agent));
        match agent {
            HarnessKind::Claude | HarnessKind::Codex => {
                assert_verdict(&r, Verdict::Pass);
                for act in Act::ALL {
                    assert!(r.evidence.contains(&act.to_string()), "{agent:?}: {r:?}");
                }
                assert!(r.evidence.starts_with("refused: "), "{agent:?}: {r:?}");
            }
            _ => {
                assert_verdict(&r, Verdict::NotSupported);
                assert!(
                    r.evidence
                        .contains(&format!("the {} sandbox", agent.as_str())),
                    "{agent:?}: {r:?}"
                );
            }
        }
    }
    m.assert_no_violation();
    m.assert_clean();
}

#[test]
fn operator_boundary_passes_when_the_double_fork_orphan_is_refused_each_act() {
    let m = Machine::new(Codex::WithNetworkProxy);
    for agent in [HarnessKind::Claude, HarnessKind::Codex] {
        let r = m.check(&profile(agent));
        assert_verdict(&r, Verdict::Pass);
        for act in Act::ALL {
            assert!(
                r.evidence.contains(&format!("orphan: {act}")),
                "{agent:?}: {r:?}"
            );
        }
    }
    m.assert_no_violation();
    m.assert_clean();
}

#[test]
fn operator_boundary_fails_when_the_double_fork_orphan_escapes_the_sandbox() {
    let m = Machine::new(Codex::WithNetworkProxy);
    let r = m.check_in(&profile(HarnessKind::Claude), "conformance,orphan_escapes");
    let orphan: Vec<String> = Act::ALL.iter().map(|a| format!("orphan: {a}")).collect();
    assert_verdict(&r, Verdict::Fail);
    assert!(
        r.evidence
            .starts_with(&format!("allowed: {};", orphan.join(", "))),
        "{r:?}"
    );
    for act in Act::ALL {
        let refused = r.evidence.split_once("refused: ").map(|(_, r)| r);
        assert!(
            refused.is_some_and(|r| r.starts_with(&act.to_string()) || r.contains(&format!(", {act}"))),
            "the agent act {act} is not refused: {r:?}"
        );
    }
    m.assert_clean();
}

#[test]
fn operator_boundary_fails_when_the_rules_allow_the_token_read() {
    let m = Machine::new(Codex::WithNetworkProxy);
    let p = m.operator_paths();
    let t = m.claude_with_rules(json!({"denyRead": [p.channel], "allowWrite": []}));
    assert_only_allowed(&m.check(&t), Act::ReadToken);
    m.assert_clean();
}

#[test]
fn operator_boundary_fails_when_the_rules_allow_the_channel() {
    let m = Machine::new(Codex::WithNetworkProxy);
    let p = m.operator_paths();
    let t = m.claude_with_rules(json!({"denyRead": [p.token], "allowWrite": []}));
    assert_only_allowed(&m.check(&t), Act::ConnectChannel);
    m.assert_clean();
}

#[test]
fn operator_boundary_fails_when_the_rules_allow_a_request_write() {
    let m = Machine::new(Codex::WithNetworkProxy);
    let p = m.operator_paths();
    let t = m.claude_with_rules(json!({
        "denyRead": [p.token, p.channel],
        "allowWrite": [p.requests],
    }));
    assert_only_allowed(&m.check(&t), Act::WriteRequests);
    m.assert_clean();
}

#[test]
fn operator_boundary_fails_when_the_fleet_root_has_no_token() {
    // A refusal of a read of a file that is not there proves nothing.
    let m = Machine::new(Codex::WithNetworkProxy);
    std::fs::remove_file(m.operator_paths().token).unwrap();
    let r = m.check(&profile(HarnessKind::Claude));
    assert_verdict(&r, Verdict::Fail);
    assert!(r.evidence.contains(&Act::ReadToken.to_string()), "{r:?}");
    assert!(r.evidence.contains("proves nothing"), "{r:?}");
}

#[test]
fn operator_boundary_fails_when_the_request_folder_is_not_writable() {
    let m = Machine::new(Codex::WithNetworkProxy);
    let requests = m.operator_paths().requests;
    std::fs::set_permissions(&requests, PermissionsExt::from_mode(0o500)).unwrap();
    let r = m.check(&profile(HarnessKind::Claude));
    std::fs::set_permissions(&requests, PermissionsExt::from_mode(0o700)).unwrap();
    assert_verdict(&r, Verdict::Fail);
    assert!(
        r.evidence.contains(&Act::WriteRequests.to_string()),
        "{r:?}"
    );
    assert!(r.evidence.contains("proves nothing"), "{r:?}");
    m.assert_clean();
}

#[test]
fn operator_boundary_not_supported_is_not_pass() {
    let m = Machine::new(Codex::WithoutNetworkProxy);
    let agy = m.check(&profile(HarnessKind::Antigravity));
    assert_verdict(&agy, Verdict::NotSupported);
    assert!(agy.evidence.contains("agy has no rule"), "{agy:?}");
    // A Codex that drops the network_proxy feature cannot deny the connect
    // to the channel.
    let codex = m.check(&profile(HarnessKind::Codex));
    assert_verdict(&codex, Verdict::NotSupported);
    assert!(codex.evidence.contains("network_proxy"), "{codex:?}");

    // `conformance.json` writes the word `not supported`, and the run has
    // no failed check for it.
    let subject = SuiteSubject {
        cli: "antigravity".into(),
        version: "1.0.0".into(),
        adapter_hash: "sha256:0123".into(),
    };
    let report = ConformanceReport {
        cli: subject.cli,
        version: subject.version,
        adapter_hash: subject.adapter_hash,
        checks: vec![agy],
    };
    assert!(report.failed().is_empty());
    assert!(report.failure().is_none());
    let text = conformance_json(&report);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["checks"][0]["verdict"], "not supported", "{text}");
    assert_eq!(read_conformance_json(&text).unwrap(), report);
}
