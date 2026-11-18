//! The herdr notification of a needs-you item.
//!
//! A pass that writes the file of a new open item reports the agent state
//! `blocked` on the pane [`PANE`], with the count of open items in the agent
//! label. herdr shows its toast for a pane that needs input. A herdr that
//! does not answer gives 1 log line, not an error (OR-3, MAP.md Q12).

use crate::fleetd::ideas::adapters::needs_you_folder::Mirrored;
use crate::workspace::herdr::Herdr;

/// The pane that shows the needs-you list.
pub const PANE: &str = "fleet-needs-you";

/// The `--source` of each report: stable and unique to fleetd.
pub const SOURCE: &str = "custom:fleetd-needs-you";

/// What [`notify`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The pass wrote no file of a new item: no report.
    Quiet,
    /// herdr took the report.
    Sent,
    /// herdr did not take the report. The text is 1 line for the fleetd log.
    Failed(String),
}

/// Report 1 notification when `mirrored` wrote the file of a new open
/// item. `seq` increases strictly from report to report, so herdr ignores
/// a stale one.
pub fn notify(herdr: &Herdr, mirrored: &Mirrored, seq: u64) -> Notice {
    if mirrored.new == 0 {
        return Notice::Quiet;
    }
    let agent = format!("needs-you ({})", mirrored.open);
    match herdr.report_agent(PANE, SOURCE, &agent, "blocked", seq) {
        Ok(()) => Notice::Sent,
        Err(e) => {
            let reason = format!("{e:#}").replace(['\r', '\n'], " ");
            Notice::Failed(format!(
                "needs-you: no herdr notification for {} open items: {}",
                mirrored.open,
                reason.trim()
            ))
        }
    }
}
