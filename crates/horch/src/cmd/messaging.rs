//! The channel between panes: `tell`, `inbox`, `assign`, `note`, `done`.
//!
//! Ports `bin/herdr-tell`, `bin/herdr-inbox`, `bin/horch-assign`,
//! `bin/horch-note`, `bin/horch-done` and `scripts/lib/herdr-register.sh`.
//!
//! There is no AI-native agent-to-agent protocol here: a message is literally
//! typed into the target pane's terminal and submitted, exactly as if someone
//! switched panes and typed it by hand.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use horch_core::compaction::policy;
use horch_core::competition::budget::UsageMeter;
use horch_core::competition::observe::TelemetryUsage;
use horch_core::execution::lifecycle::{
    self, ClaimLine, DoneRequest, DoneSteps, ReportStatus, ReportTarget,
};
use horch_core::execution::records::{Ledger, EVENT_REPORT_FAILED, EVENT_REPORT_QUEUED};
use horch_core::execution::store::to_execution;
use horch_core::execution::ExecutionKind;
use horch_core::fleet_runs::facts::{record_run, RunSources};
use horch_core::fleet_runs::rows::WrittenBy;
use horch_core::harness::launch;
use horch_core::measure::paths::DatasetPaths;
use horch_core::messaging::channel;
use horch_core::messaging::delivery;
use horch_core::messaging::mail::Mail;
use horch_core::messaging::mailbox::{Mailbox, PaneState, RoleEntry};
use horch_core::messaging::message;
use horch_core::prompts;
use horch_core::runtime::RuntimeContext;
use horch_core::usage::Locations;
use horch_core::vcs::git::GitCli;
use horch_core::vcs::git::GitClient;
use horch_core::vcs::landing::Landing;
use horch_core::workspace::arrange;
use horch_core::workspace::herdr::Herdr;

use super::context::{self, Sources};
use crate::output;

/// Deliver `text` into the pane registered as `role`.
///
/// From a worker (`HORCH_ROLE` set) the message always starts with the worker's
/// `[<role>]` tag: an untagged line reads as the human operator. A message over
/// [`message::MAX_INLINE`] chars is written to a file, and the pane gets one
/// short line that quotes its head and names the file. An agent that is
/// still starting in that pane is waited for first; it would drop the text.
pub fn tell(ctx: &RuntimeContext, role: &str, text: &str) -> Result<()> {
    match tell_outcome(ctx, role, text)? {
        ReportStatus::Delivered => Ok(()),
        ReportStatus::Queued(cause) => {
            eprintln!("horch tell: the push did not land ({cause}); the courier will redeliver it");
            Ok(())
        }
    }
}

fn tell_outcome(ctx: &RuntimeContext, role: &str, text: &str) -> Result<ReportStatus> {
    let sender = ctx.worker.as_ref().and_then(|w| w.role.clone());
    tell_as(ctx, sender, role, text)
}

