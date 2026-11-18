//! The use of `transition()`: 1 state change and its event in 1 transaction.
//! `record()` writes 1 event of a row that does not change.

use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::sqlite::{self as door, Event, Fact};
use crate::fleetd::fleet_runtime::domain::ledger::{
    decide, refused_cause, Decision, Entity, Generation,
};
use crate::fleetd::task_execution::domain::lease::Holder;
use crate::taskledger::model::{Actor, EventKind};

/// Where [`transition`] puts a refused row: the needs-you list. The ideas
/// context implements it, so this context does not depend on ideas.
pub trait RecordRefusal {
    /// Open the needs-you item of the refused row `id` of `entity`, with
    /// `text`, in `tx`.
    fn record_refusal(
        &self,
        tx: &Connection,
        entity: Entity,
        id: &str,
        text: &str,
    ) -> anyhow::Result<()>;
}

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
    /// so the caller goes on and commits. A caller that rolls back records
    /// it again with [`record_refused`]: `cause` and `actor` are the cause
    /// of the refused move.
    Refused {
        entity: Entity,
        id: String,
        from: String,
        to: String,
        cause: String,
        actor: Actor,
    },
    /// The `fleetd` row has another generation. Nothing is written.
    GenerationLost {
        held: Generation,
        found: Option<Generation>,
    },
    /// No row of the entity has the id.
    NoRow { entity: Entity, id: String },
    /// The connection has no open transaction, so a write of the door
    /// cannot go with the other writes of its caller. Nothing is written.
    NoTransaction,
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

/// Move the row `id` of `entity` to `to`, in the caller's transaction
/// `tx`. A connection with no open transaction gives
/// [`DoorError::NoTransaction`]. An allowed move writes the row and 1 event. A refused move writes
/// no row change, 1 event and 1 needs-you item, and returns
/// [`DoorError::Refused`]. With `Some(guard)`, a `fleetd` row of another
/// generation gives [`DoorError::GenerationLost`] and writes nothing. Only
/// fleetd passes a guard. `refusals` opens the needs-you item of a refused
/// move.
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
    match decide(entity, &from, to) {
        Decision::Unchanged => Ok(Moved::Unchanged),
        Decision::Allowed => {
            door::set_state(tx, entity, id, to)?;
            // A task in a state that holds no lease has no lease row (I1,
            // FM-14): each exit releases it here, so no command forgets it.
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

/// Record the new row `id` of `entity`, which the caller inserted in `tx`:
/// 1 event from no state to the state of the row. `guard` is as in
/// [`transition`].
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

/// Record 1 event of kind `kind` about the row `id` of `entity`, with
/// `data` in `data_json`, in the caller's transaction `tx`. The row does
/// not change. A connection with no open transaction gives
/// [`DoorError::NoTransaction`], and a missing row gives
/// [`DoorError::NoRow`]. With `Some(guard)`, a `fleetd` row of another
/// generation gives [`DoorError::GenerationLost`]. Each error writes
/// nothing.
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

/// [`DoorError::NoTransaction`] when `tx` is in autocommit mode.
fn check_open(tx: &Connection) -> Result<(), DoorError> {
    if tx.is_autocommit() {
        return Err(DoorError::NoTransaction);
    }
    Ok(())
}

/// [`DoorError::GenerationLost`] when `guard` is not the generation of the
/// `fleetd` row.
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

/// The state of the row `id` of `entity`, or [`DoorError::NoRow`].
fn state_of(tx: &Connection, entity: Entity, id: &str) -> Result<String, DoorError> {
    door::state_of(tx, entity, id)?.ok_or_else(|| DoorError::NoRow {
        entity,
        id: id.to_string(),
    })
}

/// Record the refused move of `err` in `tx`, as [`transition`] does: for a
/// caller whose transaction rolled back. `false`, with no write, when `err`
/// is not [`DoorError::Refused`] or the row is no longer in its `from`
/// state.
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
    // Another writer can move the row between the rollback and `tx`. The
    // event then names a state that the row no longer has, so it is not
    // written (review F4).
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

/// Record a refused move: 1 event with the refused state, and the open
/// needs-you item of the row.
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
