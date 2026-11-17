//! The content of `heartbeat.json`, and the 2 rules that read it.
//!
//! fleetd writes `<fleet root>/run/heartbeat.json` after each full pass
//! (PL-5, FM-12; `docs/tickets/fleetd-p1/MAP.md`, Q20). `horch fleetd check`
//! reads it: [`is_hung`] says whether fleetd is hung, and [`is_older`]
//! whether it runs another binary than the CLI.

use crate::fleetd::fleet_runtime::domain::clock::AwakeReading;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;

/// The binary file of a process, as `ExeIdentity` of `exeid.rs` records
/// it when the process starts. A rebuilt or reinstalled binary at the same
/// path has another size or modification time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exe {
    pub path: String,
    pub len: u64,
    /// Nanoseconds since the Unix epoch.
    pub mtime_ns: u64,
}

/// 1 heartbeat: the fleetd that wrote it and the end of its last full
/// pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heartbeat {
    pub pid: u32,
    /// The start time of the process, in the unit of the platform layer.
    pub pid_start: u64,
    pub generation: Generation,
    /// The end of the pass on the awake clock. The hang rule reads it.
    pub awake: AwakeReading,
    /// The calendar time of the end of the pass, for status only.
    pub at: String,
    /// The binary of fleetd when it started.
    pub exe: Exe,
    pub version: String,
}

/// Whether the fleetd of `hb` is hung at the awake reading `now`: more
/// than `limit_s` seconds of awake time passed since its last full pass.
/// The awake clock stops while the machine sleeps, so a sleep is not a
/// hang. A reading of another boot, or a reading before the heartbeat,
/// does not compare: fleetd did not write a heartbeat in this boot, so it
/// is hung.
pub fn is_hung(hb: &Heartbeat, now: &AwakeReading, limit_s: u64) -> bool {
    if hb.awake.boot_id != now.boot_id {
        return true;
    }
    match now.awake_ms.checked_sub(hb.awake.awake_ms) {
        Some(elapsed_ms) => elapsed_ms > limit_s.saturating_mul(1000),
        None => true,
    }
}

/// Whether the fleetd of `hb` runs another binary than `this`, the binary
/// at the same path now: an install or a rebuild replaced it after fleetd
/// started.
pub fn is_older(hb: &Heartbeat, this: &Exe) -> bool {
    hb.exe != *this
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(boot: &str, awake_ms: u64) -> AwakeReading {
        AwakeReading {
            boot_id: boot.into(),
            awake_ms,
        }
    }

    fn hb(awake: AwakeReading) -> Heartbeat {
        Heartbeat {
            pid: 1,
            pid_start: 2,
            generation: Generation(1),
            awake,
            at: String::new(),
            exe: Exe {
                path: "/h".into(),
                len: 1,
                mtime_ns: 1,
            },
            version: "1".into(),
        }
    }

    #[test]
    fn a_reading_before_the_heartbeat_does_not_compare() {
        assert!(is_hung(&hb(at("b", 10_000)), &at("b", 9_000), 30));
    }

    #[test]
    fn another_path_is_another_binary() {
        let h = hb(at("b", 0));
        let moved = Exe {
            path: "/other".into(),
            ..h.exe.clone()
        };
        assert!(is_older(&h, &moved));
    }
}