/// [`tell`] with an explicit sender tag. horch itself sends as `horch` (the
/// task ledger's wakes): a tagged message to the orchestrator is durable
/// mail, redelivered until `horch inbox` acknowledges it.
pub fn tell_as(
    ctx: &RuntimeContext,
    sender: Option<String>,
    role: &str,
    text: &str,
) -> Result<ReportStatus> {
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let mailbox = Mailbox::resolve_in(&herdr, ctx)
        .context("horch tell must run inside a herdr pane, or with HORCH_WORKSPACE_ID set")?;

    let text = match &sender {
        Some(me) => message::ensure_tag(text, me),
        None => text.to_string(),
    };
    let line = if text.chars().count() > message::MAX_INLINE {
        let from = sender.as_deref().unwrap_or("orchestrator");
        let workspace = mailbox.workspace_id();
        let path = message::spool(
            &message::spool_dir(&ctx.paths.state_root, workspace),
            workspace,
            from,
            role,
            &text,
        )?;
        message::reference_line(&text, &path)
    } else {
        text
    };
    let target = mailbox.pane_for(role);
    let socket = channel::socket_path(&ctx.paths.state_root, mailbox.workspace_id());
    let durable = role == "orchestrator" && sender.is_some();
    if durable && target.is_none() && !channel::is_live(&socket) {
        // Nobody to queue for. Skip the relay: one that starts now would take
        // the message with nothing queued behind it. The not-registered error
        // makes `done` stay open and retry.
        push_line(
            ctx,
            &herdr,
            &mailbox,
            role,
            sender.as_deref(),
            target,
            line,
            false,
        )?;
        return Ok(ReportStatus::Delivered);
    }
    if durable {
        Mail::under(&ctx.paths.state_root, mailbox.workspace_id())
            .enqueue(sender.as_deref().expect("durable mail has a sender"), &line)?;
    }
    let pushed = push_line(
        ctx,
        &herdr,
        &mailbox,
        role,
        sender.as_deref(),
        target,
        line,
        true,
    );
    if durable {
        super::mail::start_courier(ctx, mailbox.workspace_id());
        return Ok(match pushed {
            Ok(()) => ReportStatus::Delivered,
            Err(error) => ReportStatus::Queued(format!("{error:#}")),
        });
    }
    pushed?;
    Ok(ReportStatus::Delivered)
}

