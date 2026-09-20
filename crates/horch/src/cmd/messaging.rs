//! The channel between panes: `tell`, `inbox`, `assign`, `note`, `done`.
//!
//! Ports `bin/herdr-tell`, `bin/herdr-inbox`, `bin/horch-assign`,
//! `bin/horch-note`, `bin/horch-done` and `scripts/lib/herdr-register.sh`.
//!
//! There is no AI-native agent-to-agent protocol here: a message is literally
//! typed into the target pane's terminal and submitted, exactly as if someone
//! switched panes and typed it by hand.

use anyhow::{bail, Context, Result};
use horch_core::herdr::Herdr;
use horch_core::ledger::Ledger;
use horch_core::mailbox::Mailbox;

use crate::output;

/// Deliver `message` into the pane registered as `role`.
pub fn tell(role: &str, message: &str) -> Result<()> {
    let herdr = Herdr::new();
    let mailbox = Mailbox::resolve(&herdr)
        .context("horch tell must run inside a herdr pane, or with HORCH_WORKSPACE_ID set")?;

    let Some(target) = mailbox.pane_for(role) else {
        let known: Vec<String> = mailbox.roles().into_iter().map(|(r, _)| r).collect();
        let known = if known.is_empty() {
            "none yet".to_string()
        } else {
            known.join(" ")
        };
        bail!(
            "role '{role}' is not registered in {}\n       known roles: {known}",
            mailbox.dir().display()
        );
    };
    herdr.send_line(&target, message)
}

/// List roles registered - and so reachable via `horch tell` - in this workspace.
pub fn inbox() -> Result<()> {
    let herdr = Herdr::new();
    let mailbox = Mailbox::resolve(&herdr)?;
    if !mailbox.exists() {
        println!("no roles registered yet");
        return Ok(());
    }
    let roles = mailbox.roles();
    if roles.is_empty() {
        println!("no roles registered yet");
        return Ok(());
    }
    let mut listing = String::new();
    for (role, pane) in roles {
        listing.push_str(&format!("{role:<12} {pane}\n"));
    }
    output::print(&listing);
    Ok(())
}

/// Orchestrator-facing: give a task to a LIVE worker.
///
/// Records the task on that worker's ledger record, so the session history
/// reflects what it is doing, then delivers it into the worker's terminal. For a
/// worker that does not exist yet, use `horch spawn`.
pub fn assign(role: &str, task: &str) -> Result<()> {
    Ledger::open()?.assign(role, task)?;
    tell(role, task)
}

/// Worker-facing: append a progress note to this worker's own ledger record, so
/// the session history shows what it is doing, not just start and end states.
pub fn note(text: &str) -> Result<()> {
    let record_id = require_env("HORCH_RECORD_ID")?;
    Ledger::open()?.note(&record_id, text)
}

/// Worker-facing self-shutdown, run only when the work is truly complete (not
/// while waiting on a question).
///
/// Ordering matters: the ledger write, the DONE report and the mailbox cleanup all
/// happen BEFORE the pane close, because closing the pane kills this very process
/// tree. The underlying agent session stays on disk and resumable.
pub fn done(summary: &str) -> Result<()> {
    let record_id = require_env("HORCH_RECORD_ID")?;
    let role = require_env("HORCH_ROLE")?;
    let pane_env = require_env("HERDR_PANE_ID")
        .context("horch done must run inside a herdr pane")?;

    Ledger::open()?.done(&record_id, summary)?;

    let herdr = Herdr::new();
    if let Err(e) = tell("orchestrator", &format!("[{role}] DONE: {summary}")) {
        eprintln!(
            "horch done: could not reach orchestrator ({e:#}); ledger is updated, \
             shutting down anyway"
        );
    }

    let pane = herdr.pane_get(&pane_env)?;
    let workspace = std::env::var("HORCH_WORKSPACE_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| pane.workspace_id.clone())
        .context("could not resolve this pane's workspace")?;

    Mailbox::new(&workspace).unregister(&role);

    // A departing worker leaves a hole and hands its width to whichever neighbour
    // happens to be its split sibling, which lumps the grid rather than spreading
    // it. Hand the tidy to a detached child: the close below kills this process
    // tree, so by the time the hole exists, this process is gone. The child pulls
    // the last worker into the free slot, and an overflow tab that lost its last
    // worker closes itself.
    crate::cmd::tilecmd::settle_after_close(&workspace);

    herdr.pane_close(&pane.pane_id)
}

/// Record this pane's public id under `role` so other panes can address it.
pub fn register(role: &str) -> Result<()> {
    let herdr = Herdr::new();
    let (mailbox, pane_id) = Mailbox::register(&herdr, role)?;
    println!(
        "registered {role} as {pane_id} in {}",
        mailbox.dir().display()
    );
    Ok(())
}

/// Read a required environment variable, with a message naming what sets it.
fn require_env(key: &str) -> Result<String> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .with_context(|| {
            format!("{key} is unset (this command runs inside a `horch spawn` worker pane)")
        })
}
