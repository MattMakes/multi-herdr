//! The fleet clock: 1 count of seconds that stops while the machine sleeps
//! and while fleetd is down (`docs/specs/fleetd/design.md`, "Supervision,
//! liveness and reconciliation", "One fleet clock for each timer").
//!
//! Each pass adds the awake time since the last pass of the same process,
//! at most 2 pass periods. So a late pass of a live fleetd adds at most 2
//! pass periods. A sleep adds nothing, because the awake clock stops in
//! it. A downtime of fleetd adds nothing: the new process has no reading,
//! so its first tick adds 0. Each deadline of the
//! fleet (a job time limit, a backoff, a start timeout) is a value on this
//! clock.

/// A value on the fleet clock, in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FleetSeconds(pub i64);

/// 1 reading of the awake clock of the platform layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwakeReading {
    /// The id of the boot of the reading.
    pub boot_id: String,
    /// The awake time since that boot, in milliseconds.
    pub awake_ms: u64,
}

/// The result of 1 tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    /// The new value of the fleet clock.
    pub clock: FleetSeconds,
    /// The awake reading that the next tick counts from. Below the cap it
    /// keeps the part of a second that this tick did not add, so the clock
    /// does not lose a fraction of a second at each pass.
    pub mark: AwakeReading,
}

/// Add to `clock` the awake time from `last` to `now`, at most 2 times
/// `pass_period_s`. With no `last` (the first tick of a process), with
/// readings of different boots, or with `now` before `last`, the tick adds
/// 0.
pub fn tick(
    clock: FleetSeconds,
    last: Option<&AwakeReading>,
    now: &AwakeReading,
    pass_period_s: u64,
) -> Tick {
    let elapsed_ms = last
        .filter(|l| l.boot_id == now.boot_id)
        .and_then(|l| now.awake_ms.checked_sub(l.awake_ms));
    let Some(elapsed_ms) = elapsed_ms else {
        return Tick {
            clock,
            mark: now.clone(),
        };
    };
    let cap_s = pass_period_s.saturating_mul(2);
    let whole_s = elapsed_ms / 1000;
    let (added_s, mark_ms) = if whole_s >= cap_s {
        (cap_s, now.awake_ms)
    } else {
        (whole_s, now.awake_ms - elapsed_ms % 1000)
    };
    Tick {
        clock: FleetSeconds(clock.0.saturating_add_unsigned(added_s)),
        mark: AwakeReading {
            boot_id: now.boot_id.clone(),
            awake_ms: mark_ms,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD_S: u64 = 5;

    fn at(boot: &str, awake_ms: u64) -> AwakeReading {
        AwakeReading {
            boot_id: boot.into(),
            awake_ms,
        }
    }

    #[test]
    fn fleet_clock_does_not_advance_during_sleep_or_downtime() {
        // A gap of 1 h of awake time (a downtime of fleetd) adds at most
        // 2 pass periods: 10 s.
        let t = tick(
            FleetSeconds(100),
            Some(&at("b", 1_000)),
            &at("b", 1_000 + 3_600_000),
            PERIOD_S,
        );
        assert_eq!(t.clock, FleetSeconds(110));
        assert_eq!(t.mark, at("b", 1_000 + 3_600_000));
        // A sleep: the awake clock did not move, so the tick adds 0.
        let t = tick(
            FleetSeconds(100),
            Some(&at("b", 1_000)),
            &at("b", 1_000),
            PERIOD_S,
        );
        assert_eq!(t.clock, FleetSeconds(100));
    }

    #[test]
    fn tick_adds_the_elapsed_awake_time_below_2_periods() {
        let t = tick(
            FleetSeconds(7),
            Some(&at("b", 0)),
            &at("b", 5_000),
            PERIOD_S,
        );
        assert_eq!(t.clock, FleetSeconds(12));
        let t = tick(
            FleetSeconds(7),
            Some(&at("b", 0)),
            &at("b", 9_999),
            PERIOD_S,
        );
        assert_eq!(t.clock, FleetSeconds(16));
    }

    #[test]
    fn tick_keeps_the_part_of_a_second_for_the_next_tick() {
        // 2 passes of 5.6 s add 11 s in total, not 10 s.
        let first = tick(
            FleetSeconds(0),
            Some(&at("b", 0)),
            &at("b", 5_600),
            PERIOD_S,
        );
        assert_eq!(first.clock, FleetSeconds(5));
        assert_eq!(first.mark, at("b", 5_000));
        let second = tick(first.clock, Some(&first.mark), &at("b", 11_200), PERIOD_S);
        assert_eq!(second.clock, FleetSeconds(11));
    }

    #[test]
    fn readings_of_different_boots_add_0() {
        let t = tick(
            FleetSeconds(50),
            Some(&at("old", 1_000)),
            &at("new", 900_000),
            PERIOD_S,
        );
        assert_eq!(t.clock, FleetSeconds(50));
        assert_eq!(t.mark, at("new", 900_000));
    }

    #[test]
    fn a_reading_before_the_last_one_adds_0() {
        let t = tick(
            FleetSeconds(50),
            Some(&at("b", 9_000)),
            &at("b", 1_000),
            PERIOD_S,
        );
        assert_eq!(t.clock, FleetSeconds(50));
        assert_eq!(t.mark, at("b", 1_000));
    }

    #[test]
    fn the_first_tick_of_a_process_adds_0() {
        let t = tick(FleetSeconds(50), None, &at("b", 900_000), PERIOD_S);
        assert_eq!(t.clock, FleetSeconds(50));
        assert_eq!(t.mark, at("b", 900_000));
    }
}
