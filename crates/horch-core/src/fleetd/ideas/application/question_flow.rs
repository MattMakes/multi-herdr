//! The question flow: the sub-orchestrator of the feature owns each worker
//! question, and an unanswered question escalates to you
//! (`docs/specs/fleetd/design.md`, "Ideas": How a worker's question is
//! resolved, steps 1 to 3 and 7; RR-12; FM-35; Ledger, `questions`;
//! `docs/tickets/fleetd-p6/30-questions.md`).
//!
//! - The owner of a question is [`owner_of`] the feature of the idea of its
//!   task: `idea-lead:<feature id>` (MAP.md of fleetd-p6, Q11). The live
//!   process of the feature is the live sub-orchestrator attempt of an idea
//!   of the feature. Ticket 28 starts it when the feature has an open
//!   question ([`super::sub_orchestrators`]); this use case does not start
//!   a process.
//! - A change of the owner is [`move_to_feature`]: the row and 1 event with
//!   its cause, in 1 transaction. Each pass moves an open question whose
//!   task is now in another feature.
//! - The time limit `due_at` is a deadline on the fleet clock that counts
//!   only while the owner runs: each pass moves the limit of each open
//!   question of a feature with no live process later by the seconds that
//!   the fleet clock moved in the pass (`elapsed_s`). So a stopped or dead
//!   owner does not use the 15 min of the question.
//! - fleetd rings the live process of the feature with the doorbell (FR-7):
//!   1 `doorbell` event of the attempt that names the open questions it
//!   announces, and 1 [`Ring`] for part 3. Each process of the feature gets
//!   1 ring for each open question, so the delivery repeats to each new
//!   process until an answer is recorded.
//! - At its limit (`clock >= due_at`), an open question goes to
//!   `escalated`, with 1 `blocked` event of its task and 1 needs-you item
//!   [`needs_you_key`]; the needs-you mirror gives the herdr notification
//!   (ticket 21). The wait for you has no limit: the task stays in
//!   `blocked_on_user` until an answer is recorded
//!   ([`super::questions::answered`]), which resolves the item.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use rusqlite::{Connection, Transaction};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::cursors_store;
use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::application::ledger::{record, Cause};
use crate::fleetd::fleet_runtime::domain::budget::PassBudget;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::adapters::{ideas_store, questions_store as store};
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::ideas::application::questions::asked_text;
use crate::fleetd::ideas::application::sub_orchestrators::PERSONA;
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::ideas::domain::questions::{printable, Question};
use crate::fleetd::task_execution::adapters::sqlite as attempts;
use crate::fleetd::task_execution::domain::task::AttemptState;
use crate::taskledger::model::{Actor, EventKind};

/// The kind of the row of `pass_cursors` of the question flow.
pub const CURSOR: &str = "questions";

/// The kind of the needs-you item of an escalated question.
pub const NEEDS_YOU_KIND: &str = "question";

/// The owner of the questions of feature `feature_id`: its
/// sub-orchestrator, `idea-lead:<feature id>`.
pub fn owner_of(feature_id: &str) -> String {
    format!("{PERSONA}:{feature_id}")
}

/// The key of the needs-you item of the question `id`.
pub fn needs_you_key(id: &str) -> String {
    format!("{NEEDS_YOU_KIND}:{id}")
}

/// The feature of the idea of task `task_id`, or `None` when the task has
/// no idea (an imported task).
pub fn feature_of_task(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    let Some(idea) = attempts::idea_of_task(conn, task_id)? else {
        return Ok(None);
    };
    Ok(ideas_store::idea(conn, &idea)?.map(|i| i.feature_id))
}

/// What 1 pass needs besides the ledger.
pub struct Flow<'a> {
    /// The generation of the fleetd that writes.
    pub guard: Option<Generation>,
    /// The fleet clock of the pass.
    pub clock: FleetSeconds,
    /// The seconds that the tick of this pass added to the fleet clock.
    pub elapsed_s: i64,
    /// The time stamp of the pass, for the cursor.
    pub stamp: &'a str,
    pub budget: PassBudget,
}

