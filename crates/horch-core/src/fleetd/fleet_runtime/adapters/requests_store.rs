use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::fleetd::fleet_runtime::adapters::request_wire::Request;
use crate::taskledger::util;

pub const UNKNOWN_CALLER: &str = "(unknown)";

#[derive(Clone, Debug, PartialEq)]
pub enum Stored {
    Pending,
    Applied(Value),
    Refused(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub payload_sha256: String,
    pub stored: Stored,
}

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

pub fn set_applied(conn: &Connection, request_id: &str, auth: &str, result: &Value) -> Result<()> {
    decide(
        conn,
        request_id,
        "UPDATE requests SET state = 'applied', auth = ?2, result_json = ?3, decided_at = ?4
         WHERE request_id = ?1 AND state = 'pending'",
        params![request_id, auth, result.to_string(), util::now()],
    )
}

pub fn set_refused(conn: &Connection, request_id: &str, reason: &str) -> Result<()> {
    decide(
        conn,
        request_id,
        "UPDATE requests SET state = 'refused', auth = NULL, reason = ?2, decided_at = ?3
         WHERE request_id = ?1 AND state = 'pending'",
        params![request_id, reason, util::now()],
    )
}

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
