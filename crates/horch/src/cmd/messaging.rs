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
use horch_core::message;
use horch_core::messaging::delivery;
use horch_core::runtime::RuntimeContext;

use crate::output;

/// Deliver `text` into the pane registered as `role`.
///
/// From a worker (`HORCH_ROLE` set) the message always starts with the worker's
/// `[<role>]` tag: an untagged line reads as the human operator. A message over
/// [`message::MAX_INLINE`] chars is written to a file, and the pane gets one
/// short line that quotes its head and names the file.
pub fn tell(ctx: &RuntimeContext, role: &str, text: &str) -> Result<()> {
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let mailbox = Mailbox::resolve_in(&herdr, ctx)
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
    let sender = ctx.worker.as_ref().and_then(|w| w.role.clone());
    let text = match &sender {
        Some(me) => message::ensure_tag(text, me),
        None => text.to_string(),
    };
    let line = if text.chars().count() > message::MAX_INLINE {
        let from = sender.as_deref().unwrap_or("orchestrator");
        let path = message::spool(
            &message::spool_dir(&ctx.paths.state_root),
            from,
            role,
            &text,
        )?;
        message::reference_line(&text, &path)
    } else {
        text
    };
    delivery::send_line(&herdr, &target, &line)
}

/// List roles registered - and so reachable via `horch tell` - in this workspace.
pub fn inbox(ctx: &RuntimeContext) -> Result<()> {
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let mailbox = Mailbox::resolve_in(&herdr, ctx)?;
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
pub fn assign(ctx: &RuntimeContext, role: &str, task: &str) -> Result<()> {
    Ledger::open_in(ctx)?.assign(role, task)?;
    tell(ctx, role, task)
}

/// Worker-facing: append a progress note to this worker's own ledger record, so
/// the session history shows what it is doing, not just start and end states.
pub fn note(ctx: &RuntimeContext, text: &str) -> Result<()> {
    let record_id = require_env("HORCH_RECORD_ID", worker_var(ctx, |w| &w.record_id))?;
    Ledger::open_in(ctx)?.note(&record_id, text)
}

/// Worker-facing self-shutdown, run only when the work is truly complete (not
/// while waiting on a question).
///
/// Ordering matters: the ledger write, the DONE report and the mailbox cleanup all
/// happen BEFORE the pane close, because closing the pane kills this very process
/// tree. The underlying agent session stays on disk and resumable.
pub fn done(ctx: &RuntimeContext, summary: &str) -> Result<()> {
    let record_id = require_env("HORCH_RECORD_ID", worker_var(ctx, |w| &w.record_id))?;
    let role = require_env("HORCH_ROLE", worker_var(ctx, |w| &w.role))?;
    let pane_env = require_env(
        "HERDR_PANE_ID",
        ctx.herdr.pane.as_ref().map(|p| p.to_string()),
    )
    .context("horch done must run inside a herdr pane")?;

    // `done` adds the tag and keyword itself; a summary that repeats them would
    // arrive as `[r] DONE: [r] DONE: ...`.
    let summary = message::strip_done_prefix(summary, &role);
    Ledger::open_in(ctx)?.done(&record_id, summary)?;

    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    if let Err(e) = tell(ctx, "orchestrator", &format!("[{role}] DONE: {summary}")) {
        eprintln!(
            "horch done: could not reach orchestrator ({e:#}); ledger is updated, \
             shutting down anyway"
        );
    }

    let pane = herdr.pane_get(&pane_env)?;
    let workspace = ctx
        .herdr
        .workspace
        .as_ref()
        .map(|w| w.to_string())
        .or_else(|| pane.workspace_id.clone())
        .context("could not resolve this pane's workspace")?;

    Mailbox::in_context(ctx, &workspace).unregister(&role);

    // A departing worker leaves a hole and hands its width to whichever neighbour
    // happens to be its split sibling, which lumps the grid rather than spreading
    // it. Hand the tidy to a detached child: the close below kills this process
    // tree, so by the time the hole exists, this process is gone. The child pulls
    // the last worker into the free slot, and an overflow tab that lost its last
    // worker closes itself.
    crate::cmd::tilecmd::settle_after_close(ctx, &workspace);

    herdr.pane_close(&pane.pane_id)
}

/// Record this pane's public id under `role` so other panes can address it.
pub fn register(ctx: &mut RuntimeContext, role: &str) -> Result<()> {
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let (mailbox, pane_id) = Mailbox::register_in(&herdr, ctx, role)?;
    println!(
        "registered {role} as {pane_id} in {}",
        mailbox.dir().display()
    );
    Ok(())
}

/// One worker variable from the context, when it is set.
fn worker_var(
    ctx: &RuntimeContext,
    field: impl Fn(&horch_core::runtime::WorkerEnv) -> &Option<String>,
) -> Option<String> {
    ctx.worker.as_ref().and_then(|w| field(w).clone())
}

/// A required environment value, with a message naming what sets it.
fn require_env(key: &str, value: Option<String>) -> Result<String> {
    value.filter(|v| !v.is_empty()).with_context(|| {
        format!("{key} is unset (this command runs inside a `horch spawn` worker pane)")
    })
}
