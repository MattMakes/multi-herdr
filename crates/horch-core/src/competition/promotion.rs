//! Promotion of a frozen winner into a target branch (phase B5, PRO-01 to
//! PRO-06, dataset design §4.8 and §5.2).
//!
//! [`GitPromotionEngine::promote`] runs the opt-in path (OD5):
//!
//! 1. The candidate's worktree HEAD and branch must still be the frozen
//!    commit; otherwise the judgment is stale (PRO-01).
//! 2. The target's commit is `dest_before`. A target branch that does not
//!    exist has the all-zero `dest_before` (fleet-dataset §7.3).
//! 3. A target that does not exist, or one at or behind the candidate's
//!    base, is fast-forwarded: an absent target is created. A target
//!    that moved gets `base..head` cherry-picked onto it in a temp worktree
//!    under `plan.integration_root`. A conflict needs the operator, and
//!    every candidate worktree is kept (PRO-04).
//! 4. The integrated commit is validated again (PRO-02).
//! 5. Publish: a compare-and-swap `update-ref`; with an all-zero old value
//!    it creates the target only while it does not exist. When a clean worktree has
//!    the target checked out, its index and files then follow by
//!    `read-tree -m -u` (the receipt calls that mode
//!    `update_ref_cas_read_tree`). A dirty checkout is never touched
//!    (PRO-03).
//! 6. `promotion.started` is recorded before the publish, the receipt is
//!    written with `create_immutable` after it, then `promotion.completed`
//!    (PRO-05).
//!
//! A crash between those records is repaired by
//! [`GitPromotionEngine::resume_promotion`], which never moves the target
//! twice (PRO-06). [`GitPromotionEngine::rollback`] moves the target back
//! by compare-and-swap.
//!
//! A fault point returns an error at its place, as the marketplace
//! installer's fault does: nothing after the point runs, the same as a
//! crash. The binary decides whether to exit.

use std::fmt;
use std::path::PathBuf;

use anyhow::{bail, ensure, Context, Result};
use chrono::SecondsFormat;
use serde::{Deserialize, Serialize};

use crate::clock;
use crate::evaluation::validator::{ValidationReport, Validator};
use crate::evaluation::winner::RejectReason;
use crate::fsx;
use crate::ids::{ExperimentId, JudgmentId, RoundId};
use crate::measure::digest::sha256_bytes;
use crate::measure::event::{
    Actor, EventKind, InterventionSource, PromotionCompleted, PromotionConflicted,
    PromotionRolledBack, PromotionStarted, RoundNeedsIntervention, WinnerRejected,
};
use crate::measure::paths::{component, DatasetPaths};
use crate::measure::recorder::{NewEvent, Recorder};
use crate::runtime::fault::Faults;
use crate::vcs::git::{CheckoutLocation, CherryPick, GitClient, GitIdentity};
use crate::vcs::worktree::FrozenCandidate;

pub(crate) const RECEIPT_SCHEMA_VERSION: &str = "1.0.0";

/// Fault: stop after `promotion.started`, before the publish.
pub const ABORT_AFTER_PROMOTION_STARTED: &str = "abort-after-promotion-started";
/// Fault: stop after the publish, before the receipt.
pub const ABORT_AFTER_UPDATE_REF: &str = "abort-after-update-ref";
/// Fault: stop after the receipt, before `promotion.completed`.
pub const ABORT_AFTER_RECEIPT: &str = "abort-after-receipt";

/// A branch of a repository. `name` is short (`main`) or a full ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchRef {
    pub repo: PathBuf,
    pub name: String,
}

impl BranchRef {
    /// `refs/heads/<name>`.
    pub fn refname(&self) -> String {
        if self.name.starts_with("refs/") {
            self.name.clone()
        } else {
            format!("refs/heads/{}", self.name)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionStrategy {
    FastForward,
    CherryPick,
}

/// The receipt's `publish` value of a checked-out publish.
const UPDATE_REF_CAS_READ_TREE: &str = "update_ref_cas_read_tree";
/// The same publish as recorded before D17, when the checked-out mode was
/// `merge --ff-only`. Old events and receipts keep it; resume accepts it.
const MERGE_FF_ONLY: &str = "merge_ff_only";

/// How the target branch moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PublishMode {
    /// No worktree has the target checked out.
    UpdateRefCas,
    /// The target is checked out, clean, in `checkout`: a ref swap, then
    /// `read-tree -m -u` in the checkout.
    CasReadTree { checkout: PathBuf },
}

impl PublishMode {
    /// The receipt's `publish` value.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            PublishMode::UpdateRefCas => "update_ref_cas",
            PublishMode::CasReadTree { .. } => UPDATE_REF_CAS_READ_TREE,
        }
    }
}

