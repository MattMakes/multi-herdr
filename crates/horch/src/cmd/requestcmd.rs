//! `horch request <request_id>`: the state and the result of 1 request
//! (RR-8). It reads the `requests` row read-only and changes nothing, so it
//! has no request kind. A request file that fleetd did not read yet waits
//! in `<fleet root>/run/requests/`.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::fleetd::fleet_runtime::adapters::scans::requests_dir;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::open_existing_read_only;
use rusqlite::OptionalExtension;
use serde_json::{Map, Value};

#[derive(Args, Debug)]
pub struct RequestArgs {
    /// The `request_id` that the command sent.
    pub request_id: String,
    /// Print the `requests` row as JSON.
    #[arg(long)]
    pub json: bool,
}

/// The columns of a `requests` row, in the order of the table.
const COLUMNS: [&str; 12] = [
    "request_id",
    "kind",
    "payload_json",
    "payload_sha256",
    "caller",
    "class",
    "auth",
    "state",
    "result_json",
    "reason",
    "received_at",
    "decided_at",
];

pub fn run(ctx: &RuntimeContext, args: RequestArgs) -> Result<ExitCode> {
    let id = &args.request_id;
    if let Some(row) = read_row(ctx, id)? {
        if args.json {
            println!("{}", Value::Object(row));
        } else {
            print!("{}", describe(id, &row));
        }
        return Ok(ExitCode::SUCCESS);
    }
    if requests_dir(&ctx.paths.fleet_root)
        .join(format!("{id}.json"))
        .is_file()
    {
        println!("request {id}: waits for fleetd");
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!("no request {id}");
    Ok(ExitCode::from(1))
}

/// The `requests` row of `id`, column by column. No ledger has no row.
fn read_row(ctx: &RuntimeContext, id: &str) -> Result<Option<Map<String, Value>>> {
    let Some(store) = open_existing_read_only(ctx)? else {
        return Ok(None);
    };
    let sql = format!(
        "SELECT {} FROM requests WHERE request_id = ?1",
        COLUMNS.join(", ")
    );
    let row = store
        .conn()
        .query_row(&sql, [id], |r| {
            let mut row = Map::new();
            for (i, name) in COLUMNS.iter().enumerate() {
                let v: Option<String> = r.get(i)?;
                row.insert((*name).to_string(), v.map_or(Value::Null, Value::String));
            }
            Ok(row)
        })
        .optional()?;
    Ok(row)
}

/// The text form: the state, then the result or the reason, then the kind,
/// the class, the caller and the `auth` text. A missing value prints `-`.
fn describe(id: &str, row: &Map<String, Value>) -> String {
    let text = |name: &str| row.get(name).and_then(Value::as_str).unwrap_or("-");
    let mut out = format!("request {id}: {}\n", text("state"));
    if let Some(result) = row.get("result_json").and_then(Value::as_str) {
        out.push_str(&format!("result: {result}\n"));
    }
    if let Some(reason) = row.get("reason").and_then(Value::as_str) {
        out.push_str(&format!("reason: {reason}\n"));
    }
    for name in ["kind", "class", "caller", "auth"] {
        out.push_str(&format!("{name}: {}\n", text(name)));
    }
    out
}
