use horch_platform::group;
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::time::{Duration, Instant};

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

#[cfg(unix)]
#[test]
fn is_group_leader_needs_the_leader_with_its_start_time() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "read line; sleep 60 >/dev/null & echo $!"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());
    group::new_group(&mut cmd);
    let mut leader = cmd.spawn().expect("start sh");
    let pgid = leader.id();
    let started = horch_platform::process::start_time(pgid).expect("sh runs");
    let live = group::is_group_leader(pgid, started);
    let wrong = group::is_group_leader(pgid, started.wrapping_add(1));
    use std::io::{Read, Write};
    leader.stdin.take().unwrap().write_all(b"go\n").unwrap();
    let mut out = String::new();
    leader
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .unwrap();
    leader.wait().unwrap();
    let member: i32 = out.trim().parse().unwrap();
    let gone = group::is_group_leader(pgid, started);
    let same = group::is_same_group(pgid, started);
    // SAFETY: the test started the member; a gone pid answers -1.
    unsafe { libc::kill(member, libc::SIGKILL) };
    assert_eq!(live, Ok(true));
    assert_eq!(wrong, Ok(false));
    assert_eq!(gone, Ok(false), "a gone leader proves nothing");
    assert_eq!(same, Ok(true), "the member keeps the group id");
    assert_eq!(group::is_group_leader(0, started), Ok(false));
}

#[cfg(unix)]
const REFUSE_CHILD: &str = "HORCH_PLATFORM_TEST_REFUSE_CHILD";

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

#[test]
fn a_detached_child_does_not_hold_the_pipe_of_its_parent() {
    const HELPER: &str = "HORCH_PLATFORM_DETACHED_CHILD_PID_FILE";
    if let Some(pid_file) = std::env::var_os(HELPER) {
        let mut cmd = long_child();
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let pid = group::spawn_detached(&mut cmd)
            .expect("start the detached child")
            .id();
        std::fs::write(pid_file, pid.to_string()).expect("write the pid");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("child.pid");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "a_detached_child_does_not_hold_the_pipe_of_its_parent",
        ])
        .env(HELPER, &pid_file)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the helper");
    let mut pipes = (helper.stdout.take().unwrap(), helper.stderr.take().unwrap());
    let (closed, eof) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut text = Vec::new();
        let _ = pipes.0.read_to_end(&mut text);
        let _ = pipes.1.read_to_end(&mut text);
        let _ = closed.send(text);
    });
    let status = helper.wait().expect("reap the helper");
    let output = eof.recv_timeout(std::time::Duration::from_secs(30));
    let pid = std::fs::read_to_string(&pid_file).unwrap_or_default();
    stop(pid.trim());
    let output = output.expect("the pipe of the helper stayed open after its exit");
    assert!(
        status.success(),
        "{status}: {}",
        String::from_utf8_lossy(&output)
    );
    assert!(!pid.trim().is_empty(), "the helper wrote no pid");
}

fn long_child() -> Command {
    #[cfg(unix)]
    {
        let mut cmd = Command::new("sleep");
        cmd.arg("60");
        cmd
    }
    #[cfg(windows)]
    {
        let mut cmd = Command::new("ping");
        cmd.args(["-n", "60", "127.0.0.1"]);
        cmd
    }
}

fn stop(pid: &str) {
    let Ok(pid) = pid.parse::<u32>() else { return };
    #[cfg(unix)]
    // SAFETY: plain call; a pid that has gone answers -1.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/F", "/PID", &pid.to_string()])
        .output();
}