#[allow(clippy::too_many_arguments)]
fn push_line(
    ctx: &RuntimeContext,
    herdr: &Herdr,
    mailbox: &Mailbox,
    role: &str,
    sender: Option<&str>,
    target: Option<String>,
    mut line: String,
    relay: bool,
) -> Result<()> {
    if role == "orchestrator" && relay {
        let socket = channel::socket_path(&ctx.paths.state_root, mailbox.workspace_id());
        let from = sender.unwrap_or("orchestrator");
        let send_timeout = if cfg!(debug_assertions) {
            std::env::var("HORCH_TEST_CHANNEL_SEND_TIMEOUT_MS")
                .ok()
                .and_then(|value| value.parse().ok())
                .map(std::time::Duration::from_millis)
                .unwrap_or(channel::SEND_TIMEOUT)
        } else {
            channel::SEND_TIMEOUT
        };
        match channel::send(&socket, from, &line, send_timeout) {
            Ok(()) => return Ok(()),
            Err(channel::SendError::NotDelivered(error)) if socket.exists() => eprintln!(
                "horch tell: the channel relay did not take the message ({error}); typing it instead"
            ),
            Err(channel::SendError::Unconfirmed(error)) => {
                eprintln!(
                    "horch tell: the channel relay did not confirm the message ({error}); typing it again with a duplicate warning"
                );
                line.push_str(" (sent again: the channel relay did not confirm)");
            }
            Err(_) => {}
        }
    }
    let Some(target) = target else {
        let known: Vec<String> = mailbox.roles().into_iter().map(|(r, _)| r).collect();
        let known = if known.is_empty() {
            "none yet".to_string()
        } else {
            known.join(" ")
        };
        // A relay that answers is an orchestrator that refused this one
        // message: `horch done` would not get through either.
        let relay_live = role == "orchestrator"
            && channel::is_live(&channel::socket_path(
                &ctx.paths.state_root,
                mailbox.workspace_id(),
            ));
        let hint = if relay_live {
            ""
        } else {
            no_orchestrator_hint(role, sender)
        };
        bail!(
            "role '{role}' is not registered in {}\n       known roles: {known}{hint}",
            mailbox.dir().display(),
        );
    };
    // An agent that is still starting drops typed text, so wait it out. A
    // role that registered just now may not show its agent yet.
    let grace = ctx.settings.tell_grace.unwrap_or(delivery::TELL_GRACE);
    let age = mailbox.registered_at(role).and_then(|at| at.elapsed().ok());
    delivery::send_line_when_ready(
        herdr,
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
    if ctx
        .worker
        .as_ref()
        .and_then(|worker| worker.role.as_ref())
        .is_none()
    {
        let mail = Mail::under(&ctx.paths.state_root, mailbox.workspace_id());
        for message in mail.pending()? {
            println!("{}", message.text);
            mail.ack(&message.id)?;
        }
    }
    let roles = mailbox.role_states(&herdr);
    if roles.is_empty() {
        println!("no roles registered yet");
        return Ok(());
    }
    if roles.iter().any(|r| r.state == PaneState::Unknown) {
        eprintln!("horch inbox: herdr did not answer, so pane states are unknown");
    }
    let socket = channel::socket_path(&ctx.paths.state_root, mailbox.workspace_id());
    output::print(&inbox_listing(&roles, channel::is_live(&socket)));
    Ok(())
}

/// Tell an orchestrator that durable mail still waits for explicit acknowledgement.
pub fn remind_pending(ctx: &RuntimeContext) {
    if ctx
        .worker
        .as_ref()
        .and_then(|worker| worker.role.as_ref())
        .is_some()
    {
        return;
    }
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let Some(count) = Mailbox::resolve_in(&herdr, ctx)
        .ok()
        .and_then(|mailbox| {
            Mail::under(&ctx.paths.state_root, mailbox.workspace_id())
                .pending()
                .ok()
        })
        .map(|messages| messages.len())
        .filter(|count| *count > 0)
    else {
        return;
    };
    let (message, verb) = if count == 1 {
        ("message", "is")
    } else {
        ("messages", "are")
    };
    eprintln!("horch: {count} worker {message} {verb} not acknowledged; run horch inbox");
}

/// One line for each role: name, pane, and `(pane closed)` when herdr does
/// not list the pane.
fn inbox_listing(roles: &[RoleEntry], channel_live: bool) -> String {
    let mut listing = String::new();
    for r in roles {
        let mark = match r.state {
            PaneState::Closed => "  (pane closed)",
            PaneState::Open | PaneState::Unknown => "",
        };
        let channel = if channel_live && r.role == "orchestrator" {
            "  (channel)"
        } else {
            ""
        };
        listing.push_str(&format!("{:<12} {}{mark}{channel}\n", r.role, r.pane));
    }
    listing
}

/// Orchestrator-facing: give a task to a LIVE worker.
///
/// Records the task on that worker's ledger record, so the session history
/// reflects what it is doing, then delivers it into the worker's terminal. For a
/// worker that does not exist yet, use `horch spawn`.
///
/// The record is the role's live record in the mailbox's workspace
/// ([`Ledger::live_for_role`], review finding 13): 2 fleets of 1 project can
/// both have the role.
pub fn assign(ctx: &RuntimeContext, role: &str, task: &str) -> Result<()> {
    if let Some(run) = super::taskledger::active_run_slug(ctx) {
        bail!(
            "run {run} uses the task ledger: assignment is queue plus claim. Queue the work with \
             horch ledger add --file <ticket> --primary <persona>, then spawn the workers \
             horch ledger plan-workers names"
        );
    }
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let mailbox = Mailbox::resolve_in(&herdr, ctx)
        .context("horch assign must run inside a herdr pane, or with HORCH_WORKSPACE_ID set")?;
    Ledger::open_in(ctx)?.assign_in(role, Some(mailbox.workspace_id()), task)?;
    tell(ctx, role, task)
}

/// Worker-facing: append a progress note to this worker's own ledger record, so
/// the session history shows what it is doing, not just start and end states.
///
/// Then the context check (CTX-10, design §6.10): when the session is over
/// its threshold and was not asked in this compaction cycle, it records
/// `context-warned` and prints the rendered `warning` line (the stopping
/// point). The check never fails the note: any error or panic means no
/// output, and nothing goes to stderr.
pub fn note(ctx: &RuntimeContext, text: &str) -> Result<()> {
    let record_id = require_env("HORCH_RECORD_ID", worker_var(ctx, |w| &w.record_id))?;
    Ledger::open_in(ctx)?.note(&record_id, text)?;
    super::taskledger::note_from_worker(ctx, text);
    if let Some(line) = quiet_note_check(ctx, &record_id) {
        println!("{line}");
    }
    Ok(())
}

/// [`note_check`] with its own sources, inside `catch_unwind` and with the
/// panic hook silenced: a check that panics or fails prints nothing.
fn quiet_note_check(ctx: &RuntimeContext, record_id: &str) -> Option<String> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ledger = Ledger::open_in(ctx).ok()?;
        let roster = super::load_roster_unwarned(ctx, None).ok()?;
        let loc = Locations::from_context(ctx);
        let cache = context::cache_dir(ctx);
        let src = Sources {
            ctx,
            ledger: &ledger,
            roster: &roster,
            loc: &loc,
            cache_dir: &cache,
        };
        note_check(&src, record_id).ok().flatten()
    }));
    std::panic::set_hook(hook);
    result.ok().flatten()
}

