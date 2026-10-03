//! Harness modules: capabilities, the launch values the A0 oracle does not
//! cover, and session discovery by workdir.

use std::path::{Path, PathBuf};

use horch_core::launch::{self, LaunchEnv, Session};
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::teammates::Roster;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/harness")
}

/// Compare `actual` with a fixture captured before the A4 move. A missing
/// fixture is written once under `HORCH_BLESS=1`; an existing one never is.
fn check_fixture(name: &str, actual: &str) {
    let path = fixtures().join(name);
    if !path.exists() && std::env::var("HORCH_BLESS").ok().as_deref() == Some("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(want, actual, "{name} differs from the pre-A4 snapshot");
}

fn ctx(home: &Path) -> RuntimeContext {
    RuntimeContext::from_env(
        &MapEnv::new("/")
            .with("HOME", &home.to_string_lossy())
            .with("PATH", "/nonexistent")
            .with("HORCH_PRIME_BIN", "/fake/prime-agent"),
    )
    .unwrap()
}

/// The execpolicy rules a codex worker and a codex orchestrator get.
#[cfg(not(windows))]
#[test]
fn harness_codex_rules_snapshot() {
    use horch_core::codex;
    let roster = Roster::builtin().unwrap();
    for (name, rules) in [
        ("codex_rules_worker.rules", roster.exec_rules()),
        (
            "codex_rules_orchestrator.rules",
            roster.orchestrator_exec_rules(),
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let state = tmp.path().join("state");
        let installed =
            codex::Rules::install(&home, &home.join(".codex"), &state, "codex-1", rules).unwrap();
        let codex::Rules::Private { dir } = &installed else {
            panic!("expected a private CODEX_HOME");
        };
        let text = std::fs::read_to_string(dir.join("rules/horch.rules")).unwrap();
        check_fixture(name, &text);
    }
}

/// The argv a Prime worker launches with, daemon socket and session
/// directory included.
#[test]
fn harness_prime_daemon_args_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = ctx(tmp.path());
    let state = tmp.path().join("state");
    let roster = Roster::builtin().unwrap();
    let teammate = roster.require("prime").unwrap().clone();

    let mut out = String::new();
    for (label, session) in [
        ("fresh", Session::Unmanaged),
        ("resume", Session::Resume("/s/old.jsonl")),
    ] {
        let daemon = horch_core::prime::Daemon::install(&state, "prime-1").unwrap();
        let mut t = teammate.clone();
        t.args.extend([
            "--daemon-socket".into(),
            daemon.socket().to_string_lossy().into_owned(),
            "--session-dir".into(),
            daemon.sessions_dir().to_string_lossy().into_owned(),
        ]);
        let cmd =
            launch::command_in(&LaunchEnv::from_context(&ctx), &t, session, "P", None).unwrap();
        let base = daemon
            .socket()
            .parent()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let args: Vec<String> = std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|a| a.to_string_lossy().replace(&base, "<DAEMON>"))
            .collect();
        out.push_str(&format!(
            "{label}: {}\n",
            serde_json::to_string(&args).unwrap()
        ));
    }
    check_fixture("prime_daemon_args.txt", &out);
}
