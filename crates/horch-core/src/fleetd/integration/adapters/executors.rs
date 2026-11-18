//! The router of the integration job kinds: `bundle_build`, `fast_gate`,
//! `full_gate`, `fetch`, `push`, `rebase` and `trial_merge`. Ticket 03
//! routes `run_job` here.
//!
//! The job process writes no row (RR-5): [`run`] gives the facts of a run
//! in its result, and [`record`] in fleetd writes them from the result file
//! in the transaction of the end of the run. The result file is untrusted
//! input, so each `record` checks each fact before it writes.

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde::de::DeserializeOwned;

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, Ran};
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
        Kind::FastGate | Kind::FullGate => exec_gate::run(conn, row),
        Kind::Fetch => exec_fetch::run(conn, row),
        Kind::Push => exec_push::run(conn, row),
        Kind::Rebase => exec_rebase::run(conn, row),
        Kind::TrialMerge => exec_trial_merge::run(conn, row),
        kind => Ok(no_executor(kind).into()),
    }
}

/// Record the facts of the result `ran` of the run `row`, in `tx` of
/// fleetd. A kind whose facts are read by a later pass from `result_json`
/// records nothing here. An error is a refused fact: the caller rolls back
/// each write of this call.
pub fn record(ctx: &JobCtx<'_>, tx: &Connection, row: &JobRow, ran: &Ran) -> Result<()> {
    match row.kind {
        Kind::FastGate | Kind::FullGate => exec_gate::record(ctx, tx, row, ran),
        Kind::Fetch => exec_fetch::record(ctx, tx, row, ran),
        Kind::Push => exec_push::record(ctx, tx, row, ran),
        _ => Ok(()),
    }
}

/// The longest text of 1 fact of a result file, in bytes.
pub(crate) const FACT_TEXT_MAX: usize = 4096;

/// The facts `data` of a result file in the form `T`. `None` when the file
/// has no facts. A file whose facts have another form is refused.
pub(crate) fn facts_of<T: DeserializeOwned>(data: Option<&serde_json::Value>) -> Result<Option<T>> {
    data.map(|d| serde_json::from_value(d.clone()))
        .transpose()
        .context("the facts of the result file have the wrong form")
}

/// Refuse the text `text` of the fact `name` when it is longer than
/// `max` bytes, or holds a control character other than a new line or a
/// tab.
pub(crate) fn check_text(name: &str, text: &str, max: usize) -> Result<()> {
    if text.len() > max {
        bail!(
            "the fact {name} has {} bytes; the limit is {max}",
            text.len()
        );
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        bail!("the fact {name} holds a control character");
    }
    Ok(())
}

/// `text` cut to at most [`FACT_TEXT_MAX`] bytes on a character boundary,
/// so that the job process writes a fact that [`check_text`] accepts.
pub(crate) fn clip(mut text: String) -> String {
    if text.len() > FACT_TEXT_MAX {
        let mut end = FACT_TEXT_MAX;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text.retain(|c| !c.is_control() || c == '\n' || c == '\t');
    text
}
