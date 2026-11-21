use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::sqlite::{self as door, Event, Fact};
use crate::fleetd::fleet_runtime::domain::ledger::{
    decide, is_resume, refused_cause, Decision, Entity, Generation,
};
use crate::fleetd::task_execution::domain::lease::Holder;
use crate::taskledger::model::{Actor, EventKind};

pub trait RecordRefusal {
    fn record_refusal(
        &self,
        tx: &Connection,
        entity: Entity,
        id: &str,
        text: &str,
    ) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub struct Cause<'a> {
    pub actor: Actor,
    pub text: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    Changed,
    Unchanged,
}

#[derive(Debug)]
pub enum DoorError {
    Refused {
        entity: Entity,
        id: String,
        from: String,
        to: String,
        cause: String,
        actor: Actor,
    },
    GenerationLost {
        held: Generation,
        found: Option<Generation>,
    },
    NoRow {
        entity: Entity,
        id: String,
    },
    NoTransaction,
    Ledger(anyhow::Error),
}

impl std::fmt::Display for DoorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DoorError::Refused {
                entity,
                id,
                from,
                to,
                ..
            } => write!(f, "illegal transition of {entity} {id}: {from} -> {to}"),
            DoorError::GenerationLost { held, found } => write!(
                f,
                "the fleetd generation is {:?}, not {}: another fleetd holds the ledger",
                found.map(|g| g.0),
                held.0
            ),
            DoorError::NoRow { entity, id } => write!(f, "no {entity} {id} in the ledger"),
            DoorError::NoTransaction => {
                write!(
                    f,
                    "a write of the door needs an open transaction of the ledger"
                )
            }
            DoorError::Ledger(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for DoorError {}

impl From<anyhow::Error> for DoorError {
    fn from(e: anyhow::Error) -> Self {
        DoorError::Ledger(e)
    }
}

pub fn transition(
    tx: &Connection,
    guard: Option<Generation>,
    entity: Entity,
    id: &str,
    to: &str,
    cause: Cause<'_>,
    refusals: &dyn RecordRefusal,
) -> Result<Moved, DoorError> {
    check_open(tx)?;
    check_generation(tx, guard)?;
    let from = state_of(tx, entity, id)?;
    match decide_move(tx, entity, id, &from, to)? {
        Decision::Unchanged => Ok(Moved::Unchanged),
        Decision::Allowed => {
            door::set_state(tx, entity, id, to)?;
            if entity == Entity::Task && Holder::of(to).is_none() {
                door::delete_lease(tx, id)?;
            }
            door::append_event(
                tx,
                &Event {
                    entity,
                    id,
                    from: Some(&from),
                    to,
                    cause: cause.text,
                    actor: cause.actor,
                },
            )?;
            Ok(Moved::Changed)
        }
        Decision::Refused => {
            refuse(tx, entity, id, &from, to, cause, refusals)?;
            Err(DoorError::Refused {
                entity,
                id: id.to_string(),
                from,
                to: to.to_string(),
                cause: cause.text.to_string(),
                actor: cause.actor,
            })
        }
    }
}

pub fn created(
    tx: &Connection,
    guard: Option<Generation>,
    entity: Entity,
    id: &str,
    cause: Cause<'_>,
) -> Result<(), DoorError> {
    check_open(tx)?;
    check_generation(tx, guard)?;
    let state = state_of(tx, entity, id)?;
    door::append_event(
        tx,
        &Event {
            entity,
            id,
            from: None,
            to: &state,
            cause: cause.text,
            actor: cause.actor,
        },
    )?;
    Ok(())
}

pub fn record(
    tx: &Connection,
    guard: Option<Generation>,
    kind: EventKind,
    entity: Entity,
    id: &str,
    cause: Cause<'_>,
    data: &serde_json::Value,
) -> Result<(), DoorError> {
    check_open(tx)?;
    check_generation(tx, guard)?;
    state_of(tx, entity, id)?;
    let data_json = data.to_string();
    door::append_fact(
        tx,
        &Fact {
            kind,
            entity,
            id,
            cause: cause.text,
            actor: cause.actor,
            data_json: &data_json,
        },
    )?;
    Ok(())
}

fn check_open(tx: &Connection) -> Result<(), DoorError> {
    if tx.is_autocommit() {
        return Err(DoorError::NoTransaction);
    }
    Ok(())
}

fn check_generation(tx: &Connection, guard: Option<Generation>) -> Result<(), DoorError> {
    let Some(held) = guard else {
        return Ok(());
    };
    let found = door::generation(tx)?;
    if found != Some(held) {
        return Err(DoorError::GenerationLost { held, found });
    }
    Ok(())
}

fn decide_move(
    tx: &Connection,
    entity: Entity,
    id: &str,
    from: &str,
    to: &str,
) -> anyhow::Result<Decision> {
    let decision = decide(entity, from, to);
    if decision == Decision::Allowed
        && is_resume(entity, from, to)
        && door::left_state(tx, entity, id, from)?.as_deref() != Some(to)
    {
        return Ok(Decision::Refused);
    }
    Ok(decision)
}

fn state_of(tx: &Connection, entity: Entity, id: &str) -> Result<String, DoorError> {
    door::state_of(tx, entity, id)?.ok_or_else(|| DoorError::NoRow {
        entity,
        id: id.to_string(),
    })
}

pub fn record_refused(
    tx: &Connection,
    err: &DoorError,
    refusals: &dyn RecordRefusal,
) -> anyhow::Result<bool> {
    let DoorError::Refused {
        entity,
        id,
        from,
        to,
        cause,
        actor,
    } = err
    else {
        return Ok(false);
    };
    if door::state_of(tx, *entity, id)?.as_deref() != Some(from.as_str()) {
        return Ok(false);
    }
    let cause = Cause {
        actor: *actor,
        text: cause,
    };
    refuse(tx, *entity, id, from, to, cause, refusals)?;
    Ok(true)
}

fn refuse(
    tx: &Connection,
    entity: Entity,
    id: &str,
    from: &str,
    to: &str,
    cause: Cause<'_>,
    refusals: &dyn RecordRefusal,
) -> anyhow::Result<()> {
    let why = refused_cause(cause.text);
    door::append_event(
        tx,
        &Event {
            entity,
            id,
            from: Some(from),
            to,
            cause: &why,
            actor: cause.actor,
        },
    )?;
    let text = format!(
        "The ledger refused {entity} {id}: {from} -> {to} ({}). Read the events of the row.",
        cause.text
    );
    refusals.record_refusal(tx, entity, id, &text)
}
