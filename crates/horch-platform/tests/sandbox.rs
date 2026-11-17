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

use horch_platform::sandbox::sandboxed;
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

/// True when the host has no sandbox for this test: Windows, or Linux
/// without `bwrap` on `PATH`. The result of `sandboxed` must then be
/// `NotSupported`.
fn host_has_no_sandbox() -> bool {
    cfg!(windows) || (cfg!(target_os = "linux") && !bwrap_on_path())
}

fn bwrap_on_path() -> bool {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .any(|dir| dir.join("bwrap").is_file())
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
