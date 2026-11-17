//! The store of the fleet clock: `fleet_clock_s` and `clock_ticked_at` of
//! the `fleetd` row.
//!
//! [`ClockStore::tick`] runs in the write transaction of the pass. It
//! reads the clock, adds the awake time since the last tick of this
//! process ([`clock::tick`]) and writes the clock and the calendar time of
//! the tick. The last awake reading lives only in the memory of the
//! process, so the first tick after a restart adds 0. The row exists after
//! the first start of fleetd (`fleetd_row::start`).
//!
//! A transaction that rolls back after a tick loses the time of that tick:
//! the clock then runs late, never early, so no deadline fires too soon.

use std::io;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Transaction};

use crate::fleetd::fleet_runtime::domain::clock::{self, AwakeReading, FleetSeconds};
use crate::taskledger::util;

/// Read the awake clock of the platform layer.
pub fn awake_now() -> io::Result<AwakeReading> {
    let now = horch_platform::awake::now()?;
    Ok(AwakeReading {
        boot_id: now.boot_id,
        awake_ms: now.awake_ms,
    })
}

/// The value of the fleet clock in the `fleetd` row.
pub fn read(conn: &Connection) -> Result<FleetSeconds> {
    let s = conn
        .query_row("SELECT fleet_clock_s FROM fleetd WHERE id = 1", [], |r| {
            r.get(0)
        })
        .context("reading the fleet clock of the fleetd row")?;
    Ok(FleetSeconds(s))
}

/// The fleet clock of 1 fleetd process.
#[derive(Debug, Default)]
pub struct ClockStore {
    /// The awake reading that the next tick counts from; `None` until the
    /// first tick of this process.
    mark: Option<AwakeReading>,
}

impl ClockStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tick the fleet clock in `tx`, the write transaction of the pass, at
    /// the awake reading `now`. Gives the new value of the clock.
    pub fn tick(
        &mut self,
        tx: &Transaction<'_>,
        now: &AwakeReading,
        pass_period_s: u64,
    ) -> Result<FleetSeconds> {
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
        self.mark = Some(t.mark);
        Ok(t.clock)
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

    /// A ledger with the `fleetd` row at a fleet clock of `clock_s`.
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
        l.store.immediate(|tx| c.tick(tx, &now, PERIOD_S)).unwrap()
    }

    #[test]
    fn fleet_clock_does_not_advance_during_sleep_or_downtime() {
        let mut l = ledger(0);
        let mut c = ClockStore::new();
        tick(&mut l, &mut c, at("b", 0));
        tick(&mut l, &mut c, at("b", 5_000));
        // 1 h of awake time with no pass adds at most 2 pass periods.
        assert_eq!(tick(&mut l, &mut c, at("b", 3_605_000)), FleetSeconds(15));
        // A sleep of 1 h does not move the awake clock: it adds 0.
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
        // The next tick counts from the reading of the new boot.
        assert_eq!(tick(&mut l, &mut c, at("new", 6_000)), FleetSeconds(43));
    }

    #[test]
    fn first_tick_after_a_restart_adds_0() {
        let mut l = ledger(0);
        let mut old = ClockStore::new();
        tick(&mut l, &mut old, at("b", 0));
        assert_eq!(tick(&mut l, &mut old, at("b", 5_000)), FleetSeconds(5));
        // A new process starts with no reading: its first tick adds 0.
        let mut new = ClockStore::new();
        assert_eq!(tick(&mut l, &mut new, at("b", 600_000)), FleetSeconds(5));
        assert_eq!(tick(&mut l, &mut new, at("b", 605_000)), FleetSeconds(10));
    }

    #[test]
    fn tick_writes_the_clock_in_the_transaction_of_the_pass() {
        let mut l = ledger(0);
        let mut c = ClockStore::new();
        tick(&mut l, &mut c, at("b", 0));
        // A pass that rolls back writes no clock and no tick time.
        let out: Result<()> = l.store.immediate(|tx| {
            assert_eq!(c.tick(tx, &at("b", 5_000), PERIOD_S)?, FleetSeconds(5));
            assert_eq!(read(tx)?, FleetSeconds(5));
            bail!("the pass fails")
        });
        assert!(out.is_err());
        assert_eq!(read(l.store.conn()).unwrap(), FleetSeconds(0));
        // A pass that commits writes both.
        assert_eq!(tick(&mut l, &mut c, at("b", 10_000)), FleetSeconds(5));
        let ticked_at: String = l
            .store
            .conn()
            .query_row("SELECT clock_ticked_at FROM fleetd WHERE id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_ne!(ticked_at, "then");
        assert_eq!(read(l.store.conn()).unwrap(), FleetSeconds(5));
    }

    #[test]
    fn tick_without_the_fleetd_row_fails() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let mut c = ClockStore::new();
        let out = store.immediate(|tx| c.tick(tx, &at("b", 0), PERIOD_S));
        assert!(out.is_err());
    }

    #[test]
    fn awake_now_reads_the_platform_clock() {
        let a = awake_now().unwrap();
        let b = awake_now().unwrap();
        assert_eq!(a.boot_id, b.boot_id);
        assert!(b.awake_ms >= a.awake_ms);
    }
}