/// One promotion attempt of one round.
///
/// Deviation from design §4.8: the revalidation gates live in the
/// [`Validator`] the engine holds, so the plan has no `gates`. The plan adds
/// `experiment` (every event names it) and `attempt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionPlan {
    pub experiment: ExperimentId,
    pub round: RoundId,
    pub judgment_id: JudgmentId,
    /// Commits the cherry-pick makes use this identity and date.
    pub identity: GitIdentity,
    /// The parent of the temp integration worktree.
    pub integration_root: PathBuf,
    /// 1 for the first promotion of the round; 1 more for each
    /// `operator.promote` since. It keys this attempt's events, so a retry
    /// after NEEDS_INTERVENTION records again, and a restart of the same
    /// attempt does not.
    pub attempt: u32,
}

/// `promotions/<round>.json`. Written once, before any cleanup (PRO-05).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionReceipt {
    pub schema_version: String,
    pub round_id: RoundId,
    pub label: String,
    /// `base..head` of the candidate, oldest first.
    pub source_shas: Vec<String>,
    pub dest_ref: String,
    pub dest_before: String,
    pub dest_after: String,
    pub strategy: PromotionStrategy,
    /// `update_ref_cas` or `update_ref_cas_read_tree`; `merge_ff_only` in
    /// a receipt written before D17 for the second mode.
    pub publish: String,
    pub validation_ids: Vec<String>,
    pub judgment_id: JudgmentId,
    pub promoted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
// One value per promotion attempt: boxing the receipt saves nothing that
// matters and adds a `Box` to every caller.
#[allow(clippy::large_enum_variant)]
pub enum PromotionResult {
    Promoted(PromotionReceipt),
    /// The target is untouched (or, after a lost race, someone else's), and
    /// every candidate worktree is kept.
    NeedsIntervention {
        reason: String,
    },
    Rejected {
        reason: RejectReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollbackResult {
    /// The target holds `restored` (the receipt's `dest_before`) again.
    RolledBack { restored: String },
    /// Nothing changed.
    NeedsIntervention { reason: String },
}

/// A fault point fired. Nothing after it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultFired(pub String);

impl fmt::Display for FaultFired {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HORCH_FAULT={}: stopped here", self.0)
    }
}

impl std::error::Error for FaultFired {}

pub trait PromotionEngine {
    fn promote(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
    ) -> Result<PromotionResult>;
}

pub struct GitPromotionEngine<'a, G: GitClient, V: Validator, R: Recorder> {
    pub git: &'a G,
    pub validator: &'a V,
    pub recorder: &'a R,
    pub paths: &'a DatasetPaths,
    pub faults: &'a Faults,
}

/// What the integration step planned: how the target moves, from where to
/// where.
struct Landing {
    strategy: PromotionStrategy,
    dest_before: String,
    planned_after: String,
}

/// What the integration step did.
enum Integrated {
    Ready(Landing),
    /// The cherry-pick conflicted: the result to give, already recorded.
    Conflict(Box<PromotionResult>),
}

/// What the publish step did.
enum Published {
    Moved,
    /// The target is not at `dest_before` any more, or its checkout changed.
    Lost(String),
}

impl<G: GitClient, V: Validator, R: Recorder> PromotionEngine for GitPromotionEngine<'_, G, V, R> {
    fn promote(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
    ) -> Result<PromotionResult> {
        self.ensure_unpromoted(plan)?;
        // 1. The judged commit is still the candidate's commit (PRO-01).
        if self.is_fresh(candidate, target)? {
            self.promote_fresh(candidate, target, plan)
        } else {
            self.reject(candidate, plan, RejectReason::StaleJudgment)
        }
    }
}

