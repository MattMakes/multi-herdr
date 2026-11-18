//! The router of the integration job kinds: `bundle_build`, `fast_gate`,
//! `full_gate`, `fetch`, `push` and `rebase`. Ticket 03 routes `run_job` here.

use anyhow::Result;
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::domain::jobs::{no_executor, Kind, Outcome};
use crate::fleetd::integration::adapters::{
    exec_bundle, exec_fetch, exec_gate, exec_push, exec_rebase,
};

/// Runs the job `row` of an integration kind. Another kind gets the
/// "no executor" outcome.
pub fn run(conn: &Connection, row: &JobRow) -> Result<Outcome> {
    match row.kind {
        Kind::BundleBuild => exec_bundle::run(conn, row),
        Kind::FastGate | Kind::FullGate => exec_gate::run(conn, row),
        Kind::Fetch => exec_fetch::run(conn, row),
        Kind::Push => exec_push::run(conn, row),
        Kind::Rebase => exec_rebase::run(conn, row),
        kind => Ok(no_executor(kind)),
    }
}
