//! The awake clock: it does not count sleep, and 2 processes of 1 boot can
//! compare their readings (design, "One fleet clock for each timer"; PL-5).
//!
//! The test of 2 processes starts this test binary again as a child. The
//! env var `READER_ENV` makes the test `reader` print 1 reading.

use horch_platform::awake::{self, AwakeInstant};
use std::process::Command;
use std::time::Duration;

const READER_ENV: &str = "HORCH_PLATFORM_TEST_AWAKE_READER";
const READING: &str = "HORCH-PLATFORM-TEST-READING";

/// The child mode: print 1 reading as `READING <boot_id> <awake_ms>`. Not a
/// test of its own: it returns at once when the env var is not set.
#[test]
fn reader() {
    if std::env::var_os(READER_ENV).is_none() {
        return;
    }
    let now = awake::now().expect("read the awake clock");
    println!("\n{READING} {} {}", now.boot_id, now.awake_ms);
}

fn reading(boot_id: &str, awake_ms: u64) -> AwakeInstant {
    AwakeInstant {
        boot_id: boot_id.to_string(),
        awake_ms,
    }
}

#[test]
fn awake_clock_moves_forward() {
    let first = awake::now().unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let second = awake::now().unwrap();

    assert!(!first.boot_id.is_empty());
    assert_eq!(first.boot_id, second.boot_id);
    let elapsed = second.elapsed_since(&first).expect("1 boot");
    // Each reading drops its part of a millisecond.
    assert!(elapsed >= Duration::from_millis(49), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
}

#[test]
fn readings_of_2_processes_of_1_boot_compare() {
    let before = awake::now().unwrap();
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "reader", "--nocapture", "--test-threads=1"])
        .env(READER_ENV, "1")
        .output()
        .expect("run the reader");
    let after = awake::now().unwrap();
    assert!(out.status.success());

    let stdout = String::from_utf8(out.stdout).unwrap();
    let line = stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix(READING))
        .unwrap_or_else(|| panic!("no reading in:\n{stdout}"));
    let mut fields = line.split_whitespace();
    let child = reading(
        fields.next().expect("boot_id"),
        fields.next().expect("awake_ms").parse().unwrap(),
    );

    assert_eq!(child.boot_id, before.boot_id);
    assert!(child.elapsed_since(&before).is_some());
    assert!(after.elapsed_since(&child).is_some());
}

#[test]
fn readings_of_different_boots_do_not_compare() {
    let first = reading("boot-a", 1_000);
    let second = reading("boot-b", 5_000);

    assert_eq!(second.elapsed_since(&first), None);
    assert_eq!(first.elapsed_since(&second), None);
}

#[test]
fn readings_of_1_boot_give_their_difference() {
    let first = reading("boot-a", 1_000);
    let second = reading("boot-a", 31_500);

    assert_eq!(
        second.elapsed_since(&first),
        Some(Duration::from_millis(30_500))
    );
    assert_eq!(first.elapsed_since(&first), Some(Duration::ZERO));
}

#[test]
fn an_earlier_reading_gives_no_time() {
    let first = reading("boot-a", 1_000);
    let second = reading("boot-a", 5_000);

    assert_eq!(first.elapsed_since(&second), None);
}

/// Windows: `boot_id` is the boot time in whole seconds since the Unix
/// epoch, from the system time minus `GetTickCount64`. It is in the
/// past.
#[cfg(windows)]
#[test]
fn windows_boot_id_is_the_boot_time_in_seconds() {
    let now = awake::now().unwrap();
    let boot: u64 = now.boot_id.parse().expect("whole seconds");
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    assert!(boot <= wall, "boot {boot} after now {wall}");
    // 2017-01-01: no CI machine booted before it.
    assert!(boot > 1_483_228_800, "boot {boot}");
}

/// Windows: `awake_ms` comes from `QueryUnbiasedInterruptTime`, so it is
/// at most the time since boot.
#[cfg(windows)]
#[test]
fn windows_awake_ms_is_at_most_the_time_since_boot() {
    let now = awake::now().unwrap();
    let boot: u64 = now.boot_id.parse().unwrap();
    let wall = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 1 s for the rounding of the boot time.
    assert!(now.awake_ms / 1000 <= wall - boot + 1);
}
