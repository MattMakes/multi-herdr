use std::io;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Transaction};

use crate::fleetd::fleet_runtime::domain::clock::{self, AwakeReading, FleetSeconds, Tick};
use crate::taskledger::util;

pub fn awake_now() -> io::Result<AwakeReading> {
    let now = horch_platform::awake::now()?;
    Ok(AwakeReading {
        boot_id: now.boot_id,
        awake_ms: now.awake_ms,
    })
}

pub fn read(conn: &Connection) -> Result<FleetSeconds> {
    let s = conn
        .query_row("SELECT fleet_clock_s FROM fleetd WHERE id = 1", [], |r| {
            r.get(0)
        })
        .context("reading the fleet clock of the fleetd row")?;
    Ok(FleetSeconds(s))
}

#[derive(Debug, Default)]
pub struct ClockStore {
    mark: Option<AwakeReading>,
}

impl ClockStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tick(
        &self,
        tx: &Transaction<'_>,
        now: &AwakeReading,
        pass_period_s: u64,
    ) -> Result<Tick> {
        let t = clock::tick(read(tx)?, self.mark.as_ref(), now, pass_period_s);
        let changed = tx
            .execute(
                "UPDATE fleetd SET fleet_clock_s = ?1, clock_ticked_at = ?2 WHERE id = 1",
                params![t.clock.0, util::now()],
            )
            .context("writing the fleet clock of the fleetd row")?;
        if changed != 1 {
            bail!("the fleetd row is missing");
        }
        Ok(t)
    }

    pub fn committed(&mut self, tick: Tick) {
        self.mark = Some(tick.mark);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::Store;

    const PERIOD_S: u64 = 5;

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
    }

    fn ledger(clock_s: i64) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, 1, 1, 'then', ?1, 'then')",
                [clock_s],
            )
            .unwrap();
        Fixture { _dir: dir, store }
    }

    fn at(boot: &str, awake_ms: u64) -> AwakeReading {
        AwakeReading {
            boot_id: boot.into(),
            awake_ms,
        }
    }

    fn tick(l: &mut Fixture, c: &mut ClockStore, now: AwakeReading) -> FleetSeconds {
        let t = l.store.immediate(|tx| c.tick(tx, &now, PERIOD_S)).unwrap();
        let clock = t.clock;
        c.committed(t);
        clock
    }

    #[test]
    fn fleet_clock_does_not_advance_during_sleep_or_downtime() {
        let mut l = ledger(0);
        let mut c = ClockStore::new();
        tick(&mut l, &mut c, at("b", 0));
        tick(&mut l, &mut c, at("b", 5_000));
        assert_eq!(tick(&mut l, &mut c, at("b", 3_605_000)), FleetSeconds(15));
        assert_eq!(tick(&mut l, &mut c, at("b", 3_605_000)), FleetSeconds(15));
        assert_eq!(read(l.store.conn()).unwrap(), FleetSeconds(15));
    }

    #[test]
    fn tick_adds_the_elapsed_awake_time_below_2_periods() {
        let mut l = ledger(40);
        let mut c = ClockStore::new();
        tick(&mut l, &mut c, at("b", 1_000));
        assert_eq!(tick(&mut l, &mut c, at("b", 8_000)), FleetSeconds(47));
        assert_eq!(read(l.store.conn()).unwrap(), FleetSeconds(47));
    }

    #[test]
    fn readings_of_different_boots_add_0() {
        let mut l = ledger(40);
        let mut c = ClockStore::new();
        tick(&mut l, &mut c, at("old", 900_000));
        assert_eq!(tick(&mut l, &mut c, at("new", 3_000)), FleetSeconds(40));
        assert_eq!(tick(&mut l, &mut c, at("new", 6_000)), FleetSeconds(43));
    }

    #[test]
    fn first_tick_after_a_restart_adds_0() {
        let mut l = ledger(0);
        let mut old = ClockStore::new();
        tick(&mut l, &mut old, at("b", 0));
        assert_eq!(tick(&mut l, &mut old, at("b", 5_000)), FleetSeconds(5));
        let mut new = ClockStore::new();
        assert_eq!(tick(&mut l, &mut new, at("b", 600_000)), FleetSeconds(5));
        assert_eq!(tick(&mut l, &mut new, at("b", 605_000)), FleetSeconds(10));
    }

    #[test]
    fn tick_writes_the_clock_in_the_transaction_of_the_pass() {
        let mut l = ledger(0);
        let mut c = ClockStore::new();
        tick(&mut l, &mut c, at("b", 0));
        let out: Result<()> = l.store.immediate(|tx| {
            assert_eq!(
                c.tick(tx, &at("b", 5_000), PERIOD_S)?.clock,
                FleetSeconds(5)
            );
            assert_eq!(read(tx)?, FleetSeconds(5));
            bail!("the pass fails")
        });
        assert!(out.is_err());
        assert_eq!(read(l.store.conn()).unwrap(), FleetSeconds(0));
        assert_eq!(tick(&mut l, &mut c, at("b", 10_000)), FleetSeconds(10));
        let ticked_at: String = l
            .store
            .conn()
            .query_row("SELECT clock_ticked_at FROM fleetd WHERE id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_ne!(ticked_at, "then");
        assert_eq!(read(l.store.conn()).unwrap(), FleetSeconds(10));
    }

    #[test]
    fn tick_without_the_fleetd_row_fails() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let c = ClockStore::new();
        let out = store.immediate(|tx| c.tick(tx, &at("b", 0), PERIOD_S));
        assert!(out.is_err());
    }
}
