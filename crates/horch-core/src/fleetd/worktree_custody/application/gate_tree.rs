//! The gate tree of a feature, at `.worktrees/gate-<feature id>` (ticket 28).
//!
//! `docs/tickets/fleetd-p4/MAP.md`, Q18 and Q24: each feature has 1 gate
//! tree, where its `fast_gate` and `full_gate` jobs check out the tip that
//! they judge, so the build cache of the tree stays from 1 gate to the
//! next. It is a `worktrees` row before it exists (FR-11): no task, no
//! worker and no owner attempt, as fleetd owns it. Its `worktree_add` job
//! adds it detached at a tip, as the tree of a run gate (row W11). The
//! observation leaves it `created`, a state that the reconciler does not
//! orphan. When its feature is `merged` or `abandoned`, [`release`] gives
//! it to the rescue and the removal of custody.
//!
//! The gate cache of the feature, at `<fleet root>/gate-cache/<feature
//! id>/`, is the only other folder that a gate can write.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::Kind;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::worktree_custody::adapters::git::{worktree_add_key, DETACHED, GATE_PREFIX};
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as store, NewWorktree, WorktreeMode, WorktreeRow, WorktreeState,
};
use crate::fleetd::worktree_custody::application::remove::{record_owner_end, OwnerEnded};
use crate::taskledger::model::{Actor, WorktreeStatus};
use crate::taskledger::util;

/// The folder of the gate tree of `feature_id` in the repository `repo`.
pub fn path(repo: &Path, feature_id: &str) -> PathBuf {
    repo.join(".worktrees").join(format!("gate-{feature_id}"))
}

/// The gate cache of `feature_id` in the fleet root `fleet_root`.
pub fn cache_dir(fleet_root: &Path, feature_id: &str) -> PathBuf {
    fleet_root.join("gate-cache").join(feature_id)
}

/// The request for the gate tree of a feature.
#[derive(Debug, Clone, Copy)]
pub struct GateTreeRequest<'a> {
    pub feature_id: &'a str,
    /// The run of the feature: the run of the mode row of the tree.
    pub run_id: &'a str,
    pub repo: &'a Path,
    /// The branch of the feature, as the base of the row.
    pub base: &'a str,
    /// The tip that the tree is added at.
    pub tip: &'a str,
}

/// What [`reserve`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateTree {
    /// The tree exists in custody: a gate can run in it when it is
    /// `created`.
    Ready(WorktreeRow),
    /// The row is `reserved`, and `job` is its `worktree_add` run.
    Reserved { row: WorktreeRow, job: i64 },
}

/// The gate tree of `req.feature_id`, in `tx`, at `clock`: the row of its
/// folder, or a new `reserved` row with no task and no owner, and its
/// `worktree_add` run (row first, FR-11).
pub fn reserve(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    req: &GateTreeRequest<'_>,
    clock: FleetSeconds,
) -> Result<GateTree> {
    let folder = path(req.repo, req.feature_id);
    let row = match store::by_path(tx, &folder.to_string_lossy())? {
        Some(row) if row.state != WorktreeState::Reserved => return Ok(GateTree::Ready(row)),
        Some(row) => row,
        None => {
            let new = NewWorktree {
                worktree_id: util::new_id(),
                task_id: None,
                worker_id: None,
                repo_root: req.repo.to_string_lossy().into_owned(),
                path: folder.to_string_lossy().into_owned(),
                branch: DETACHED.into(),
                base: req.base.into(),
                base_commit: Some(req.tip.into()),
                status: WorktreeStatus::Open,
                mode: Some(WorktreeMode {
                    run_id: req.run_id.into(),
                    name: format!("{GATE_PREFIX}{}", req.feature_id),
                    scratch: true,
                }),
            };
            let cause = by_fleetd("the gates of the feature need a tree");
            store::insert_reserved(tx, ctx.guard, &new, cause)?;
            store::get(tx, &new.worktree_id)?.context("the reserved gate tree is gone")?
        }
    };
    let input_key = worktree_add_key(&row);
    let key = JobKey {
        kind: Kind::WorktreeAdd,
        subject: &row.worktree_id,
        input_key: &input_key,
    };
    let job = match jobs::queue(ctx, tx, &key, Some(&row.repo_root), clock)? {
        Queued::New(id) => id,
        Queued::Active(r) | Queued::Reused(r) => r.id,
        Queued::Held(r) => bail!(
            "the worktree_add job of the gate tree {} ended abandoned {} times; run horch job requeue {}",
            row.path,
            r.abandoned_runs,
            r.id
        ),
    };
    Ok(GateTree::Reserved { row, job })
}

/// Whether a gate of `req.feature_id` can run in its gate tree now: the
/// tree is `created` and its folder exists. A feature with no gate tree
/// gets its reserved row and its `worktree_add` run (Q18), and a later pass
/// runs the gate.
pub fn ready(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    req: &GateTreeRequest<'_>,
    clock: FleetSeconds,
) -> Result<bool> {
    Ok(match reserve(ctx, tx, req, clock)? {
        GateTree::Ready(tree) => {
            tree.state == WorktreeState::Created && Path::new(&tree.path).is_dir()
        }
        GateTree::Reserved { .. } => false,
    })
}

/// The gate tree of `feature_id` in `repo` that is not `removed`.
pub fn of_feature(conn: &Connection, repo: &Path, feature_id: &str) -> Result<Option<WorktreeRow>> {
    store::by_path(conn, &path(repo, feature_id).to_string_lossy())
}

/// Give the gate tree of the ended feature `feature_id` to custody, in
/// `tx`: a `created` tree moves to `orphaned` with the fact that its owner
/// gave it up, and the reconciler rescues it, then removes it. `false` when the feature has no `created` gate
/// tree.
pub fn release(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    repo: &Path,
    feature_id: &str,
    why: &str,
) -> Result<bool> {
    let Some(row) = of_feature(tx, repo, feature_id)? else {
        return Ok(false);
    };
    if row.state != WorktreeState::Created {
        return Ok(false);
    }
    transition(
        tx,
        ctx.guard,
        Entity::Worktree,
        &row.worktree_id,
        "orphaned",
        by_fleetd(why),
        ctx.needs_you,
    )?;
    let ended = OwnerEnded {
        keep_branch: false,
        rescue_job: None,
    };
    record_owner_end(ctx, tx, &row.worktree_id, Actor::Horch, &ended)?;
    Ok(true)
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}
