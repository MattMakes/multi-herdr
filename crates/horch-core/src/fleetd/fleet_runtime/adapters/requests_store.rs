//! The `requests` rows in SQLite: insert a pending row, record its decision
//! and its result, and read the stored result of a seen `request_id`.
//!
//! Requirements: RR-8, SR-8, FM-15. Ticket 12 of fleetd-p5 fills it.
//!
//! Each function writes in the transaction of its caller. The `requests`
//! row is not an entity of `transition()`: the design lists no state table
//! for it. A row leaves `pending` only once, to `applied` or `refused`.

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::fleetd::fleet_runtime::adapters::request_wire::Request;
use crate::taskledger::util;

/// The text of the `caller` column when the fact of the caller is unknown.
/// A user name never has parentheses.
pub const UNKNOWN_CALLER: &str = "(unknown)";

/// The state of a `requests` row and what it stores.
#[derive(Clone, Debug, PartialEq)]
pub enum Stored {
    /// Received, not yet applied or refused.
    Pending,
    /// Applied, with its result.
    Applied(Value),
    /// Refused, with its reason.
    Refused(String),
}

/// The `requests` row of 1 `request_id`: the hash of its payload and its
/// state.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub payload_sha256: String,
    pub stored: Stored,
}

/// The row of `request_id`, or `None` when fleetd never read it.
pub fn get(conn: &Connection, request_id: &str) -> Result<Option<Row>> {
    let found = conn
        .query_row(
            "SELECT payload_sha256, state, result_json, reason FROM requests
             WHERE request_id = ?1",
            [request_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .with_context(|| format!("reading request {request_id}"))?;
    let Some((payload_sha256, state, result, reason)) = found else {
        return Ok(None);
    };
    let stored = match (state.as_str(), result, reason) {
        ("pending", _, _) => Stored::Pending,
        ("applied", Some(result), _) => Stored::Applied(
            serde_json::from_str(&result)
                .with_context(|| format!("the stored result of request {request_id}"))?,
        ),
        ("refused", _, Some(reason)) => Stored::Refused(reason),
        (state, ..) => bail!("request {request_id} is {state} with no stored result or reason"),
    };
    Ok(Some(Row {
        payload_sha256,
        stored,
    }))
}

/// Insert the `pending` row of `request`, from `caller`: the peer user, the
/// owner of the request file, or [`UNKNOWN_CALLER`].
pub fn insert_pending(conn: &Connection, request: &Request, caller: &str) -> Result<()> {
    let id = request.request_id.as_str();
    conn.execute(
        "INSERT INTO requests (request_id, kind, payload_json, payload_sha256, caller, class,
                               state, received_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
        params![
            id,
            request.kind.as_str(),
            Value::Object(request.payload.clone()).to_string(),
            request.payload_sha256(),
            caller,
            request.kind.class().as_str(),
            util::now(),
        ],
    )
    .with_context(|| format!("recording request {id}"))?;
    Ok(())
}

/// Move the pending row of `request_id` to `applied`, with `auth` and
/// `result`.
pub fn set_applied(conn: &Connection, request_id: &str, auth: &str, result: &Value) -> Result<()> {
    decide(
        conn,
        request_id,
        "UPDATE requests SET state = 'applied', auth = ?2, result_json = ?3, decided_at = ?4
         WHERE request_id = ?1 AND state = 'pending'",
        params![request_id, auth, result.to_string(), util::now()],
    )
}

/// Move the pending row of `request_id` to `refused`, with `reason` and no
/// `auth`.
pub fn set_refused(conn: &Connection, request_id: &str, reason: &str) -> Result<()> {
    decide(
        conn,
        request_id,
        "UPDATE requests SET state = 'refused', auth = NULL, reason = ?2, decided_at = ?3
         WHERE request_id = ?1 AND state = 'pending'",
        params![request_id, reason, util::now()],
    )
}

/// Run the update `sql` of the decision of `request_id`. It fails when no
/// pending row has the id, so a decided row never changes.
fn decide(
    conn: &Connection,
    request_id: &str,
    sql: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<()> {
    let changed = conn
        .execute(sql, args)
        .with_context(|| format!("recording the decision of request {request_id}"))?;
    if changed != 1 {
        bail!("request {request_id} has no pending row");
    }
    Ok(())
}
