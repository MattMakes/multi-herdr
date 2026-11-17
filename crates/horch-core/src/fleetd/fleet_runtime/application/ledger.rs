//! The use of `transition()`: 1 state change and its event in 1 transaction.

use rusqlite::Transaction;

use crate::fleetd::fleet_runtime::adapters::sqlite::{self as door, Event};
use crate::fleetd::fleet_runtime::domain::ledger::{
    decide, refused_cause, refused_key, Decision, Entity, Generation, REFUSED_KIND,
};
use crate::fleetd::ideas::application::needs_you;
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::taskledger::model::Actor;

/// Why a row moves, and who moves it.
#[derive(Debug, Clone, Copy)]
pub struct Cause<'a> {
    pub actor: Actor,
    pub text: &'a str,
}

/// What [`transition`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moved {
    /// The row is in the new state, and 1 event says so.
    Changed,
    /// The row was already in that state: no write and no event.
    Unchanged,
}

/// Why [`transition`] did not move the row.
#[derive(Debug)]
pub enum DoorError {
    /// The table does not allow the move. The row did not change. 1 event
    /// and 1 needs-you item record the refusal in the caller's transaction,
    /// so the caller goes on and commits.
    Refused {
        entity: Entity,
        id: String,
        from: String,
        to: String,
    },
    /// The `fleetd` row has another generation. Nothing is written.
    GenerationLost {
        held: Generation,
        found: Option<Generation>,
    },
    /// No row of the entity has the id.
    NoRow { entity: Entity, id: String },
    /// The ledger failed.
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
            } => write!(f, "illegal transition of {entity} {id}: {from} -> {to}"),
            DoorError::GenerationLost { held, found } => write!(
                f,
                "the fleetd generation is {:?}, not {}: another fleetd holds the ledger",
                found.map(|g| g.0),
                held.0
            ),
            DoorError::NoRow { entity, id } => write!(f, "no {entity} {id} in the ledger"),
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

/// Move the row `id` of `entity` to `to`, in the caller's transaction
/// `tx`. An allowed move writes the row and 1 event. A refused move writes
/// no row change, 1 event and 1 needs-you item, and returns
/// [`DoorError::Refused`]. With `Some(guard)`, a `fleetd` row of another
/// generation gives [`DoorError::GenerationLost`] and writes nothing. Only
/// fleetd passes a guard.
pub fn transition(
    tx: &Transaction<'_>,
    guard: Option<Generation>,
    entity: Entity,
    id: &str,
    to: &str,
    cause: Cause<'_>,
) -> Result<Moved, DoorError> {
    check_generation(tx, guard)?;
    let from = state_of(tx, entity, id)?;
    match decide(entity, &from, to) {
        Decision::Unchanged => Ok(Moved::Unchanged),
        Decision::Allowed => {
            door::set_state(tx, entity, id, to)?;
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
            refuse(tx, entity, id, &from, to, cause)?;
            Err(DoorError::Refused {
                entity,
                id: id.to_string(),
                from,
                to: to.to_string(),
            })
        }
    }
}

/// Record the new row `id` of `entity`, which the caller inserted in `tx`:
/// 1 event from no state to the state of the row. `guard` is as in
/// [`transition`].
pub fn created(
    tx: &Transaction<'_>,
    guard: Option<Generation>,
    entity: Entity,
    id: &str,
    cause: Cause<'_>,
) -> Result<(), DoorError> {
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

/// [`DoorError::GenerationLost`] when `guard` is not the generation of the
/// `fleetd` row.
fn check_generation(tx: &Transaction<'_>, guard: Option<Generation>) -> Result<(), DoorError> {
    let Some(held) = guard else {
        return Ok(());
    };
    let found = door::generation(tx)?;
    if found != Some(held) {
        return Err(DoorError::GenerationLost { held, found });
    }
    Ok(())
}

/// The state of the row `id` of `entity`, or [`DoorError::NoRow`].
fn state_of(tx: &Transaction<'_>, entity: Entity, id: &str) -> Result<String, DoorError> {
    door::state_of(tx, entity, id)?.ok_or_else(|| DoorError::NoRow {
        entity,
        id: id.to_string(),
    })
}

/// Record a refused move: 1 event with the refused state, and the open
/// needs-you item of the row.
fn refuse(
    tx: &Transaction<'_>,
    entity: Entity,
    id: &str,
    from: &str,
    to: &str,
    cause: Cause<'_>,
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
    let key = refused_key(entity, id);
    let text = format!(
        "The ledger refused {entity} {id}: {from} -> {to} ({}). Read the events of the row.",
        cause.text
    );
    needs_you::open_item(
        tx,
        &Write {
            key: &key,
            kind: REFUSED_KIND,
            entity: entity.as_str(),
            entity_id: id,
            text: &text,
        },
    )
}
