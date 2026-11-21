use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use horch_core::compaction::policy;
use horch_core::execution::lifecycle::{
    self, ClaimLine, DoneRequest, DoneSteps, ReportStatus, ReportTarget,
};
use horch_core::execution::records::{SessionLedger, EVENT_REPORT_FAILED, EVENT_REPORT_QUEUED};
use horch_core::harness::launch;
use horch_core::messaging::channel;
use horch_core::messaging::delivery;
use horch_core::messaging::mail::{Enqueued, Id, Mail};
use horch_core::messaging::mailbox::{Mailbox, PaneState, RoleEntry};
use horch_core::messaging::message;
use horch_core::messaging::report;
use horch_core::prompts;
use horch_core::runtime::RuntimeContext;
use horch_core::skills::catalog::marketplace::GitError;
use horch_core::taskledger::model::EventKind;
use horch_core::vcs::git::GitCli;
use horch_core::vcs::git::GitClient;
use horch_core::vcs::landing::Landing;
use horch_core::workspace::arrange;
use horch_core::workspace::PaneHost;

use super::context::{self, Sources};
use crate::output;

pub fn report_mode(socket: &std::path::Path, request: Option<report::Request>) -> Result<()> {
    let Some(request) = request else {
        bail!(
            "this pane has horch_access: report. Only `horch ledger progress <text>`, \
             `horch tell <report target> <text>` and `horch done <summary>` work here"
        );
    };
    report::send(socket, &request)
}

pub fn tell(ctx: &RuntimeContext, role: &str, text: &str) -> Result<()> {
    match tell_outcome(ctx, role, text)? {
        ReportStatus::Delivered => Ok(()),
        ReportStatus::Queued(cause) => {
            eprintln!("horch tell: queued, not yet acknowledged ({cause}); redelivered until horch inbox reads it");
            Ok(())
        }
    }
}

pub fn tell_answer(ctx: &RuntimeContext, role: &str, question: &str, text: &str) -> Result<()> {
    let sender = ctx.worker.as_ref().and_then(|w| w.role.clone());
    if sender.as_deref().is_some_and(|s| s != "orchestrator") {
        bail!("only the orchestrator answers a question with --answers; send a QUESTION: or NOTE: line instead");
    }
    match queue_as(ctx, sender, role, text, false, Some(question))? {
        Some(outgoing) => outgoing.push().map(|_| ()),
        None => Ok(()),
    }
}

fn tell_outcome(ctx: &RuntimeContext, role: &str, text: &str) -> Result<ReportStatus> {
    let sender = ctx.worker.as_ref().and_then(|w| w.role.clone());
    tell_as(ctx, sender, role, text)
}

pub fn tell_as(
    ctx: &RuntimeContext,
    sender: Option<String>,
    role: &str,
    text: &str,
) -> Result<ReportStatus> {
    let _budget = (sender.as_deref() == Some(HORCH))
        .then(|| delivery::start_push_budget(delivery::done_push_budget(&ctx.settings.faults)))
        .flatten();
    match queue_as(ctx, sender, role, text, false, None)? {
        Some(outgoing) => outgoing.push(),
        None => Ok(ReportStatus::Delivered),
    }
}

const HORCH: &str = "horch";

fn is_durable(sender: Option<&str>, role: &str) -> bool {
    match sender {
        Some(HORCH) => true,
        Some(_) => role == "orchestrator",
        None => false,
    }
}

struct Outgoing<'a> {
    ctx: &'a RuntimeContext,
    herdr: PaneHost,
    mailbox: Mailbox,
    role: String,
    sender: Option<String>,
    target: Option<String>,
    line: String,
    durable: bool,
    queued: Option<Id>,
    courier_after: bool,
    answers: Option<String>,
}

fn queue_as<'a>(
    ctx: &'a RuntimeContext,
    sender: Option<String>,
    role: &str,
    text: &str,
    take_back: bool,
    answers: Option<&str>,
) -> Result<Option<Outgoing<'a>>> {
    let mailbox = Mailbox::resolve_in(&PaneHost::of(ctx).until(delivery::push_deadline()), ctx)
        .context("horch tell must run inside a herdr pane, or with HORCH_WORKSPACE_ID set")?;
    let herdr =
        PaneHost::for_workspace(ctx, mailbox.workspace_id()).until(delivery::push_deadline());
    let horch_text;
    let text = if sender.as_deref() == Some(HORCH) {
        horch_text = horch_core::messaging::mail::horch_line(text);
        horch_text.as_str()
    } else {
        text
    };
    let question = answers
        .map(|key| open_question(ctx, mailbox.workspace_id(), role, key))
        .transpose()?;
    let mut line = match &question {
        Some(q) => render_line(
            ctx,
            mailbox.workspace_id(),
            None,
            role,
            &answer_line(&q.id.short(), text),
        )?,
        None => render_line(ctx, mailbox.workspace_id(), sender.as_deref(), role, text)?,
    };
    let target = mailbox.pane_for(role);
    let durable = is_durable(sender.as_deref(), role);
    let courier_now = durable && delivery::push_time_left().is_some();
    let mut queued = None;
    if durable {
        let from = sender.as_deref().expect("durable mail has a sender");
        let enqueued = enqueue_durable(
            ctx,
            mailbox.workspace_id(),
            role,
            from,
            &line,
            text,
            courier_now,
            take_back,
        )?;
        if enqueued.already_published {
            return Ok(None);
        }
        let id = enqueued.id;
        line = message::with_mail_id(&line, from, &id.short());
        queued = Some(id);
        if role != "orchestrator" {
        } else if let Err(error) = sync_question_waits(ctx, mailbox.workspace_id()) {
            eprintln!(
                "horch tell: question wait projection failed ({error:#}); mail remains durable"
            );
        }
    }
    Ok(Some(Outgoing {
        ctx,
        herdr,
        mailbox,
        role: role.to_string(),
        sender,
        target,
        line,
        durable,
        queued,
        courier_after: durable && !courier_now,
        answers: question.map(|q| q.id.as_str().to_string()),
    }))
}

