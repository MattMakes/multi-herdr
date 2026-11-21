//! The concern sandbox for Linux: bubblewrap (`bwrap`), when it is on the
//! `PATH` of the command.
//!
//! The whole disk is bound read-only. `/dev` and `/proc` are new. With a
//! temp folder `/tmp` is a new empty tmpfs. Then 1 mount for each rule,
//! from the shortest path to the longest: a readable path read-only, a
//! writable path writable, and a new empty tmpfs on a hidden folder. With
//! no network the command gets a new network namespace (`--unshare-net`).
//!
//! Each command runs in a new session, which takes the controlling
//! terminal away, so the command cannot type into the terminal outside the
//! sandbox (`TIOCSTI`).
//!
//! A job gets a new PID namespace (`--unshare-pid`), and `/proc` is the
//! `/proc` of that namespace. The init of the namespace stays in the
//! process group of `bwrap`, so a KILL of that group ends the init, and the
//! kernel then kills each process of the sandbox. A TERM does not reach the
//! init. `setsid` ([`SETSID`]) starts the command in its new session inside
//! the sandbox. A job does not use the `--new-session` of `bwrap`: it takes
//! the init out of the group of `bwrap` too. It does not use
//! `--die-with-parent`: the kernel sends that signal when the thread that
//! started `bwrap` ends, so it kills a live command.
//!
//! A command with the PIDs of the host gets `--new-session`, so a kill of
//! the group of `bwrap` does not reach it.

use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::sandbox::{Network, Rule};
use crate::NotSupported;

/// The `setsid` of util-linux, at the first of these paths that exists.
/// A fixed path, not the `PATH` of the command: the disk is bound
/// read-only, so the path is the same inside the sandbox.
const SETSID: [&str; 2] = ["/usr/bin/setsid", "/bin/setsid"];

/// The first executable `bwrap` in the absolute folders of `path`. A host
/// with no [`SETSID`] has no sandbox.
pub(crate) fn program(path: Option<&OsStr>) -> Result<PathBuf, NotSupported> {
    let bwrap = find_bwrap(path).ok_or(
        NotSupported::new("sandbox", "linux").because("bwrap is not on PATH; install bubblewrap"),
    )?;
    setsid().ok_or(
        NotSupported::new("sandbox", "linux")
            .because("setsid is not in /usr/bin or /bin; install util-linux"),
    )?;
    Ok(bwrap)
}

fn setsid() -> Option<&'static Path> {
    SETSID.iter().map(Path::new).find(|file| executable(file))
}

