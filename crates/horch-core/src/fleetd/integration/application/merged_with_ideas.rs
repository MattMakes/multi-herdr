//! A merge with ideas in flight: fleetd records the feature as `merged` and
//! moves each idea in flight to a new feature (ticket 35 of fleetd-p6).
//!
//! `docs/specs/fleetd/design.md`, "Features" (A merge with ideas in
//! flight), I7, I9, I10, I13 and FM-41. You can merge a pull request at any
//! time, so no idea is left without a feature and an owner, and no branch
//! lands on the branch of a `merged` feature:
//! - [`move_ideas`] (`MoveIdeasToNewFeature`) is the pass part. For each
//!   `open` feature with an idea in flight (not `integrating`, `cancelled`,
//!   `parked` or `merged`) whose changes git shows in the fetched primary
//!   tip by content (I8), 1 transaction moves the feature to `merged`,
//!   drops each open bundle of it (its `in_bundle` members go back to
//!   `ready`, and no bundle moves the old feature ref), writes 1 new feature
//!   with its internal run and its branch row from the fetched primary tip
//!   (ticket 15), moves each idea in flight and its tasks to the new
//!   feature and its run, and gives each open question of the old feature
//!   to the owner of the new one. The sub-orchestrator attempt and the pane
//!   of a moved idea stay with the idea. A second pass sees the feature
//!   `merged` and moves nothing.
//! - The effects after the commit are keyed by the rows. The new branch ref
//!   is [`new_features::pending_refs`]. Each task branch that is not
//!   `landed` or `abandoned`, on a `merged` feature, of an idea that now
//!   belongs to another feature in flight, is a [`pending_moves`] row. For
//!   each, [`rescue`] writes and reads back the rescue ref of its tip (I10),
//!   then [`record_move`] moves the row to the new feature and queues 1
//!   `rebase` job onto the new feature branch, in 1 transaction. A crash
//!   between 2 steps leaves the row on the `merged` feature, and the next
//!   [`complete_moves`] gets the same rescue ref for the same tip.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::sqlite::check_writable;
use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey};
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::Kind;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::{ideas_store, questions_store};
use crate::fleetd::integration::adapters::sqlite::{self as rows, BranchFact};
use crate::fleetd::integration::application::features;
use crate::fleetd::integration::application::landing::{self, BranchRescues};
use crate::fleetd::integration::application::new_features::{
    self, BranchRef, NameSource, Named, Project,
};
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::integration::domain::task_branch::LandingState;
use crate::fleetd::task_execution::adapters::sqlite as task_rows;
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::{runs, Store};

/// The states of an idea that is not in flight: it stays on its feature
/// when the feature is merged.
const NOT_IN_FLIGHT: [&str; 4] = ["integrating", "cancelled", "parked", "merged"];

/// What [`move_ideas`] works with.
pub struct MergedWithIdeas<'a> {
    pub jobs: &'a JobCtx<'a>,
    pub git: &'a dyn IntegrationGitGateway,
}

/// 1 merged feature whose ideas in flight moved to a new feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Moved {
    /// The feature that is `merged` now.
    pub feature: String,
    pub new_feature: String,
    /// The moved ideas, in the order of their rows.
    pub ideas: Vec<String>,
    /// The effect after the commit: the ref of the new feature branch.
    pub branch_ref: BranchRef,
}

/// 1 task branch on a `merged` feature whose idea moved to `feature`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchMove {
    pub branch: String,
    pub repo: PathBuf,
    /// The feature of the idea now.
    pub feature: String,
    /// The branch of `feature`, which the task branch is rebased onto.
    pub feature_branch: String,
}

/// The rescue of a [`BranchMove`], written before [`record_move`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rescued {
    pub branch: String,
    pub repo: PathBuf,
    pub feature: String,
    /// The rescue ref of the tip, or `None` when git has no such branch.
    pub rescue_ref: Option<String>,
    /// The tip that the rescue ref holds.
    pub tip: Option<String>,
    /// The tip of the new feature branch.
    pub onto: String,
}

