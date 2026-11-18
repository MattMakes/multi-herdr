//! The check sandbox (SR-4): a sandboxed command writes only in its paths
//! and in its temp folder, and an OS with no sandbox gives `NotSupported`
//! with a reason (PL-3). The hiding sandbox writes only in its paths, and
//! cannot read, write or connect to a Unix socket below a hidden path.
//!
//! The test folders are under `CARGO_TARGET_TMPDIR`, not under the temp
//! folder of the OS, because the sandbox always lets a command write in the
//! temp folder.
//!
//! On Linux the sandbox is `bwrap`. The CI installs it. On Windows, and on
//! a Linux host without `bwrap`, the write tests assert `NotSupported`
//! instead.

use horch_platform::sandbox::{host_problem, sandboxed, sandboxed_hiding, Network};
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

/// Run `cmd` in the hiding sandbox with `writable` and `hidden`. `None`
/// when the host has no sandbox, after the check that `sandboxed_hiding`
/// says so with the reason.
fn run_hiding(
    cmd: Command,
    writable: &[PathBuf],
    hidden: &[PathBuf],
) -> Option<std::process::Output> {
    run_hiding_with(cmd, writable, hidden, Network::Open)
}

/// [`run_hiding`] with the network rule `network`.
fn run_hiding_with(
    cmd: Command,
    writable: &[PathBuf],
    hidden: &[PathBuf],
    network: Network,
) -> Option<std::process::Output> {
    match sandboxed_hiding(cmd, writable, hidden, network) {
        Ok(mut sandboxed) => Some(sandboxed.output().expect("start the sandboxed command")),
        Err(gap) => {
            assert!(host_has_no_sandbox(), "unexpected gap: {gap}");
            assert!(gap.reason.is_some(), "{gap}");
            None
        }
    }
}

/// A shell command `script` with the arguments `args`.
fn sh(script: &str, args: &[&Path]) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(script).arg("sh").args(args);
    cmd
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

#[test]
fn hiding_sandbox_cannot_read_a_hidden_file_or_folder() {
    let dir = test_dir();
    let hidden = dir.path().join("run");
    std::fs::create_dir(&hidden).unwrap();
    let token = hidden.join("operator.token");
    std::fs::write(&token, "secret\n").unwrap();
    for path in [&hidden, &token] {
        let read = sh("cat \"$1\" || ls \"$2\"", &[&token, &hidden]);
        let Some(out) = run_hiding(read, &[], std::slice::from_ref(path)) else {
            return;
        };
        let seen = String::from_utf8_lossy(&out.stdout);
        assert!(
            !seen.contains("secret"),
            "read {token:?} through {path:?}: {out:?}"
        );
        if path == &hidden {
            assert!(
                !seen.contains("operator.token"),
                "listed {hidden:?}: {out:?}"
            );
        }
    }
}

#[test]
fn hiding_sandbox_cannot_write_in_a_hidden_path_that_is_also_writable() {
    let dir = test_dir();
    let hidden = dir.path().join("run");
    std::fs::create_dir(&hidden).unwrap();
    let file = hidden.join("request.json");
    let write = write_command(&file);
    let Some(_) = run_hiding(
        write,
        &[dir.path().to_path_buf()],
        std::slice::from_ref(&hidden),
    ) else {
        return;
    };
    assert!(!file.exists(), "the sandbox let the command write {file:?}");
}

#[test]
fn hiding_sandbox_cannot_connect_to_a_unix_socket_in_a_hidden_folder() {
    // A socket path has a short limit (SUN_LEN), so this folder is in /tmp.
    let dir = tempfile::tempdir_in("/tmp").unwrap();
    let hidden = dir.path().join("run");
    std::fs::create_dir(&hidden).unwrap();
    let socket = hidden.join("c.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let connect = |hidden: &[PathBuf]| {
        let mut cmd = Command::new("perl");
        cmd.args(["-MIO::Socket::UNIX", "-e"])
            .arg("IO::Socket::UNIX->new(Peer => $ARGV[0]) or exit 3; exit 0")
            .arg(&socket);
        run_hiding(cmd, &[], hidden)
    };
    let Some(open) = connect(&[]) else {
        return;
    };
    assert!(
        open.status.success(),
        "the probe connects with no hidden path: {open:?}"
    );
    let denied = connect(std::slice::from_ref(&hidden)).unwrap();
    assert_eq!(denied.status.code(), Some(3), "{denied:?}");
}

#[test]
fn hiding_sandbox_has_no_temp_folder() {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(
        "f=\"${TMPDIR:-/tmp}/horch-sandbox-$$\"; echo hello > \"$f\" || exit 0; \
         ls \"$f\" > /dev/null 2>&1 && rm \"$f\" && exit 9; exit 0",
    );
    let dir = test_dir();
    let Some(out) = run_hiding(cmd, &[dir.path().to_path_buf()], &[]) else {
        return;
    };
    // Exit 9 when the write reached the temp folder.
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn hiding_sandbox_makes_and_renames_a_writable_folder_that_does_not_exist_yet() {
    let dir = test_dir();
    let from = dir.path().join("trees").join("t1");
    let to = dir.path().join("trees").join(".removing-t1");
    std::fs::create_dir(dir.path().join("trees")).unwrap();
    let script = "mkdir -p \"$1\" && echo x > \"$1/f\" && mv \"$1\" \"$2\"";
    let writable = [from.clone(), to.clone()];
    let Some(out) = run_hiding(sh(script, &[&from, &to]), &writable, &[]) else {
        return;
    };
    assert!(out.status.success(), "{out:?}");
    assert_eq!(std::fs::read_to_string(to.join("f")).unwrap(), "x\n");
    assert!(!from.exists());
}

#[test]
fn hiding_sandbox_cannot_write_a_sibling_of_a_writable_path_that_does_not_exist_yet() {
    let dir = test_dir();
    let trees = dir.path().join("trees");
    let sibling = trees.join("t2");
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(sibling.join("f"), "work\n").unwrap();
    let new = trees.join("t1");
    let script = "mkdir \"$1\" && echo x > \"$1/f\"; echo bad > \"$2/f\"; rm -rf \"$2\"; true";
    let Some(out) = run_hiding(
        sh(script, &[&new, &sibling]),
        std::slice::from_ref(&new),
        &[],
    ) else {
        return;
    };
    assert_eq!(
        std::fs::read_to_string(new.join("f")).unwrap(),
        "x\n",
        "{out:?}"
    );
    assert_eq!(
        std::fs::read_to_string(sibling.join("f")).unwrap(),
        "work\n",
        "{out:?}"
    );
}

#[test]
fn hiding_sandbox_with_no_network_cannot_connect_to_a_tcp_listener() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    let connect = |network: Network| {
        let mut cmd = Command::new("perl");
        cmd.args(["-MIO::Socket::INET", "-e"])
            .arg("IO::Socket::INET->new(PeerAddr => '127.0.0.1', PeerPort => $ARGV[0]) or exit 3; exit 0")
            .arg(&port);
        run_hiding_with(cmd, &[], &[], network)
    };
    let Some(open) = connect(Network::Open) else {
        return;
    };
    assert!(
        open.status.success(),
        "the probe connects with the network: {open:?}"
    );
    let denied = connect(Network::Denied).unwrap();
    assert_eq!(denied.status.code(), Some(3), "{denied:?}");
}