/// The mounts of `bwrap`: the disk read-only, then 1 mount for each rule,
/// in order, so the mount of a longer path covers the ones before it. A
/// readable path is bound read-only, a writable path is bound writable, a
/// hidden folder gets a new empty tmpfs and a hidden file gets
/// `/dev/null`. A path that does not exist gets no mount. With a temp
/// folder `/tmp` is a new empty tmpfs. With no network the command gets a
/// new network namespace. With `own_pids` the command gets a new PID
/// namespace, and the last argument is [`SETSID`], so `bwrap` runs the
/// command through it. Else `bwrap` starts a new session.
pub(crate) fn args(
    rules: &[Rule<'_>],
    temp: bool,
    network: Network,
    own_pids: bool,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = ["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]
        .map(OsString::from)
        .to_vec();
    args.push(
        if own_pids {
            "--unshare-pid"
        } else {
            "--new-session"
        }
        .into(),
    );
    if network != Network::Open {
        args.push("--unshare-net".into());
    }
    if temp {
        args.extend(["--tmpfs", "/tmp"].map(OsString::from));
    }
    for rule in rules {
        match *rule {
            // The tmpfs is the temp folder when it is /tmp.
            Rule::Write(path) if temp && path == Path::new("/tmp") => {}
            Rule::Read(path) | Rule::Write(path) | Rule::Hide(path) if !path.exists() => {}
            Rule::Read(path) => args.extend(["--ro-bind".into(), path.into(), path.into()]),
            Rule::Write(path) => args.extend(["--bind".into(), path.into(), path.into()]),
            Rule::Hide(path) if path.is_dir() => args.extend(["--tmpfs".into(), path.into()]),
            Rule::Hide(path) => {
                args.extend(["--ro-bind".into(), "/dev/null".into(), path.into()]);
            }
        }
    }
    if own_pids {
        args.push(setsid().unwrap_or(Path::new(SETSID[0])).into());
    }
    args
}

/// A relative folder of `path` is skipped: it names a folder of the
/// caller, not of the system.
fn find_bwrap(path: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join("bwrap"))
        .find(|file| executable(file))
}

fn executable(file: &Path) -> bool {
    file.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;
    use std::fs::OpenOptions;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    use crate::sandbox::{host_problem, sandboxed, sandboxed_job, Denials};

    #[test]
    #[ignore = "run by the terminal test, with a pty as its controlling terminal"]
    fn terminal_child() {
        // SAFETY: the path is a C string; open gives a new fd or -1.
        let tty = unsafe { libc::open(c"/dev/tty".as_ptr(), libc::O_RDWR | libc::O_NOCTTY) };
        let byte = b'x' as libc::c_char;
        // SAFETY: TIOCSTI reads 1 byte from a valid pointer; fd 0 is the pty.
        let pushed = unsafe { libc::ioctl(0, libc::TIOCSTI, &byte) } == 0;
        let word = |yes: bool| if yes { "allowed" } else { "refused" };
        println!("RESULT tty={} sti={}", word(tty >= 0), word(pushed));
    }

    fn on_a_terminal(sandbox: Option<Sandbox>) -> Option<String> {
        // SAFETY: posix_openpt gives a new fd or -1, and the 3 calls after
        // it take that fd and a buffer of its length.
        let master = unsafe {
            let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(fd >= 0, "posix_openpt: {}", std::io::Error::last_os_error());
            OwnedFd::from_raw_fd(fd)
        };
        let mut name = [0 as libc::c_char; 128];
        // SAFETY: as above.
        unsafe {
            assert_eq!(libc::grantpt(master.as_raw_fd()), 0);
            assert_eq!(libc::unlockpt(master.as_raw_fd()), 0);
            assert_eq!(
                libc::ptsname_r(master.as_raw_fd(), name.as_mut_ptr(), name.len()),
                0
            );
        }
        // SAFETY: ptsname_r wrote a C string into `name`.
        let path = unsafe { CStr::from_ptr(name.as_ptr()) }.to_str().unwrap();
        let pty = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(path)
            .unwrap();

        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "linux::sandbox::tests::terminal_child",
            "--ignored",
        ])
        .args(["--nocapture", "--test-threads=1"]);
        if let Some(sandbox) = sandbox {
            cmd = match sandbox(cmd, &[], &Denials::default()) {
                Ok(cmd) => cmd,
                Err(_) => {
                    assert!(
                        host_problem(None).is_some(),
                        "no sandbox on a host with one"
                    );
                    return None;
                }
            };
        }
        cmd.stdin(pty).stdout(Stdio::piped()).stderr(Stdio::piped());
        // SAFETY: setsid and ioctl are async-signal-safe. The pty is fd 0,
        // so TIOCSCTTY makes it the controlling terminal of the new session.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let out = cmd.spawn().unwrap().wait_with_output().unwrap();
        drop(master);
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            out.status.success(),
            "{text}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let line = text.split("RESULT ").nth(1).and_then(|r| r.lines().next());
        Some(
            line.unwrap_or_else(|| panic!("the child printed no result: {text}"))
                .to_string(),
        )
    }

    type Sandbox =
        fn(Command, &[std::path::PathBuf], &Denials) -> Result<Command, crate::NotSupported>;

    /// The reason of the new session: a sandboxed command has no
    /// controlling terminal, so it cannot push input into the terminal of
    /// the process that started the sandbox (`TIOCSTI`). A job and a
    /// command with the PIDs of the host both hold it.
    #[test]
    fn sandboxed_command_has_no_controlling_terminal_and_cannot_push_input() {
        let outside = on_a_terminal(None).unwrap();
        assert!(
            outside.starts_with("tty=allowed"),
            "the child has the pty as its terminal: {outside}"
        );
        for (name, sandbox) in [("host pids", sandboxed as Sandbox), ("job", sandboxed_job)] {
            let Some(inside) = on_a_terminal(Some(sandbox)) else {
                return;
            };
            assert_eq!(inside, "tty=refused sti=refused", "{name}");
        }
    }
}
