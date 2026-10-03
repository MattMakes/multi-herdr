//! How an execution ends. A7 holds `done` only; the worker's run arrives in a
//! later A6 unit.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::messaging::message::strip_done_prefix;
use crate::workspace::client::WorkspaceClient;

/// Who hears about an execution when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportTarget {
    Orchestrator,
    None,
}

/// What `horch done` knows about the worker that runs it.
#[derive(Debug, Clone)]
pub struct DoneRequest<'a> {
    pub record_id: &'a str,
    pub role: &'a str,
    /// This pane's `HERDR_PANE_ID`, which `pane get` upgrades to the public id.
    pub pane: &'a str,
    /// `HORCH_WORKSPACE_ID` when it is set; else the pane's own workspace.
    pub workspace: Option<&'a str>,
    pub summary: &'a str,
    pub report_to: ReportTarget,
}

/// The steps of [`done`] that are not workspace calls. The CLI runs them
/// against the ledger, `horch tell`, the mailbox and the tiler.
pub trait DoneSteps {
    fn mark_done(&self, record_id: &str, summary: &str) -> Result<()>;
    fn report(&self, line: &str) -> Result<()>;
    fn unregister(&self, workspace: &str, role: &str);
    fn settle(&self, workspace: &str);
}

/// Worker-facing self-shutdown, run only when the work is truly complete (not
/// while waiting on a question).
///
/// The order is fixed: mark done in the ledger, report to the orchestrator,
/// unregister the mailbox entry, settle the grid, close the pane. Everything
/// happens BEFORE the pane close, because closing the pane kills this very
/// process tree. The underlying agent session stays on disk and resumable.
///
/// A failed report is logged and the shutdown goes on; the ledger already
/// holds the summary.
pub fn done(ws: &dyn WorkspaceClient, steps: &dyn DoneSteps, req: &DoneRequest) -> Result<()> {
    let role = req.role;
    // `done` adds the tag and keyword itself; a summary that repeats them would
    // arrive as `[r] DONE: [r] DONE: ...`.
    let summary = strip_done_prefix(req.summary, role);
    steps.mark_done(req.record_id, summary)?;

    if req.report_to == ReportTarget::Orchestrator {
        if let Err(e) = steps.report(&format!("[{role}] DONE: {summary}")) {
            eprintln!(
                "horch done: could not reach orchestrator ({e:#}); ledger is updated, \
                 shutting down anyway"
            );
        }
    }

    let pane = ws.pane_get(req.pane)?;
    let workspace = req
        .workspace
        .map(str::to_owned)
        .or_else(|| pane.workspace_id.clone())
        .context("could not resolve this pane's workspace")?;

    steps.unregister(&workspace, role);

    // A departing worker leaves a hole and hands its width to whichever neighbour
    // happens to be its split sibling, which lumps the grid rather than spreading
    // it. Hand the tidy to a detached child: the close below kills this process
    // tree, so by the time the hole exists, this process is gone. The child pulls
    // the last worker into the free slot, and an overflow tab that lost its last
    // worker closes itself.
    steps.settle(&workspace);

    ws.pane_close(&pane.pane_id)
}
