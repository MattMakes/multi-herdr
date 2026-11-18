//! The executor of the `push` job (ticket 38).

use anyhow::Result;
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::domain::jobs::{no_executor, Outcome};

/// Runs the job `row`. Today it returns the "no executor" outcome.
pub fn run(_conn: &Connection, row: &JobRow) -> Result<Outcome> {
    Ok(no_executor(row.kind))
}
