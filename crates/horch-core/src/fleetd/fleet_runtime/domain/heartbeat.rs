use crate::fleetd::fleet_runtime::domain::clock::AwakeReading;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exe {
    pub path: String,
    pub len: u64,
    pub mtime_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heartbeat {
    pub pid: u32,
    pub pid_start: u64,
    pub generation: Generation,
    pub awake: AwakeReading,
    pub at: String,
    pub exe: Exe,
    pub version: String,
}

pub fn is_hung(hb: &Heartbeat, now: &AwakeReading, limit_s: u64) -> bool {
    if hb.awake.boot_id != now.boot_id {
        return true;
    }
    match now.awake_ms.checked_sub(hb.awake.awake_ms) {
        Some(elapsed_ms) => elapsed_ms > limit_s.saturating_mul(1000),
        None => true,
    }
}

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
