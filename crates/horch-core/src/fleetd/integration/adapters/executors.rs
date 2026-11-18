//! The router of the integration job kinds: `bundle_build`, `fast_gate`,
//! `full_gate`, `fetch`, `push`, `rebase` and `trial_merge`. Ticket 03
//! routes `run_job` here.

use anyhow::Result;
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::Ran;
use crate::fleetd::fleet_runtime::domain::jobs::{no_executor, Kind};
use crate::fleetd::integration::adapters::{
    exec_bundle, exec_fetch, exec_gate, exec_push, exec_rebase, exec_trial_merge,
};

/// Runs the job `row` of an integration kind. Another kind gets the
/// "no executor" outcome. A run can give data for the next pass in
/// `result_json`, as `bundle_build` gives its candidate.
pub fn run(conn: &Connection, row: &JobRow) -> Result<Ran> {
    match row.kind {
        Kind::BundleBuild => exec_bundle::run(conn, row),
        Kind::FastGate | Kind::FullGate => exec_gate::run(conn, row).map(Ran::from),
        Kind::Fetch => exec_fetch::run(conn, row).map(Ran::from),
        Kind::Push => exec_push::run(conn, row).map(Ran::from),
        Kind::Rebase => exec_rebase::run(conn, row),
        Kind::TrialMerge => exec_trial_merge::run(conn, row),
        kind => Ok(no_executor(kind).into()),
    }
}