/// 1 doorbell to type after the commit: the event row, and the pane of the
/// live process of the feature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ring {
    pub attempt: String,
    pub pane: String,
    /// The id of the `doorbell` event.
    pub event: i64,
    /// The open questions that the ring announces, oldest first.
    pub questions: Vec<String>,
}

/// What 1 pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flowed {
    pub rings: Vec<Ring>,
    /// The questions that went to you.
    pub escalated: Vec<String>,
    /// The questions that got a new owner.
    pub moved: Vec<String>,
}

/// The live process of a feature.
#[derive(Debug, Clone)]
struct Owner {
    attempt: String,
    pane: Option<String>,
}

/// The question flow of 1 pass, in the transaction `tx` of part 2: the
/// limits of the questions whose owner does not run move later; then, for
/// the open questions of the work budget from the cursor [`CURSOR`], each
/// follows the feature of its task, escalates at its limit, or is rung to
/// the live process of its feature.
pub fn run(tx: &Transaction<'_>, ctx: &Flow<'_>) -> Result<Flowed> {
    let mut owners: BTreeMap<String, Option<Owner>> = BTreeMap::new();
    if ctx.elapsed_s > 0 {
        let features: BTreeSet<String> =
            store::open(tx)?.into_iter().map(|q| q.feature_id).collect();
        for feature in features {
            if owner(tx, &mut owners, &feature)?.is_none() {
                store::delay_open_of_feature(tx, &feature, ctx.elapsed_s)?;
            }
        }
    }
    let open = store::open(tx)?;
    let keys: Vec<String> = open.iter().map(|q| q.id.clone()).collect();
    let mut cursor = cursors_store::load(tx, CURSOR, ctx.stamp)?;
    let chosen: BTreeSet<String> = cursor
        .take(&keys, ctx.budget, ctx.stamp)
        .into_iter()
        .map(str::to_string)
        .collect();
    cursors_store::save(tx, CURSOR, &cursor)?;
    let mut out = Flowed::default();
    let mut bells: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for q in open.into_iter().filter(|q| chosen.contains(&q.id)) {
        let q = follow_task(tx, ctx, q, &mut out)?;
        if ctx.clock.0 >= q.due_at {
            escalate(tx, ctx, &q)?;
            out.escalated.push(q.id);
            continue;
        }
        let Some(Owner {
            attempt,
            pane: Some(pane),
        }) = owner(tx, &mut owners, &q.feature_id)?
        else {
            continue;
        };
        if !rung(tx, &attempt)?.contains(&q.id) {
            bells
                .entry(attempt)
                .or_insert((pane, Vec::new()))
                .1
                .push(q.id);
        }
    }
    for (attempt, (pane, questions)) in bells {
        let event = ring(tx, ctx, &attempt, &questions)?;
        out.rings.push(Ring {
            attempt,
            pane,
            event,
            questions,
        });
    }
    Ok(out)
}

/// Give the question `q` the feature `feature_id` and its owner, with 1
/// `question` event of its task whose cause is `why` and names both
/// owners, in the open transaction `tx`. `false` when `q` has that owner.
pub fn move_to_feature(
    tx: &Connection,
    guard: Option<Generation>,
    q: &Question,
    feature_id: &str,
    why: &str,
) -> Result<bool> {
    let owner = owner_of(feature_id);
    if q.feature_id == feature_id && q.owner == owner {
        return Ok(false);
    }
    store::set_owner(tx, &q.id, feature_id, &owner)?;
    let text = format!(
        "question {} moves from owner {} to owner {owner}: {why}",
        q.id, q.owner
    );
    let data = json!({ "question_id": q.id, "owner": owner, "was": q.owner });
    record(
        tx,
        guard,
        EventKind::Question,
        Entity::Task,
        &q.task_id,
        by_fleetd(&text),
        &data,
    )?;
    Ok(true)
}

