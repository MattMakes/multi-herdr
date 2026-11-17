//! The `fleetd` row: its generation and its backfill, and the fleetd lock.
//!
//! The start of fleetd ([`start`]) does 2 things in this order:
//!
//! 1. It takes the fleetd lock `<fleet root>/run/fleetd.lock` through the
//!    platform lock. When another fleetd holds the lock, [`start`] gives
//!    `None`. Then fleetd exits at once with a message that names the lock
//!    and with exit code 0, so that a service manager does not restart it
//!    in a loop of errors.
//! 2. In 1 transaction it reads the `fleetd` row and writes the next
//!    generation, its PID, the start time of its process and `started_at`.
//!    The first start writes the row with generation 1 and a fleet clock of
//!    0 (the backfill of Phase 1). A later start keeps the fleet clock.
//!
//! The pass calls [`check_generation`] first in each write transaction.
//! On [`DoorError::GenerationLost`] fleetd exits with no effect: it rolls
//! back the transaction and starts no effect. Ticket 21 calls the exit.
//!
//! A restart stops the old fleetd by the PID and start time of the row,
//! then calls [`wait_until_free`], and only then starts the new fleetd. So
//! 2 fleetd processes of 1 ledger never run at once.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use horch_platform::lock::FleetdLock;
use rusqlite::{params, Transaction};

use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::application::ledger::DoorError;
use crate::fleetd::fleet_runtime::domain::generation;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::taskledger::store::Store;
use crate::taskledger::util;

/// The time between 2 tries of [`wait_until_free`].
const WAIT_STEP: Duration = Duration::from_millis(50);

/// A started fleetd: it holds the lock and the generation of the row.
#[derive(Debug)]
pub struct Started {
    /// The fleetd lock. It is free again when this value drops or when the
    /// process ends.
    pub lock: FleetdLock,
    pub generation: Generation,
}

/// The fleetd lock of the fleet at `fleet_root`.
pub fn lock_path(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("fleetd.lock")
}

/// Start fleetd on the ledger of `store`: take the lock, then write the
/// next generation to the `fleetd` row. `None` when another fleetd holds
/// the lock: the caller exits at once with code 0.
pub fn start(fleet_root: &Path, store: &mut Store) -> Result<Option<Started>> {
    let path = lock_path(fleet_root);
    let run = path.parent().expect("the lock path has a folder");
    std::fs::create_dir_all(run).with_context(|| format!("creating {}", run.display()))?;
    let Some(lock) =
        FleetdLock::try_take(&path).with_context(|| format!("taking {}", path.display()))?
    else {
        return Ok(None);
    };
    let pid = std::process::id();
    let pid_start = horch_platform::process::start_time(pid)
        .context("reading the start time of this process")?;
    let pid_start = i64::try_from(pid_start).context("the start time of this process")?;
    let generation = store.immediate(|tx| write_start(tx, i64::from(pid), pid_start))?;
    Ok(Some(Started { lock, generation }))
}

/// Write the start of the fleetd `pid` to the `fleetd` row, with the next
/// generation.
fn write_start(tx: &Transaction<'_>, pid: i64, pid_start: i64) -> Result<Generation> {
    let found = door::generation(tx)?;
    let generation = generation::next(found);
    let now = util::now();
    let sql = match found {
        // The first start: the fleet clock starts at 0.
        None => {
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, ?1, ?2, ?3, ?4, 0, ?4)"
        }
        // A later start keeps the fleet clock and the time of its last tick.
        Some(_) => {
            "UPDATE fleetd SET generation = ?1, pid = ?2, pid_start = ?3, started_at = ?4
             WHERE id = 1"
        }
    };
    tx.execute(sql, params![generation.0, pid, pid_start, now])
        .context("writing the fleetd row")?;
    Ok(generation)
}

/// [`DoorError::GenerationLost`] when `held` is not the generation of the
/// `fleetd` row. The pass calls it first in each write transaction.
pub fn check_generation(tx: &Transaction<'_>, held: Generation) -> Result<(), DoorError> {
    let found = door::generation(tx)?;
    if !generation::is_current(held, found) {
        return Err(DoorError::GenerationLost { held, found });
    }
    Ok(())
}

/// Wait until no process holds the fleetd lock at `path`, or until
/// `deadline`. True when the lock is free. The wait does not keep the lock:
/// the new fleetd takes it at its start.
pub fn wait_until_free(path: &Path, deadline: Instant) -> io::Result<bool> {
    loop {
        if FleetdLock::try_take(path)?.is_some() {
            return Ok(true);
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(false);
        }
        std::thread::sleep(WAIT_STEP.min(deadline - now));
    }
}
