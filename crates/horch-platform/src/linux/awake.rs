//! The concern awake for Linux.

use std::io;

/// `CLOCK_MONOTONIC` does not count the time in suspend.
/// `CLOCK_BOOTTIME` does, so it is not the awake clock.
pub(crate) fn awake_ms() -> io::Result<u64> {
    crate::unix::awake::clock_ms(libc::CLOCK_MONOTONIC)
}

/// The UUID that the kernel makes at each boot.
pub(crate) fn boot_id() -> io::Result<String> {
    let id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    let id = id.trim();
    if id.is_empty() {
        return Err(io::Error::other("empty boot_id"));
    }
    Ok(id.to_string())
}