fn open_question(
    ctx: &RuntimeContext,
    workspace: &str,
    role: &str,
    key: &str,
) -> Result<horch_core::messaging::mail::Message> {
    let mail = Mail::under(&ctx.paths.state_root, workspace);
    let Some(question) = mail.find_question(key)? else {
        bail!(
            "no open question #{}; horch inbox --roles lists the open questions",
            key.trim_start_matches('#')
        );
    };
    check_answer_role(&question.from, role, &question.id.short())?;
    Ok(question)
}

fn check_answer_role(asker: &str, role: &str, short: &str) -> Result<()> {
    if asker != role {
        bail!("question #{short} is from {asker}, not {role}; run: horch tell {asker} --answers {short} \"<answer>\"");
    }
    Ok(())
}

fn answer_line(short: &str, text: &str) -> String {
    let body = text.trim_start();
    let body = body
        .strip_prefix("[orchestrator]")
        .unwrap_or(body)
        .trim_start();
    format!("[orchestrator re #{short}] {body}")
}

fn render_line(
    ctx: &RuntimeContext,
    workspace: &str,
    sender: Option<&str>,
    role: &str,
    text: &str,
) -> Result<String> {
    let text = match sender {
        Some(me) => message::ensure_tag(text, me),
        None => text.to_string(),
    };
    if text.chars().count() <= message::MAX_INLINE {
        return Ok(text);
    }
    let path = message::spool(
        &message::spool_dir(&ctx.paths.state_root, workspace),
        workspace,
        sender.unwrap_or("orchestrator"),
        role,
        &text,
    )?;
    Ok(message::reference_line(&text, &path))
}

#[allow(clippy::too_many_arguments)]
fn enqueue_durable(
    ctx: &RuntimeContext,
    workspace: &str,
    role: &str,
    from: &str,
    line: &str,
    original: &str,
    courier_now: bool,
    take_back: bool,
) -> Result<Enqueued> {
    let mail = Mail::for_role(&ctx.paths.state_root, workspace, role);
    let queued = mail.enqueue_once(from, line, original)?;
    if !courier_now || queued.already_published {
        return Ok(queued);
    }
    let id = queued.id.clone();
    let Err(error) = super::mail::start_courier(ctx, workspace) else {
        return Ok(queued);
    };
    if !take_back {
        eprintln!("horch tell: {error:#}; `horch inbox` still shows the mail");
        return Ok(queued);
    }
    if let Err(ack_error) = mail.ack(&id) {
        eprintln!("horch tell: could not take back the queued line ({ack_error:#})");
    }
    Err(error)
}

impl Outgoing<'_> {
    fn push(self) -> Result<ReportStatus> {
        let herdr = self.herdr.clone().until(delivery::push_deadline());
        let mail = Mail::under(&self.ctx.paths.state_root, self.mailbox.workspace_id());
        let to_orchestrator = self.role == "orchestrator";
        let login = self.durable
            && to_orchestrator
            && super::mail::check_login(&herdr, &self.mailbox, &mail)?;
        let pushed = if login {
            Err(anyhow::anyhow!("the orchestrator needs /login"))
        } else {
            push_line(
                &Tell {
                    ctx: self.ctx,
                    herdr: &herdr,
                    mailbox: &self.mailbox,
                    role: &self.role,
                    sender: self.sender.as_deref(),
                },
                self.target,
                self.line,
                true,
            )
        };
        if let (Ok(()), Some(id)) = (&pushed, &self.queued) {
            let state_root = &self.ctx.paths.state_root;
            let queued = Mail::for_role(state_root, self.mailbox.workspace_id(), &self.role);
            if let Err(error) = queued.mark_pushed(id) {
                eprintln!("horch tell: recording the push failed ({error:#})");
            }
        }
        if self.courier_after {
            if let Err(error) = super::mail::start_courier(self.ctx, self.mailbox.workspace_id()) {
                eprintln!("horch tell: {error:#}; `horch inbox` still shows the mail");
            }
        }
        if self.durable && !to_orchestrator {
            return Ok(match pushed {
                Ok(()) => ReportStatus::Delivered,
                Err(error) => ReportStatus::Queued(format!("{error:#}")),
            });
        }
        if self.durable {
            return Ok(match pushed {
                Ok(()) => ReportStatus::Queued(
                    "notification written; awaiting horch inbox acknowledgement".into(),
                ),
                Err(error) => ReportStatus::Queued(format!("{error:#}")),
            });
        }
        pushed?;
        if let Some(id) = &self.answers {
            mail.close_question(id)?;
            sync_question_waits(self.ctx, self.mailbox.workspace_id())?;
        }
        Ok(ReportStatus::Delivered)
    }
}

struct Tell<'a> {
    ctx: &'a RuntimeContext,
    herdr: &'a PaneHost,
    mailbox: &'a Mailbox,
    role: &'a str,
    sender: Option<&'a str>,
}

fn push_line(tell: &Tell, target: Option<String>, line: String, relay: bool) -> Result<()> {
    let Some(line) = (if relay {
        try_relay(tell, line)?
    } else {
        Some(line)
    }) else {
        return Ok(());
    };
    let Some(target) = target else {
        return Err(not_registered(tell));
    };
    let grace = tell.ctx.settings.tell_grace.unwrap_or(delivery::TELL_GRACE);
    let age = tell
        .mailbox
        .registered_at(tell.role)
        .and_then(|at| at.elapsed().ok());
    delivery::send_line_when_ready(
        tell.herdr,
        &target,
        &line,
        grace_left(age, grace),
        &delivery::Readiness::DEFAULT,
        &delivery::Timing::DEFAULT,
    )
}

