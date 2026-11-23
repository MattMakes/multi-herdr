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

const WAIT_STEP: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub struct Started {
    pub lock: FleetdLock,
    pub generation: Generation,
}

pub fn lock_path(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("fleetd.lock")
}

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

fn write_start(tx: &Transaction<'_>, pid: i64, pid_start: i64) -> Result<Generation> {
    let found = door::generation(tx)?;
    let generation = generation::next(found);
    let now = util::now();
    let sql = match found {
        None => {
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, ?1, ?2, ?3, ?4, 0, ?4)"
        }
        Some(_) => {
            "UPDATE fleetd SET generation = ?1, pid = ?2, pid_start = ?3, started_at = ?4
             WHERE id = 1"
        }
    };
    tx.execute(sql, params![generation.0, pid, pid_start, now])
        .context("writing the fleetd row")?;
    Ok(generation)
}

pub fn check_generation(tx: &Transaction<'_>, held: Generation) -> Result<(), DoorError> {
    let found = door::generation(tx)?;
    if !generation::is_current(held, found) {
        return Err(DoorError::GenerationLost { held, found });
    }
    Ok(())
}

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