/// The `horch note` warning of record `record_id`, if it warns: the row is
/// built as `horch context` builds it, and the rule is
/// [`policy::should_warn`]. On a warning it records `context-warned`
/// (its error ignored) and returns the rendered `warning` line, or
/// `warning-orchestrator` for an orchestrator record.
pub(crate) fn note_check(src: &Sources, record_id: &str) -> Result<Option<String>> {
    let record = src.ledger.get(record_id)?;
    if !record.execution_status().is_live() {
        return Ok(None);
    }
    let workspace = src.ctx.herdr.workspace.as_ref().map(|w| w.to_string());
    let row = context::build_row(src, record, workspace.as_deref());
    if !policy::should_warn(&row.inputs()) {
        return Ok(None);
    }
    let Some(tokens) = row.tokens() else {
        return Ok(None);
    };
    let key = if row.record.is_orchestrator() {
        "warning-orchestrator"
    } else {
        "warning"
    };
    let tokens = tokens.to_string();
    let threshold = row.threshold.to_string();
    let handoff = policy::handoff_path(&row.record.role);
    let vars = BTreeMap::from([
        ("role", row.record.role.as_str()),
        ("tokens", tokens.as_str()),
        ("threshold", threshold.as_str()),
        ("handoff", handoff.as_str()),
    ]);
    let line = prompts::context_message(src.roster, key, &vars)?;
    let _ = src.ledger.record_event(
        record_id,
        policy::EVENT_WARNED,
        &format!("tokens {tokens} threshold {threshold}"),
    );
    Ok(Some(line))
}

