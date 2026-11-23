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

pub const CURSOR: &str = "questions";

pub const NEEDS_YOU_KIND: &str = "question";

pub fn owner_of(feature_id: &str) -> String {
    format!("{PERSONA}:{feature_id}")
}

pub fn needs_you_key(id: &str) -> String {
    format!("{NEEDS_YOU_KIND}:{id}")
}

pub fn feature_of_task(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    let Some(idea) = attempts::idea_of_task(conn, task_id)? else {
        return Ok(None);
    };
    Ok(ideas_store::idea(conn, &idea)?.map(|i| i.feature_id))
}

pub struct Flow<'a> {
    pub guard: Option<Generation>,
    pub clock: FleetSeconds,
    pub elapsed_s: i64,
    pub stamp: &'a str,
    pub budget: PassBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ring {
    pub attempt: String,
    pub pane: String,
    pub event: i64,
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flowed {
    pub rings: Vec<Ring>,
    pub escalated: Vec<String>,
    pub moved: Vec<String>,
}

#[derive(Debug, Clone)]
struct Owner {
    attempt: String,
    pane: Option<String>,
}

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

fn rung(conn: &Connection, attempt: &str) -> Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for f in door::facts_of(conn, Entity::Attempt, attempt, EventKind::Doorbell)? {
        let data: serde_json::Value = serde_json::from_str(&f.data_json)?;
        let ids = data["question_ids"].as_array().into_iter().flatten();
        out.extend(ids.filter_map(|v| v.as_str().map(str::to_string)));
    }
    Ok(out)
}

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
