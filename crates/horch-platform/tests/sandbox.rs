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

use horch_platform::sandbox::{host_problem, sandboxed, sandboxed_hiding, Denials, Network, Reach};
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
    match sandboxed(cmd, writable, &Denials::default()) {
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
    run_reach(
        cmd,
        &Reach {
            writable,
            readable: &[],
            hidden,
            denied: &Denials::default(),
            network: Network::Open,
        },
    )
}

/// Run `cmd` in the hiding sandbox with `reach`.
fn run_reach(cmd: Command, reach: &Reach<'_>) -> Option<std::process::Output> {
    match sandboxed_hiding(cmd, reach) {
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
    let gap =
        sandboxed(cmd, &[empty.path().to_path_buf()], &Denials::default()).expect_err("no sandbox");
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
fn hiding_sandbox_cannot_write_beside_a_writable_path_that_does_not_exist_yet() {
    let dir = test_dir();
    let trees = dir.path().join("trees");
    let sibling = trees.join("t2");
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(sibling.join("f"), "work\n").unwrap();
    let new = trees.join("t1");
    let script = "echo bad > \"$1/f\"; rm -rf \"$1\"; mkdir \"$2\"; true";
    let other = trees.join("t3");
    let Some(out) = run_hiding(
        sh(script, &[&sibling, &other]),
        std::slice::from_ref(&new),
        &[],
    ) else {
        return;
    };
    assert_eq!(
        std::fs::read_to_string(sibling.join("f")).unwrap(),
        "work\n",
        "{out:?}"
    );
    assert!(!other.exists(), "{out:?}");
}

#[test]
fn hiding_sandbox_reads_a_readable_path_in_a_hidden_folder_and_nothing_else_there() {
    let dir = test_dir();
    let home = dir.path().join("home");
    let repo = home.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("a.txt"), "tree\n").unwrap();
    std::fs::write(home.join("secret"), "secret\n").unwrap();
    let read = sh(
        "cat \"$1/a.txt\"; cat \"$2\"; echo more >> \"$1/a.txt\"; true",
        &[&repo, &home.join("secret")],
    );
    let reach = Reach {
        writable: &[],
        readable: std::slice::from_ref(&repo),
        hidden: std::slice::from_ref(&home),
        denied: &Denials::default(),
        network: Network::Open,
    };
    let Some(out) = run_reach(read, &reach) else {
        return;
    };
    let seen = String::from_utf8_lossy(&out.stdout);
    assert!(seen.contains("tree"), "{out:?}");
    assert!(!seen.contains("secret"), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(repo.join("a.txt")).unwrap(),
        "tree\n"
    );
}

#[test]
fn hiding_sandbox_writes_a_writable_path_in_a_hidden_folder() {
    let dir = test_dir();
    let home = dir.path().join("home");
    let git = home.join("repo").join(".git");
    std::fs::create_dir_all(&git).unwrap();
    let file = git.join("index");
    let reach = Reach {
        writable: std::slice::from_ref(&git),
        readable: &[],
        hidden: std::slice::from_ref(&home),
        denied: &Denials::default(),
        network: Network::Open,
    };
    let Some(out) = run_reach(write_command(&file), &reach) else {
        return;
    };
    assert!(out.status.success(), "{out:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello\n");
}

/// git makes the leading folders of a path with a `stat` of each parent
/// folder. So the command can read the metadata of each folder between a
/// hidden path and a writable path in it, but it cannot list such a folder,
/// read a file in it or see a path beside the writable path.
#[test]
fn hiding_sandbox_shows_only_the_metadata_of_the_parent_folders_of_a_writable_path() {
    let dir = test_dir();
    let home = dir.path().join("home");
    let parent = home.join("a");
    let tree = parent.join("b").join("tree");
    std::fs::create_dir_all(&tree).unwrap();
    std::fs::create_dir_all(parent.join("sibling")).unwrap();
    std::fs::write(parent.join("secret"), "secret\n").unwrap();
    std::fs::write(home.join("peer"), "peer\n").unwrap();
    let script = "test -d \"$1\" && test -d \"$1/a\" && test -d \"$1/a/b\" && echo stat-ok\n\
                  mkdir -p \"$2/x/y\" && echo mkdir-ok\n\
                  ls -a \"$1\" \"$1/a\" \"$1/a/b\"\n\
                  cat \"$1/a/secret\" \"$1/peer\"\n\
                  test -e \"$1/a/sibling\" && echo sibling-seen\n\
                  true";
    let reach = Reach {
        writable: std::slice::from_ref(&tree),
        readable: &[],
        hidden: std::slice::from_ref(&home),
        denied: &Denials::default(),
        network: Network::Open,
    };
    let Some(out) = run_reach(sh(script, &[&home, &tree]), &reach) else {
        return;
    };
    let seen = String::from_utf8_lossy(&out.stdout);
    assert!(seen.contains("stat-ok"), "{out:?}");
    assert!(seen.contains("mkdir-ok"), "{out:?}");
    assert!(tree.join("x").join("y").is_dir());
    for hidden in ["secret", "sibling", "peer"] {
        assert!(!seen.contains(hidden), "{hidden} is visible: {out:?}");
    }
}

#[test]
fn hiding_sandbox_hides_a_hidden_path_in_a_writable_path() {
    let dir = test_dir();
    let run = dir.path().join("fleet").join("run");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::write(run.join("operator.token"), "secret\n").unwrap();
    let reach = Reach {
        writable: &[dir.path().join("fleet")],
        readable: &[],
        hidden: std::slice::from_ref(&run),
        denied: &Denials::default(),
        network: Network::Open,
    };
    let read = sh("cat \"$1/operator.token\"; true", &[&run]);
    let Some(out) = run_reach(read, &reach) else {
        return;
    };
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("secret"),
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
        run_reach(
            cmd,
            &Reach {
                writable: &[],
                readable: &[],
                hidden: &[],
                denied: &Denials::default(),
                network,
            },
        )
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

/// A fleet root with the 3 operator paths of the denials: the token file,
/// the channel socket and the request folder. It is in `/tmp`, because a
/// socket path has a short limit (SUN_LEN). The run folder has mode 0700,
/// as the channel needs.
#[cfg(unix)]
struct Operator {
    root: tempfile::TempDir,
    token: PathBuf,
    socket: PathBuf,
    requests: PathBuf,
}

#[cfg(unix)]
fn operator() -> Operator {
    use std::os::unix::fs::DirBuilderExt;
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let run = root.path().join("run");
    std::fs::DirBuilder::new().mode(0o700).create(&run).unwrap();
    let requests = run.join("requests");
    std::fs::create_dir(&requests).unwrap();
    let token = run.join("operator.token");
    std::fs::write(&token, "secret\n").unwrap();
    Operator {
        token,
        socket: run.join("fleetd.sock"),
        requests,
        root,
    }
}

#[cfg(unix)]
impl Operator {
    fn denials(&self) -> Denials {
        Denials {
            read: vec![self.token.clone()],
            connect: vec![self.socket.clone()],
            write: vec![self.requests.clone()],
        }
    }

    /// Run `cmd` with `denied`. The whole fleet root, and also each path of
    /// `writable`, is writable, so only the denials stop an act.
    fn run(
        &self,
        cmd: Command,
        writable: &[PathBuf],
        denied: &Denials,
    ) -> Option<std::process::Output> {
        let mut all = vec![self.root.path().to_path_buf()];
        all.extend_from_slice(writable);
        run_reach(
            cmd,
            &Reach {
                writable: &all,
                readable: &[],
                hidden: &[],
                denied,
                network: Network::Open,
            },
        )
    }
}

/// The text of the refusal of the OS for a denied act on this OS.
#[cfg(unix)]
fn refused(out: &std::process::Output, linux: &str) -> bool {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let text = if cfg!(target_os = "macos") {
        "Operation not permitted"
    } else {
        linux
    };
    stderr.contains(text) && !stderr.contains("No such file")
}

#[test]
#[cfg(unix)]
fn sandboxed_child_cannot_read_the_denied_file() {
    let op = operator();
    let read = || sh("cat \"$1\"", &[&op.token]);
    let Some(open) = op.run(read(), &[], &Denials::default()) else {
        return;
    };
    assert!(
        String::from_utf8_lossy(&open.stdout).contains("secret"),
        "the child reads the token with no denial: {open:?}"
    );

    // The token is also in a writable path: the denial wins.
    let out = op
        .run(read(), std::slice::from_ref(&op.token), &op.denials())
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("secret"),
        "the child read the token: {out:?}"
    );
    if cfg!(target_os = "macos") {
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        assert!(refused(&out, ""), "{out:?}");
    } else {
        // bwrap puts an empty file (/dev/null) over the token.
        assert!(out.stdout.is_empty(), "{out:?}");
    }
    assert_eq!(std::fs::read_to_string(&op.token).unwrap(), "secret\n");
}

#[test]
#[cfg(unix)]
fn sandboxed_child_cannot_connect_to_the_denied_socket() {
    use horch_platform::channel::{Listener, Stream};
    use std::time::{Duration, Instant};
    let op = operator();
    let listener = Listener::bind(&op.socket).unwrap();
    let connect = || {
        sh(
            "exec perl -MIO::Socket::UNIX -e \
             'IO::Socket::UNIX->new(Peer => $ARGV[0]) or do { print STDERR \"$!\\n\"; exit 3 }; exit 0' \
             \"$1\"",
            &[&op.socket],
        )
    };
    let Some(open) = op.run(connect(), &[], &Denials::default()) else {
        return;
    };
    assert!(
        open.status.success(),
        "the child connects with no denial: {open:?}"
    );
    // The child closed its connection, so the peer facts can be gone.
    let _ = listener.accept();

    // The socket is also in a writable path: the denial wins.
    let out = op
        .run(connect(), std::slice::from_ref(&op.socket), &op.denials())
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert!(refused(&out, "Connection refused"), "{out:?}");
    // The first connection in the queue is the one of this test, so the
    // child made none.
    let deadline = Instant::now() + Duration::from_secs(5);
    let _mine = Stream::connect(&op.socket, deadline).unwrap();
    let (_, peer) = listener.accept().unwrap();
    assert_eq!(peer.pid, Ok(std::process::id()), "the child connected");
}

#[test]
#[cfg(unix)]
fn sandboxed_child_cannot_write_into_the_denied_folder() {
    let op = operator();
    let request = op.requests.join("r.json");
    // The request folder is also writable by itself: the denial wins.
    let out = op.run(
        write_command(&request),
        std::slice::from_ref(&op.requests),
        &op.denials(),
    );
    let Some(out) = out else {
        return;
    };
    assert!(!out.status.success(), "{out:?}");
    assert!(refused(&out, "Read-only file system"), "{out:?}");
    assert!(!request.exists(), "the child wrote {request:?}");
    assert_eq!(std::fs::read_dir(&op.requests).unwrap().count(), 0);
}

#[test]
#[cfg(unix)]
fn sandboxed_child_still_writes_its_allowed_paths() {
    let op = operator();
    let beside = op.root.path().join("run").join("fleetd.log");
    let out = op.root.path().join("out.txt");
    let script = "echo hello > \"$1\" && echo hello > \"$2\"";
    let Some(result) = op.run(sh(script, &[&out, &beside]), &[], &op.denials()) else {
        return;
    };
    assert!(result.status.success(), "{result:?}");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "hello\n");
    assert_eq!(std::fs::read_to_string(&beside).unwrap(), "hello\n");
}
