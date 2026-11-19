//! The ref move by compare and set (ticket 31).
//!
//! `docs/specs/fleetd/design.md`, I8, I9, FM-37, F9, "The bundle state
//! machine" (`passed`, `landed`) and "A moved tip drops a bundle". The
//! bundle use cases move a bundle to `passed` on a pass of its fast gate.
//! From there:
//! - [`advance`] is the pass part. For each `passed` bundle it reads the
//!   tip of the feature branch. At the base tip, it records the tip as
//!   `seen_tip` and gives 1 [`Move`] to run after the commit. At the
//!   candidate tip, the move is observed: the bundle is `landed`, and each
//!   member whose changes are in the candidate by content is `landed` (I8).
//!   At another tip, the compare-and-set failed or the gate result is
//!   stale: fleetd records the new tip, and the bundle is `dropped` (FM-37).
//!   A member task that is cancelled drops the bundle before the move (F9).
//! - [`run`] is the effect after the commit: it checks once more that no
//!   member task is cancelled (F9), then moves the feature ref from the
//!   base tip to the candidate tip with 1 compare-and-set (I9). It records
//!   nothing: the next pass reads the ref. So a crash at any point repeats
//!   the move safely, and the bundle lands 1 time.
//! - A bundle in `passed` past its due time has 1 needs-you item, and the
//!   item ends when the bundle leaves `passed`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::integration::adapters::sqlite::{self as rows, BranchFact, BundleRow};
use crate::fleetd::integration::application::landing;
use crate::fleetd::integration::application::ports::{IntegrationGitGateway, RefMove};
use crate::fleetd::integration::domain::bundle::BundleState;
use crate::fleetd::integration::domain::task_branch::LandingState;
use crate::taskledger::model::{Actor, TaskStatus};
use crate::taskledger::{runs, tasks};

/// The kind of the needs-you item of a bundle past the limit of `passed`.
/// It is not the kind of the bundle use cases: they end each item of their
/// kind whose bundle is not past a limit of `building` or `gating`.
pub const OVERDUE_KIND: &str = "bundle_passed_overdue";

/// The key of the needs-you item of the `passed` bundle `id` past its limit.
pub fn overdue_key(id: i64) -> String {
    format!("{OVERDUE_KIND}:{id}")
}

/// What [`advance`] works with: the job context of the pass, and the git
/// gateway that reads the tips.
pub struct RefMoves<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
}

/// 1 move of a feature ref, to run after the commit of its pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    /// The `passed` bundle.
    pub bundle: i64,
    pub repo: PathBuf,
    /// The full name of the feature ref, `refs/heads/<branch>`.
    pub name: String,
    /// The expected old tip: the base tip of the bundle and `seen_tip`.
    pub from: String,
    /// The candidate tip of the bundle.
    pub to: String,
    /// The task of each member, in queue order.
    pub tasks: Vec<String>,
}

/// What [`run`] did with 1 [`Move`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    /// The ref was at the base tip, and git moved it to the candidate tip.
    Moved,
    /// The ref was not at the base tip, and nothing moved.
    Mismatch { actual: Option<String> },
    /// The member task `task` is cancelled, and nothing moved (F9).
    Cancelled { task: String },
}

/// Where the feature tip is, seen from a `passed` bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    /// At the base tip: the move is due.
    AtBase,
    /// At the candidate tip: the move is observed.
    AtCandidate,
    /// At another tip, or no tip: the move cannot be done.
    Elsewhere(Option<String>),
}

