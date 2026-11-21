use anyhow::{bail, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::{outcome, result_data, JobCtx, NewItem};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, RecordRefusal};
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::worktree_custody::adapters::rescue_git::Rescue;
use crate::fleetd::worktree_custody::adapters::sqlite::{self as store, WorktreeState};
use crate::fleetd::worktree_custody::domain::rescue;
use crate::taskledger::model::Actor;

const RESCUABLE: [WorktreeState; 3] = [
    WorktreeState::Orphaned,
    WorktreeState::Landing,
    WorktreeState::Rescued,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    Rescued(String),
    Failed,
    Kept,
}

pub fn observe(ctx: &JobCtx<'_>, tx: &Connection, job: &JobRow) -> Result<Observed> {
    if job.kind != Kind::Rescue {
        bail!("job {} is a {} job, not a rescue job", job.id, job.kind);
    }
    let id = job.subject.as_str();
    match job.state {
        State::Passed => match saved(job, id) {
            Some(r) => rescued(ctx, tx, id, &r.rescue_ref),
            None => failed(
                ctx,
                tx,
                id,
                "the passed run names no rescue ref of the tree",
            ),
        },
        State::Failed => {
            let reason = outcome(job).map(|o| o.text).unwrap_or_default();
            failed(ctx, tx, id, &reason)
        }
        State::Queued | State::Running | State::Abandoned => Ok(Observed::Kept),
    }
}

fn saved(job: &JobRow, id: &str) -> Option<Rescue> {
    let r: Rescue = serde_json::from_value(result_data(job)?).ok()?;
    r.rescue_ref
        .starts_with(&rescue::ref_folder(id))
        .then_some(r)
}

fn rescued(ctx: &JobCtx<'_>, tx: &Connection, id: &str, rescue_ref: &str) -> Result<Observed> {
    let Some(row) = store::get(tx, id)? else {
        return Ok(Observed::Kept);
    };
    if !RESCUABLE.contains(&row.state) {
        return Ok(Observed::Kept);
    }
    store::set_rescue_ref(tx, id, rescue_ref)?;
    let cause = Cause {
        actor: Actor::Horch,
        text: "its rescue ref is read back and verified",
    };
    let refusals: &dyn RecordRefusal = ctx.needs_you;
    transition(
        tx,
        ctx.guard,
        Entity::Worktree,
        id,
        "rescued",
        cause,
        refusals,
    )?;
    ctx.needs_you.resolve_item(tx, &rescue::needs_you_key(id))?;
    Ok(Observed::Rescued(rescue_ref.to_string()))
}

fn failed(ctx: &JobCtx<'_>, tx: &Connection, id: &str, reason: &str) -> Result<Observed> {
    let key = rescue::needs_you_key(id);
    let text = format!("The rescue of worktree {id} failed, so its tree stays: {reason}");
    let item = NewItem {
        key: &key,
        kind: rescue::NEEDS_YOU_KIND,
        entity: Entity::Worktree,
        entity_id: id,
        text: &text,
    };
    ctx.needs_you.open_item(tx, &item)?;
    Ok(Observed::Failed)
}