/// The input key of the `rebase` job of a branch at `tip` onto `onto`: the
/// tip of the branch and the tip that it moves onto.
pub fn rebase_input_key(tip: &str, onto: &str) -> String {
    format!("{tip} {onto}")
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// `MoveIdeasToNewFeature`, in `tx` at the fleet clock `clock`: each `open`
/// feature merged by content with an idea in flight. See the module doc.
pub fn move_ideas(
    m: &MergedWithIdeas<'_>,
    tx: &Connection,
    clock: FleetSeconds,
) -> Result<Vec<Moved>> {
    let mut out = Vec::new();
    for id in open_features(tx)? {
        if let Some(moved) = move_one(m, tx, &id, clock).with_context(|| format!("feature {id}"))? {
            out.push(moved);
        }
    }
    Ok(out)
}

/// The `open` features, by id.
fn open_features(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM features WHERE state = 'open' ORDER BY id")?;
    let ids = stmt.query_map([], |r| r.get(0))?;
    Ok(ids.collect::<rusqlite::Result<_>>()?)
}

/// The ideas in flight of the feature `id`, oldest first.
fn ideas_in_flight(conn: &Connection, id: &str) -> Result<Vec<String>> {
    Ok(ideas_store::of_feature(conn, id)?
        .into_iter()
        .filter(|i| !NOT_IN_FLIGHT.contains(&i.state.as_str()))
        .map(|i| i.id)
        .collect())
}

fn move_one(
    m: &MergedWithIdeas<'_>,
    tx: &Connection,
    id: &str,
    clock: FleetSeconds,
) -> Result<Option<Moved>> {
    let ideas = ideas_in_flight(tx, id)?;
    if ideas.is_empty() {
        return Ok(None);
    }
    let row = rows::feature(tx, id)?.context("no such feature in the ledger")?;
    let Some(run) = runs::feature_run(tx, id)? else {
        return Ok(None);
    };
    // A fact that fleetd cannot read yet holds nothing: the next pass reads
    // it again.
    let Ok(tips) = features::read_tips(m.git, tx, &run.project_root, &row)? else {
        return Ok(None);
    };
    // As the feature state machine: a feature with no landed work has no
    // changes to find in the primary branch.
    if !features::has_landed_branch(tx, id)?
        || !m
            .git
            .changes_in(&tips.repo, &tips.feature, &tips.primary, &tips.primary)?
    {
        return Ok(None);
    }
    let ctx = m.jobs;
    let why = format!(
        "git shows the changes of {} in the primary tip {}, and {} idea(s) are in flight",
        tips.feature,
        tips.primary,
        ideas.len()
    );
    transition(
        tx,
        ctx.guard,
        Entity::Feature,
        id,
        "merged",
        by_fleetd(&why),
        ctx.needs_you,
    )?;
    drop_bundles(ctx, tx, id, clock)?;
    let why = format!("the ideas in flight of the merged feature {id} move to a new feature");
    let project = Project {
        root: &tips.repo,
        primary_branch: &row.primary_branch,
    };
    let new_id = new_features::open_feature(tx, ctx, &project, by_fleetd(&why))?;
    let title = row.name.as_deref().unwrap_or(id);
    let named = new_features::name_feature_branch(
        tx,
        ctx,
        m.git,
        &new_id,
        NameSource::Direct { title },
        &tips.primary,
        clock,
        by_fleetd(&why),
    )?;
    let Named::Accepted { effect, .. } = named else {
        bail!("the branch name of the new feature {new_id} is refused");
    };
    let new_run = runs::feature_run(tx, &new_id)?
        .with_context(|| format!("the task ledger has no internal run of {new_id}"))?;
    for idea in &ideas {
        move_idea(ctx, tx, idea, id, &new_id, &new_run.run_id)?;
    }
    move_questions(ctx, tx, id, &new_id)?;
    Ok(Some(Moved {
        feature: id.to_string(),
        new_feature: new_id,
        ideas,
        branch_ref: effect,
    }))
}

/// Drop each bundle of `feature` in `building`, `gating` or `passed`, and
/// send each of its `in_bundle` members back to `ready`, in `tx`.
fn drop_bundles(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    feature: &str,
    clock: FleetSeconds,
) -> Result<()> {
    let mut stmt = tx.prepare(
        "SELECT id FROM bundles WHERE feature_id = ?1 AND state IN ('building', 'gating', 'passed')
         ORDER BY id",
    )?;
    let ids: Vec<i64> = stmt
        .query_map([feature], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for id in ids {
        let why = format!("its feature {feature} is merged: the bundle never moves its ref");
        transition(
            tx,
            ctx.guard,
            Entity::Bundle,
            &id.to_string(),
            "dropped",
            by_fleetd(&why),
            ctx.needs_you,
        )?;
        let mut stmt = tx.prepare(
            "SELECT m.branch FROM bundle_members m JOIN branches b ON b.name = m.branch
             WHERE m.bundle_id = ?1 AND b.landing_state = ?2 ORDER BY m.position",
        )?;
        let members: Vec<String> = stmt
            .query_map(params![id, LandingState::InBundle.as_str()], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let why = format!("its bundle {id} is dropped: its feature {feature} is merged");
        for name in members {
            landing::enter(ctx, tx, &name, LandingState::Ready, clock, &why)?;
        }
    }
    Ok(())
}

/// Move the idea `idea` from the feature `from` to `to`, and each of its
/// tasks to the internal run `run` of `to`, so a task that starts later
/// starts and lands on the new feature branch. 1 event of the idea for each
/// write.
fn move_idea(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    idea: &str,
    from: &str,
    to: &str,
    run: &str,
) -> Result<()> {
    let why = format!("its feature {from} is merged while it is in flight");
    if !ideas_store::set_feature(tx, ctx.guard, idea, from, to, by_fleetd(&why))? {
        bail!("the idea {idea} is not an idea of {from}");
    }
    check_writable(tx, ctx.guard)?;
    let tasks = task_rows::move_tasks_of_idea(tx, idea, run)?;
    if tasks > 0 {
        let why = format!("its tasks move to the internal run of {to}");
        let data = json!({ "run_id": run, "tasks": tasks });
        record(
            tx,
            ctx.guard,
            EventKind::Note,
            Entity::Idea,
            idea,
            by_fleetd(&why),
            &data,
        )?;
    }
    Ok(())
}

/// Give each open question of `from` to the sub-orchestrator of `to`
/// (MAP.md, Q11): a `merged` feature has no sub-orchestrator (I13). 1 event
/// of `to` when a question moved.
fn move_questions(ctx: &JobCtx<'_>, tx: &Connection, from: &str, to: &str) -> Result<()> {
    check_writable(tx, ctx.guard)?;
    let owner = format!("idea-lead:{to}");
    let moved = questions_store::move_open(tx, from, to, &owner)?;
    if moved > 0 {
        let why = format!("the open questions of the merged feature {from} get a new owner");
        let data = json!({ "from": from, "owner": owner, "questions": moved });
        record(
            tx,
            ctx.guard,
            EventKind::Note,
            Entity::Feature,
            to,
            by_fleetd(&why),
            &data,
        )?;
    }
    Ok(())
}

/// Each task branch that waits for its move, in the order of its row: it is
/// not `landed` or `abandoned`, its feature is `merged`, and the idea of its
/// task belongs to another `open` or `ready` feature with a branch.
pub fn pending_moves(conn: &Connection) -> Result<Vec<BranchMove>> {
    let mut stmt = conn.prepare(
        "SELECT b.name, b.task_id, b.feature_id FROM branches b
         JOIN features f ON f.id = b.feature_id
         WHERE f.state = 'merged' AND b.task_id IS NOT NULL
             AND b.landing_state NOT IN ('landed', 'abandoned')
         ORDER BY b.rowid",
    )?;
    let found: Vec<(String, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::new();
    for (branch, task, merged) in found {
        let Some(idea) = task_rows::idea_of_task(conn, &task)? else {
            continue;
        };
        let Some(idea) = ideas_store::idea(conn, &idea)? else {
            continue;
        };
        let feature = idea.feature_id;
        if feature == merged {
            continue;
        }
        let Some(row) = rows::feature(conn, &feature)? else {
            continue;
        };
        let (true, Some(feature_branch)) =
            (matches!(row.state.as_str(), "open" | "ready"), row.branch)
        else {
            continue;
        };
        let run = runs::feature_run(conn, &feature)?
            .with_context(|| format!("the task ledger has no internal run of {feature}"))?;
        out.push(BranchMove {
            branch,
            repo: PathBuf::from(run.project_root),
            feature,
            feature_branch,
        });
    }
    Ok(out)
}

/// Step 1, before the ledger write: write and read back the rescue ref of
/// the tip of the branch of `mv` (I10). `None`, with no write, while git
/// has no new feature branch: its ref is the first effect.
pub fn rescue(
    git: &dyn IntegrationGitGateway,
    rescues: &dyn BranchRescues,
    mv: &BranchMove,
) -> Result<Option<Rescued>> {
    let Some(onto) = git.tip(&mv.repo, &format!("refs/heads/{}", mv.feature_branch))? else {
        return Ok(None);
    };
    let rescue_ref = rescues.rescue(&mv.repo, &mv.branch)?;
    let tip = match &rescue_ref {
        Some(name) => Some(
            git.tip(&mv.repo, name)?
                .with_context(|| format!("the rescue ref {name} does not exist"))?,
        ),
        None => None,
    };
    Ok(Some(Rescued {
        branch: mv.branch.clone(),
        repo: mv.repo.clone(),
        feature: mv.feature.clone(),
        rescue_ref,
        tip,
        onto,
    }))
}

/// Step 2, in `tx` at `clock`: the branch of `rescued` names its rescue ref,
/// moves to its new feature and gets 1 `rebase` job onto the new feature
/// branch. `false`, with no write, when the branch no longer waits for this
/// move.
pub fn record_move(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    rescued: &Rescued,
    clock: FleetSeconds,
) -> Result<bool> {
    let branch = rescued.branch.as_str();
    let waits = pending_moves(tx)?
        .iter()
        .any(|p| p.branch == branch && p.feature == rescued.feature);
    if !waits {
        return Ok(false);
    }
    let row = rows::branch(tx, branch)?.with_context(|| format!("no task branch {branch}"))?;
    let why = format!(
        "its idea moved to {}: the rescue ref of its tip is written, and it is rebased onto the new feature branch",
        rescued.feature
    );
    if let Some(name) = rescued.rescue_ref.as_deref() {
        if row.rescue_ref.as_deref() != Some(name) {
            rows::set_branch_fact(
                tx,
                ctx.guard,
                branch,
                BranchFact::RescueRef(name),
                by_fleetd(&why),
            )?;
        }
    }
    check_writable(tx, ctx.guard)?;
    tx.execute(
        "UPDATE branches SET feature_id = ?2 WHERE name = ?1",
        params![branch, rescued.feature],
    )?;
    let data = json!({ "feature_id": rescued.feature, "from": row.feature_id });
    record(
        tx,
        ctx.guard,
        EventKind::Note,
        Entity::TaskBranch,
        branch,
        by_fleetd(&why),
        &data,
    )?;
    if let Some(tip) = rescued.tip.as_deref() {
        let input_key = rebase_input_key(tip, &rescued.onto);
        let key = JobKey {
            kind: Kind::Rebase,
            subject: branch,
            input_key: &input_key,
        };
        let repo = rescued.repo.to_string_lossy();
        jobs::queue(ctx, tx, &key, Some(&repo), clock)?;
    }
    Ok(true)
}

/// The effects of each [`pending_moves`] row: [`rescue`], then
/// [`record_move`] in its own transaction. Gives the moved branches.
pub fn complete_moves(
    ctx: &JobCtx<'_>,
    store: &mut Store,
    git: &dyn IntegrationGitGateway,
    rescues: &dyn BranchRescues,
    clock: FleetSeconds,
) -> Result<Vec<String>> {
    let mut moved = Vec::new();
    for mv in pending_moves(store.conn())? {
        let Some(rescued) =
            rescue(git, rescues, &mv).with_context(|| format!("the rescue of {}", mv.branch))?
        else {
            continue;
        };
        if store.immediate(|tx| record_move(ctx, tx, &rescued, clock))? {
            moved.push(rescued.branch);
        }
    }
    Ok(moved)
}