/// `q` with the feature of the idea of its task, when that moved.
fn follow_task(tx: &Connection, ctx: &Flow<'_>, q: Question, out: &mut Flowed) -> Result<Question> {
    let Some(feature) = feature_of_task(tx, &q.task_id)? else {
        return Ok(q);
    };
    let why = format!("the idea of task {} is in feature {feature}", q.task_id);
    if !move_to_feature(tx, ctx.guard, &q, &feature, &why)? {
        return Ok(q);
    }
    out.moved.push(q.id.clone());
    Ok(Question {
        owner: owner_of(&feature),
        feature_id: feature,
        ..q
    })
}

/// The live process of `feature`, read once in a pass.
fn owner(
    conn: &Connection,
    owners: &mut BTreeMap<String, Option<Owner>>,
    feature: &str,
) -> Result<Option<Owner>> {
    if let Some(known) = owners.get(feature) {
        return Ok(known.clone());
    }
    let mut found = None;
    for idea in ideas_store::of_feature(conn, feature)? {
        for (a, _) in attempts::idea_attempts(conn, &idea.id)? {
            if a.state == AttemptState::Live {
                found = Some(Owner {
                    attempt: a.id,
                    pane: a.pane,
                });
            }
        }
    }
    owners.insert(feature.to_string(), found.clone());
    Ok(found)
}

/// The questions that a `doorbell` event of the attempt `attempt`
/// announced.
fn rung(conn: &Connection, attempt: &str) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for f in door::facts_of(conn, Entity::Attempt, attempt, EventKind::Doorbell)? {
        let data: serde_json::Value = serde_json::from_str(&f.data_json)?;
        let ids = data["question_ids"].as_array().into_iter().flatten();
        out.extend(ids.filter_map(|v| v.as_str().map(str::to_string)));
    }
    Ok(out)
}

/// 1 `doorbell` event of the attempt `attempt` that announces `questions`.
/// The id of the event.
fn ring(tx: &Connection, ctx: &Flow<'_>, attempt: &str, questions: &[String]) -> Result<i64> {
    let text = format!(
        "the sub-orchestrator has {} new open questions",
        questions.len()
    );
    let data = json!({ "fleet_clock": ctx.clock.0, "question_ids": questions });
    record(
        tx,
        ctx.guard,
        EventKind::Doorbell,
        Entity::Attempt,
        attempt,
        by_fleetd(&text),
        &data,
    )?;
    door::facts_of(tx, Entity::Attempt, attempt, EventKind::Doorbell)?
        .last()
        .map(|f| f.event_id)
        .context("the doorbell event that this pass recorded")
}

/// The open question `q` goes to you: `escalated`, 1 `blocked` event of
/// its task and 1 needs-you item.
fn escalate(tx: &Connection, ctx: &Flow<'_>, q: &Question) -> Result<()> {
    if !store::set_escalated(tx, &q.id)? {
        return Ok(());
    }
    let text = format!(
        "question {} has no answer from its owner {} at its time limit, so it goes to you",
        q.id, q.owner
    );
    let data = json!({ "question_id": q.id, "owner": q.owner, "fleet_clock": ctx.clock.0 });
    record(
        tx,
        ctx.guard,
        EventKind::Blocked,
        Entity::Task,
        &q.task_id,
        by_fleetd(&text),
        &data,
    )?;
    let asked = asked_text(tx, q)?.map_or_else(|| "(no text)".to_string(), |t| printable(&t));
    let item = format!(
        "Question {} of task {} has no answer from the sub-orchestrator of feature {} after \
         its time limit. The task stays in blocked_on_user until an answer is recorded. \
         Answer it in the pane of that sub-orchestrator. The question: {asked}",
        q.id, q.task_id, q.feature_id
    );
    needs_you::open_item(
        tx,
        &Write {
            key: &needs_you_key(&q.id),
            kind: NEEDS_YOU_KIND,
            entity: Entity::Task.as_str(),
            entity_id: &q.task_id,
            text: &item,
        },
    )
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}
