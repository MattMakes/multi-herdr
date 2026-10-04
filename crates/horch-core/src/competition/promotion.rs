//! Promotion of a frozen winner into a target branch (phase B5, PRO-01 to
//! PRO-06, dataset design §4.8 and §5.2).
//!
//! [`GitPromotionEngine::promote`] runs the opt-in path (OD5):
//!
//! 1. The candidate's worktree HEAD and branch must still be the frozen
//!    commit; otherwise the judgment is stale (PRO-01).
//! 2. The target's commit is `dest_before`.
//! 3. A target at or behind the candidate's base is fast-forwarded. A target
//!    that moved gets `base..head` cherry-picked onto it in a temp worktree
//!    under `plan.integration_root`. A conflict needs the operator, and
//!    every candidate worktree is kept (PRO-04).
//! 4. The integrated commit is validated again (PRO-02).
//! 5. Publish: a compare-and-swap `update-ref`. When a clean worktree has
//!    the target checked out, its index and files then follow by
//!    `read-tree -m -u` (the receipt calls that mode `merge_ff_only`). A
//!    dirty checkout is never touched (PRO-03).
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

use anyhow::{bail, Context, Result};
use chrono::SecondsFormat;
use serde::{Deserialize, Serialize};

use crate::clock;
use crate::evaluation::validator::Validator;
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
const MERGE_FF_ONLY: &str = "merge_ff_only";

/// How the target branch moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PublishMode {
    /// No worktree has the target checked out.
    UpdateRefCas,
    /// The target is checked out, clean, in `checkout`. The name is the
    /// receipt's; the publish is a ref swap and then a tree update.
    MergeFfOnly { checkout: PathBuf },
}

impl PublishMode {
    /// The receipt's `publish` value.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            PublishMode::UpdateRefCas => "update_ref_cas",
            PublishMode::MergeFfOnly { .. } => MERGE_FF_ONLY,
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
    /// `update_ref_cas` or `merge_ff_only`.
    pub publish: String,
    pub validation_ids: Vec<String>,
    pub judgment_id: JudgmentId,
    pub promoted_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
        let repo = &target.repo;
        let receipt_path = self.paths.promotion(&plan.round)?;
        if receipt_path.exists() {
            bail!(
                "round {} is already promoted: {} exists",
                plan.round,
                receipt_path.display()
            );
        }

        // 1. The judged commit is still the candidate's commit (PRO-01).
        if !self.is_fresh(candidate, target)? {
            return self.reject(candidate, plan, RejectReason::StaleJudgment);
        }

        // 2.
        let refname = target.refname();
        let dest_before = self
            .git
            .rev_parse(repo, &refname)?
            .with_context(|| format!("target {refname} does not exist"))?;

        // 3. Integrate in a temp worktree on a temp branch.
        let temp = TempIntegration::new(target, plan)?;
        let fast_forward = self
            .git
            .is_ancestor(repo, &dest_before, &candidate.base_sha)?;
        let (strategy, start) = if fast_forward {
            (PromotionStrategy::FastForward, candidate.head_sha.as_str())
        } else {
            (PromotionStrategy::CherryPick, dest_before.as_str())
        };
        temp.discard(self.git)?;
        self.git
            .worktree_add(repo, &temp.path, &temp.branch, start)
            .context("creating the promotion integration worktree")?;
        let planned_after = match strategy {
            PromotionStrategy::FastForward => candidate.head_sha.clone(),
            PromotionStrategy::CherryPick => {
                let range = format!("{}..{}", candidate.base_sha, candidate.head_sha);
                match self.git.cherry_pick(&temp.path, &range, &plan.identity)? {
                    CherryPick::Clean { head } => head,
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
                        return Ok(PromotionResult::NeedsIntervention {
                            reason: format!(
                                "cherry-pick onto {refname} conflicts in {}",
                                paths.join(", ")
                            ),
                        });
                    }
                }
            }
        };

        // 4. Revalidate the integrated commit, in the temp worktree (PRO-02).
        let mut integrated = candidate.clone();
        integrated.worktree = temp.path.clone();
        integrated.branch = temp.branch.clone();
        integrated.head_sha = planned_after.clone();
        let report = self.validator.validate(&integrated);
        let worktree_removed = self
            .git
            .worktree_remove(repo, &temp.path, true)
            .context("removing the promotion integration worktree");
        let report = report?;
        worktree_removed?;
        if !report.eligible {
            temp.discard(self.git)?;
            return self.reject(candidate, plan, RejectReason::RevalidationFailed);
        }

        // 5. How the target moves. A dirty checkout is never touched.
        let mode = match self.publish_mode(target)? {
            Ok(mode) => mode,
            Err(reason) => {
                temp.discard(self.git)?;
                return self.needs_intervention(candidate, plan, reason);
            }
        };

        // 6. started → publish → receipt → completed.
        let started = PromotionStarted {
            target: refname.clone(),
            dest_before: dest_before.clone(),
            planned_after: planned_after.clone(),
            strategy,
            publish: mode.as_str().to_string(),
            validation_ids: vec![report.validation_id],
        };
        self.emit(
            candidate,
            plan,
            format!("promotion.started:{}:{}", plan.round, plan.attempt),
            EventKind::PromotionStarted(started.clone()),
        )?;
        self.fault(ABORT_AFTER_PROMOTION_STARTED)?;
        self.publish_and_finish(candidate, target, plan, &started, &mode, &temp)
    }
}