fn seen(tip: Option<String>, base: &str, candidate: &str) -> Seen {
    match tip {
        Some(t) if t == candidate => Seen::AtCandidate,
        Some(t) if t == base => Seen::AtBase,
        other => Seen::Elsewhere(other),
    }
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// The pass part of the ref move, in the transaction `tx` of part 2 at the
/// fleet clock `clock`: step each `passed` bundle, and keep 1 needs-you item
/// for each bundle past its limit. Gives the moves to run after the commit.
pub fn advance(r: &RefMoves<'_>, tx: &Connection, clock: FleetSeconds) -> Result<Vec<Move>> {
    advance_in(r, tx, clock, None)
}

/// [`advance`] for the work budget of 1 pass: step only the bundles of
/// `bundles`. `None` is each bundle.
pub fn advance_in(
    r: &RefMoves<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    bundles: Option<&[i64]>,
) -> Result<Vec<Move>> {
    let mut moves = Vec::new();
    for id in passed_bundles(tx)? {
        if bundles.is_some_and(|ids| !ids.contains(&id)) {
            continue;
        }
        let bundle = rows::bundle(tx, id)?.with_context(|| format!("no bundle {id}"))?;
        if let Some(mv) = step(r, tx, clock, &bundle).with_context(|| format!("bundle {id}"))? {
            moves.push(mv);
        }
    }
    overdue_items(r.jobs, tx, clock)?;
    Ok(moves)
}

/// 1 step of the `passed` bundle `bundle`.
fn step(
    r: &RefMoves<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    bundle: &BundleRow,
) -> Result<Option<Move>> {
    let Some(branch) = rows::feature(tx, &bundle.feature_id)?.and_then(|f| f.branch) else {
        return Ok(None);
    };
    let Some(run) = runs::feature_run(tx, &bundle.feature_id)? else {
        return Ok(None);
    };
    let repo = PathBuf::from(run.project_root);
    let candidate = bundle
        .candidate_tip
        .as_deref()
        .context("a passed bundle has no candidate tip")?;
    let name = format!("refs/heads/{branch}");
    let ctx = r.jobs;
    match seen(r.git.tip(&repo, &name)?, &bundle.base_tip, candidate) {
        Seen::AtCandidate => {
            record_tip(ctx, tx, &branch, candidate)?;
            land(r, tx, clock, bundle, &repo, candidate)?;
            Ok(None)
        }
        Seen::AtBase => {
            record_tip(ctx, tx, &branch, &bundle.base_tip)?;
            let member_tasks = member_tasks(tx, bundle)?;
            if let Some(task) = cancelled(tx, &member_tasks)? {
                let why = format!("F9: the task {task} of a member is cancelled");
                drop_bundle(ctx, tx, clock, bundle, &why)?;
                return Ok(None);
            }
            Ok(Some(Move {
                bundle: bundle.id,
                repo,
                name,
                from: bundle.base_tip.clone(),
                to: candidate.to_string(),
                tasks: member_tasks,
            }))
        }
        Seen::Elsewhere(tip) => {
            if let Some(tip) = &tip {
                record_tip(ctx, tx, &branch, tip)?;
            }
            let why = format!(
                "the tip of {branch} is {}, not its base tip {}",
                tip.as_deref().unwrap_or("absent"),
                bundle.base_tip
            );
            drop_bundle(ctx, tx, clock, bundle, &why)?;
            Ok(None)
        }
    }
}

/// The move of the feature ref is observed: each member whose changes are
/// in `candidate` by content goes to `landed` (I8), each other member back
/// to `ready`, and the bundle to `landed`.
fn land(
    r: &RefMoves<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    bundle: &BundleRow,
    repo: &std::path::Path,
    candidate: &str,
) -> Result<()> {
    let why = format!(
        "the feature ref is at its candidate tip {candidate}, moved from the base tip {}",
        bundle.base_tip
    );
    enter(r.jobs, tx, bundle.id, BundleState::Landed, &why)?;
    for member in in_bundle(tx, bundle)? {
        let tip = r.git.tip(repo, &format!("refs/heads/{member}"))?;
        let landed = match &tip {
            Some(tip) => r.git.changes_in(repo, tip, &bundle.base_tip, candidate)?,
            None => false,
        };
        let (to, why) = if landed {
            (
                LandingState::Landed,
                format!(
                    "its changes are in {candidate} by content (bundle {})",
                    bundle.id
                ),
            )
        } else {
            (
                LandingState::Ready,
                format!(
                    "its tip {} is not in {candidate} by content (bundle {})",
                    tip.as_deref().unwrap_or("absent"),
                    bundle.id
                ),
            )
        };
        landing::enter(r.jobs, tx, &member, to, clock, &why)?;
    }
    Ok(())
}

/// Drop `bundle`, and move each member in `in_bundle` back to `ready`.
fn drop_bundle(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    bundle: &BundleRow,
    why: &str,
) -> Result<()> {
    enter(ctx, tx, bundle.id, BundleState::Dropped, why)?;
    let why = format!("bundle {} is dropped: {why}", bundle.id);
    for member in in_bundle(tx, bundle)? {
        landing::enter(ctx, tx, &member, LandingState::Ready, clock, &why)?;
    }
    Ok(())
}

/// Move the bundle `id` to `to` through `transition()`.
fn enter(ctx: &JobCtx<'_>, tx: &Connection, id: i64, to: BundleState, why: &str) -> Result<()> {
    transition(
        tx,
        ctx.guard,
        Entity::Bundle,
        &id.to_string(),
        to.as_str(),
        by_fleetd(why),
        ctx.needs_you,
    )?;
    Ok(())
}

/// Write `tip` as the `seen_tip` of the row of the feature branch `branch`,
/// when the row exists and has another tip.
fn record_tip(ctx: &JobCtx<'_>, tx: &Connection, branch: &str, tip: &str) -> Result<()> {
    let Some(row) = rows::branch(tx, branch)? else {
        return Ok(());
    };
    if row.seen_tip.as_deref() == Some(tip) {
        return Ok(());
    }
    rows::set_branch_fact(
        tx,
        ctx.guard,
        branch,
        BranchFact::SeenTip(tip),
        by_fleetd("fleetd read the tip of the feature branch"),
    )?;
    Ok(())
}

/// The members of `bundle` that are still `in_bundle`, in queue order.
fn in_bundle(tx: &Connection, bundle: &BundleRow) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for member in &bundle.members {
        let row = rows::branch(tx, member)?.with_context(|| format!("no task branch {member}"))?;
        if row.landing_state.as_deref() == Some(LandingState::InBundle.as_str()) {
            out.push(member.clone());
        }
    }
    Ok(out)
}

