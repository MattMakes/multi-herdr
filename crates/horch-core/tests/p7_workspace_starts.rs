#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use horch_core::harness::launch::FORBIDDEN_ENV;
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::usage::Locations;
use horch_core::workspace::herdr::Herdr;

const CONTROL: &str = "HORCH_P7_WORKSPACE_STARTS_CONTROL";

fn arm_parent() {
    for key in FORBIDDEN_ENV {
        std::env::set_var(key, "forbidden-in-test");
    }
    std::env::set_var(CONTROL, "1");
}

fn env_writer(dir: &Path, name: &str, out: &Path) -> PathBuf {
    let bin = dir.join(name);
    let tmp = out.with_extension("tmp");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\nenv > '{}' && mv '{}' '{}'\n",
            tmp.display(),
            tmp.display(),
            out.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

fn child_env(out: &Path) -> String {
    let cap = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(text) = std::fs::read_to_string(out) {
            return text;
        }
        assert!(Instant::now() < cap, "the child wrote no {}", out.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_clean(env: &str) {
    assert!(
        env.lines().any(|l| l == format!("{CONTROL}=1")),
        "the child did not inherit the parent environment:\n{env}"
    );
    for key in FORBIDDEN_ENV {
        assert!(
            !env.lines().any(|l| l.starts_with(&format!("{key}="))),
            "the child has {key}"
        );
    }
}

#[test]
fn workspace_herdr_calls_remove_each_forbidden_name() {
    arm_parent();
    let dir = tempfile::tempdir().unwrap();

    let out = dir.path().join("unbounded.env");
    let herdr = Herdr::with_bin(env_writer(dir.path(), "herdr-unbounded", &out));
    assert!(herdr.server_reachable());
    assert_clean(&child_env(&out));

    let out = dir.path().join("bounded.env");
    let herdr = Herdr::with_bin(env_writer(dir.path(), "herdr-bounded", &out))
        .with_timeout(Duration::from_secs(60));
    herdr.agent_prompt("p_1", "text").unwrap();
    assert_clean(&child_env(&out));
}

#[test]
fn messaging_mail_courier_removes_each_forbidden_name() {
    arm_parent();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("courier.env");
    let exe = env_writer(dir.path(), "horch", &out);
    let env = MapEnv::new(dir.path())
        .with("HOME", &dir.path().join("home").to_string_lossy())
        .with(
            "HORCH_STATE_DIR",
            &dir.path().join("state").to_string_lossy(),
        )
        .with("HORCH_DATA_DIR", &dir.path().join("data").to_string_lossy())
        .with(
            "HORCH_LEDGER_DB",
            &dir.path().join("ledger.sqlite3").to_string_lossy(),
        );
    let mut ctx = RuntimeContext::from_env(&env).unwrap();
    ctx.bins.current_exe = Some(exe);
    horch_core::messaging::mail::start_courier(&ctx, "wTest").unwrap();
    assert_clean(&child_env(&out));
}

#[test]
fn telemetry_sqlite_read_removes_each_forbidden_name() {
    arm_parent();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("sqlite3.env");
    let db = dir.path().join("opencode.db");
    std::fs::write(&db, "").unwrap();
    let home = dir.path().join("home");
    let loc = Locations {
        claude_projects: home.join("claude"),
        codex_sessions: home.join("codex"),
        pi_sessions: home.join("pi"),
        opencode_db: db,
        sqlite3: env_writer(dir.path(), "sqlite3", &out),
        home,
    };
    let _ = horch_core::telemetry::context::read_current(
        &loc,
        &dir.path().join("cache"),
        "opencode",
        Some("ses_s1"),
    );
    assert_clean(&child_env(&out));
}
