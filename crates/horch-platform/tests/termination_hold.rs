use horch_platform::signal;

#[cfg(unix)]
const HELPER: &str = "HORCH_PLATFORM_HOLD_HELPER";

#[cfg(unix)]
fn held_then_ended(mode: &str) {
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::Command;
    let dir = tempfile::tempdir().unwrap();
    let in_hold = dir.path().join("in-hold");
    let mut group = Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .expect("start a group");
    let helper = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "hold_helper", "--nocapture", "--test-threads=1"])
        .env(
            HELPER,
            format!("{} {mode} {}", group.id(), in_hold.display()),
        )
        .output()
        .expect("run the helper");
    let reaped = group.wait().unwrap();
    assert_eq!(
        std::fs::read_to_string(&in_hold).ok().as_deref(),
        Some("alive"),
        "the helper lives on in the hold: {helper:?}"
    );
    assert_eq!(
        helper.status.signal(),
        Some(libc::SIGTERM),
        "the helper ends by the first signal at the end of the hold: {helper:?}"
    );
    assert_eq!(reaped.signal(), Some(libc::SIGKILL), "the group is killed");
}

#[cfg(unix)]
#[test]
fn a_signal_in_the_hold_acts_at_its_end_one_time() {
    held_then_ended("twice");
}

#[cfg(unix)]
#[test]
fn a_panic_in_the_hold_ends_it_and_the_signal_acts() {
    held_then_ended("panic");
}

#[cfg(unix)]
#[test]
fn only_the_end_of_the_last_hold_acts() {
    held_then_ended("nested");
}

#[cfg(unix)]
#[test]
fn hold_helper() {
    use std::sync::atomic::{AtomicI32, Ordering};
    static GROUP: AtomicI32 = AtomicI32::new(0);
    let Ok(value) = std::env::var(HELPER) else {
        return;
    };
    let mut words = value.splitn(3, ' ');
    let pgid: i32 = words.next().unwrap().parse().unwrap();
    let mode = words.next().unwrap().to_string();
    let in_hold = std::path::PathBuf::from(words.next().unwrap());
    signal::kill_group_on_termination(&GROUP).expect("supported on unix");
    let outer = signal::hold_termination();
    GROUP.store(pgid, Ordering::SeqCst);
    // SAFETY: the handler of the hold only records the signal.
    unsafe { libc::raise(libc::SIGTERM) };
    match mode.as_str() {
        "twice" => {
            // SAFETY: as above.
            unsafe { libc::raise(libc::SIGINT) };
            std::fs::write(&in_hold, "alive").unwrap();
            drop(outer);
        }
        "panic" => {
            std::fs::write(&in_hold, "alive").unwrap();
            panic!("a panic in the hold");
        }
        "nested" => {
            let inner = signal::hold_termination();
            drop(inner);
            std::fs::write(&in_hold, "alive").unwrap();
            drop(outer);
        }
        other => panic!("no mode {other}"),
    }
    unreachable!("the signal of the hold ends the helper");
}

#[cfg(windows)]
#[test]
fn the_hold_changes_nothing_on_windows() {
    let hold = signal::hold_termination();
    drop(hold);
}