/// Worker-facing self-shutdown, run only when the work is truly complete (not
/// while waiting on a question). The order of the steps is
/// [`lifecycle::done`]'s.
pub fn done(ctx: &RuntimeContext, summary: &str, unlanded: Option<&str>) -> Result<()> {
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
    let closed = lifecycle::done(
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
            unlanded,
        },
    );
    // After the close, and only when it worked: a Prime pane's tools (this
    // process too) run under Prime's daemon, in another session, so the
    // close does not end this process, and a stop before the close could.
    // herdr kills a closed pane's processes within about 1 s, often before
    // the launcher stops the daemon. No Prime launch in this pane: nothing.
    if closed.is_ok() {
        horch_core::harness::prime::finish_pane(
            &ctx.paths.state_root,
            &ctx.bins.harness.prime,
            &pane_env,
        );
    }
    closed
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
    fn open_claims(&self, record_id: &str) -> Result<Vec<ClaimLine>> {
        let landing = Landing::new(
            self.ctx.bins.harness.git.clone(),
            self.ctx.paths.state_root.clone(),
        );
        if !landing.record_has_open_claims(record_id)? {
            return Ok(Vec::new());
        }
        let dir = self.ctx.paths.project()?;
        landing
            .open_claims_for_record(&dir, record_id)?
            .into_iter()
            .map(|claim| claim_line(self.ctx, &landing, claim))
            .collect()
    }

    fn mark_unlanded(&self, record_id: &str, reason: &str) -> Result<Vec<ClaimLine>> {
        let landing = Landing::new(
            self.ctx.bins.harness.git.clone(),
            self.ctx.paths.state_root.clone(),
        );
        if !landing.record_has_open_claims(record_id)? {
            return Ok(Vec::new());
        }
        let dir = self.ctx.paths.project()?;
        let claims = landing.open_claims_for_record(&dir, record_id)?;
        claims
            .into_iter()
            .map(|claim| {
                let line = claim_line(self.ctx, &landing, claim.clone())?;
                landing.mark_unlanded(&dir, &claim.id, reason)?;
                Ok(line)
            })
            .collect()
    }

    fn unregistered(&self) -> Result<Vec<String>> {
        let dir = self.ctx.paths.project()?;
        let git = GitCli::new(self.ctx.bins.harness.git.clone());
        if let Err(error) = git.toplevel(&dir) {
            let git_not_found = error.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|source| source.kind() == std::io::ErrorKind::NotFound)
            });
            let not_work_tree = error
                .root_cause()
                .to_string()
                .starts_with("git rev-parse --show-toplevel failed (128):");
            if git_not_found || not_work_tree {
                return Ok(Vec::new());
            }
            return Err(error);
        }
        Landing::new(
            self.ctx.bins.harness.git.clone(),
            self.ctx.paths.state_root.clone(),
        )
        .unregistered(&dir)
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| {
                    format!(
                        "{} branch {} ahead {} dirty {}",
                        entry.path.display(),
                        entry.branch.as_deref().unwrap_or("detached"),
                        entry.ahead,
                        entry.dirty
                    )
                })
                .collect()
        })
    }

    fn mark_done(&self, record_id: &str, summary: &str) -> Result<()> {
        Ledger::open_in(self.ctx)?.done(record_id, summary)
    }

    fn recorded(&self, record_id: &str) -> Result<bool> {
        Ledger::open_in(self.ctx)?.done_recorded(record_id)
    }

    fn report_failed(&self, record_id: &str, error: &str) {
        let noted = Ledger::open_in(self.ctx)
            .and_then(|l| l.record_event(record_id, EVENT_REPORT_FAILED, error));
        if let Err(e) = noted {
            eprintln!("horch done: could not record the failed report ({e:#})");
        }
    }

    fn report_queued(&self, record_id: &str, error: &str) {
        let noted = Ledger::open_in(self.ctx)
            .and_then(|ledger| ledger.record_event(record_id, EVENT_REPORT_QUEUED, error));
        if let Err(event_error) = noted {
            eprintln!("horch done: could not record the queued report ({event_error:#})");
        }
    }

    /// Tell the orchestrator. A workspace with no registered orchestrator
    /// (a scratch workspace, a worker spawned by hand) has nobody to tell:
    /// that is not a failure, so `done` goes on and exits 0. A live channel
    /// relay is an orchestrator too, with or without a registered pane.
    fn report(&self, line: &str) -> Result<ReportStatus> {
        let herdr = Herdr::with_bin(&self.ctx.bins.harness.herdr);
        let registered = Mailbox::resolve_in(&herdr, self.ctx)
            .map(|m| {
                m.pane_for("orchestrator").is_some()
                    || channel::is_live(&channel::socket_path(
                        &self.ctx.paths.state_root,
                        m.workspace_id(),
                    ))
            })
            .unwrap_or(true);
        if !registered {
            eprintln!("{NO_ORCHESTRATOR}");
            return Ok(ReportStatus::Delivered);
        }
        tell_outcome(self.ctx, "orchestrator", line)
    }

    fn unregister(&self, workspace: &str, role: &str) -> Result<()> {
        Mailbox::in_context(self.ctx, workspace).try_unregister(role)
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

    fn record_run(&self, record_id: &str) -> Result<()> {
        let record = Ledger::open_in(self.ctx)?.get(record_id)?;
        let project = self.ctx.paths.project()?;
        let paths = DatasetPaths::new(&self.ctx.paths.state_root, &project);
        let usage = TelemetryUsage {
            locations: Locations::from_context(self.ctx),
        };
        let git = GitCli::new(self.ctx.bins.harness.git.clone());
        let sources = RunSources {
            usage: &usage,
            meter: &UsageMeter::default(),
            git: &git,
        };
        let now = horch_core::clock::now();
        record_run(&paths, &project, &record, &sources, now, WrittenBy::Done).map(|_| ())
    }
}

