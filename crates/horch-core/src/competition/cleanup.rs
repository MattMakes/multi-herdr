//! Round cleanup (phase B5, PRO-05, PRO-07).
//!
//! Cleanup removes the candidates' worktrees and keeps their branches, so
//! every frozen commit stays reachable. It runs:
//!
//! - at the end of a round that does not promote (DECIDED or REJECTED);
//! - after a promotion, only once its receipt is on disk (PROMOTED);
//! - for NEEDS_INTERVENTION, only when the operator forces it.
//!
//! A worktree that cannot be removed is recorded as
//! `worktree.cleanup_failed` and the others are still removed. The fault
//! `abort-during-cleanup:<n>` stops after the n-th removal.

use std::path::PathBuf;

use anyhow::Result;

use crate::clock;
use crate::competition::model::RoundState;
use crate::competition::promotion::{same_dir, FaultFired};
use crate::ids::RoundId;
use crate::measure::event::{
    Actor, EventKind, FinalOutcome, RoundCleanupStarted, RoundCompleted, WorktreeCleanupFailed,
};
use crate::measure::paths::DatasetPaths;
use crate::measure::projection::RoundView;
use crate::measure::recorder::{NewEvent, Recorder};
use crate::runtime::fault::Faults;
use crate::vcs::git::GitClient;

/// The fault prefix: `abort-during-cleanup:<n>` stops after removal `n`.
pub(crate) const ABORT_DURING_CLEANUP: &str = "abort-during-cleanup";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CleanupOptions {
    /// Delete each candidate branch after its worktree (`--prune-branches`).
    pub prune_branches: bool,
    /// The operator's `cleanup` of a NEEDS_INTERVENTION round.
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupOutcome {
    /// The round is COMPLETE. `removed` and `failed` hold labels.
    Done {
        removed: Vec<String>,
        failed: Vec<String>,
    },
    /// Nothing was touched.
    Skipped { reason: String },
}

/// The repository the round's worktrees belong to, and where its records go.
pub struct RoundCleanup<'a, G: GitClient, R: Recorder> {
    pub git: &'a G,
    pub recorder: &'a R,
    pub paths: &'a DatasetPaths,
    pub faults: &'a Faults,
    pub repo: PathBuf,
}

impl<G: GitClient, R: Recorder> RoundCleanup<'_, G, R> {
    /// Clean up `round` as the projection `view` shows it. A round already in
    /// CLEANUP (a restart) continues from the state cleanup started from.
    pub fn run(
        &self,
        round: &RoundId,
        view: &RoundView,
        options: CleanupOptions,
    ) -> Result<CleanupOutcome> {
        let resuming = view.state == RoundState::Cleanup;
        let from = if resuming {
            view.cleaned_from.unwrap_or(RoundState::NeedsIntervention)
        } else {
            view.state
        };
        let final_outcome = match from {
            RoundState::Decided => FinalOutcome::Winner,
            RoundState::Rejected => FinalOutcome::Rejected,
            RoundState::Promoted => {
                // PRO-05: the receipt is durable before any worktree goes.
                if !self.paths.promotion(round)?.exists() {
                    return Ok(skipped(format!(
                        "round {round} is PROMOTED but has no receipt yet"
                    )));
                }
                FinalOutcome::Promoted
            }
            RoundState::NeedsIntervention if options.force || resuming => {
                FinalOutcome::NeedsIntervention
            }
            RoundState::NeedsIntervention => {
                return Ok(skipped(format!(
                    "round {round} needs intervention; only the operator's cleanup removes its worktrees"
                )));
            }
            other => {
                return Ok(skipped(format!(
                    "round {round} is {other}; cleanup waits for its end"
                )));
            }
        };
        let key = |what: &str| format!("{what}:{round}:{from}");
        if !resuming {
            self.emit(
                round,
                view,
                key("round.cleanup_started"),
                EventKind::RoundCleanupStarted(RoundCleanupStarted {}),
            )?;
        }

        let mut removed = Vec::new();
        let mut failed = Vec::new();
        let listed = self.git.worktree_list(&self.repo)?;
        for (label, candidate) in &view.candidates {
            let Some(worktree) = &candidate.worktree else {
                continue;
            };
            if !listed.iter().any(|(p, _)| same_dir(p, &worktree.path)) {
                continue;
            }
            if let Err(e) = self.git.worktree_remove(&self.repo, &worktree.path, true) {
                self.emit(
                    round,
                    view,
                    format!("{}:{label}", key("worktree.cleanup_failed")),
                    EventKind::WorktreeCleanupFailed(WorktreeCleanupFailed {
                        label: label.clone(),
                        path: worktree.path.clone(),
                        error: format!("{e:#}"),
                    }),
                )?;
                failed.push(label.clone());
                continue;
            }
            removed.push(label.clone());
            if options.prune_branches {
                self.prune(&worktree.branch)?;
            }
            let point = format!("{ABORT_DURING_CLEANUP}:{}", removed.len());
            if self.faults.has(&point) {
                return Err(FaultFired(point).into());
            }
        }

        self.emit(
            round,
            view,
            key("round.completed"),
            EventKind::RoundCompleted(RoundCompleted { final_outcome }),
        )?;
        Ok(CleanupOutcome::Done { removed, failed })
    }

    fn prune(&self, branch: &str) -> Result<()> {
        let refname = format!("refs/heads/{branch}");
        if let Some(sha) = self.git.rev_parse(&self.repo, &refname)? {
            let zero = "0".repeat(sha.len());
            self.git.update_ref_cas(&self.repo, &refname, &zero, &sha)?;
        }
        Ok(())
    }

    fn emit(&self, round: &RoundId, view: &RoundView, key: String, kind: EventKind) -> Result<()> {
        self.recorder.append(NewEvent {
            kind,
            actor: Actor::Coordinator,
            experiment_id: view.experiment_id.clone(),
            round_id: Some(round.clone()),
            execution_id: None,
            idempotency_key: key,
            occurred_at: clock::now(),
        })?;
        Ok(())
    }
}

fn skipped(reason: String) -> CleanupOutcome {
    CleanupOutcome::Skipped { reason }
}
