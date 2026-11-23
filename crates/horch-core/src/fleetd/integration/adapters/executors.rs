use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use serde::de::DeserializeOwned;

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, Ran};
use crate::fleetd::fleet_runtime::domain::jobs::{no_executor, Kind};
use crate::fleetd::integration::adapters::{
    exec_bundle, exec_fetch, exec_gate, exec_push, exec_rebase, exec_trial_merge,
};

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

pub fn record(ctx: &JobCtx<'_>, tx: &Connection, row: &JobRow, ran: &Ran) -> Result<()> {
    match row.kind {
        Kind::FastGate | Kind::FullGate => exec_gate::record(ctx, tx, row, ran),
        Kind::Fetch => exec_fetch::record(ctx, tx, row, ran),
        Kind::Push => exec_push::record(ctx, tx, row, ran),
        _ => Ok(()),
    }
}

pub(crate) const FACT_TEXT_MAX: usize = 4096;

pub(crate) fn facts_of<T: DeserializeOwned>(data: Option<&serde_json::Value>) -> Result<Option<T>> {
    data.map(|d| serde_json::from_value(d.clone()))
        .transpose()
        .context("the facts of the result file have the wrong form")
}

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
