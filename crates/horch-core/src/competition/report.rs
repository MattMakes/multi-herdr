//! The 1 report line of a competition round (fleet-dataset §7.2, FDS-11).
//!
//! `run --report-to <pane>` and `run --detach` save a pane with the
//! experiment. The coordinator that brings the round to a terminal state
//! types 1 line into that pane, the way `horch tell` types:
//!
//! ```text
//! [compete-<exp8>] DONE: round <round-id> is <state>. Winner: <config_id or none>. Promoted: <branch>@<sha or none>. Reason: <text or none>.
//! ```
//!
//! The fleet orchestrator's briefing (`teammates/_base/fleet-compete.md`)
//! matches the line by `[compete-`. A failed delivery prints 1 `NOTE:` line
//! on stderr and never changes the round.

use std::time::Duration;

use crate::competition::coordinator::RoundOutcome;
use crate::ids::{ExperimentId, RoundId};
use crate::measure::projection::RoundView;
use crate::messaging::delivery::{send_line_when_ready, Readiness, Timing};
use crate::workspace::client::WorkspaceClient;

/// The state a round that stopped on an error reports. `resume` drives it
/// on.
pub const STOPPED: &str = "STOPPED";

/// What the report line says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundReport {
    /// The 8-character experiment prefix of the dataset workspace's name.
    pub exp8: String,
    pub round: String,
    /// `DECIDED`, `NEEDS_INTERVENTION`, `REJECTED`, or [`STOPPED`].
    pub state: String,
    /// The winner's `config_id`.
    pub winner: Option<String>,
    /// The promotion's branch (short name) and the commit it holds now.
    pub promoted: Option<(String, String)>,
    pub reason: Option<String>,
}

impl RoundReport {
    /// The report of a round that ended with `outcome`. `reason` is the
    /// round's latest intervention reason, as `run` prints it.
    pub fn from_view(
        experiment: &ExperimentId,
        round: &RoundId,
        outcome: &RoundOutcome,
        view: &RoundView,
        reason: Option<&str>,
    ) -> RoundReport {
        let (state, reason) = match outcome {
            RoundOutcome::Decided => ("DECIDED", None),
            RoundOutcome::Rejected { budget: true } => (
                "REJECTED",
                Some("the budget stopped the candidates".to_string()),
            ),
            RoundOutcome::Rejected { budget: false } => {
                ("REJECTED", view.rejected.map(|r| r.to_string()))
            }
            RoundOutcome::NeedsIntervention => (
                "NEEDS_INTERVENTION",
                reason
                    .map(str::to_string)
                    .or_else(|| view.needs_intervention.as_ref().map(|n| n.reason.clone())),
            ),
        };
        // A winner that the promotion rejected is no winner.
        let winner = view
            .winner
            .as_ref()
            .filter(|_| !matches!(outcome, RoundOutcome::Rejected { .. }))
            .map(|w| {
                view.candidates
                    .get(&w.label)
                    .and_then(|c| c.planned.as_ref())
                    .map_or_else(|| w.label.clone(), |p| p.config_id.clone())
            });
        let p = &view.promotion;
        let promoted = match (&p.started, &p.completed) {
            (Some(started), Some(done)) => Some((
                started
                    .target
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&started.target)
                    .to_string(),
                done.dest_after.clone(),
            )),
            _ => None,
        };
        RoundReport {
            exp8: experiment.short(),
            round: round.to_string(),
            state: state.to_string(),
            winner,
            promoted,
            reason,
        }
    }

    /// The report of a coordinator that stopped on `error` before the round
    /// reached a terminal state.
    pub fn stopped(experiment: &ExperimentId, round: &RoundId, error: &str) -> RoundReport {
        RoundReport {
            exp8: experiment.short(),
            round: round.to_string(),
            state: STOPPED.to_string(),
            winner: None,
            promoted: None,
            reason: Some(format!(
                "the coordinator stopped ({error}); run multi-herdr-dataset resume {experiment}"
            )),
        }
    }

    /// The line, on 1 line: a newline in the reason becomes a space.
    pub fn line(&self) -> String {
        let winner = self.winner.as_deref().unwrap_or("none");
        let promoted = match &self.promoted {
            Some((branch, sha)) => format!("{branch}@{sha}"),
            None => "none".to_string(),
        };
        let reason = self
            .reason
            .as_deref()
            .map(|r| r.split_whitespace().collect::<Vec<_>>().join(" "))
            .map(|r| r.trim_end_matches('.').to_string())
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| "none".to_string());
        format!(
            "[compete-{}] DONE: round {} is {}. Winner: {winner}. Promoted: {promoted}. Reason: {reason}.",
            self.exp8, self.round, self.state
        )
    }
}

/// Type the report into `pane`. A failure prints 1 `NOTE:` line on stderr;
/// false then.
pub fn deliver(ws: &dyn WorkspaceClient, pane: &str, report: &RoundReport) -> bool {
    match send_line_when_ready(
        ws,
        pane,
        &report.line(),
        Duration::ZERO,
        &Readiness::DEFAULT,
        &Timing::DEFAULT,
    ) {
        Ok(()) => true,
        Err(e) => {
            eprintln!(
                "NOTE: multi-herdr-dataset: the report of round {} did not reach pane {pane}: {e:#}",
                report.round
            );
            false
        }
    }
}