fn try_relay(tell: &Tell, mut line: String) -> Result<Option<String>> {
    let spent = delivery::push_time_left().is_some_and(|left| left.is_zero());
    if tell.role != "orchestrator" || spent {
        return Ok(Some(line));
    }
    let socket = channel::socket_path(&tell.ctx.paths.state_root, tell.mailbox.workspace_id());
    let from = tell.sender.unwrap_or("orchestrator");
    match channel::send(&socket, from, &line, delivery::capped(send_timeout())) {
        Ok(()) => return Ok(None),
        Err(error) if tell.sender.is_some() && channel::is_live(&socket) => {
            bail!("the channel relay did not take or confirm the message ({error}); the courier redelivers it through the relay")
        }
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
    Ok(Some(line))
}

fn send_timeout() -> std::time::Duration {
    #[cfg(debug_assertions)]
    if let Some(ms) = std::env::var("HORCH_TEST_CHANNEL_SEND_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse().ok())
    {
        return std::time::Duration::from_millis(ms);
    }
    channel::SEND_TIMEOUT
}

fn not_registered(tell: &Tell) -> anyhow::Error {
    let known: Vec<String> = tell.mailbox.roles().into_iter().map(|(r, _)| r).collect();
    let known = if known.is_empty() {
        "none yet".to_string()
    } else {
        known.join(" ")
    };
    let relay_live = tell.role == "orchestrator"
        && channel::is_live(&channel::socket_path(
            &tell.ctx.paths.state_root,
            tell.mailbox.workspace_id(),
        ));
    let hint = if relay_live {
        ""
    } else {
        no_orchestrator_hint(tell.role, tell.sender)
    };
    anyhow::anyhow!(
        "role '{}' is not registered in {}\n       known roles: {known}{hint}",
        tell.role,
        tell.mailbox.dir().display(),
    )
}

fn grace_left(age: Option<std::time::Duration>, grace: std::time::Duration) -> std::time::Duration {
    age.map_or(std::time::Duration::ZERO, |age| grace.saturating_sub(age))
}

fn no_orchestrator_hint(role: &str, sender: Option<&str>) -> &'static str {
    if role == "orchestrator" && sender.is_some() {
        "\n       no orchestrator runs in this workspace, so nobody reads reports here.\n       \
         `horch done \"<summary>\"` still records your summary and closes your pane."
    } else {
        ""
    }
}

pub fn inbox(ctx: &RuntimeContext, roles_only: bool, peek: bool) -> Result<()> {
    let mailbox = Mailbox::resolve_in(&PaneHost::of(ctx).with_timeout(pane_list_timeout()), ctx)?;
    let herdr =
        PaneHost::for_workspace(ctx, mailbox.workspace_id()).with_timeout(pane_list_timeout());
    let roles = mailbox.role_states(&herdr);
    if let Err(error) = sync_question_waits(ctx, mailbox.workspace_id()) {
        eprintln!("horch inbox: question wait projection failed ({error:#})");
    }
    let question_mail = Mail::under(&ctx.paths.state_root, mailbox.workspace_id());
    let questions = question_mail.open_questions()?;
    let now: std::time::SystemTime = horch_core::clock::now().into();
    let print_question = |question: &horch_core::messaging::mail::Message| {
        let age = now
            .duration_since(question.created_at)
            .unwrap_or_default()
            .as_secs();
        output::println(&format!(
            "  {} #{} open ({age}s ago): {}",
            question.from,
            question.id.short(),
            question.text
        ));
    };
    if roles.is_empty() {
        output::println("no roles registered yet");
    } else {
        if roles.iter().any(|r| r.state == PaneState::Unknown) {
            eprintln!("roles unavailable: herdr did not answer, so pane states are unknown");
        }
        let socket = channel::socket_path(&ctx.paths.state_root, mailbox.workspace_id());
        let channel_live = channel::is_live(&socket);
        for r in &roles {
            output::print(&inbox_listing(std::slice::from_ref(r), channel_live));
            let status = match r.state {
                PaneState::Closed => "closed".to_string(),
                PaneState::Unknown => "unknown".to_string(),
                PaneState::Open => herdr
                    .pane_get(&r.pane)
                    .ok()
                    .and_then(|p| p.agent_status)
                    .filter(|s| matches!(s.as_str(), "idle" | "working" | "blocked" | "done"))
                    .unwrap_or_else(|| "unknown".into()),
            };
            output::println(&format!(
                "  {}: {status}{}",
                r.role,
                ledger_role_state(ctx, mailbox.workspace_id(), &r.role)
            ));
            for question in questions.iter().filter(|q| q.from == r.role) {
                print_question(question);
            }
        }
    }
    for question in questions
        .iter()
        .filter(|q| !roles.iter().any(|r| r.role == q.from))
    {
        print_question(question);
    }
    if roles_only {
        return Ok(());
    }
    if let Some(role) = ctx.worker.as_ref().and_then(|w| w.role.as_deref()) {
        let mail = Mail::to_role(&ctx.paths.state_root, mailbox.workspace_id(), role);
        return print_mail(&mail, peek);
    }
    print_orchestrator_mail(ctx, &herdr, &mailbox, peek)
}

fn print_orchestrator_mail(
    ctx: &RuntimeContext,
    herdr: &PaneHost,
    mailbox: &Mailbox,
    peek: bool,
) -> Result<()> {
    let seat = may_acknowledge(
        ctx.herdr.pane.as_ref().map(|p| p.as_str()),
        |pane| herdr.pane_get(pane).ok().map(|p| p.pane_id),
        mailbox.pane_for("orchestrator").as_deref(),
    );
    if !peek && !seat {
        eprintln!(
            "horch inbox: this pane is not the orchestrator's, so the mail is shown, not acknowledged"
        );
    }
    print_mail(
        &Mail::under(&ctx.paths.state_root, mailbox.workspace_id()),
        peek || !seat,
    )
}

fn may_acknowledge(
    caller: Option<&str>,
    public_id: impl FnOnce(&str) -> Option<String>,
    orchestrator: Option<&str>,
) -> bool {
    let (Some(caller), Some(orchestrator)) = (caller, orchestrator) else {
        return true;
    };
    caller == orchestrator || public_id(caller).is_some_and(|id| id == orchestrator)
}

pub fn ack_role_mail(ctx: &RuntimeContext, role: &str) {
    let herdr = match ctx.herdr.workspace.as_ref() {
        Some(ws) => PaneHost::for_workspace(ctx, ws.as_str()),
        None => PaneHost::of(ctx),
    }
    .with_timeout(pane_list_timeout());
    let Ok(mailbox) = Mailbox::resolve_in(&herdr, ctx) else {
        return;
    };
    let mail = Mail::to_role(&ctx.paths.state_root, mailbox.workspace_id(), role);
    if let Err(error) = mail.ack_all() {
        eprintln!("horch: acknowledging the mail of {role} failed: {error:#}");
    }
}

fn print_mail(mail: &Mail, peek: bool) -> Result<()> {
    let pending = mail.pending()?;
    for message in &pending {
        output::println(&message.line());
        if !peek {
            mail.ack(&message.id)?;
        }
    }
    let count = pending.len();
    let noun = if count == 1 { "message" } else { "messages" };
    if peek {
        output::println(&format!("{count} {noun} pending, not acknowledged"));
    } else {
        output::println(&format!("{count} {noun} acknowledged"));
    }
    Ok(())
}

fn pane_list_timeout() -> std::time::Duration {
    #[cfg(debug_assertions)]
    if let Some(ms) = std::env::var("HORCH_TEST_PANE_LIST_MS")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        return std::time::Duration::from_millis(ms);
    }
    std::time::Duration::from_secs(5)
}

