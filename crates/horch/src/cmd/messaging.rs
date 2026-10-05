//! The channel between panes: `tell`, `inbox`, `assign`, `note`, `done`.
//!
//! Ports `bin/herdr-tell`, `bin/herdr-inbox`, `bin/horch-assign`,
//! `bin/horch-note`, `bin/horch-done` and `scripts/lib/herdr-register.sh`.
//!
//! There is no AI-native agent-to-agent protocol here: a message is literally
//! typed into the target pane's terminal and submitted, exactly as if someone
//! switched panes and typed it by hand.

use anyhow::{bail, Context, Result};
use horch_core::execution::lifecycle::{self, DoneRequest, DoneSteps, ReportTarget};
use horch_core::execution::records::Ledger;
use horch_core::execution::store::to_execution;
use horch_core::execution::ExecutionKind;
use horch_core::harness::launch;
use horch_core::messaging::delivery;
use horch_core::messaging::mailbox::{Mailbox, PaneState, RoleEntry};
use horch_core::messaging::message;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::arrange;
use horch_core::workspace::herdr::Herdr;

use crate::output;

/// Deliver `text` into the pane registered as `role`.
///
/// From a worker (`HORCH_ROLE` set) the message always starts with the worker's
/// `[<role>]` tag: an untagged line reads as the human operator. A message over
/// [`message::MAX_INLINE`] chars is written to a file, and the pane gets one
/// short line that quotes its head and names the file. An agent that is
/// still starting in that pane is waited for first; it would drop the text.
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
        let sender = ctx.worker.as_ref().and_then(|w| w.role.as_deref());
        bail!(
            "role '{role}' is not registered in {}\n       known roles: {known}{}",
            mailbox.dir().display(),
            no_orchestrator_hint(role, sender)
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
    // An agent that is still starting drops typed text, so wait it out. A
    // role that registered just now may not show its agent yet.
    let grace = ctx.settings.tell_grace.unwrap_or(delivery::TELL_GRACE);
    let age = mailbox.registered_at(role).and_then(|at| at.elapsed().ok());
    delivery::send_line_when_ready(
        &herdr,
        &target,
        &line,
        grace_left(age, grace),
        &delivery::Readiness::DEFAULT,
        &delivery::Timing::DEFAULT,
    )
}

/// How long `tell` may still wait for an agent to show in a role's pane: the
/// rest of `grace` after the role registered `age` ago. A role registered
/// 29 s ago gets 1 s, not another full `grace`. Zero when the age is unknown.
fn grace_left(age: Option<std::time::Duration>, grace: std::time::Duration) -> std::time::Duration {
    age.map_or(std::time::Duration::ZERO, |age| grace.saturating_sub(age))
}

/// The line a worker's failed `tell` to a missing orchestrator ends with.
/// Without it, a worker reads the failure as a block and never runs `done`.
fn no_orchestrator_hint(role: &str, sender: Option<&str>) -> &'static str {
    if role == "orchestrator" && sender.is_some() {
        "\n       no orchestrator runs in this workspace, so nobody reads reports here.\n       \
         `horch done \"<summary>\"` still records your summary and closes your pane."
    } else {
        ""
    }
}

/// List roles registered in this workspace, and mark each one whose pane
/// herdr no longer has.
///
/// A closed role stays in the list, marked, rather than hidden: its id file
/// is still there, and a reader who sees why `horch tell` to it fails does
/// not have to guess where the role went.
pub fn inbox(ctx: &RuntimeContext) -> Result<()> {
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let mailbox = Mailbox::resolve_in(&herdr, ctx)?;
    let roles = mailbox.role_states(&herdr);
    if roles.is_empty() {
        println!("no roles registered yet");
        return Ok(());
    }
    if roles.iter().any(|r| r.state == PaneState::Unknown) {
        eprintln!("horch inbox: herdr did not answer, so pane states are unknown");
    }
    output::print(&inbox_listing(&roles));
    Ok(())
}

