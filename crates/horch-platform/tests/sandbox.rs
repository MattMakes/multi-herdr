//! The check sandbox (SR-4): a sandboxed command writes only in its paths
//! and in its temp folder, and an OS with no sandbox gives `NotSupported`
//! with a reason (PL-3).
//!
//! The test folders are under `CARGO_TARGET_TMPDIR`, not under the temp
//! folder of the OS, because the sandbox always lets a command write in the
//! temp folder.
//!
//! On Linux the sandbox is `bwrap`. The CI installs it. On Windows, and on
//! a Linux host without `bwrap`, the write tests assert `NotSupported`
//! instead.

use horch_platform::sandbox::{host_problem, sandboxed};
use std::path::{Path, PathBuf};
use std::process::Command;

fn test_dir() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

/// A command that writes `hello` to `file`.
fn write_command(file: &Path) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg("echo hello > \"$1\"").arg("sh").arg(file);
    cmd
}

/// True when this host has no sandbox for a command with the `PATH` of
/// this process: Windows, or Linux without `bwrap`. The result of
/// `sandboxed` must then be `NotSupported`.
/// `linux_finds_an_executable_bwrap_on_the_path` tests the lookup itself.
fn host_has_no_sandbox() -> bool {
    host_problem(std::env::var_os("PATH").as_deref()).is_some()
}

/// Run `cmd` in the sandbox with `writable`. `None` when the host has no
/// sandbox, after the check that `sandboxed` says so with the reason.
fn run_sandboxed(cmd: Command, writable: &[PathBuf]) -> Option<std::process::Output> {
    match sandboxed(cmd, writable) {
        Ok(mut sandboxed) => Some(sandboxed.output().expect("start the sandboxed command")),
        Err(gap) => {
            assert!(host_has_no_sandbox(), "unexpected gap: {gap}");
            assert!(gap.reason.is_some(), "{gap}");
            None
        }
    }
}

#[test]
fn sandboxed_command_writes_inside_its_paths() {
    let dir = test_dir();
    let file = dir.path().join("inside.txt");
    let Some(out) = run_sandboxed(write_command(&file), &[dir.path().to_path_buf()]) else {
        return;
    };
    assert!(out.status.success(), "{out:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello\n");
}

#[test]
fn sandboxed_command_writes_in_its_temp_folder() {
    let dir = test_dir();
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg("echo hello > \"${TMPDIR:-/tmp}/horch-sandbox-$$\" && rm \"${TMPDIR:-/tmp}/horch-sandbox-$$\"");
    let Some(out) = run_sandboxed(cmd, &[dir.path().to_path_buf()]) else {
        return;
    };
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn sandboxed_command_cannot_write_outside_its_paths() {
    let allowed = test_dir();
    let other = test_dir();
    let file = other.path().join("outside.txt");
    let Some(out) = run_sandboxed(write_command(&file), &[allowed.path().to_path_buf()]) else {
        return;
    };
    assert!(!out.status.success(), "{out:?}");
    assert!(!file.exists(), "the sandbox let the command write {file:?}");
}

#[test]
fn sandboxed_command_keeps_its_folder_and_environment() {
    let dir = test_dir();
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c")
        .arg("echo \"$HORCH_SANDBOX_TEST\" > out.txt")
        .current_dir(dir.path())
        .env("HORCH_SANDBOX_TEST", "kept");
    let Some(out) = run_sandboxed(cmd, &[dir.path().to_path_buf()]) else {
        return;
    };
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("out.txt")).unwrap(),
        "kept\n"
    );
}

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn windows_and_linux_without_bwrap_are_not_supported_with_a_reason() {
    let empty = test_dir();
    let mut cmd = Command::new("sh");
    // Linux looks for `bwrap` on the PATH of the command: here an empty folder.
    cmd.env("PATH", empty.path());
    let gap = sandboxed(cmd, &[empty.path().to_path_buf()]).expect_err("no sandbox");
    assert_eq!(gap.os, std::env::consts::OS);
    assert_eq!(gap.concern, "sandbox");
    let reason = gap.reason.expect("a reason");
    if cfg!(target_os = "linux") {
        assert!(reason.contains("bwrap"), "{reason}");
    }
    assert!(gap.to_string().ends_with(reason), "{gap}");
}

#[test]
#[cfg(target_os = "linux")]
fn linux_finds_an_executable_bwrap_on_the_path() {
    use std::os::unix::fs::PermissionsExt;
    let dir = test_dir();
    let bwrap = dir.path().join("bwrap");
    std::fs::write(&bwrap, "#!/bin/sh\n").unwrap();
    let path = std::env::join_paths([Path::new("relative"), dir.path()]).unwrap();
    std::fs::set_permissions(&bwrap, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        host_problem(Some(&path)).is_some(),
        "a bwrap that is not executable"
    );
    std::fs::set_permissions(&bwrap, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(host_problem(Some(&path)), None);
}

#[test]
#[cfg(target_os = "macos")]
fn macos_has_a_sandbox_on_any_path() {
    assert_eq!(host_problem(None), None);
}
