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
//!
//! A cleaned-up round leaves no empty directory: [`remove_empty_dirs`] takes
//! away the round's `<root>/<experiment>/_promote/` and
//! `<root>/<experiment>/` when nothing is left in them. The coordinator's
//! cleanup, the operator's `cleanup`, `promote` and `resume` all end here.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::clock;
use crate::competition::model::RoundState;
use crate::competition::promotion::{same_dir, FaultFired};
use crate::ids::RoundId;
use crate::measure::event::{
    Actor, EventKind, FinalOutcome, RoundCleanupStarted, RoundCompleted, WorktreeCleanupFailed,
};
use crate::measure::paths::DatasetPaths;
use crate::measure::projection::{CandidateView, RoundView};
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
        match self.final_outcome(round, from, resuming, options)? {
            Err(skip) => Ok(skip),
            Ok(final_outcome) => {
                let pass = Pass {
                    round,
                    view,
                    from,
                    options,
                };
                self.clean_round(&pass, resuming, final_outcome)
            }
        }
    }

    /// What the round ends as when cleanup starts from `from`, or the
    /// `Skipped` outcome when cleanup must wait.
    fn final_outcome(
        &self,
        round: &RoundId,
        from: RoundState,
        resuming: bool,
        options: CleanupOptions,
    ) -> Result<std::result::Result<FinalOutcome, CleanupOutcome>> {
        match from {
            RoundState::Decided => Ok(Ok(FinalOutcome::Winner)),
            RoundState::Rejected => Ok(Ok(FinalOutcome::Rejected)),
            RoundState::Promoted => self.promoted_outcome(round),
            RoundState::NeedsIntervention => {
                Ok(intervention_outcome(round, options.force || resuming))
            }
            other => Ok(Err(skipped(format!(
                "round {round} is {other}; cleanup waits for its end"
            )))),
        }
    }

    /// PRO-05: the receipt is durable before any worktree goes.
    fn promoted_outcome(
        &self,
        round: &RoundId,
    ) -> Result<std::result::Result<FinalOutcome, CleanupOutcome>> {
        Ok(if self.paths.promotion(round)?.exists() {
            Ok(FinalOutcome::Promoted)
        } else {
            Err(skipped(format!(
                "round {round} is PROMOTED but has no receipt yet"
            )))
        })
    }

    /// Remove each candidate's worktree, then record the round complete.
    fn clean_round(
        &self,
        pass: &Pass,
        resuming: bool,
        final_outcome: FinalOutcome,
    ) -> Result<CleanupOutcome> {
        if !resuming {
            self.emit(
                pass,
                pass.key("round.cleanup_started"),
                EventKind::RoundCleanupStarted(RoundCleanupStarted {}),
            )?;
        }
        let (removed, failed) = self.clean_worktrees(pass)?;
        remove_empty_dirs(pass.view);
        self.emit(
            pass,
            pass.key("round.completed"),
            EventKind::RoundCompleted(RoundCompleted { final_outcome }),
        )?;
        Ok(CleanupOutcome::Done { removed, failed })
    }

    /// Remove every candidate worktree: the labels removed, and the labels
    /// whose removal failed.
    fn clean_worktrees(&self, pass: &Pass) -> Result<(Vec<String>, Vec<String>)> {
        let mut removed = Vec::new();
        let mut failed = Vec::new();
        let listed = self.git.worktree_list(&self.repo)?;
        for (label, candidate) in &pass.view.candidates {
            match self.clean_candidate(pass, label, candidate, &listed)? {
                Removal::Skipped => {}
                Removal::Failed => failed.push(label.clone()),
                Removal::Removed => {
                    removed.push(label.clone());
                    self.fault_after(removed.len())?;
                }
            }
        }
        Ok((removed, failed))
    }

    /// Remove the worktree of one candidate, when git still lists it.
    fn clean_candidate(
        &self,
        pass: &Pass,
        label: &str,
        candidate: &CandidateView,
        listed: &[(PathBuf, Option<String>)],
    ) -> Result<Removal> {
        let Some(worktree) = candidate
            .worktree
            .as_ref()
            .filter(|w| listed.iter().any(|(p, _)| same_dir(p, &w.path)))
        else {
            return Ok(Removal::Skipped);
        };
        match self.git.worktree_remove(&self.repo, &worktree.path, true) {
            Err(e) => {
                self.emit(
                    pass,
                    format!("{}:{label}", pass.key("worktree.cleanup_failed")),
                    EventKind::WorktreeCleanupFailed(WorktreeCleanupFailed {
                        label: label.to_string(),
                        path: worktree.path.clone(),
                        error: format!("{e:#}"),
                    }),
                )?;
                Ok(Removal::Failed)
            }
            Ok(()) => {
                if pass.options.prune_branches {
                    self.prune(&worktree.branch)?;
                }
                Ok(Removal::Removed)
            }
        }
    }

    /// The fault `abort-during-cleanup:<n>` stops after removal `n`.
    fn fault_after(&self, removals: usize) -> Result<()> {
        let point = format!("{ABORT_DURING_CLEANUP}:{removals}");
        if self.faults.has(&point) {
            return Err(FaultFired(point).into());
        }
        Ok(())
    }

    fn prune(&self, branch: &str) -> Result<()> {
        let refname = format!("refs/heads/{branch}");
        if let Some(sha) = self.git.rev_parse(&self.repo, &refname)? {
            let zero = "0".repeat(sha.len());
            self.git.update_ref_cas(&self.repo, &refname, &zero, &sha)?;
        }
        Ok(())
    }

    fn emit(&self, pass: &Pass, key: String, kind: EventKind) -> Result<()> {
        self.recorder.append(NewEvent {
            kind,
            actor: Actor::Coordinator,
            experiment_id: pass.view.experiment_id.clone(),
            round_id: Some(pass.round.clone()),
            execution_id: None,
            idempotency_key: key,
            occurred_at: clock::now(),
        })?;
        Ok(())
    }
}

