use horch_platform::{process, signal};
use std::process::{Child, Command};

fn long_child() -> Child {
    #[cfg(unix)]
    let child = Command::new("sleep").arg("30").spawn();
    #[cfg(windows)]
    let child = Command::new("ping")
        .args(["-n", "30", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn();
    child.expect("start a long child")
}

fn identity(child: &Child) -> (u32, u64) {
    let pid = child.id();
    (pid, process::start_time(pid).expect("the child runs"))
}

#[cfg(unix)]
#[test]
fn signal_same_signals_only_the_recorded_process() {
    let mut child = long_child();
    let (pid, started) = identity(&child);
    assert_eq!(
        signal::signal_same(pid, started + 1, libc::SIGKILL),
        Ok(false),
        "a reused pid"
    );
    assert_eq!(child.try_wait().unwrap(), None, "the child still runs");
    assert_eq!(signal::signal_same(0, started, libc::SIGKILL), Ok(false));
    assert_eq!(
        signal::signal_same(u32::MAX, started, libc::SIGKILL),
        Ok(false)
    );
    assert_eq!(signal::signal_same(pid, started, libc::SIGKILL), Ok(true));
    assert!(!child.wait().unwrap().success(), "killed");
    assert_eq!(
        signal::signal_same(pid, started, libc::SIGKILL),
        Ok(false),
        "gone"
    );
}

#[cfg(windows)]
#[test]
fn signal_same_is_not_supported_on_windows() {
    let mut child = long_child();
    let (pid, started) = identity(&child);
    let got = signal::signal_same(pid, started, 9);
    assert_eq!(child.try_wait().unwrap(), None, "nothing was sent");
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        got,
        Err(horch_platform::NotSupported::new("signal", "windows"))
    );
}

#[test]
fn stop_same_stops_only_the_recorded_process() {
    use signal::Stop;
    let mut child = long_child();
    let (pid, started) = identity(&child);
    assert!(
        !signal::stop_same(pid, started + 1, Stop::Term),
        "a reused pid"
    );
    assert!(
        !signal::stop_same(pid, started + 1, Stop::Kill),
        "a reused pid"
    );
    assert_eq!(child.try_wait().unwrap(), None, "the child still runs");
    assert!(!signal::stop_same(0, started, Stop::Kill));
    assert!(!signal::stop_same(u32::MAX, started, Stop::Kill));
    assert!(signal::stop_same(pid, started, Stop::Term));
    assert!(!child.wait().unwrap().success(), "stopped");
    assert!(!signal::stop_same(pid, started, Stop::Kill), "gone");
}

#[cfg(unix)]
#[test]
fn terminate_politely_ends_a_child_that_handles_sigterm() {
    use std::io::{BufRead, BufReader};
    let mut child = Command::new("sh")
        .args([
            "-c",
            "trap 'exit 0' TERM; echo ready; while :; do sleep 0.1; done",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("start sh");
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .expect("read the ready line");
    assert_eq!(ready.trim(), "ready", "the trap is in place");
    let (pid, started) = identity(&child);

    assert_eq!(signal::terminate_politely(pid, started + 1), Ok(false));
    assert_eq!(child.try_wait().unwrap(), None, "the child still runs");
    assert_eq!(signal::terminate_politely(pid, started), Ok(true));
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0), "the trap ran: {status:?}");
    assert_eq!(signal::terminate_politely(pid, started), Ok(false), "gone");
}

#[cfg(windows)]
#[test]
fn terminate_politely_is_not_supported_on_windows() {
    let mut child = long_child();
    let (pid, started) = identity(&child);
    let got = signal::terminate_politely(pid, started);
    assert_eq!(child.try_wait().unwrap(), None, "nothing was sent");
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        got,
        Err(horch_platform::NotSupported::new("signal", "windows"))
    );
}

#[cfg(unix)]
#[test]
fn termination_flag_is_set_by_sigterm() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static FLAG: AtomicBool = AtomicBool::new(false);
    signal::on_termination_set(&FLAG, &[signal::Termination::Terminate])
        .expect("supported on unix");
    assert!(!FLAG.load(Ordering::SeqCst));
    // SAFETY: the handler above catches SIGTERM, so this process lives on.
    assert_eq!(unsafe { libc::raise(libc::SIGTERM) }, 0);
    assert!(FLAG.load(Ordering::SeqCst));
}

#[cfg(unix)]
#[test]
fn outlive_hangup_survives_sighup() {
    signal::outlive_hangup().expect("supported on unix");
    // SAFETY: the empty handler above catches SIGHUP, so this process lives on.
    assert_eq!(unsafe { libc::raise(libc::SIGHUP) }, 0);
}

#[cfg(unix)]
const HELPER: &str = "HORCH_PLATFORM_SIGNAL_HELPER";

#[cfg(unix)]
#[test]
fn kill_group_on_termination_kills_the_group_and_ends_by_the_signal() {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    let mut group = Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .expect("start a group");
    let pgid = group.id();
    let helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "kill_group_helper",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(HELPER, pgid.to_string())
        .output()
        .expect("run the helper");
    let reaped = group.wait().unwrap();
    assert_eq!(
        helper.status.signal(),
        Some(libc::SIGTERM),
        "the helper ends by SIGTERM: {helper:?}"
    );
    assert_eq!(reaped.signal(), Some(libc::SIGKILL), "the group is killed");
}

#[cfg(unix)]
#[test]
fn kill_group_helper() {
    use std::sync::atomic::AtomicI32;
    static GROUP: AtomicI32 = AtomicI32::new(0);
    let Ok(pgid) = std::env::var(HELPER) else {
        return;
    };
    GROUP.store(pgid.parse().unwrap(), std::sync::atomic::Ordering::SeqCst);
    signal::kill_group_on_termination(&GROUP).expect("supported on unix");
    // SAFETY: the handler kills the group and then ends this helper process.
    unsafe { libc::raise(libc::SIGTERM) };
    unreachable!("SIGTERM ends the helper");
}

#[cfg(windows)]
#[test]
fn handlers_are_not_supported_on_windows() {
    use std::sync::atomic::{AtomicBool, AtomicI32};
    static FLAG: AtomicBool = AtomicBool::new(false);
    static GROUP: AtomicI32 = AtomicI32::new(0);
    let gap = horch_platform::NotSupported::new("signal", "windows");
    assert_eq!(
        signal::on_termination_set(&FLAG, &[signal::Termination::Terminate]),
        Err(gap)
    );
    assert_eq!(signal::kill_group_on_termination(&GROUP), Err(gap));
    assert_eq!(signal::outlive_hangup(), Err(gap));
}
