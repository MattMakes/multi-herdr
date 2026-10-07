//! Harness modules: the launch values the A0 oracle does not cover, and
//! session discovery by workdir.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use horch_core::harness::{CommandSpec, HarnessKind, LaunchEnv, PrepareRequest, Session};
use horch_core::roster::Roster;
use horch_core::runtime::{MapEnv, RuntimeContext};

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

/// The execpolicy rules a codex worker and a codex orchestrator get, through
/// the codex adapter's `prepare`.
#[cfg(not(windows))]
#[test]
fn harness_codex_rules_snapshot() {
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
        let ctx = ctx(&home);
        let prepared = HarnessKind::Codex
            .adapter()
            .prepare(
                &ctx,
                &PrepareRequest {
                    role: "codex-1",
                    exec_rules: rules,
                    skills: None,
                    compact_window: None,
                },
            )
            .unwrap();
        let (_, dir) = prepared
            .env
            .iter()
            .find(|(k, _)| k == "CODEX_HOME")
            .expect("a private CODEX_HOME");
        let text = std::fs::read_to_string(Path::new(dir).join("rules/horch.rules")).unwrap();
        check_fixture(name, &text);
    }
}

/// The argv a Prime worker launches with, daemon socket and session
/// directory included, through the Prime adapter's `prepare` and
/// `build_command`.
#[test]
fn harness_prime_daemon_args_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let ctx = ctx(tmp.path());
    let roster = Roster::builtin().unwrap();
    let teammate = roster.require("prime").unwrap().clone();
    let adapter = HarnessKind::Prime.adapter();

    let mut out = String::new();
    for (label, session) in [
        ("fresh", Session::Unmanaged),
        ("resume", Session::Resume("/s/old.jsonl")),
    ] {
        let prepared = adapter
            .prepare(
                &ctx,
                &PrepareRequest {
                    role: "prime-1",
                    exec_rules: &[],
                    skills: None,
                    compact_window: None,
                },
            )
            .unwrap();
        let mut t = teammate.clone();
        t.args.extend(prepared.extra_args.iter().cloned());
        let cmd = adapter
            .build_command(
                &LaunchEnv::from_context(&ctx),
                &CommandSpec {
                    teammate: &t,
                    session,
                    prompt: "P",
                    model_override: None,
                },
            )
            .unwrap();
        let base = prepared
            .sessions_dir
            .as_deref()
            .unwrap()
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
        // Dropped, not finished: finishing asks a real `prime-agent` for its
        // daemon's pid, and a test runs no real harness.
        drop(prepared);
    }
    check_fixture("prime_daemon_args.txt", &out);
}

// ─── ARC-11: discovery by canonical workdir ─────────────────────────────────

const UUID_A: &str = "11111111-2222-4333-8444-555555555555";
const UUID_B: &str = "66666666-7777-4888-8999-aaaaaaaaaaaa";

fn write_rollout(sessions: &Path, uuid: &str, cwd: &Path) {
    let day = sessions.join("2026/10/02");
    std::fs::create_dir_all(&day).unwrap();
    let line = serde_json::json!({
        "type": "session_meta",
        "payload": {"id": uuid, "cwd": cwd},
    });
    std::fs::write(
        day.join(format!("rollout-2026-10-02T10-00-00-{uuid}.jsonl")),
        format!("{line}\n"),
    )
    .unwrap();
}

fn codex_sessions(home: &Path, workdir: &Path) -> Vec<String> {
    let since = SystemTime::now() - Duration::from_secs(60);
    HarnessKind::Codex
        .adapter()
        .discover_sessions(&ctx(home), workdir, since, None)
}

#[test]
fn arc_11_codex_discovery_by_workdir() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let sessions = home.join(".codex/sessions");
    write_rollout(&sessions, UUID_A, &a);
    write_rollout(&sessions, UUID_B, &b);

    assert_eq!(codex_sessions(&home, &a), vec![UUID_A.to_string()]);
    assert_eq!(codex_sessions(&home, &b), vec![UUID_B.to_string()]);
    assert!(codex_sessions(&home, &tmp.path().join("c")).is_empty());
}

#[test]
fn arc_11_opencode_discovery_by_workdir() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let json = serde_json::json!([
        {"id": "ses_a", "directory": a, "created": now_ms},
        {"id": "ses_b", "directory": b, "created": now_ms},
    ])
    .to_string();
    let since = SystemTime::now() - Duration::from_secs(60);
    let ids = |dir: &Path| -> Vec<String> {
        horch_core::harness::opencode::parse_sessions(&json, &dir.to_string_lossy(), since)
            .into_iter()
            .map(|c| c.session_id)
            .collect()
    };
    assert_eq!(ids(&a), vec!["ses_a"]);
    assert_eq!(ids(&b), vec!["ses_b"]);
}

/// A workdir given as the temp dir's own spelling (`/var/...` on macOS)
/// matches a session recorded under its canonical path
/// (`/private/var/...`). On other systems a symlinked temp dir stands in.
#[test]
fn arc_11_canonical_tmp_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    std::fs::create_dir_all(&real).unwrap();
    let canonical = std::fs::canonicalize(&real).unwrap();

    #[cfg(target_os = "macos")]
    let given = {
        assert_ne!(real, canonical, "macOS temp dirs live under /private");
        real.clone()
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let given = {
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        link
    };
    #[cfg(not(unix))]
    let given = real.clone();

    let home = tmp.path().join("home");
    write_rollout(&home.join(".codex/sessions"), UUID_A, &canonical);
    assert_eq!(codex_sessions(&home, &given), vec![UUID_A.to_string()]);

    let json = serde_json::json!([{
        "id": "ses_a",
        "directory": canonical,
        "created": SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
    }])
    .to_string();
    let since = SystemTime::now() - Duration::from_secs(60);
    let found =
        horch_core::harness::opencode::parse_sessions(&json, &given.to_string_lossy(), since);
    assert_eq!(found.len(), 1);
    assert!(horch_core::harness::Workdir::new(&given).matches(&canonical));
}

/// The launch flow's session shape comes from capabilities: a fresh id is
/// passed only where horch mints it; every other fresh launch is unmanaged.
#[test]
fn arc_10_session_for_follows_capabilities() {
    use horch_core::execution::SessionMode;
    use horch_core::harness::launch::session_for;
    use horch_core::ids::SessionId;

    let fresh = SessionMode::Fresh(Some(SessionId::new(UUID_A).unwrap()));
    let none = SessionMode::Fresh(None);
    let resume = SessionMode::Resume(SessionId::new(UUID_B).unwrap());
    for kind in [
        HarnessKind::Claude,
        HarnessKind::Codex,
        HarnessKind::OpenCode,
        HarnessKind::Pi,
        HarnessKind::Prime,
    ] {
        let caps = kind.capabilities();
        let shape = |s: Session<'_>| format!("{s:?}");
        let want_fresh = if caps.caller_minted_session {
            format!("Fresh({UUID_A:?})")
        } else {
            "Unmanaged".to_string()
        };
        assert_eq!(shape(session_for(caps, &fresh)), want_fresh, "{kind}");
        assert_eq!(shape(session_for(caps, &none)), "Unmanaged", "{kind}");
        assert_eq!(
            shape(session_for(caps, &resume)),
            format!("Resume({UUID_B:?})"),
            "{kind}"
        );
    }
}
