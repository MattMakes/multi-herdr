//! The concern group: process groups, job objects and the detach of a child.

use horch_platform::group;
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::time::{Duration, Instant};

/// Poll `done` every 10 ms until it is true or 5 s pass.
#[cfg(unix)]
fn within_5s(mut done: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[test]
fn kill_child_group_ends_a_grandchild() {
    use std::io::BufRead;
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "sleep 60 & echo $!; wait"])
        .stdout(Stdio::piped());
    group::new_group(&mut cmd);
    let mut child = cmd.spawn().expect("start sh");
    let mut line = String::new();
    std::io::BufReader::new(child.stdout.take().expect("piped stdout"))
        .read_line(&mut line)
        .expect("read the pid of sleep");
    let grandchild: u32 = line.trim().parse().expect("a pid");
    assert!(horch_platform::process::alive(grandchild));

    assert!(group::kill_child_group(&mut child), "the signal is sent");
    child.wait().expect("reap sh");
    assert!(
        within_5s(|| !horch_platform::process::alive(grandchild)),
        "sleep {grandchild} outlived the kill of its group"
    );
}

#[cfg(unix)]
#[test]
fn exited_without_reap_leaves_the_child_reapable() {
    let mut child = Command::new("true").spawn().expect("start true");
    assert!(
        within_5s(|| group::exited_without_reap(&mut child).expect("waitid")),
        "true did not exit"
    );
    let status = child.wait().expect("the child is still unreaped");
    assert!(status.success());
}

#[cfg(unix)]
#[test]
fn detach_gives_a_new_session() {
    let mut cmd = Command::new("sleep");
    cmd.arg("60");
    group::detach(&mut cmd).expect("detach is supported");
    let mut child = cmd.spawn().expect("start sleep");
    let pid = child.id() as libc::pid_t;
    // SAFETY: plain calls; a bad pid answers -1.
    let (theirs, ours) = unsafe { (libc::getsid(pid), libc::getsid(0)) };
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(theirs, pid, "the child leads its own session");
    assert_ne!(theirs, ours);
}

#[cfg(unix)]
#[test]
fn is_same_group_is_false_for_a_wrong_start_time() {
    let mut cmd = Command::new("sleep");
    cmd.arg("60");
    group::new_group(&mut cmd);
    let mut child = cmd.spawn().expect("start sleep");
    let pgid = child.id();
    let started = horch_platform::process::start_time(pgid).expect("sleep runs");
    let right = group::is_same_group(pgid, started);
    let wrong = group::is_same_group(pgid, started.wrapping_add(1));
    group::kill_child_group(&mut child);
    let _ = child.wait();
    assert_eq!(right, Ok(true));
    assert_eq!(wrong, Ok(false));
}

/// Set in the child process of
/// `kill_group_id_refuses_an_id_that_names_no_group_it_started`.
#[cfg(unix)]
const REFUSE_CHILD: &str = "HORCH_PLATFORM_TEST_REFUSE_CHILD";

/// 0 names the group of the caller, and an id above `i32::MAX` becomes a
/// negative `pid_t`. `kill_group_id` refuses both and sends no signal. The
/// check runs in a child process that leads a new group, so a wrong kill
/// ends only that child.
#[cfg(unix)]
#[test]
fn kill_group_id_refuses_an_id_that_names_no_group_it_started() {
    if std::env::var_os(REFUSE_CHILD).is_some() {
        for pgid in [0, i32::MAX as u32 + 1, u32::MAX] {
            assert_eq!(group::kill_group_id(pgid), Ok(false), "pgid {pgid}");
        }
        return;
    }
    let mut cmd = Command::new(std::env::current_exe().expect("test binary"));
    cmd.args([
        "--exact",
        "kill_group_id_refuses_an_id_that_names_no_group_it_started",
    ])
    .env(REFUSE_CHILD, "1");
    group::new_group(&mut cmd);
    let status = cmd.status().expect("run the test binary");
    assert!(status.success(), "the child ended with {status:?}");
}

/// A `cmd` child whose `ping` grandchild holds stdout for about 60 s.
#[cfg(windows)]
fn cmd_with_a_grandchild() -> Command {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "ping -n 60 127.0.0.1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd
}

#[cfg(windows)]
#[test]
fn contained_child_is_killed_with_its_children() {
    use std::io::Read;
    use std::time::Duration;
    let mut cmd = cmd_with_a_grandchild();
    group::Contained::prepare(&mut cmd);
    let mut child = cmd.spawn().expect("start cmd");
    let contained = group::Contained::start(&child).expect("contain cmd");
    let mut stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut sink = Vec::new();
        let _ = sender.send(stdout.read_to_end(&mut sink));
    });

    assert!(contained.terminate(), "the job is terminated");
    child.wait().expect("reap cmd");
    // `ping` holds the pipe while it runs, so the end of the pipe proves it
    // has ended too.
    assert!(
        receiver.recv_timeout(Duration::from_secs(10)).is_ok(),
        "ping outlived the job"
    );
}

#[cfg(windows)]
#[test]
fn kill_child_group_on_windows_kills_the_child() {
    let mut child = Command::new("ping")
        .args(["-n", "60", "127.0.0.1"])
        .stdout(Stdio::null())
        .spawn()
        .expect("start ping");
    assert!(group::kill_child_group(&mut child));
    let status = child.wait().expect("reap ping");
    assert!(!status.success());
}

#[cfg(windows)]
#[test]
fn detach_and_kill_group_id_are_not_supported_on_windows() {
    let mut cmd = Command::new("cmd");
    assert!(group::detach(&mut cmd).is_err());
    assert!(group::kill_group_id(std::process::id()).is_err());
    assert!(group::is_same_group(std::process::id(), 0).is_err());
}