/// One line for each role: name, pane, and `(pane closed)` when herdr does
/// not list the pane.
fn inbox_listing(roles: &[RoleEntry]) -> String {
    let mut listing = String::new();
    for r in roles {
        let mark = match r.state {
            PaneState::Closed => "  (pane closed)",
            PaneState::Open | PaneState::Unknown => "",
        };
        listing.push_str(&format!("{:<12} {}{mark}\n", r.role, r.pane));
    }
    listing
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
/// while waiting on a question). The order of the steps is
/// [`lifecycle::done`]'s.
pub fn done(ctx: &RuntimeContext, summary: &str) -> Result<()> {
    let record_id = require_env("HORCH_RECORD_ID", worker_var(ctx, |w| &w.record_id))?;
    let role = require_env("HORCH_ROLE", worker_var(ctx, |w| &w.role))?;
    let pane_env = require_env(
        "HERDR_PANE_ID",
        ctx.herdr.pane.as_ref().map(|p| p.to_string()),
    )
    .context("horch done must run inside a herdr pane")?;
    let workspace = ctx.herdr.workspace.as_ref().map(|w| w.to_string());

    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let ledger = Ledger::open_in(ctx)?;
    lifecycle::done(
        &herdr,
        &CliDone {
            ctx,
            settle: !coordinator_owns_layout(&ledger, &record_id),
        },
        &DoneRequest {
            record_id: &record_id,
            role: &role,
            pane: &pane_env,
            workspace: workspace.as_deref(),
            summary,
            report_to: report_target(ctx, workspace.as_deref(), &role),
        },
    )
}

/// Who this worker reports to: its brief says. A candidate's brief says
/// nobody. A worker whose brief cannot be read reports to the orchestrator,
/// as every worker did before briefs carried it.
fn report_target(ctx: &RuntimeContext, workspace: Option<&str>, role: &str) -> ReportTarget {
    workspace
        .and_then(|w| Mailbox::in_context(ctx, w).read_brief(role).ok())
        .map_or(ReportTarget::Orchestrator, |b| b.report_to)
}

/// Whether the dataset coordinator owns the layout this execution's pane is
/// in. A candidate runs in the dataset workspace, which the coordinator lays
/// out itself (its `watch` root pane and its candidate panes), and spawns
/// with tiling off. A judge has no pane. Neither settles the grid on `done`.
/// A record that cannot be read or typed settles, as every worker did.
fn coordinator_owns_layout(ledger: &Ledger, record_id: &str) -> bool {
    ledger
        .get(record_id)
        .ok()
        .and_then(|r| to_execution(&r).ok())
        .is_some_and(|e| {
            matches!(
                e.kind,
                ExecutionKind::Candidate { .. } | ExecutionKind::Judge { .. }
            )
        })
}

/// What `done` prints when no orchestrator is registered.
const NO_ORCHESTRATOR: &str = "horch done: no orchestrator is registered in this workspace, \
     so nobody is told; the summary is recorded";

/// The `done` steps against this process's ledger, mailbox and workspace.
struct CliDone<'a> {
    ctx: &'a RuntimeContext,
    /// `false` when the coordinator owns the layout: see
    /// [`coordinator_owns_layout`].
    settle: bool,
}

impl DoneSteps for CliDone<'_> {
    fn mark_done(&self, record_id: &str, summary: &str) -> Result<()> {
        Ledger::open_in(self.ctx)?.done(record_id, summary)
    }

    /// Tell the orchestrator. A workspace with no registered orchestrator
    /// (a scratch workspace, a worker spawned by hand) has nobody to tell:
    /// that is not a failure, so `done` goes on and exits 0.
    fn report(&self, line: &str) -> Result<()> {
        let herdr = Herdr::with_bin(&self.ctx.bins.harness.herdr);
        let registered = Mailbox::resolve_in(&herdr, self.ctx)
            .map(|m| m.pane_for("orchestrator").is_some())
            .unwrap_or(true);
        if !registered {
            eprintln!("{NO_ORCHESTRATOR}");
            return Ok(());
        }
        tell(self.ctx, "orchestrator", line)
    }

    fn unregister(&self, workspace: &str, role: &str) {
        Mailbox::in_context(self.ctx, workspace).unregister(role);
    }

    fn settle(&self, workspace: &str) {
        if self.settle {
            arrange::settle_after_close(self.ctx, workspace);
        }
    }

    fn record_session(&self, workspace: &str, role: &str, record_id: &str) -> Result<()> {
        launch::discover_now(
            self.ctx,
            &Mailbox::in_context(self.ctx, workspace),
            role,
            record_id,
        )
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::execution::legacy::Record;
    use std::time::Duration;

    #[test]
    fn tell_waits_only_the_rest_of_the_grace() {
        let grace = Duration::from_secs(30);
        let left = |s| grace_left(Some(Duration::from_secs(s)), grace);
        assert_eq!(left(0), grace);
        assert_eq!(left(29), Duration::from_secs(1));
        assert_eq!(left(30), Duration::ZERO);
        assert_eq!(left(3600), Duration::ZERO);
        assert_eq!(grace_left(None, grace), Duration::ZERO);
    }

    fn record(
        id: &str,
        experiment: Option<&str>,
        round: Option<&str>,
        label: Option<&str>,
    ) -> Record {
        Record {
            record_id: id.into(),
            agent: "claude".into(),
            tier: "sonnet".into(),
            model: "sonnet".into(),
            role: format!("{id}-1"),
            task: "work".into(),
            experiment_id: experiment.map(Into::into),
            round_id: round.map(Into::into),
            label: label.map(Into::into),
            ..Record::default()
        }
    }

    /// A candidate's or a judge's `done` leaves the dataset workspace's
    /// layout to the coordinator. A fleet worker, and a record that is not
    /// there, settle the grid as before.
    #[test]
    fn done_settles_only_outside_the_coordinator_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let ledger = Ledger::for_project(tmp.path(), "/work/alpha");
        ledger.insert(record("worker", None, None, None)).unwrap();
        ledger
            .insert(record("candidate", Some("e1"), Some("r1"), Some("A")))
            .unwrap();
        ledger
            .insert(record("judge", None, Some("r1"), Some("judge:1")))
            .unwrap();

        assert!(!coordinator_owns_layout(&ledger, "worker"));
        assert!(coordinator_owns_layout(&ledger, "candidate"));
        assert!(coordinator_owns_layout(&ledger, "judge"));
        assert!(!coordinator_owns_layout(&ledger, "missing"));
    }
}