impl<G: GitClient, V: Validator, R: Recorder> GitPromotionEngine<'_, G, V, R> {
    fn ensure_unpromoted(&self, plan: &PromotionPlan) -> Result<()> {
        let receipt_path = self.paths.promotion(&plan.round)?;
        ensure!(
            !receipt_path.exists(),
            "round {} is already promoted: {} exists",
            plan.round,
            receipt_path.display()
        );
        Ok(())
    }

    /// 2. The target's commit; a target that does not exist is created
    ///    (fleet-dataset §7.3). Then integrate, revalidate and publish.
    fn promote_fresh(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
    ) -> Result<PromotionResult> {
        let dest_before = self
            .git
            .rev_parse(&target.repo, &target.refname())?
            .unwrap_or_else(|| absent(&candidate.head_sha));
        let temp = TempIntegration::new(target, plan)?;
        match self.integrate(candidate, target, plan, dest_before, &temp)? {
            Integrated::Ready(landing) => {
                self.promote_integrated(candidate, target, plan, &temp, landing)
            }
            Integrated::Conflict(result) => Ok(*result),
        }
    }

    /// 3. Integrate in a temp worktree on a temp branch.
    fn integrate(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
        dest_before: String,
        temp: &TempIntegration,
    ) -> Result<Integrated> {
        let repo = &target.repo;
        let fast_forward = is_absent(&dest_before)
            || self
                .git
                .is_ancestor(repo, &dest_before, &candidate.base_sha)?;
        let start = if fast_forward {
            candidate.head_sha.as_str()
        } else {
            dest_before.as_str()
        };
        temp.discard(self.git)?;
        self.git
            .worktree_add(repo, &temp.path, &temp.branch, start)
            .context("creating the promotion integration worktree")?;
        if fast_forward {
            Ok(Integrated::Ready(Landing {
                strategy: PromotionStrategy::FastForward,
                planned_after: candidate.head_sha.clone(),
                dest_before,
            }))
        } else {
            self.cherry_pick_onto(candidate, target, plan, dest_before, temp)
        }
    }

    /// `base..head` cherry-picked onto the moved target. A conflict needs
    /// the operator, and every candidate worktree is kept (PRO-04).
    fn cherry_pick_onto(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
        dest_before: String,
        temp: &TempIntegration,
    ) -> Result<Integrated> {
        let range = format!("{}..{}", candidate.base_sha, candidate.head_sha);
        match self.git.cherry_pick(&temp.path, &range, &plan.identity)? {
            CherryPick::Clean { head } => Ok(Integrated::Ready(Landing {
                strategy: PromotionStrategy::CherryPick,
                planned_after: head,
                dest_before,
            })),
            CherryPick::Conflict { paths } => {
                temp.discard(self.git)?;
                self.emit(
                    candidate,
                    plan,
                    format!("promotion.conflicted:{}:{}", plan.round, plan.attempt),
                    EventKind::PromotionConflicted(PromotionConflicted {
                        paths: paths.clone(),
                    }),
                )?;
                Ok(Integrated::Conflict(Box::new(
                    PromotionResult::NeedsIntervention {
                        reason: format!(
                            "cherry-pick onto {} conflicts in {}",
                            target.refname(),
                            paths.join(", ")
                        ),
                    },
                )))
            }
        }
    }

    /// 4. Revalidate the integrated commit, in the temp worktree (PRO-02).
    fn promote_integrated(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
        temp: &TempIntegration,
        landing: Landing,
    ) -> Result<PromotionResult> {
        if let Some(report) = self.revalidate(candidate, temp, &landing.planned_after)? {
            self.promote_validated(candidate, target, plan, temp, landing, report)
        } else {
            self.abandon(temp, || {
                self.reject(candidate, plan, RejectReason::RevalidationFailed)
            })
        }
    }

    /// The integrated commit's report, or `None` when it is not eligible.
    fn revalidate(
        &self,
        candidate: &FrozenCandidate,
        temp: &TempIntegration,
        planned_after: &str,
    ) -> Result<Option<ValidationReport>> {
        let mut integrated = candidate.clone();
        integrated.worktree = temp.path.clone();
        integrated.branch = temp.branch.clone();
        integrated.head_sha = planned_after.to_string();
        let report = self.validator.validate(&integrated);
        let worktree_removed = self
            .git
            .worktree_remove(&temp.repo, &temp.path, true)
            .context("removing the promotion integration worktree");
        let report = report?;
        worktree_removed?;
        Ok(report.eligible.then_some(report))
    }

    /// Discard the temp worktree, then give `result`: every exit that stops
    /// before the publish goes through here.
    fn abandon(
        &self,
        temp: &TempIntegration,
        result: impl FnOnce() -> Result<PromotionResult>,
    ) -> Result<PromotionResult> {
        temp.discard(self.git)?;
        result()
    }

    /// 5. How the target moves. A dirty checkout is never touched.
    fn promote_validated(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
        temp: &TempIntegration,
        landing: Landing,
        report: ValidationReport,
    ) -> Result<PromotionResult> {
        match self.publish_mode(target)? {
            Err(reason) => self.abandon(temp, || self.needs_intervention(candidate, plan, reason)),
            Ok(mode) => {
                let started = PromotionStarted {
                    target: target.refname(),
                    dest_before: landing.dest_before,
                    planned_after: landing.planned_after,
                    strategy: landing.strategy,
                    publish: mode.as_str().to_string(),
                    validation_ids: vec![report.validation_id],
                };
                self.start_and_publish(candidate, target, plan, &started, &mode, temp)
            }
        }
    }

    /// 6. started → publish → receipt → completed.
    fn start_and_publish(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
        started: &PromotionStarted,
        mode: &PublishMode,
        temp: &TempIntegration,
    ) -> Result<PromotionResult> {
        self.emit(
            candidate,
            plan,
            format!("promotion.started:{}:{}", plan.round, plan.attempt),
            EventKind::PromotionStarted(started.clone()),
        )?;
        self.fault(ABORT_AFTER_PROMOTION_STARTED)?;
        self.publish_and_finish(candidate, target, plan, started, mode, temp)
    }

    /// Finish an attempt that recorded `promotion.started` and then stopped.
    ///
    /// The restart rule (design §6 B5): the target at `planned_after` means
    /// the publish happened, so only the receipt and `promotion.completed`
    /// are missing; the target at `dest_before` means it did not, so the
    /// publish runs again; anything else needs the operator. The target
    /// moves at most once (PRO-06).
    pub fn resume_promotion(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
        started: &PromotionStarted,
    ) -> Result<PromotionResult> {
        let temp = TempIntegration::new(target, plan)?;
        let current = self.git.rev_parse(&target.repo, &started.target)?;
        // A created target that is still absent holds the all-zero value.
        let held = current
            .clone()
            .unwrap_or_else(|| absent(&started.planned_after));
        if held == started.planned_after {
            self.resume_published(candidate, target, plan, started, &temp)
        } else if held == started.dest_before {
            self.resume_unpublished(candidate, target, plan, started, &temp)
        } else {
            let at = current.as_deref().unwrap_or("nothing");
            self.abandon(&temp, || {
                self.needs_intervention(
                    candidate,
                    plan,
                    format!(
                        "{} is at {at}, neither dest_before {} nor planned_after {}",
                        started.target, started.dest_before, started.planned_after
                    ),
                )
            })
        }
    }

    /// The target holds `planned_after`: only the checkout's files, the
    /// receipt and `promotion.completed` can be missing.
    fn resume_published(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
        started: &PromotionStarted,
        temp: &TempIntegration,
    ) -> Result<PromotionResult> {
        if let Some(reason) = self.settle_checkout(target, started)? {
            self.abandon(temp, || self.needs_intervention(candidate, plan, reason))
        } else {
            self.finish(candidate, target, plan, started, &started.publish, temp)
        }
    }

    /// The target holds `dest_before`: the publish did not happen, so it
    /// runs again.
    fn resume_unpublished(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
        started: &PromotionStarted,
        temp: &TempIntegration,
    ) -> Result<PromotionResult> {
        match self.publish_mode(target)? {
            Err(reason) => self.abandon(temp, || self.needs_intervention(candidate, plan, reason)),
            Ok(mode) => self.publish_and_finish(candidate, target, plan, started, &mode, temp),
        }
    }

    /// A checked-out publish that stopped between its ref swap and its tree
    /// update leaves the checkout's index and files at `dest_before`. Run
    /// the tree update again: when it already ran, the index matches
    /// `planned_after` and git keeps it as it is. `Some(reason)` when git
    /// refuses.
    fn settle_checkout(
        &self,
        target: &BranchRef,
        started: &PromotionStarted,
    ) -> Result<Option<String>> {
        if ![UPDATE_REF_CAS_READ_TREE, MERGE_FF_ONLY].contains(&started.publish.as_str()) {
            return Ok(None);
        }
        let CheckoutLocation::CheckedOut { path, .. } = self
            .git
            .branch_checkout_location(&target.repo, &started.target)?
        else {
            return Ok(None);
        };
        Ok(self
            .git
            .read_tree_update(&path, &started.dest_before, &started.planned_after)
            .err()
            .map(|e| {
                format!(
                    "{} is at {} but the files of {} could not follow ({e:#})",
                    started.target,
                    started.planned_after,
                    path.display()
                )
            }))
    }

    /// Move the target back from the receipt's `dest_after` to its
    /// `dest_before` by compare-and-swap. A target that moved since, or that
    /// a worktree has checked out, is left alone.
    pub fn rollback(
        &self,
        experiment: &ExperimentId,
        round: &RoundId,
        target: &BranchRef,
    ) -> Result<RollbackResult> {
        let receipt = self.load_receipt(round, target)?;
        let current = self.git.rev_parse(&target.repo, &receipt.dest_ref)?;
        // A created target is rolled back by deleting it: its `dest_before`
        // is all zeros, as an absent ref reads here.
        let held = current
            .clone()
            .unwrap_or_else(|| absent(&receipt.dest_after));
        let restored = RollbackResult::RolledBack {
            restored: receipt.dest_before.clone(),
        };
        // A restart after the swap records it and moves nothing.
        let blocker = if held == receipt.dest_before {
            None
        } else {
            self.rollback_blocker(target, &receipt, &held, current.as_deref())?
        };
        if let Some(reason) = blocker {
            Ok(RollbackResult::NeedsIntervention { reason })
        } else {
            self.record_rollback(experiment, round, &receipt)?;
            Ok(restored)
        }
    }

    /// The receipt of `round`, which must be for `target`.
    fn load_receipt(&self, round: &RoundId, target: &BranchRef) -> Result<PromotionReceipt> {
        let path = self.paths.promotion(round)?;
        let bytes = std::fs::read(&path)
            .with_context(|| format!("round {round} has no receipt at {}", path.display()))?;
        let receipt: PromotionReceipt = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing {}", path.display()))?;
        if receipt.dest_ref != target.refname() {
            bail!(
                "the receipt of round {round} is for {}, not {}",
                receipt.dest_ref,
                target.refname()
            );
        }
        Ok(receipt)
    }

    /// Why the target must not move back, or `None` after it moved back by
    /// compare-and-swap. The 3 refusals: the branch moved since the
    /// promotion, a worktree has it checked out, the swap lost a race.
    fn rollback_blocker(
        &self,
        target: &BranchRef,
        receipt: &PromotionReceipt,
        held: &str,
        current: Option<&str>,
    ) -> Result<Option<String>> {
        let refname = &receipt.dest_ref;
        if held != receipt.dest_after {
            return Ok(Some(format!(
                "{refname} moved to {} after the promotion to {}",
                current.unwrap_or("nothing"),
                receipt.dest_after
            )));
        }
        if let CheckoutLocation::CheckedOut { path, .. } =
            self.git.branch_checkout_location(&target.repo, refname)?
        {
            return Ok(Some(format!(
                "{refname} is checked out in {}; a rollback would leave its files behind the branch",
                path.display()
            )));
        }
        let swapped = self.git.update_ref_cas(
            &target.repo,
            refname,
            &receipt.dest_before,
            &receipt.dest_after,
        )?;
        Ok((!swapped).then(|| format!("{refname} moved during the rollback")))
    }

    fn record_rollback(
        &self,
        experiment: &ExperimentId,
        round: &RoundId,
        receipt: &PromotionReceipt,
    ) -> Result<()> {
        self.recorder
            .append(NewEvent {
                kind: EventKind::PromotionRolledBack(PromotionRolledBack {
                    target: receipt.dest_ref.clone(),
                    restored: receipt.dest_before.clone(),
                }),
                actor: Actor::Operator,
                experiment_id: experiment.clone(),
                round_id: Some(round.clone()),
                execution_id: None,
                idempotency_key: format!("promotion.rolled_back:{round}"),
                occurred_at: clock::now(),
            })
            .map(drop)
    }

    /// The worktree (when it is still there) and the branch both hold the
    /// frozen commit. A round cleaned up before `promote <round>` has no
    /// worktree; its branch alone decides.
    fn is_fresh(&self, c: &FrozenCandidate, target: &BranchRef) -> Result<bool> {
        let branch = format!("refs/heads/{}", c.branch);
        let on_branch = self.git.rev_parse(&target.repo, &branch)?;
        if on_branch.as_deref() != Some(c.head_sha.as_str()) {
            return Ok(false);
        }
        if c.worktree.exists() {
            // A worktree that git cannot read is not the judged commit.
            return Ok(self.git.head(&c.worktree).ok().as_deref() == Some(c.head_sha.as_str()));
        }
        Ok(true)
    }

    /// The publish mode, or why the target must not be touched.
    fn publish_mode(&self, target: &BranchRef) -> Result<std::result::Result<PublishMode, String>> {
        Ok(
            match self
                .git
                .branch_checkout_location(&target.repo, &target.refname())?
            {
                CheckoutLocation::NotCheckedOut => Ok(PublishMode::UpdateRefCas),
                CheckoutLocation::CheckedOut { path, clean: true } => {
                    Ok(PublishMode::CasReadTree { checkout: path })
                }
                CheckoutLocation::CheckedOut { path, clean: false } => Err(format!(
                    "{} is checked out with local changes in {}",
                    target.refname(),
                    path.display()
                )),
            },
        )
    }

    fn publish(
        &self,
        target: &BranchRef,
        started: &PromotionStarted,
        mode: &PublishMode,
    ) -> Result<Published> {
        let repo = &target.repo;
        match mode {
            PublishMode::UpdateRefCas => {
                if self.git.update_ref_cas(
                    repo,
                    &started.target,
                    &started.planned_after,
                    &started.dest_before,
                )? {
                    Ok(Published::Moved)
                } else {
                    Ok(Published::Lost(format!(
                        "{} moved from {} before the update",
                        started.target, started.dest_before
                    )))
                }
            }
            PublishMode::CasReadTree { checkout } => self.publish_checked_out(checkout, started),
        }
    }

    /// Publish into a clean checkout of the target by compare-and-swap.
    ///
    /// 1. The checkout is on the target, at `dest_before`, and clean.
    /// 2. `update-ref <target> <planned_after> <dest_before>` moves the ref.
    ///    This is the one atomic step: a commit that landed after step 1
    ///    makes it fail, and a `git commit` that read HEAD before it fails
    ///    its own swap on HEAD.
    /// 3. `read-tree -m -u <dest_before> <planned_after>` moves the index
    ///    and files. Git refuses before it writes when a local change is in
    ///    a path the promotion changes; the ref then goes back by
    ///    compare-and-swap.
    /// 4. The checkout is on the target at `planned_after`. This catches a
    ///    commit made in the short window between steps 2 and 3.
    ///
    /// Any mismatch is `Lost`, so the round needs the operator; before
    /// step 4 nothing stays moved.
    fn publish_checked_out(
        &self,
        checkout: &std::path::Path,
        started: &PromotionStarted,
    ) -> Result<Published> {
        let steps: [&dyn Fn() -> Result<Option<String>>; 4] = [
            &|| self.checkout_unchanged(checkout, started),
            &|| self.swap_ref(checkout, started),
            &|| self.move_files(checkout, started),
            &|| self.checkout_at_planned(checkout, started),
        ];
        let lost = steps
            .iter()
            .find_map(|step| step().transpose())
            .transpose()?;
        Ok(lost.map_or(Published::Moved, Published::Lost))
    }

    /// The checkout is on `target` at `at`.
    fn on_target(&self, checkout: &std::path::Path, target: &str, at: &str) -> Result<bool> {
        let short = target.strip_prefix("refs/heads/").unwrap_or(target);
        Ok(self.git.current_branch(checkout)?.as_deref() == Some(short)
            && self.git.head(checkout)? == at)
    }

    /// Step 1: `Some(reason)` when the checkout is not on the target at
    /// `dest_before`, or not clean.
    fn checkout_unchanged(
        &self,
        checkout: &std::path::Path,
        started: &PromotionStarted,
    ) -> Result<Option<String>> {
        let target = &started.target;
        let changed = !self.on_target(checkout, target, &started.dest_before)?
            || !self.git.status_porcelain(checkout)?.is_empty();
        Ok(changed.then(|| {
            format!(
                "the checkout {} of {target} changed before the publish",
                checkout.display()
            )
        }))
    }

    /// Step 2: the ref swap. `Some(reason)` when it lost a race.
    fn swap_ref(
        &self,
        checkout: &std::path::Path,
        started: &PromotionStarted,
    ) -> Result<Option<String>> {
        let swapped = self.git.update_ref_cas(
            checkout,
            &started.target,
            &started.planned_after,
            &started.dest_before,
        )?;
        Ok((!swapped).then(|| {
            format!(
                "{} moved from {} before the publish into {}",
                started.target,
                started.dest_before,
                checkout.display()
            )
        }))
    }

    /// Step 3: the tree update. `Some(reason)` when git refuses it.
    fn move_files(
        &self,
        checkout: &std::path::Path,
        started: &PromotionStarted,
    ) -> Result<Option<String>> {
        match self
            .git
            .read_tree_update(checkout, &started.dest_before, &started.planned_after)
        {
            Ok(()) => Ok(None),
            Err(e) => self.undo_publish(checkout, started, &e).map(Some),
        }
    }

    /// The tree update failed: put the ref back by compare-and-swap, and say
    /// whether that worked.
    fn undo_publish(
        &self,
        checkout: &std::path::Path,
        started: &PromotionStarted,
        e: &anyhow::Error,
    ) -> Result<String> {
        let target = &started.target;
        let undone = self.git.update_ref_cas(
            checkout,
            target,
            &started.dest_before,
            &started.planned_after,
        )?;
        let state = if undone {
            format!("{target} is back at {}", started.dest_before)
        } else {
            format!("{target} moved again and is not restored")
        };
        Ok(format!(
            "the files of {} could not move to {} ({e:#}); {state}",
            checkout.display(),
            started.planned_after
        ))
    }

    /// Step 4: the checkout is on the target at `planned_after`. This
    /// catches a commit made between steps 2 and 3.
    fn checkout_at_planned(
        &self,
        checkout: &std::path::Path,
        started: &PromotionStarted,
    ) -> Result<Option<String>> {
        let target = &started.target;
        let moved = !self.on_target(checkout, target, &started.planned_after)?;
        Ok(moved.then(|| {
            format!(
                "the checkout {} of {target} is not at {} after the publish",
                checkout.display(),
                started.planned_after
            )
        }))
    }

    fn publish_and_finish(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
        started: &PromotionStarted,
        mode: &PublishMode,
        temp: &TempIntegration,
    ) -> Result<PromotionResult> {
        match self.publish(target, started, mode)? {
            Published::Moved => {
                self.fault(ABORT_AFTER_UPDATE_REF)?;
                self.finish(candidate, target, plan, started, mode.as_str(), temp)
            }
            Published::Lost(reason) => {
                self.abandon(temp, || self.needs_intervention(candidate, plan, reason))
            }
        }
    }

    /// Write the receipt (or accept the one a crashed run wrote), then record
    /// `promotion.completed`.
    fn finish(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
        started: &PromotionStarted,
        publish: &str,
        temp: &TempIntegration,
    ) -> Result<PromotionResult> {
        let (receipt, bytes) =
            self.read_or_write_receipt(candidate, target, plan, started, publish)?;
        self.fault(ABORT_AFTER_RECEIPT)?;
        self.emit(
            candidate,
            plan,
            format!("promotion.completed:{}", plan.round),
            EventKind::PromotionCompleted(PromotionCompleted {
                receipt_digest: sha256_bytes(&bytes),
                dest_after: receipt.dest_after.clone(),
            }),
        )?;
        // The target holds the commits now; the temp branch can go.
        temp.discard(self.git)?;
        Ok(PromotionResult::Promoted(receipt))
    }

    /// The receipt a crashed run wrote, or a new one.
    fn read_or_write_receipt(
        &self,
        candidate: &FrozenCandidate,
        target: &BranchRef,
        plan: &PromotionPlan,
        started: &PromotionStarted,
        publish: &str,
    ) -> Result<(PromotionReceipt, Vec<u8>)> {
        let path = self.paths.promotion(&plan.round)?;
        match std::fs::read(&path) {
            Ok(bytes) => {
                let receipt = check_receipt(&bytes, started, &path)?;
                Ok((receipt, bytes))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.write_receipt(&path, candidate, &target.repo, plan, started, publish)
            }
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    fn write_receipt(
        &self,
        path: &std::path::Path,
        candidate: &FrozenCandidate,
        repo: &std::path::Path,
        plan: &PromotionPlan,
        started: &PromotionStarted,
        publish: &str,
    ) -> Result<(PromotionReceipt, Vec<u8>)> {
        let mut source_shas = self.git.rev_list(
            repo,
            &format!("{}..{}", candidate.base_sha, candidate.head_sha),
        )?;
        source_shas.reverse();
        let receipt = PromotionReceipt {
            schema_version: RECEIPT_SCHEMA_VERSION.to_string(),
            round_id: plan.round.clone(),
            label: candidate.label.clone(),
            source_shas,
            dest_ref: started.target.clone(),
            dest_before: started.dest_before.clone(),
            dest_after: started.planned_after.clone(),
            strategy: started.strategy,
            publish: publish.to_string(),
            validation_ids: started.validation_ids.clone(),
            judgment_id: plan.judgment_id.clone(),
            promoted_at: clock::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        };
        let mut bytes = serde_json::to_vec_pretty(&receipt)?;
        bytes.push(b'\n');
        fsx::ensure_private_dir(&self.paths.promotions_dir())?;
        fsx::create_immutable(path, &bytes, fsx::PRIVATE_FILE)?;
        Ok((receipt, bytes))
    }

    fn reject(
        &self,
        candidate: &FrozenCandidate,
        plan: &PromotionPlan,
        reason: RejectReason,
    ) -> Result<PromotionResult> {
        self.emit(
            candidate,
            plan,
            format!("promotion.rejected:{}:{}", plan.round, plan.attempt),
            EventKind::WinnerRejected(WinnerRejected { reason }),
        )?;
        Ok(PromotionResult::Rejected { reason })
    }

    fn needs_intervention(
        &self,
        candidate: &FrozenCandidate,
        plan: &PromotionPlan,
        reason: String,
    ) -> Result<PromotionResult> {
        self.emit(
            candidate,
            plan,
            format!(
                "round.needs_intervention:promotion:{}:{}",
                plan.round, plan.attempt
            ),
            EventKind::RoundNeedsIntervention(RoundNeedsIntervention {
                reason: reason.clone(),
                source: InterventionSource::Promotion,
            }),
        )?;
        Ok(PromotionResult::NeedsIntervention { reason })
    }

    fn emit(
        &self,
        candidate: &FrozenCandidate,
        plan: &PromotionPlan,
        idempotency_key: String,
        kind: EventKind,
    ) -> Result<()> {
        self.recorder.append(NewEvent {
            kind,
            actor: Actor::Coordinator,
            experiment_id: plan.experiment.clone(),
            round_id: Some(plan.round.clone()),
            execution_id: Some(candidate.execution_id.clone()),
            idempotency_key,
            occurred_at: clock::now(),
        })?;
        Ok(())
    }

    fn fault(&self, point: &str) -> Result<()> {
        if self.faults.has(point) {
            return Err(FaultFired(point.to_string()).into());
        }
        Ok(())
    }
}

/// The receipt a crashed run wrote must record this promotion.
fn check_receipt(
    bytes: &[u8],
    started: &PromotionStarted,
    path: &std::path::Path,
) -> Result<PromotionReceipt> {
    let receipt: PromotionReceipt =
        serde_json::from_slice(bytes).with_context(|| format!("parsing {}", path.display()))?;
    if receipt.dest_after != started.planned_after || receipt.dest_before != started.dest_before {
        bail!(
            "{} records {}..{}, not this promotion",
            path.display(),
            receipt.dest_before,
            receipt.dest_after
        );
    }
    Ok(receipt)
}

/// The all-zero object id as long as `like`: what `update-ref` takes as the
/// value of a ref that does not exist.
fn absent(like: &str) -> String {
    "0".repeat(like.len())
}

fn is_absent(sha: &str) -> bool {
    !sha.is_empty() && sha.bytes().all(|b| b == b'0')
}

/// The temp integration worktree and its branch,
/// `mh/promote/<round>/a<attempt>`.
struct TempIntegration {
    repo: PathBuf,
    path: PathBuf,
    branch: String,
}

impl TempIntegration {
    fn new(target: &BranchRef, plan: &PromotionPlan) -> Result<TempIntegration> {
        let round = component("round id", plan.round.as_str())?;
        let plain = round
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'));
        if !plain || round.starts_with(['-', '.']) || round.ends_with(".lock") {
            bail!("round id {round:?} cannot name a branch");
        }
        Ok(TempIntegration {
            repo: target.repo.clone(),
            path: plan
                .integration_root
                .join(format!("{round}-a{}", plan.attempt)),
            branch: format!("mh/promote/{round}/a{}", plan.attempt),
        })
    }

    /// Remove the worktree and the branch, when they exist.
    fn discard<G: GitClient>(&self, git: &G) -> Result<()> {
        let listed = git
            .worktree_list(&self.repo)?
            .into_iter()
            .any(|(p, _)| same_dir(&p, &self.path));
        if listed {
            git.worktree_remove(&self.repo, &self.path, true)?;
        }
        let refname = format!("refs/heads/{}", self.branch);
        if let Some(sha) = git.rev_parse(&self.repo, &refname)? {
            // An all-zero new value deletes the ref.
            let zero = "0".repeat(sha.len());
            git.update_ref_cas(&self.repo, &refname, &zero, &sha)?;
        }
        Ok(())
    }
}

/// Git prints resolved paths (`/private/var/...` on macOS).
pub(crate) fn same_dir(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
