//! The concern lock: the fleetd lock (PL-2), an exclusive file lock that 1
//! process holds for its whole life (design, "One fleetd for each ledger").
//!
//! The function is [`FleetdLock::try_take`]. The lock is an OS lock on an
//! open file, not a lock file with an owner: the OS frees it when the
//! holder ends in any way, also by `SIGKILL` or `TerminateProcess`. So a
//! killed fleetd never leaves the lock held.
//!
//! It is `std`'s [`File::try_lock`]: `flock(LOCK_EX | LOCK_NB)` on macOS
//! and Linux, and `LockFileEx` with `LOCKFILE_EXCLUSIVE_LOCK |
//! LOCKFILE_FAIL_IMMEDIATELY` on Windows. So the OS modules have no code
//! for this concern. Every OS has the lock, so no function of this concern
//! gives [`crate::NotSupported`].

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// The held fleetd lock. The lock is free again when this value drops or
/// when the process ends.
///
/// The lock belongs to the open file, and `std` opens each file so that a
/// child process does not get it (`O_CLOEXEC` on unix, a handle that is not
/// inherited on Windows). So a process that the holder starts never keeps
/// the lock after the holder ends.
#[derive(Debug)]
pub struct FleetdLock {
    _file: File,
}

impl FleetdLock {
    /// Take the lock at `path`. It gives `None` when another holder has it,
    /// also another open of the same path in this process. The file is made
    /// when it does not exist, and its content stays as it is. The folder
    /// of `path` must exist.
    pub fn try_take(path: &Path) -> io::Result<Option<FleetdLock>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(FleetdLock { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(err)) => Err(err),
        }
    }
}
