//! SR-7 for the harness adapters (fleetd-p7 ticket 05): a process that an
//! adapter of `crates/horch-core/src/harness/` starts never has a forbidden
//! name, also when the parent has it.
//!
//! The parent test runs this test binary again with each forbidden name set,
//! as the shell of the operator can have it. In that child, the version
//! probe of the Codex adapter starts a fake `codex` that writes its
//! environment to a file.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;

use horch_core::harness::codex::CodexFacts;
use horch_core::harness::launch::FORBIDDEN_ENV;

const DIR_ENV: &str = "HORCH_TEST_P7_HARNESS_DIR";

#[test]
fn a_harness_version_probe_removes_a_forbidden_name_of_the_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let mut parent = std::process::Command::new(std::env::current_exe().unwrap());
    parent
        .args([
            "--exact",
            "harness_probe_child",
            "--ignored",
            "--test-threads=1",
        ])
        .env(DIR_ENV, tmp.path());
    for key in FORBIDDEN_ENV {
        parent.env(key, "set-by-the-parent-test");
    }
    let out = parent.output().unwrap();
    assert!(
        out.status.success(),
        "child test failed: {}",
        String::from_utf8_lossy(&out.stdout)
    );

    let seen = std::fs::read_to_string(tmp.path().join("env.txt")).unwrap();
    assert!(seen.contains("PATH="), "the fake codex did not run: {seen}");
    for key in FORBIDDEN_ENV {
        assert!(
            !seen.lines().any(|l| l.starts_with(&format!("{key}="))),
            "the child has {key}: {seen}"
        );
    }
}

#[test]
#[ignore = "run by a_harness_version_probe_removes_a_forbidden_name_of_the_parent"]
fn harness_probe_child() {
    let dir = std::env::var_os(DIR_ENV).expect("run by its parent test");
    let dir = Path::new(&dir);
    for key in FORBIDDEN_ENV {
        assert!(
            std::env::var_os(key).is_some(),
            "the parent did not set {key}"
        );
    }
    let codex = dir.join("codex");
    let seen = dir.join("env.txt");
    std::fs::write(
        &codex,
        format!(
            "#!/bin/sh\n/usr/bin/env > '{}'\necho 'codex-cli 0.0.1'\n",
            seen.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).unwrap();

    let facts = CodexFacts::probe(&codex);
    assert_eq!(facts.version.as_deref(), Some("codex-cli 0.0.1"));
}
