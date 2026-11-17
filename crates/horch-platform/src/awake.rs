//! The concern awake: sleep and wake: the awake clock (PL-2).
//!
//! The functions are [`now`] and [`AwakeInstant::elapsed_since`]. The awake
//! clock counts the time since boot that the machine was awake: it stops
//! while the machine sleeps. 2 processes of 1 boot read the same clock, so
//! a reading of 1 process compares with a reading of another. The hang
//! check of fleetd and `max_minutes` of the fleet-run wrapper use it
//! (PL-4, PL-5).
//!
//! Each OS gives the clock and an id of the boot:
//! - macOS: `clock_gettime(CLOCK_UPTIME_RAW)`, the clock of
//!   `mach_absolute_time`, and `sysctl kern.bootsessionuuid`. That UUID is
//!   new at each boot. `kern.boottime` is not the id: the kernel moves it
//!   when the wall clock is set.
//! - Linux: `clock_gettime(CLOCK_MONOTONIC)`, which stops in suspend, and
//!   `/proc/sys/kernel/random/boot_id`.
//! - Windows: `QueryUnbiasedInterruptTime`, and the boot time in whole
//!   seconds since the Unix epoch: the system time minus
//!   `GetTickCount64`, rounded to 1 s.
//!
//! Every OS has the clock, so no function of this concern gives
//! [`crate::NotSupported`].

use crate::os::awake as os;
use std::io;
use std::time::Duration;

/// 1 reading of the awake clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwakeInstant {
    /// The id of the boot of the reading.
    pub boot_id: String,
    /// The awake time since that boot, in milliseconds.
    pub awake_ms: u64,
}

/// Read the awake clock.
pub fn now() -> io::Result<AwakeInstant> {
    Ok(AwakeInstant {
        boot_id: os::boot_id()?,
        awake_ms: os::awake_ms()?,
    })
}

impl AwakeInstant {
    /// The awake time from `earlier` to this reading. `None` when the 2
    /// readings are of different boots, or when `earlier` is later than
    /// this reading: such readings do not compare.
    pub fn elapsed_since(&self, earlier: &AwakeInstant) -> Option<Duration> {
        if self.boot_id != earlier.boot_id {
            return None;
        }
        self.awake_ms
            .checked_sub(earlier.awake_ms)
            .map(Duration::from_millis)
    }
}
