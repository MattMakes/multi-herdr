//! `horch sessions` and `horch ledger` - the port of `bin/horch-ledger` and
//! `bin/horch-sessions`.
//!
//! The `ledger` subcommands mirror the bash CLI one for one; they are the
//! plumbing `spawn`, `note`, `done` and `assign` are built on, kept exposed for
//! inspection and debugging.

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use horch_core::ledger::Ledger;

use crate::output;

#[derive(Subcommand)]
pub enum LedgerCommand {
    /// Record a freshly spawned session.
    Add {
        #[arg(long)]
        record_id: String,
        #[arg(long)]
        agent: String,
        #[arg(long)]
        tier: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        role: String,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        task: Option<String>,
    },
    /// Attach a session id discovered after launch.
    SetSession { key: String, session_id: String },
    /// Exit 0 when any record already claims this session id.
    HasSession { session_id: String },
    /// Re-open a finished session under a role.
    Resume {
        key: String,
        role: String,
        task: Option<String>,
    },
    /// Point the live worker for a role at a new task.
    Assign {
        #[arg(long)]
        role: String,
        task: String,
    },
    Note { key: String, text: String },
    Done { key: String, summary: String },
    /// Print one record as JSON.
    Get { key: String },
    List {
        #[arg(long)]
        json: bool,
    },
    /// Print the ledger file path.
    Path,
}

/// `horch sessions` - the orchestrator-facing ledger view.
///
/// Read this before spawning: a `[done]` session whose task or history overlaps
/// the new task is a resume candidate
/// (`horch spawn --resume <session-or-record-id> "task"`).
pub fn sessions(json: bool) -> Result<()> {
    print_list(json)
}

fn print_list(json: bool) -> Result<()> {
    let ledger = Ledger::open()?;
    if json {
        output::println(&serde_json::to_string_pretty(&ledger.read()?)?);
    } else {
        output::print(&ledger.render()?);
    }
    Ok(())
}

pub fn run(command: LedgerCommand) -> Result<ExitCode> {
    let ledger = Ledger::open()?;
    match command {
        LedgerCommand::Add {
            record_id,
            agent,
            tier,
            model,
            role,
            session_id,
            task,
        } => ledger.add(
            &record_id,
            &agent,
            &tier,
            &model,
            &role,
            session_id.as_deref(),
            task.as_deref().unwrap_or_default(),
        )?,
        LedgerCommand::SetSession { key, session_id } => ledger.set_session(&key, &session_id)?,
        LedgerCommand::HasSession { session_id } => {
            return Ok(if ledger.has_session(&session_id)? {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        LedgerCommand::Resume { key, role, task } => {
            ledger.resume(&key, &role, task.as_deref().unwrap_or_default())?
        }
        LedgerCommand::Assign { role, task } => ledger.assign(&role, &task)?,
        LedgerCommand::Note { key, text } => ledger.note(&key, &text)?,
        LedgerCommand::Done { key, summary } => ledger.done(&key, &summary)?,
        LedgerCommand::Get { key } => {
            output::println(&serde_json::to_string_pretty(&ledger.get(&key)?)?)
        }
        LedgerCommand::List { json } => print_list(json)?,
        LedgerCommand::Path => output::println(&ledger.path().display().to_string()),
    }
    Ok(ExitCode::SUCCESS)
}