impl<G: GitClient, V: Validator, R: Recorder> GitPromotionEngine<'_, G, V, R> {
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
        if current.as_deref() == Some(started.planned_after.as_str()) {
            if let Some(reason) = self.settle_checkout(target, started)? {
                temp.discard(self.git)?;
                return self.needs_intervention(candidate, plan, reason);
            }
            return self.finish(candidate, target, plan, started, &started.publish, &temp);
        }
        if current.as_deref() == Some(started.dest_before.as_str()) {
            let mode = match self.publish_mode(target)? {
                Ok(mode) => mode,
                Err(reason) => {
                    temp.discard(self.git)?;
                    return self.needs_intervention(candidate, plan, reason);
                }
            };
            return self.publish_and_finish(candidate, target, plan, started, &mode, &temp);
        }
        temp.discard(self.git)?;
        self.needs_intervention(
            candidate,
            plan,
            format!(
                "{} is at {}, neither dest_before {} nor planned_after {}",
                started.target,
                current.as_deref().unwrap_or("nothing"),
                started.dest_before,
                started.planned_after
            ),
        )
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
        if started.publish != MERGE_FF_ONLY {
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
        let refname = &receipt.dest_ref;
        let current = self.git.rev_parse(&target.repo, refname)?;
        let emit = || {
            self.recorder.append(NewEvent {
                kind: EventKind::PromotionRolledBack(PromotionRolledBack {
                    target: refname.clone(),
                    restored: receipt.dest_before.clone(),
                }),
                actor: Actor::Operator,
                experiment_id: experiment.clone(),
                round_id: Some(round.clone()),
                execution_id: None,
                idempotency_key: format!("promotion.rolled_back:{round}"),
                occurred_at: clock::now(),
            })
        };
        let restored = RollbackResult::RolledBack {
            restored: receipt.dest_before.clone(),
        };
        // A restart after the swap: record it, move nothing.
        if current.as_deref() == Some(receipt.dest_before.as_str()) {
            emit()?;
            return Ok(restored);
        }
        if current.as_deref() != Some(receipt.dest_after.as_str()) {
            return Ok(RollbackResult::NeedsIntervention {
                reason: format!(
                    "{refname} moved to {} after the promotion to {}",
                    current.as_deref().unwrap_or("nothing"),
                    receipt.dest_after
                ),
            });
        }
        if let CheckoutLocation::CheckedOut { path, .. } =
            self.git.branch_checkout_location(&target.repo, refname)?
        {
            return Ok(RollbackResult::NeedsIntervention {
                reason: format!(
                    "{refname} is checked out in {}; a rollback would leave its files behind the branch",
                    path.display()
                ),
            });
        }
        if !self.git.update_ref_cas(
            &target.repo,
            refname,
            &receipt.dest_before,
            &receipt.dest_after,
        )? {
            return Ok(RollbackResult::NeedsIntervention {
                reason: format!("{refname} moved during the rollback"),
            });
        }
        emit()?;
        Ok(restored)
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
                    Ok(PublishMode::MergeFfOnly { checkout: path })
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
            PublishMode::MergeFfOnly { checkout } => self.publish_checked_out(checkout, started),
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
    /// step 4 nothing stays moved. The receipt keeps the name `merge_ff_only` for this mode.
    fn publish_checked_out(
        &self,
        checkout: &std::path::Path,
        started: &PromotionStarted,
    ) -> Result<Published> {
        let target = &started.target;
        let short = target.strip_prefix("refs/heads/").unwrap_or(target);
        let on_target = |at: &str| -> Result<bool> {
            Ok(self.git.current_branch(checkout)?.as_deref() == Some(short)
                && self.git.head(checkout)? == at)
        };
        if !on_target(&started.dest_before)? || !self.git.status_porcelain(checkout)?.is_empty() {
            return Ok(Published::Lost(format!(
                "the checkout {} of {target} changed before the publish",
                checkout.display()
            )));
        }
        if !self.git.update_ref_cas(
            checkout,
            target,
            &started.planned_after,
            &started.dest_before,
        )? {
            return Ok(Published::Lost(format!(
                "{target} moved from {} before the publish into {}",
                started.dest_before,
                checkout.display()
            )));
        }
        if let Err(e) =
            self.git
                .read_tree_update(checkout, &started.dest_before, &started.planned_after)
        {
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
            return Ok(Published::Lost(format!(
                "the files of {} could not move to {} ({e:#}); {state}",
                checkout.display(),
                started.planned_after
            )));
        }
        if !on_target(&started.planned_after)? {
            return Ok(Published::Lost(format!(
                "the checkout {} of {target} is not at {} after the publish",
                checkout.display(),
                started.planned_after
            )));
        }
        Ok(Published::Moved)
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
            Published::Moved => {}
            Published::Lost(reason) => {
                temp.discard(self.git)?;
                return self.needs_intervention(candidate, plan, reason);
            }
        }
        self.fault(ABORT_AFTER_UPDATE_REF)?;
        self.finish(candidate, target, plan, started, mode.as_str(), temp)
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
        let path = self.paths.promotion(&plan.round)?;
        let (receipt, bytes) = match std::fs::read(&path) {
            Ok(bytes) => {
                let receipt: PromotionReceipt = serde_json::from_slice(&bytes)
                    .with_context(|| format!("parsing {}", path.display()))?;
                if receipt.dest_after != started.planned_after
                    || receipt.dest_before != started.dest_before
                {
                    bail!(
                        "{} records {}..{}, not this promotion",
                        path.display(),
                        receipt.dest_before,
                        receipt.dest_after
                    );
                }
                (receipt, bytes)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut source_shas = self.git.rev_list(
                    &target.repo,
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
                fsx::create_immutable(&path, &bytes, fsx::PRIVATE_FILE)?;
                (receipt, bytes)
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
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