/// The task of each member of `bundle`, in queue order.
fn member_tasks(tx: &Connection, bundle: &BundleRow) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for member in &bundle.members {
        let row = rows::branch(tx, member)?.with_context(|| format!("no task branch {member}"))?;
        out.extend(row.task_id);
    }
    Ok(out)
}

/// The first task of `task_ids` that is cancelled.
fn cancelled(conn: &Connection, task_ids: &[String]) -> Result<Option<String>> {
    for id in task_ids {
        if tasks::get(conn, id)?.status == TaskStatus::Cancelled.as_str() {
            return Ok(Some(id.clone()));
        }
    }
    Ok(None)
}

/// Run `mv` after the commit of its pass: check once more that no member
/// task is cancelled (F9), then move the feature ref by 1 compare-and-set
/// from the base tip (I9). `conn` is only read.
pub fn run(conn: &Connection, git: &dyn IntegrationGitGateway, mv: &Move) -> Result<Ran> {
    if let Some(task) = cancelled(conn, &mv.tasks)? {
        return Ok(Ran::Cancelled { task });
    }
    Ok(
        match git.update_ref(&mv.repo, &mv.name, &mv.to, Some(&mv.from))? {
            RefMove::Moved => Ran::Moved,
            RefMove::Mismatch { actual } => Ran::Mismatch { actual },
        },
    )
}

/// The ids of the `passed` bundles, by id.
fn passed_bundles(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM bundles WHERE state = ?1 ORDER BY id")?;
    let ids = stmt.query_map([BundleState::Passed.as_str()], |r| r.get(0))?;
    Ok(ids.collect::<rusqlite::Result<_>>()?)
}

/// Open 1 needs-you item for each `passed` bundle past its due time, and
/// end the item of each bundle that is no longer `passed`.
fn overdue_items(ctx: &JobCtx<'_>, tx: &Connection, clock: FleetSeconds) -> Result<()> {
    let passed = passed_bundles(tx)?;
    for key in needs_you::open_keys(tx, OVERDUE_KIND)? {
        let still = key
            .strip_prefix(&format!("{OVERDUE_KIND}:"))
            .and_then(|id| id.parse::<i64>().ok())
            .is_some_and(|id| passed.contains(&id));
        if !still {
            ctx.needs_you.resolve_item(tx, &key)?;
        }
    }
    for id in passed {
        let bundle = rows::bundle(tx, id)?.with_context(|| format!("no bundle {id}"))?;
        if clock.0 < bundle.due_at {
            continue;
        }
        let key = overdue_key(id);
        let text = format!(
            "Bundle {id} of {} is passed past its time limit, and the feature ref did not move to its candidate. Read the events of the bundle.",
            bundle.feature_id
        );
        ctx.needs_you.open_item(
            tx,
            &NewItem {
                key: &key,
                kind: OVERDUE_KIND,
                entity: Entity::Bundle,
                entity_id: &id.to_string(),
                text: &text,
            },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feature_tip_says_where_the_move_is() {
        let s = |tip: Option<&str>| seen(tip.map(String::from), "b", "c");
        assert_eq!(s(Some("b")), Seen::AtBase);
        assert_eq!(s(Some("c")), Seen::AtCandidate);
        assert_eq!(s(Some("x")), Seen::Elsewhere(Some("x".into())));
        assert_eq!(s(None), Seen::Elsewhere(None));
    }

    #[test]
    fn each_overdue_item_has_its_own_kind() {
        assert_eq!(overdue_key(7), "bundle_passed_overdue:7");
        assert_ne!(OVERDUE_KIND, super::super::bundles::OVERDUE_KIND);
    }
}