/// One cleanup of one round: what the idempotency keys and each removal
/// need to know.
struct Pass<'a> {
    round: &'a RoundId,
    view: &'a RoundView,
    /// The state cleanup started from.
    from: RoundState,
    options: CleanupOptions,
}

impl Pass<'_> {
    fn key(&self, what: &str) -> String {
        format!("{what}:{}:{}", self.round, self.from)
    }
}

/// What became of one candidate's worktree.
enum Removal {
    /// It has none, or git no longer lists it.
    Skipped,
    Removed,
    Failed,
}

/// A NEEDS_INTERVENTION round is cleaned only when the operator forces it,
/// or when a forced cleanup was already under way.
fn intervention_outcome(
    round: &RoundId,
    allowed: bool,
) -> std::result::Result<FinalOutcome, CleanupOutcome> {
    if allowed {
        Ok(FinalOutcome::NeedsIntervention)
    } else {
        Err(skipped(format!(
            "round {round} needs intervention; only the operator's cleanup removes its worktrees"
        )))
    }
}

fn skipped(reason: String) -> CleanupOutcome {
    CleanupOutcome::Skipped { reason }
}

/// Remove the directories a round's worktrees lived in, when they are
/// empty: `<root>/<experiment>/_promote/`, then `<root>/<experiment>/`.
/// Never recursive: a directory with anything left in it stays. A directory
/// whose name is not the experiment id is not one the run created, and
/// stays.
pub fn remove_empty_dirs(view: &RoundView) {
    let worktrees = view
        .candidates
        .values()
        .filter_map(|c| c.worktree.as_ref().map(|w| w.path.as_path()));
    remove_empty_parents(worktrees, view.experiment_id.as_str());
}

/// [`remove_empty_dirs`] on the worktree paths of experiment `experiment`.
fn remove_empty_parents<'a>(worktrees: impl Iterator<Item = &'a Path>, experiment: &str) {
    let mut dirs: Vec<PathBuf> = worktrees
        .filter_map(Path::parent)
        .filter(|d| d.file_name().is_some_and(|n| n == experiment))
        .map(Path::to_path_buf)
        .collect();
    dirs.sort();
    dirs.dedup();
    for dir in dirs {
        let _ = std::fs::remove_dir(dir.join("_promote"));
        let _ = std::fs::remove_dir(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F7: an empty `_promote/` and an empty experiment dir go; a dir with
    /// a file left in it, or not named for the experiment, stays.
    #[test]
    fn cleanup_removes_only_empty_round_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Experiment e1: every worktree is gone, `_promote/` is empty.
        let e1 = root.join("e1");
        std::fs::create_dir_all(e1.join("_promote")).unwrap();
        // Experiment e2: one worktree could not be removed.
        let e2 = root.join("e2");
        std::fs::create_dir_all(e2.join("B")).unwrap();
        std::fs::write(e2.join("B/kept.txt"), "x").unwrap();
        // A worktree root not named for its experiment.
        let other = root.join("other");
        std::fs::create_dir_all(&other).unwrap();

        remove_empty_parents(
            [e1.join("A"), e1.join("B")].iter().map(PathBuf::as_path),
            "e1",
        );
        remove_empty_parents(
            [e2.join("A"), e2.join("B")].iter().map(PathBuf::as_path),
            "e2",
        );
        remove_empty_parents([other.join("A")].iter().map(PathBuf::as_path), "e3");

        assert!(!e1.exists(), "empty experiment dir stays");
        assert!(e2.join("B/kept.txt").is_file(), "non-empty dir removed");
        assert!(other.is_dir(), "a dir not named for the experiment went");
        assert!(root.is_dir());
    }
}
