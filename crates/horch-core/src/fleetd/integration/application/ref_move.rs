use std::path::PathBuf;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::{JobCtx, NewItem};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::integration::adapters::sqlite::{self as rows, BranchFact, BundleRow};
use crate::fleetd::integration::application::ports::{IntegrationGitGateway, RefMove};
use crate::fleetd::integration::application::{bundles, landing};
use crate::fleetd::integration::domain::bundle::BundleState;
use crate::fleetd::integration::domain::task_branch::LandingState;
use crate::taskledger::model::{Actor, TaskStatus};
use crate::taskledger::{runs, tasks};

pub const OVERDUE_KIND: &str = "bundle_passed_overdue";

pub fn overdue_key(id: i64) -> String {
    format!("{OVERDUE_KIND}:{id}")
}

pub struct RefMoves<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    pub bundle: i64,
    pub repo: PathBuf,
    pub name: String,
    pub from: String,
    pub to: String,
    pub tasks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ran {
    Moved,
    Mismatch { actual: Option<String> },
    Cancelled { task: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    AtBase,
    AtCandidate,
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

pub fn advance(r: &RefMoves<'_>, tx: &Connection, clock: FleetSeconds) -> Result<Vec<Move>> {
    advance_in(r, tx, clock, None)
}

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
        let tip = match r.git.tip(repo, &format!("refs/heads/{member}"))? {
            Some(tip) => Some(tip),
            None => bundles::recorded_tip(r.git, tx, repo, &member)?,
        };
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

fn member_tasks(tx: &Connection, bundle: &BundleRow) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for member in &bundle.members {
        let row = rows::branch(tx, member)?.with_context(|| format!("no task branch {member}"))?;
        out.extend(row.task_id);
    }
    Ok(out)
}

fn cancelled(conn: &Connection, task_ids: &[String]) -> Result<Option<String>> {
    for id in task_ids {
        if tasks::get(conn, id)?.status == TaskStatus::Cancelled.as_str() {
            return Ok(Some(id.clone()));
        }
    }
    Ok(None)
}

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

fn passed_bundles(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM bundles WHERE state = ?1 ORDER BY id")?;
    let ids = stmt.query_map([BundleState::Passed.as_str()], |r| r.get(0))?;
    Ok(ids.collect::<rusqlite::Result<_>>()?)
}

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
