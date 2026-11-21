#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FleetSeconds(pub i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwakeReading {
    pub boot_id: String,
    pub awake_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    pub clock: FleetSeconds,
    pub mark: AwakeReading,
}

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
        let t = tick(
            FleetSeconds(100),
            Some(&at("b", 1_000)),
            &at("b", 1_000 + 3_600_000),
            PERIOD_S,
        );
        assert_eq!(t.clock, FleetSeconds(110));
        assert_eq!(t.mark, at("b", 1_000 + 3_600_000));
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
