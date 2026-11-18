//! SR-7 for the process starts of `routing/` (fleetd-p7 ticket 08): a child
//! that a quota probe starts never has a forbidden name, also when the
//! parent has it in its environment.
//!
//! The test runs this test binary again with each forbidden name in its
//! environment, as a shell of the operator can have it. The ignored test
//! that it runs makes 1 start, and the child of that start writes its
//! environment to the file of `ENV_OUT`.
//!
//! The start of `execution/` (the `horch` call of the wrapper) is private,
//! so its test is in the test module of `execution/lifecycle.rs`. The
//! starts of the fleetd files of this ticket are all test code.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use horch_core::harness::launch::FORBIDDEN_ENV;
use horch_core::routing::quota_probe::{probe_version, VersionProbe};

/// The file that the child of the start writes its environment to.
const ENV_OUT: &str = "HORCH_TEST_START_ENV";

/// Run the ignored test `child` in a new process of this binary, with each
/// forbidden name set, and return the environment that its started child
/// wrote.
fn env_of_child(child: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("env.txt");
    let mut parent = Command::new(std::env::current_exe().unwrap());
    parent
        .args(["--exact", child, "--ignored", "--test-threads=1"])
        .env(ENV_OUT, &out);
    for key in FORBIDDEN_ENV {
        parent.env(key, "from-the-parent");
    }
    let run = parent.output().unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(
        run.status.success() && stdout.contains("1 passed"),
        "{stdout}\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let seen = std::fs::read_to_string(&out).unwrap();
    assert!(
        seen.lines().any(|l| l.starts_with(&format!("{ENV_OUT}="))),
        "the rest of the environment is missing:\n{seen}"
    );
    seen
}

#[test]
fn routing_version_probe_starts_without_a_forbidden_name() {
    let seen = env_of_child("routing_version_probe_child");
    for key in FORBIDDEN_ENV {
        assert!(
            !seen.lines().any(|l| l.starts_with(&format!("{key}="))),
            "{key} reached the child"
        );
    }
}

#[test]
#[ignore = "run by routing_version_probe_starts_without_a_forbidden_name"]
fn routing_version_probe_child() {
    let out = PathBuf::from(std::env::var_os(ENV_OUT).expect("run by its parent test"));
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("fake-harness");
    std::fs::write(
        &bin,
        format!("#!/bin/sh\nenv > \"${ENV_OUT}\"\necho 'fake 1.0'\n"),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        probe_version(&bin),
        VersionProbe::Version("fake 1.0".into())
    );
    assert!(out.exists(), "the probe did not start the fake harness");
}