fn claim_line(
    ctx: &RuntimeContext,
    landing: &Landing,
    claim: horch_core::vcs::landing::Claim,
) -> Result<ClaimLine> {
    let status = landing.status(&claim)?;
    let git = GitCli::new(ctx.bins.harness.git.clone());
    let sha = git
        .rev_parse(&ctx.paths.project()?, &claim.branch)?
        .or(claim.landed_commit)
        .unwrap_or_else(|| "unknown".into());
    Ok(ClaimLine {
        branch: claim.branch,
        short_sha: sha.chars().take(7).collect(),
        ahead: status.ahead,
        base: claim.base,
        path: claim.path,
    })
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

/// A required environment value, with a message naming what sets it. A
/// worker that sets it inline instead runs sandboxed in Codex, where its
/// report cannot get out (W2 C1), so the message says not to.
fn require_env(key: &str, value: Option<String>) -> Result<String> {
    value.filter(|v| !v.is_empty()).with_context(|| {
        format!(
            "{key} is unset (this command runs inside a `horch spawn` worker pane). \
             Do not set it yourself: in Codex, a command with a variable prefix runs in \
             the sandbox and cannot reach the orchestrator. Run the plain horch command \
             from the project directory, and tell the operator if it fails again"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::execution::legacy::Record;
    use std::time::Duration;

    #[test]
    fn inbox_listing_marks_closed_panes() {
        let roles = [RoleEntry {
            role: "sonnet-1".into(),
            pane: "p2".into(),
            state: PaneState::Closed,
        }];
        assert_eq!(
            inbox_listing(&roles, false),
            "sonnet-1     p2  (pane closed)\n"
        );
    }

    #[test]
    fn arc_30_inbox_marks_a_live_relay() {
        let roles = [
            RoleEntry {
                role: "orchestrator".into(),
                pane: "p1".into(),
                state: PaneState::Open,
            },
            RoleEntry {
                role: "sonnet-1".into(),
                pane: "p2".into(),
                state: PaneState::Open,
            },
        ];
        assert_eq!(
            inbox_listing(&roles, true),
            "orchestrator p1  (channel)\nsonnet-1     p2\n"
        );
    }

    /// W2 C1: a worker that met this error set the variable inline, ran
    /// sandboxed in Codex, and lost its report. The error says not to.
    #[test]
    fn w2_an_unset_variable_says_not_to_set_it_inline() {
        let text = require_env("HORCH_RECORD_ID", None)
            .unwrap_err()
            .to_string();
        assert!(text.starts_with("HORCH_RECORD_ID is unset"), "{text}");
        assert!(text.contains("Do not set it yourself"), "{text}");
        assert_eq!(require_env("HORCH_ROLE", Some("r".into())).unwrap(), "r");
    }

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

    mod note_warning {
        use super::super::context::testkit::*;
        use super::super::*;

        /// The rendered `warning` line of `sonnet-1` at 311,225 tokens.
        fn sonnet_warning(w: &World) -> String {
            let vars = BTreeMap::from([
                ("role", "sonnet-1"),
                ("tokens", "311225"),
                ("threshold", "300000"),
                ("handoff", "ai_docs/handoffs/sonnet-1-whats-next.md"),
            ]);
            prompts::context_message(&w.roster, "warning", &vars).unwrap()
        }

        /// What `horch note` does: the note, then the check.
        fn note_in(w: &World, id: &str, text: &str) -> Option<String> {
            w.ledger.note(id, text).unwrap();
            note_check(&w.sources(), id).ok().flatten()
        }

        /// CTX-10: over the threshold, the first note warns; the second is
        /// silent (asked in this cycle).
        #[test]
        fn ctx_10_note_warns_once_over_threshold() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            let first = note_in(&w, "r-1", "step 1 done");
            // The exact line of design §6.3.
            assert_eq!(
                first.as_deref(),
                Some(
                    "NOTE: Context warning. Your context is 311225 tokens. Your threshold is \
                     300000 tokens. This horch note call is your stopping point. Do not start \
                     new work. Write ai_docs/handoffs/sonnet-1-whats-next.md with the \
                     horch:handoff skill. Then run: horch note \"handoff: \
                     ai_docs/handoffs/sonnet-1-whats-next.md\". Then send: horch tell \
                     orchestrator \"[sonnet-1] NOTE: COMPACT-READY \
                     ai_docs/handoffs/sonnet-1-whats-next.md\". Then end your turn."
                )
            );
            assert_eq!(first, Some(sonnet_warning(&w)));
            assert_eq!(note_in(&w, "r-1", "step 2 done"), None);
            let events: Vec<String> = w.history("r-1").into_iter().map(|(e, _)| e).collect();
            assert_eq!(events, ["spawned", "note", "context-warned", "note"]);
        }

        /// CTX-10: no transcript, a pending reading, a harness without a
        /// reader and a broken roster are silent; the note is recorded.
        #[test]
        fn ctx_10_note_silent_on_unknown_pending_or_error() {
            let w = World::new();
            w.worker(
                "r-none",
                "sonnet-1",
                "claude",
                "sonnet",
                Some("99999999-9999-4999-8999-999999999999"),
            );
            w.worker("r-pend", "sonnet-2", "claude", "sonnet", Some(PENDING));
            w.worker("r-ag", "ag-1", "antigravity", "antigravity", Some("s-ag"));
            for id in ["r-none", "r-pend", "r-ag"] {
                assert_eq!(note_in(&w, id, "x"), None, "{id}");
                assert_eq!(w.events(id, "note"), ["x"], "{id}");
                assert!(w.events(id, policy::EVENT_WARNED).is_empty(), "{id}");
            }

            // A roster dir that does not read (an unreadable directory is
            // the 1 roster error): `horch note` stays silent.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let broken = w.tmp.path().join("broken-roster");
                std::fs::create_dir_all(&broken).unwrap();
                std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o000)).unwrap();
                let bad = World::with_env(&fixture_home(), |e| {
                    e.with("HORCH_TEAMMATES_DIR", &broken.to_string_lossy())
                });
                assert!(super::super::super::load_roster_unwarned(&bad.ctx, None).is_err());
                bad.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
                bad.ledger.note("r-1", "x").unwrap();
                let line = quiet_note_check(&bad.ctx, "r-1");
                std::fs::set_permissions(&broken, std::fs::Permissions::from_mode(0o700)).unwrap();
                assert_eq!(line, None);
                assert_eq!(bad.events("r-1", "note"), ["x"]);
            }
        }

        /// CTX-10: a compaction after the warning starts a new cycle.
        #[test]
        fn ctx_10_note_warns_again_after_compaction() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            w.event_at(
                "r-1",
                "2026-10-06T10:00:00Z",
                policy::EVENT_WARNED,
                "tokens 311225 threshold 300000",
            );
            assert_eq!(note_in(&w, "r-1", "x"), None, "asked in this cycle");
            w.event_at(
                "r-1",
                "2026-10-06T10:05:00Z",
                policy::EVENT_COMPACTED,
                "311225 -> 10486 tokens; handoff h",
            );
            assert_eq!(note_in(&w, "r-1", "y"), Some(sonnet_warning(&w)));
        }

        /// CTX-10 (review finding 2): a marker cache dir that cannot be
        /// written does not hide the warning.
        #[cfg(unix)]
        #[test]
        fn ctx_10_note_warns_with_a_read_only_cache_dir() {
            use std::os::unix::fs::PermissionsExt;
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            std::fs::create_dir_all(&w.cache).unwrap();
            std::fs::set_permissions(&w.cache, std::fs::Permissions::from_mode(0o500)).unwrap();
            let line = note_in(&w, "r-1", "x");
            std::fs::set_permissions(&w.cache, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(line, Some(sonnet_warning(&w)));
            assert!(!w.cache.join("markers").exists());
        }
    }
}