pub fn remind_pending(ctx: &RuntimeContext) {
    if ctx
        .worker
        .as_ref()
        .and_then(|worker| worker.role.as_ref())
        .is_some()
    {
        return;
    }
    let herdr = match ctx.herdr.workspace.as_ref() {
        Some(ws) => PaneHost::for_workspace(ctx, ws.as_str()),
        None => PaneHost::of(ctx),
    }
    .with_timeout(pane_list_timeout());
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
    eprintln!("horch: {count} {message} {verb} not acknowledged; run horch inbox");
}

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

pub fn note(ctx: &RuntimeContext, text: &str) -> Result<std::process::ExitCode> {
    note_as(ctx, text, EventKind::Note)
}

pub fn note_as(
    ctx: &RuntimeContext,
    text: &str,
    kind: EventKind,
) -> Result<std::process::ExitCode> {
    let record_id = worker_var(ctx, |w| &w.record_id);
    let worker_id = super::taskledger::worker_id_opt(ctx);
    if record_id.is_none() && worker_id.is_none() {
        require_env("HORCH_RECORD_ID", record_id.clone())?;
    }
    let session = record_id
        .as_deref()
        .map(|id| SessionLedger::open_in(ctx).and_then(|l| l.note(id, text)));
    let ledger = super::taskledger::progress_from_worker(ctx, kind, text);
    match (session, ledger) {
        (None, Ok(true)) | (Some(Ok(())), Ok(_)) => {}
        (None, Ok(false)) => bail!(
            "the task ledger has no worker {}",
            worker_id.unwrap_or_default()
        ),
        (None, Err(e)) => return Err(e),
        (Some(Ok(())), Err(e)) => {
            eprintln!("horch: the task ledger did not record the note: {e:#}")
        }
        (Some(Err(e)), Ok(true)) => {
            eprintln!("horch: the session record did not record the note: {e:#}")
        }
        (Some(Err(e)), _) => return Err(e),
    }
    if let Some(line) = super::taskledger::note_handoff_warning(ctx) {
        output::println(&line);
    }
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        super::compact::worker_note_context(ctx, text)
    }));
    std::panic::set_hook(hook);
    Ok(context_followup_exit(result))
}

