use crate::fleetd::ideas::adapters::needs_you_folder::Mirrored;
use crate::workspace::PaneHost;

pub const PANE: &str = "fleet-needs-you";

pub const SOURCE: &str = "custom:fleetd-needs-you";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Quiet,
    Sent,
    Failed(String),
}

pub fn notify(herdr: &PaneHost, mirrored: &Mirrored, seq: u64) -> Notice {
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