fn context_followup_exit(result: std::thread::Result<Result<()>>) -> std::process::ExitCode {
    if let Ok(Err(error)) = result {
        eprintln!("horch note: context follow-up: {error:#}");
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
fn quiet_note_check(ctx: &RuntimeContext, record_id: &str) -> Option<String> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let ledger = SessionLedger::open_in(ctx).ok()?;
        let roster = super::load_roster_unwarned(ctx, None).ok()?;
        let loc = horch_core::usage::Locations::from_context(ctx);
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

pub(crate) fn note_check(src: &Sources, record_id: &str) -> Result<Option<String>> {
    let record = src.ledger.get(record_id)?;
    let Some((row, tokens)) = warn_row(src, record) else {
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

fn warn_row(
    src: &Sources,
    record: horch_core::execution::legacy::Record,
) -> Option<(context::Row, u64)> {
    if !record.execution_status().is_live() {
        return None;
    }
    let workspace = src.ctx.herdr.workspace.as_ref().map(|w| w.to_string());
    let row = context::build_row(src, record, workspace.as_deref());
    if !policy::should_warn(&row.inputs()) {
        return None;
    }
    let tokens = row.tokens()?;
    Some((row, tokens))
}

pub fn done(ctx: &RuntimeContext, summary: &str, unlanded: Option<&str>) -> Result<()> {
    if super::taskledger::is_ledger_worker(ctx) {
        let store = horch_core::taskledger::open(ctx)?;
        return super::taskledger::done_cmd(ctx, store, summary, unlanded);
    }
    done_then(ctx, summary, unlanded, &|| end_taskless_row(ctx))
}

fn end_taskless_row(ctx: &RuntimeContext) {
    use horch_core::taskledger::{model::WorkerStatus, workers};
    let Some(wid) = super::taskledger::worker_id_opt(ctx) else {
        return;
    };
    let ended =
        horch_core::taskledger::open(ctx).and_then(|s| match workers::find(s.conn(), &wid)? {
            Some(w) if w.task_id.is_none() => {
                workers::end(s.conn(), &wid, WorkerStatus::Done, "done", None).map(|_| ())
            }
            _ => Ok(()),
        });
    if let Err(e) = ended {
        eprintln!("horch done: the task ledger did not end worker {wid}: {e:#}");
    }
}

pub fn done_then(
    ctx: &RuntimeContext,
    summary: &str,
    unlanded: Option<&str>,
    after_mark_done: &dyn Fn(),
) -> Result<()> {
    let record_id = require_env("HORCH_RECORD_ID", worker_var(ctx, |w| &w.record_id))?;
    let role = require_env("HORCH_ROLE", worker_var(ctx, |w| &w.role))?;
    let pane_env = require_env(
        "HERDR_PANE_ID",
        ctx.herdr.pane.as_ref().map(|p| p.to_string()),
    )
    .context("horch done must run inside a herdr pane")?;
    let workspace = ctx.herdr.workspace.as_ref().map(|w| w.to_string());
    let budget = delivery::start_push_budget(delivery::done_push_budget(&ctx.settings.faults));
    if budget.is_some() {
        eprintln!("{CLOSING}");
    }

    let herdr = match workspace.as_deref() {
        Some(ws) => PaneHost::for_workspace(ctx, ws),
        None => PaneHost::of(ctx),
    };
    let closed = lifecycle::done(
        &herdr,
        &CliDone {
            ctx,
            outgoing: std::cell::RefCell::new(None),
            after_mark_done,
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
    if closed.is_ok() {
        horch_core::harness::prime::finish_pane(
            &ctx.paths.state_root,
            &ctx.bins.harness.prime,
            &pane_env,
        );
    }
    closed
}

fn report_target(ctx: &RuntimeContext, workspace: Option<&str>, role: &str) -> ReportTarget {
    workspace
        .and_then(|w| Mailbox::in_context(ctx, w).read_brief(role).ok())
        .map_or(ReportTarget::Orchestrator, |b| b.report_to)
}

fn outside_repo(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        let git_not_found = cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|source| source.kind() == std::io::ErrorKind::NotFound);
        let not_work_tree = matches!(
            cause.downcast_ref::<GitError>(),
            Some(GitError::Failed { args, status: 128, .. })
                if args == &["rev-parse", "--show-toplevel"]
        );
        git_not_found || not_work_tree
    })
}

pub const CLOSING: &str = "closing: reporting to the orchestrator";

struct CliDone<'a> {
    ctx: &'a RuntimeContext,
    outgoing: std::cell::RefCell<Option<Outgoing<'a>>>,
    after_mark_done: &'a dyn Fn(),
}

impl CliDone<'_> {
    fn scan_unregistered(&self, dir: &std::path::Path) -> Result<Vec<String>> {
        Landing::new(
            self.ctx.bins.harness.git.clone(),
            self.ctx.paths.state_root.clone(),
        )
        .with_recorded_worktrees(super::worktree::recorded_worktrees(self.ctx))
        .unregistered(dir)
        .map(|scan| {
            super::worktree::note_skipped("done", &scan.skipped);
            scan.entries
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

    fn drop_role_mail(&self, workspace: &str, role: &str) {
        let mail = Mail::to_role(&self.ctx.paths.state_root, workspace, role);
        if let Err(error) = mail.ack_all() {
            eprintln!("horch: acknowledging the mail of {role} failed: {error:#}");
        }
    }
}

impl DoneSteps for CliDone<'_> {
    fn open_claims(&self, record_id: &str) -> Result<Vec<ClaimLine>> {
        let landing = Landing::new(
            self.ctx.bins.harness.git.clone(),
            self.ctx.paths.state_root.clone(),
        );
        landing.reopen_landed_for_record(record_id)?;
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
        let Err(error) = git.toplevel(&dir) else {
            return self.scan_unregistered(&dir);
        };
        if outside_repo(&error) {
            Ok(Vec::new())
        } else {
            Err(error)
        }
    }

    fn mark_done(&self, record_id: &str, summary: &str) -> Result<()> {
        SessionLedger::open_in(self.ctx)?.done(record_id, summary)
    }

    fn recorded(&self, record_id: &str) -> Result<bool> {
        SessionLedger::open_in(self.ctx)?.done_recorded(record_id)
    }

    fn report_failed(&self, record_id: &str, error: &str) {
        let noted = SessionLedger::open_in(self.ctx)
            .and_then(|l| l.record_event(record_id, EVENT_REPORT_FAILED, error));
        if let Err(e) = noted {
            eprintln!("horch done: could not record the failed report ({e:#})");
        }
    }

    fn report_queued(&self, record_id: &str, error: &str) {
        let noted = SessionLedger::open_in(self.ctx)
            .and_then(|ledger| ledger.record_event(record_id, EVENT_REPORT_QUEUED, error));
        if let Err(event_error) = noted {
            eprintln!("horch done: could not record the queued report ({event_error:#})");
        }
    }

    fn queue_report(&self, line: &str) -> Result<()> {
        let sender = self.ctx.worker.as_ref().and_then(|w| w.role.clone());
        *self.outgoing.borrow_mut() = queue_as(self.ctx, sender, "orchestrator", line, true, None)?;
        Ok(())
    }

    fn after_mark_done(&self) {
        (self.after_mark_done)();
    }

    fn push_report(&self) -> ReportStatus {
        match self.outgoing.borrow_mut().take().map(Outgoing::push) {
            None | Some(Ok(ReportStatus::Delivered)) => ReportStatus::Delivered,
            Some(Ok(queued @ ReportStatus::Queued(_))) => queued,
            Some(Err(error)) => ReportStatus::Queued(format!("{error:#}")),
        }
    }

    fn unregister(&self, workspace: &str, role: &str) {
        Mailbox::in_context(self.ctx, workspace).unregister(role);
        self.drop_role_mail(workspace, role);
    }

    fn try_unregister(&self, workspace: &str, role: &str) -> Result<()> {
        Mailbox::in_context(self.ctx, workspace).try_unregister(role)?;
        self.drop_role_mail(workspace, role);
        Ok(())
    }

    fn settle(&self, workspace: &str) {
        arrange::settle_after_close(self.ctx, workspace);
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

pub fn register(ctx: &mut RuntimeContext, role: &str) -> Result<()> {
    let (mailbox, pane_id, _lock) = super::recipes::register_pane(
        ctx,
        role,
        &super::recipes::PaneIdentity::default(),
        role == "orchestrator",
    )?;
    println!(
        "registered {role} as {pane_id} in {}",
        mailbox.dir().display()
    );
    if role == "orchestrator"
        && !Mail::under(&ctx.paths.state_root, mailbox.workspace_id())
            .pending()?
            .is_empty()
    {
        super::mail::start_courier(ctx, mailbox.workspace_id())?;
    }
    Ok(())
}

fn worker_var(
    ctx: &RuntimeContext,
    field: impl Fn(&horch_core::runtime::WorkerEnv) -> &Option<String>,
) -> Option<String> {
    ctx.worker.as_ref().and_then(|w| field(w).clone())
}

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

pub fn peek(ctx: &RuntimeContext, role: &str, lines: usize) -> Result<()> {
    let mailbox = Mailbox::resolve_in(&PaneHost::of(ctx).with_timeout(pane_list_timeout()), ctx)?;
    let herdr =
        PaneHost::for_workspace(ctx, mailbox.workspace_id()).with_timeout(pane_list_timeout());
    let pane = mailbox
        .pane_for(role)
        .with_context(|| format!("role '{role}' is not registered"))?;
    let screen = herdr.pane_read(&pane, "recent-unwrapped")?;
    let tail: Vec<_> = screen
        .trim_end_matches('\n')
        .lines()
        .rev()
        .take(lines)
        .collect();
    for line in tail.into_iter().rev() {
        output::println(line);
    }
    Ok(())
}

fn sync_question_waits(ctx: &RuntimeContext, workspace: &str) -> Result<()> {
    if !ctx.paths.ledger_db.exists() {
        return Ok(());
    }
    let store = horch_core::taskledger::open(ctx)?;
    horch_core::taskledger::workers::sync_questions(store.conn(), &ctx.paths.state_root, workspace)
}

fn ledger_role_state(ctx: &RuntimeContext, workspace: &str, role: &str) -> String {
    if !ctx.paths.ledger_db.exists() {
        return String::new();
    }
    let Ok(store) = horch_core::taskledger::open(ctx) else {
        return String::new();
    };
    horch_core::taskledger::workers::role_state(store.conn(), workspace, role).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn register_ctx(dir: &std::path::Path) -> (RuntimeContext, std::path::PathBuf) {
        let bin = dir.join("herdr");
        let warm = crate::test_support::write_warmed(
            &bin,
            "#!/bin/sh\n\
             case \"$*\" in\n\
             'pane get p2') echo '{\"result\":{\"pane\":{\"pane_id\":\"p2\",\"workspace_id\":\"w1\"}}}' ;;\n\
             warm*) ;;\n\
             *) exit 1 ;;\n\
             esac\n",
            &["warm"],
        );
        assert!(warm.status.success());
        let db = dir.join("ledger.sqlite3");
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with_exe("/opt/horch/bin/horch")
                .with("HORCH_STATE_DIR", dir.join("state").to_str().unwrap())
                .with("HORCH_LEDGER_DB", db.to_str().unwrap())
                .with("HORCH_HERDR_BIN", bin.to_str().unwrap())
                .with("HERDR_PANE_ID", "p2"),
        )
        .unwrap();
        let locks = horch_core::taskledger::orchestrator::lock_dir(&db);
        std::fs::create_dir_all(&locks).unwrap();
        (ctx, locks)
    }

    #[test]
    fn trf_39_register_refuses_the_orchestrator_role_while_another_holds_the_lock() {
        use horch_core::taskledger::orchestrator;
        let dir = tempfile::tempdir().unwrap();
        let (mut ctx, locks) = register_ctx(dir.path());
        let _live = orchestrator::lock(&locks, "w1").unwrap().unwrap();
        let mailbox = Mailbox::under(&ctx.paths.state_root, "w1");
        std::fs::create_dir_all(mailbox.dir()).unwrap();
        mailbox.register_role("orchestrator", "p1").unwrap();

        let err = register(&mut ctx, "orchestrator").unwrap_err().to_string();
        assert_eq!(
            err,
            format!(
                "an orchestrator already runs in workspace w1 (PID {}); \
                 1 workspace has 1 orchestrator",
                std::process::id()
            )
        );
        assert!(!err.contains('\n'), "1 line: {err}");
        assert_eq!(mailbox.pane_for("orchestrator").as_deref(), Some("p1"));

        register(&mut ctx, "dev-1").unwrap();
        assert_eq!(mailbox.pane_for("dev-1").as_deref(), Some("p2"));
    }

    #[test]
    fn trf_39_register_takes_the_orchestrator_role_when_the_lock_is_free() {
        use horch_core::taskledger::orchestrator;
        let dir = tempfile::tempdir().unwrap();
        let (mut ctx, locks) = register_ctx(dir.path());

        register(&mut ctx, "orchestrator").unwrap();
        let mailbox = Mailbox::under(&ctx.paths.state_root, "w1");
        assert_eq!(mailbox.pane_for("orchestrator").as_deref(), Some("p2"));
        assert!(
            orchestrator::lock(&locks, "w1").unwrap().is_some(),
            "register frees the lock when it ends"
        );
    }

    #[test]
    fn trf_45_note_and_ledger_progress_are_1_command_and_write_no_json() {
        use horch_core::runtime::MapEnv;
        use horch_core::taskledger::runs::{self, NewRun};
        use horch_core::taskledger::workers::{self, NewWorker};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        let state = dir.path().join("state");
        std::fs::create_dir_all(&root).unwrap();
        let db = dir.path().join("ledger.sqlite3");
        let mut store = horch_core::taskledger::Store::open(&db).unwrap();
        let wid = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )?;
                let id = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id,
                        persona: "opus".into(),
                        role: Some("opus-1".into()),
                        legacy_record_id: Some("r-1".into()),
                        ..Default::default()
                    },
                )?;
                tx.execute(
                    "UPDATE workers SET status = 'working' WHERE worker_id = ?1",
                    [&id],
                )?;
                Ok(id)
            })
            .unwrap();
        let ctx = |record: Option<&str>, worker: Option<&str>| {
            let mut env = MapEnv::new(&root)
                .with("HOME", &dir.path().to_string_lossy())
                .with("HORCH_STATE_DIR", &state.to_string_lossy())
                .with("HORCH_LEDGER_DB", &db.to_string_lossy())
                .with("HORCH_PROJECT_DIR", &root.to_string_lossy())
                .with("HORCH_ROLE", "opus-1");
            if let Some(id) = record {
                env = env.with("HORCH_RECORD_ID", id);
            }
            if let Some(id) = worker {
                env = env.with("HORCH_WORKER_ID", id);
            }
            RuntimeContext::from_env(&env).unwrap()
        };
        let both = ctx(Some("r-1"), Some(&wid));
        SessionLedger::open_in(&both)
            .unwrap()
            .add(horch_core::execution::records::NewRecord {
                record_id: "r-1",
                agent: "claude",
                tier: "opus",
                model: "opus",
                role: "opus-1",
                session_id: None,
                task: "t",
                phase: None,
            })
            .unwrap();

        note(&both, "step one").unwrap();
        note_as(&both, "milestone", EventKind::Progress).unwrap();
        note_as(&ctx(None, Some(&wid)), "ledger only", EventKind::Progress).unwrap();
        let err = note(&ctx(None, None), "x").unwrap_err();
        assert!(format!("{err:#}").contains("HORCH_RECORD_ID"), "{err:#}");

        let history: Vec<String> = SessionLedger::open_in(&both)
            .unwrap()
            .get("r-1")
            .unwrap()
            .history
            .into_iter()
            .filter(|h| h.event == "note")
            .map(|h| h.text)
            .collect();
        assert_eq!(history, ["step one", "milestone"]);
        let texts: Vec<String> = store
            .conn()
            .prepare("SELECT text FROM events WHERE worker_id = ?1 AND kind IN ('note', 'progress') ORDER BY rowid")
            .unwrap()
            .query_map([&wid], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(texts, ["step one", "milestone", "ledger only"]);
        let json: Vec<_> = std::fs::read_dir(&state)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".json"))
            .collect();
        assert!(json.is_empty(), "{json:?}");
    }

    #[test]
    fn trf_25_an_answer_line_names_the_question_id() {
        assert_eq!(
            answer_line("0a1b2c3d", "Use A"),
            "[orchestrator re #0a1b2c3d] Use A"
        );
        assert_eq!(
            answer_line("0a1b2c3d", "  [orchestrator] Use A"),
            "[orchestrator re #0a1b2c3d] Use A"
        );
    }

    #[test]
    fn trf_25_an_answer_goes_only_to_the_role_that_asked() {
        assert!(check_answer_role("opus-1", "opus-1", "0a1b2c3d").is_ok());
        let err = check_answer_role("opus-1", "opus-2", "0a1b2c3d")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("horch tell opus-1 --answers 0a1b2c3d"),
            "{err}"
        );
    }

    #[test]
    fn a_durable_line_without_a_courier_is_taken_back() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with("HORCH_STATE_DIR", &tmp.path().to_string_lossy())
                .with_exe("/nonexistent/horch"),
        )
        .unwrap();
        let err = enqueue_durable(
            &ctx,
            "w1",
            "orchestrator",
            "opus-1",
            "[opus-1] DONE: x",
            "DONE: x",
            true,
            true,
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("mail courier"), "{err:#}");
        let mail = Mail::under(&ctx.paths.state_root, "w1");
        assert!(mail.pending().unwrap().is_empty(), "no mail stays queued");
    }

    #[test]
    fn a_durable_wake_without_a_courier_stays_queued() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with("HORCH_STATE_DIR", &tmp.path().to_string_lossy())
                .with_exe("/nonexistent/horch"),
        )
        .unwrap();
        enqueue_durable(
            &ctx,
            "w1",
            "orchestrator",
            "horch",
            "[horch] all tasks are done",
            "all tasks are done",
            true,
            false,
        )
        .expect("the wake stays queued");
        let mail = Mail::under(&ctx.paths.state_root, "w1");
        assert_eq!(
            mail.pending().unwrap().len(),
            1,
            "the wake stays in the mail"
        );
    }

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

    #[test]
    fn w2_an_unset_variable_says_not_to_set_it_inline() {
        let text = require_env("HORCH_RECORD_ID", None)
            .unwrap_err()
            .to_string();
        assert!(text.starts_with("HORCH_RECORD_ID is unset"), "{text}");
        assert!(text.contains("Do not set it yourself"), "{text}");
        assert_eq!(require_env("HORCH_ROLE", Some("r".into())).unwrap(), "r");
    }

    fn failed(status: i32, args: &[&str]) -> anyhow::Error {
        GitError::Failed {
            args: args.iter().map(|a| a.to_string()).collect(),
            status,
            stderr: String::new(),
        }
        .into()
    }

    #[test]
    fn outside_repo_means_no_git_or_no_work_tree() {
        let toplevel = ["rev-parse", "--show-toplevel"];
        assert!(outside_repo(&failed(128, &toplevel)));
        assert!(!outside_repo(&failed(1, &toplevel)));
        assert!(!outside_repo(&failed(128, &["status"])));
        let missing = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert!(outside_repo(&missing.context("running git")));
        let denied = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert!(!outside_repo(&denied));
    }

    #[test]
    fn a_line_to_an_unregistered_role_names_the_known_roles() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = RuntimeContext::from_env(
            &horch_core::runtime::MapEnv::new("/")
                .with("HORCH_STATE_DIR", &tmp.path().to_string_lossy())
                .with_exe("/nonexistent/horch"),
        )
        .unwrap();
        let herdr = PaneHost::herdr_bin(&ctx.bins.harness.herdr);
        let mailbox = Mailbox::under(&ctx.paths.state_root, "w1");
        let tell = |role, sender| {
            not_registered(&Tell {
                ctx: &ctx,
                herdr: &herdr,
                mailbox: &mailbox,
                role,
                sender,
            })
            .to_string()
        };
        let dir = mailbox.dir().display().to_string();
        assert_eq!(
            tell("sonnet-9", Some("sonnet-1")),
            format!("role 'sonnet-9' is not registered in {dir}\n       known roles: none yet")
        );
        let hinted = tell("orchestrator", Some("sonnet-1"));
        assert!(
            hinted.contains("no orchestrator runs in this workspace"),
            "{hinted}"
        );
        let operator = tell("orchestrator", None);
        assert!(!operator.contains("no orchestrator runs"), "{operator}");
    }

    #[test]
    fn only_the_orchestrator_pane_acknowledges_the_inbox() {
        let public = |pane: &str| Some(format!("w1:{pane}"));
        assert!(may_acknowledge(Some("p1"), public, Some("w1:p1")));
        assert!(may_acknowledge(Some("w1:p1"), |_| None, Some("w1:p1")));
        assert!(!may_acknowledge(Some("p2"), public, Some("w1:p1")));
        assert!(!may_acknowledge(Some("w1:p2"), |_| None, Some("w1:p1")));
        assert!(may_acknowledge(None, public, Some("w1:p1")));
        assert!(may_acknowledge(Some("p2"), public, None));
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

    mod note_warning {
        use super::super::context::testkit::*;
        use super::super::*;

        fn sonnet_warning(w: &World) -> String {
            let vars = BTreeMap::from([
                ("role", "sonnet-1"),
                ("tokens", "311225"),
                ("threshold", "300000"),
                ("handoff", "ai_docs/handoffs/sonnet-1-whats-next.md"),
            ]);
            prompts::context_message(&w.roster, "warning", &vars).unwrap()
        }

        fn note_in(w: &World, id: &str, text: &str) -> Option<String> {
            w.ledger.note(id, text).unwrap();
            note_check(&w.sources(), id).ok().flatten()
        }

        #[test]
        fn ctx_10_note_warns_once_over_threshold() {
            let w = World::new();
            w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(OVER));
            let first = note_in(&w, "r-1", "step 1 done");
            assert_eq!(
                first.as_deref(),
                Some(
                    "NOTE: Context warning. Your context is 311225 tokens. Your threshold is \
                     300000 tokens. This horch note call is your stopping point. Do not start \
                     new work. Write ai_docs/handoffs/sonnet-1-whats-next.md with the \
                     horch:handoff skill. Then run: horch note \"handoff: \
                     ai_docs/handoffs/sonnet-1-whats-next.md\". Then send: horch tell \
                     orchestrator \"[sonnet-1] NOTE: COMPACT-READY \
                     ai_docs/handoffs/sonnet-1-whats-next.md\". Then end your turn. If DONE WHEN \
                     is met, run horch ledger finish, then horch ledger done (or horch done) instead."
                )
            );
            assert_eq!(first, Some(sonnet_warning(&w)));
            assert_eq!(note_in(&w, "r-1", "step 2 done"), None);
            let events: Vec<String> = w.history("r-1").into_iter().map(|(e, _)| e).collect();
            assert_eq!(events, ["spawned", "note", "context-warned", "note"]);
        }

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

        #[test]
        fn a_saved_note_with_a_refused_compaction_follow_up_exits_0() {
            let success = std::process::ExitCode::SUCCESS;
            let refused = Err(anyhow::anyhow!("handoff file is missing"));
            assert_eq!(context_followup_exit(Ok(refused)), success);
            assert_eq!(context_followup_exit(Ok(Ok(()))), success);
            assert_eq!(context_followup_exit(Err(Box::new("panic"))), success);
        }
    }
}
