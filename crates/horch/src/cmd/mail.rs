use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use horch_core::messaging::channel;
use horch_core::messaging::delivery;
use horch_core::messaging::mail::{
    digest_line, is_note, redelivery_due, Id, Mail, Message, DIGEST_FIRST, DIGEST_GAP,
};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::PaneHost;

const POLL: Duration = Duration::from_secs(5);

pub fn start_courier(ctx: &RuntimeContext, workspace: &str) -> Result<()> {
    horch_core::messaging::mail::start_courier(ctx, workspace)
}

enum Step {
    Stop,
    Retry,
    Wait,
    Deliver {
        pane: Option<(String, bool)>,
        relay_live: bool,
    },
}

struct Courier<'a> {
    ctx: &'a RuntimeContext,
    workspace: &'a str,
    mail: Mail,
    lock: File,
    herdr: PaneHost,
    mailbox: Mailbox,
    socket: PathBuf,
    poll: Duration,
    attempts: Attempts,
    orchestrator: OrchestratorPush,
}

pub fn courier(ctx: &RuntimeContext, workspace: &str) -> Result<()> {
    let mail = Mail::under(&ctx.paths.state_root, workspace);
    mail.ensure_dir()?;
    let Some(lock) = acquire_lock(&mail)? else {
        return Ok(());
    };
    Courier::new(ctx, workspace, mail, lock).run()
}

fn acquire_lock(mail: &Mail) -> Result<Option<File>> {
    let lock_path = mail.dir().join("courier.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("opening {}", lock_path.display()))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(error) => Err(error).with_context(|| format!("locking {}", lock_path.display())),
    }
}

fn send_if_idle(herdr: &PaneHost, id: &str, idle: bool, line: &str) -> bool {
    if idle {
        let _ = delivery::send_line(herdr, id, line);
    }
    idle
}

type RoleMail = Vec<(String, Mail, Vec<Message>)>;

impl<'a> Courier<'a> {
    fn new(ctx: &'a RuntimeContext, workspace: &'a str, mail: Mail, lock: File) -> Self {
        Self {
            ctx,
            workspace,
            mail,
            lock,
            herdr: PaneHost::for_workspace(ctx, workspace).with_timeout(Duration::from_secs(5)),
            mailbox: Mailbox::in_context(ctx, workspace),
            socket: channel::socket_path(&ctx.paths.state_root, workspace),
            poll: test_duration("HORCH_TEST_MAIL_POLL_MS", POLL),
            attempts: HashMap::new(),
            orchestrator: OrchestratorPush::default(),
        }
    }

    fn run(&mut self) -> Result<()> {
        loop {
            super::taskledger_wake::drain_workspace(self.ctx, self.workspace);
            let pending = self.mail.pending()?;
            let role_mail = pending_role_mail(&self.mail);
            match self.next_step(&pending, &role_mail)? {
                Step::Stop => return Ok(()),
                Step::Retry => {}
                Step::Wait => std::thread::sleep(self.poll),
                Step::Deliver { pane, relay_live } => {
                    self.deliver_due(&pending, pane, relay_live);
                    std::thread::sleep(self.poll);
                }
            }
        }
    }

    fn next_step(&mut self, pending: &[Message], role_mail: &RoleMail) -> Result<Step> {
        if pending.is_empty() && role_mail.is_empty() {
            return self.settle_empty();
        }
        let roles_reachable = self.deliver_roles(role_mail);
        let (pane, relay_live) = self.orchestrator_target(pending);
        let has_target = pane.is_some() || relay_live;
        if !(roles_reachable || has_target) {
            self.orchestrator.observe(pending, true);
            return Ok(Step::Wait);
        }
        let missed = !has_target || check_login(&self.herdr, &self.mailbox, &self.mail)?;
        self.orchestrator.observe(pending, missed);
        if missed {
            return Ok(Step::Wait);
        }
        Ok(Step::Deliver { pane, relay_live })
    }

    fn orchestrator_target(&self, pending: &[Message]) -> (Option<(String, bool)>, bool) {
        if pending.is_empty() {
            return (None, false);
        }
        if channel::is_live(&self.socket) {
            return (None, true);
        }
        let pane = self.mailbox.pane_for("orchestrator").and_then(|id| {
            self.herdr
                .pane_get(&id)
                .ok()
                .map(|state| (id, state.at_prompt()))
        });
        (pane, false)
    }

    fn settle_empty(&self) -> Result<Step> {
        let _ = self.lock.unlock();
        let still = !self.mail.pending()?.is_empty() || !pending_role_mail(&self.mail).is_empty();
        if !still || self.lock.try_lock().is_err() {
            return Ok(Step::Stop);
        }
        Ok(Step::Retry)
    }

    fn deliver_roles(&mut self, role_mail: &RoleMail) -> bool {
        let registered = if role_mail.is_empty() {
            Vec::new()
        } else {
            self.mailbox.roles()
        };
        let mut reachable = false;
        for (dir_name, role_box, messages) in role_mail {
            reachable |= self.deliver_role(&registered, dir_name, role_box, messages);
        }
        reachable
    }

    fn deliver_role(
        &mut self,
        registered: &[(String, String)],
        dir_name: &str,
        role_box: &Mail,
        messages: &[Message],
    ) -> bool {
        let state_root = &self.ctx.paths.state_root;
        let Some((_, id)) = registered.iter().find(|(role, _)| {
            Mail::to_role(state_root, self.workspace, role).dir() == role_box.dir()
        }) else {
            if let Err(error) = role_box.ack_all() {
                eprintln!("horch: acknowledging the mail of {dir_name} failed: {error:#}");
            }
            return false;
        };
        let Ok(state) = self.herdr.pane_get(id) else {
            return false;
        };
        let idle = state.at_prompt();
        redeliver_due(self.ctx, &mut self.attempts, messages, |_, line| {
            send_if_idle(&self.herdr, id, idle, line)
        });
        true
    }

    fn deliver_due(&mut self, pending: &[Message], pane: Option<(String, bool)>, relay_live: bool) {
        let now = mail_now(self.ctx);
        self.orchestrator
            .push(pending, now, |from, line| match &pane {
                Some((id, idle)) => send_if_idle(&self.herdr, id, *idle, line),
                None if relay_live => {
                    let _ = channel::send(&self.socket, from, line, channel::SEND_TIMEOUT);
                    true
                }
                None => false,
            });
    }
}

fn pending_role_mail(mail: &Mail) -> Vec<(String, Mail, Vec<Message>)> {
    let roles = match mail.roles() {
        Ok(roles) => roles,
        Err(error) => {
            eprintln!("horch: {error:#}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for (role, role_mail) in roles {
        match role_mail.pending() {
            Ok(pending) if pending.is_empty() => {}
            Ok(pending) => out.push((role, role_mail, pending)),
            Err(error) => eprintln!("horch: the mail of {role} is not read: {error:#}"),
        }
    }
    out
}

type Attempts = HashMap<Id, (usize, Option<SystemTime>)>;

#[derive(Default)]
struct OrchestratorPush {
    resends: HashMap<Id, Resend>,
    last_digest: Option<SystemTime>,
}

#[derive(Clone, Copy)]
enum Resend {
    Missed,
    IfUnpushed,
    Sent,
}

impl OrchestratorPush {
    fn observe(&mut self, messages: &[Message], missed: bool) {
        self.resends
            .retain(|id, _| messages.iter().any(|m| &m.id == id));
        for message in messages {
            self.resends.entry(message.id.clone()).or_insert(if missed {
                Resend::Missed
            } else {
                Resend::IfUnpushed
            });
        }
    }

    fn push(
        &mut self,
        messages: &[Message],
        now: SystemTime,
        mut send: impl FnMut(&str, &str) -> bool,
    ) {
        for message in messages {
            let Some(resend) = self.resends.get_mut(&message.id) else {
                continue;
            };
            let owes = match resend {
                Resend::Missed => true,
                Resend::IfUnpushed => !message.first_push,
                Resend::Sent => false,
            };
            let due = due_at(message.created_at, 1).is_some_and(|due| due <= now);
            if owes && due {
                let line = format!(
                    "{} (redelivery 1: not acknowledged; run horch inbox)",
                    message.line()
                );
                if send(&message.from, &line) {
                    *resend = Resend::Sent;
                }
            }
        }
        let unread: Vec<Message> = messages
            .iter()
            .filter(|m| !is_note(&m.text))
            .cloned()
            .collect();
        let Some(oldest) = unread.iter().map(|m| m.created_at).min() else {
            return;
        };
        let first_due = oldest
            .checked_add(scaled(DIGEST_FIRST))
            .is_some_and(|due| due <= now);
        let gap_over = self
            .last_digest
            .is_none_or(|last| now.duration_since(last).unwrap_or_default() >= scaled(DIGEST_GAP));
        if first_due && gap_over && send("horch", &digest_line(&unread)) {
            self.last_digest = Some(now);
        }
    }
}

fn redeliver_due(
    ctx: &RuntimeContext,
    attempts: &mut Attempts,
    messages: &[Message],
    mut send: impl FnMut(&Message, &str) -> bool,
) {
    for message in messages {
        let next = attempts.entry(message.id.clone()).or_insert_with(|| {
            let attempt = first_attempt_after(message.created_at, mail_now(ctx));
            (attempt, due_at(message.created_at, attempt))
        });
        let now = mail_now(ctx);
        let Some(due) = next.1 else {
            continue;
        };
        if now < due {
            continue;
        }
        let latest_due = if due_at(message.created_at, 6).is_some_and(|due| due <= now) {
            first_attempt_after(message.created_at, now)
        } else {
            (1..=6)
                .rev()
                .find(|n| due_at(message.created_at, *n).is_some_and(|due| due <= now))
                .unwrap_or(1)
        };
        let attempt = next.0.max(latest_due);
        let line = format!(
            "{} (redelivery {attempt}: not acknowledged; run horch inbox)",
            message.line()
        );
        if send(message, &line) {
            let following = attempt.saturating_add(1);
            let gap = due_at(message.created_at, following)
                .zip(due_at(message.created_at, attempt))
                .and_then(|(after, before)| after.duration_since(before).ok());
            *next = (
                following,
                gap.and_then(|gap| mail_now(ctx).checked_add(gap)),
            );
        }
    }
}

fn first_attempt_after(created_at: SystemTime, now: SystemTime) -> usize {
    let age = now.duration_since(created_at).unwrap_or_default();
    for attempt in 1..=6 {
        if due_at(created_at, attempt).is_some_and(|due| due > now) {
            return attempt;
        }
    }
    let base = due_at(created_at, 6).unwrap_or(created_at);
    let gap = due_at(created_at, 7)
        .and_then(|d| d.duration_since(base).ok())
        .unwrap_or(Duration::from_secs(300));
    let late_age = now.duration_since(base).unwrap_or(age);
    6usize.saturating_add((late_age.as_millis() / gap.as_millis().max(1)) as usize)
}

fn due_at(created_at: SystemTime, attempt: usize) -> Option<SystemTime> {
    #[cfg(debug_assertions)]
    if let Ok(milliseconds) = std::env::var("HORCH_TEST_MAIL_REDELIVERY_MS") {
        if let Ok(first) = milliseconds.parse::<u64>() {
            let offset = horch_core::messaging::mail::redelivery_offset(attempt)?;
            let scaled = offset.as_millis() as u64 * first / 30_000;
            return created_at.checked_add(Duration::from_millis(scaled));
        }
    }
    redelivery_due(created_at, attempt)
}

fn scaled(duration: Duration) -> Duration {
    #[cfg(debug_assertions)]
    if let Some(first) = std::env::var("HORCH_TEST_MAIL_REDELIVERY_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
    {
        return Duration::from_millis(duration.as_millis() as u64 * first / 30_000);
    }
    duration
}

fn test_duration(name: &str, default: Duration) -> Duration {
    #[cfg(debug_assertions)]
    if let Some(milliseconds) = std::env::var(name).ok().and_then(|v| v.parse().ok()) {
        return Duration::from_millis(milliseconds);
    }
    let _ = name;
    default
}

fn mail_now(ctx: &RuntimeContext) -> SystemTime {
    #[cfg(any(test, debug_assertions))]
    if let Some(at) = ctx
        .settings
        .test_mail_clock_file
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .as_deref()
        .and_then(horch_core::clock::parse)
    {
        return at.into();
    }
    let _ = ctx;
    horch_core::clock::now().into()
}

pub(super) fn check_login(herdr: &PaneHost, mailbox: &Mailbox, mail: &Mail) -> Result<bool> {
    let herdr = herdr.clone().with_timeout(Duration::from_secs(5));
    if let Some(pane) = mailbox.pane_for("orchestrator") {
        if let Ok(screen) = herdr.pane_read(&pane, "recent-unwrapped") {
            let required = horch_core::messaging::mail::needs_login(&screen);
            if mail.set_login_required(required)? {
                if let Err(error) = herdr.notification_show(
                    "Orchestrator needs /login",
                    "The orchestrator needs /login in its Claude Code pane. Mail remains queued.",
                ) {
                    eprintln!("horch: login notification failed: {error:#}");
                }
            }
            return Ok(required);
        }
    }
    Ok(mail.login_required())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use horch_core::messaging::mail::Mail;

    use super::first_attempt_after;

    #[test]
    fn a_courier_that_cannot_start_is_an_error() {
        use horch_core::runtime::{MapEnv, RuntimeContext};
        let gone =
            RuntimeContext::from_env(&MapEnv::new("/").with_exe("/nonexistent/horch")).unwrap();
        let err = super::start_courier(&gone, "w1").unwrap_err();
        assert!(
            format!("{err:#}").contains("starting the mail courier"),
            "{err:#}"
        );
        let unknown = RuntimeContext::from_env(&MapEnv::new("/")).unwrap();
        assert!(super::start_courier(&unknown, "w1").is_err());
    }

    #[test]
    fn a_redelivery_that_is_not_sent_stays_due() {
        use horch_core::messaging::mail::Mail;
        use horch_core::runtime::{MapEnv, RuntimeContext};
        let tmp = tempfile::tempdir().unwrap();
        let clock = tmp.path().join("clock");
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/").with("HORCH_TEST_MAIL_CLOCK_FILE", clock.to_str().unwrap()),
        )
        .unwrap();
        let mail = Mail::to_role(tmp.path(), "w1", "sonnet-1");
        mail.enqueue("horch", "the reviews are in").unwrap();
        let messages = mail.pending().unwrap();
        let created: chrono::DateTime<chrono::Utc> = messages[0].created_at.into();
        let mut attempts = super::Attempts::new();
        std::fs::write(&clock, horch_core::clock::stamp(created)).unwrap();
        super::redeliver_due(&ctx, &mut attempts, &messages, |_, _| {
            panic!("nothing is due at once")
        });
        let later = created + chrono::Duration::seconds(31);
        std::fs::write(&clock, horch_core::clock::stamp(later)).unwrap();

        let mut busy = 0;
        super::redeliver_due(&ctx, &mut attempts, &messages, |_, _| {
            busy += 1;
            false
        });
        let mut sent = Vec::new();
        super::redeliver_due(&ctx, &mut attempts, &messages, |_, line| {
            sent.push(line.to_string());
            true
        });
        let mut again = 0;
        super::redeliver_due(&ctx, &mut attempts, &messages, |_, _| {
            again += 1;
            true
        });

        assert_eq!(busy, 1);
        assert_eq!(
            sent,
            [format!(
                "[horch #{}] the reviews are in (redelivery 1: not acknowledged; run horch inbox)",
                messages[0].id.short()
            )]
        );
        assert_eq!(again, 0, "redelivery 2 is not due 31 s after the mail");
    }

    #[test]
    fn a_new_message_owes_the_first_redelivery() {
        let at = SystemTime::UNIX_EPOCH;
        assert_eq!(first_attempt_after(at, at), 1);
    }

    #[test]
    fn a_restarted_courier_skips_redeliveries_already_due() {
        let at = SystemTime::UNIX_EPOCH;
        assert_eq!(first_attempt_after(at, at + Duration::from_secs(100)), 3);
    }

    #[test]
    fn a_restarted_courier_continues_on_the_fixed_interval() {
        let at = SystemTime::UNIX_EPOCH;
        assert_eq!(first_attempt_after(at, at + Duration::from_secs(1050)), 6);
        assert_eq!(
            first_attempt_after(at, at + Duration::from_secs(86_400)),
            290
        );
    }

    fn courier_for(tmp: &std::path::Path) -> (horch_core::runtime::RuntimeContext, Mail) {
        use horch_core::runtime::{MapEnv, RuntimeContext};
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/").with("HORCH_STATE_DIR", tmp.to_str().unwrap()),
        )
        .unwrap();
        let mail = Mail::under(&ctx.paths.state_root, "w1");
        mail.ensure_dir().unwrap();
        (ctx, mail)
    }

    #[test]
    fn a_held_lock_leaves_the_second_courier_without_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (_ctx, mail) = courier_for(tmp.path());
        let first = super::acquire_lock(&mail).unwrap();
        assert!(first.is_some());
        assert!(super::acquire_lock(&mail).unwrap().is_none());
        drop(first);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while super::acquire_lock(&mail).unwrap().is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "the dropped lock stayed held for 5 s"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn an_empty_queue_stops_and_mail_that_arrives_meanwhile_retries() {
        let tmp = tempfile::tempdir().unwrap();
        let (ctx, mail) = courier_for(tmp.path());
        let lock = super::acquire_lock(&mail).unwrap().unwrap();
        let courier = super::Courier::new(&ctx, "w1", mail.clone(), lock);
        assert!(matches!(courier.settle_empty().unwrap(), super::Step::Stop));

        mail.enqueue("worker", "late").unwrap();
        assert!(matches!(
            courier.settle_empty().unwrap(),
            super::Step::Retry
        ));
        assert!(super::acquire_lock(&mail).unwrap().is_none());
    }

    fn orchestrator_mail(
        tmp: &std::path::Path,
        texts: &[String],
        at: SystemTime,
        told: bool,
    ) -> Vec<horch_core::messaging::mail::Message> {
        let mail = Mail::under(tmp, "w1");
        for text in texts {
            let from = text[1..].split_once(']').unwrap().0;
            let id = mail.enqueue(from, text).unwrap();
            if told {
                mail.mark_pushed(&id).unwrap();
            }
        }
        let mut messages = mail.pending().unwrap();
        for message in &mut messages {
            message.created_at = at;
        }
        messages
    }

    fn an_hour_of_pushes(
        push: &mut super::OrchestratorPush,
        messages: &[horch_core::messaging::mail::Message],
        start: SystemTime,
        mut idle: impl FnMut(u64) -> bool,
    ) -> Vec<(u64, String)> {
        let mut sent = Vec::new();
        for second in (0..=3600).step_by(5) {
            let now = start + Duration::from_secs(second);
            push.push(messages, now, |_, line| {
                if idle(second) {
                    sent.push((second, line.to_string()));
                }
                idle(second)
            });
        }
        sent
    }

    #[test]
    fn a_reachable_orchestrator_with_25_unread_lines_gets_digests_only() {
        let tmp = tempfile::tempdir().unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let kinds = ["DONE", "QUESTION", "BLOCKED", "NOTE", "NOTE"];
        let texts: Vec<String> = (0..25)
            .map(|n| format!("[worker-{n}] {}: report text {n}", kinds[n % 5]))
            .collect();
        let messages = orchestrator_mail(tmp.path(), &texts, at, true);
        let mut push = super::OrchestratorPush::default();
        push.observe(&messages, false);

        let sent = an_hour_of_pushes(&mut push, &messages, at, |_| true);

        let seconds: Vec<u64> = sent.iter().map(|(s, _)| *s).collect();
        assert_eq!(seconds, [300, 1200, 2100, 3000], "{sent:?}");
        for (_, line) in &sent {
            assert!(
                line.starts_with("[horch] NOTE: 15 unread messages ("),
                "{line}"
            );
            assert!(line.contains("worker-0 DONE, worker-1 QUESTION, worker-2 BLOCKED"));
            assert!(!line.contains("worker-3"), "a NOTE is not in the digest");
            assert!(!line.contains("report text"), "{line}");
        }
    }

    #[test]
    fn mail_that_missed_its_first_push_gets_its_full_text_once() {
        let tmp = tempfile::tempdir().unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let texts = [
            "[sonnet-1] NOTE: progress".to_string(),
            "[sonnet-2] DONE: all of it".to_string(),
        ];
        let messages = orchestrator_mail(tmp.path(), &texts, at, true);
        let mut push = super::OrchestratorPush::default();
        push.observe(&messages, true);

        let sent = an_hour_of_pushes(&mut push, &messages, at, |second| second >= 60);

        let lines: Vec<&str> = sent.iter().map(|(_, l)| l.as_str()).collect();
        let resend = |m: &horch_core::messaging::mail::Message| {
            format!(
                "{} (redelivery 1: not acknowledged; run horch inbox)",
                m.line()
            )
        };
        assert_eq!(lines[..2], [resend(&messages[0]), resend(&messages[1])]);
        assert!(lines[0].starts_with("[sonnet-1 #"), "{lines:?}");
        assert_eq!(sent[0].0, 60, "the busy pane kept the resend due");
        let digest = "[horch] NOTE: 1 unread message (sonnet-2 DONE). Run horch inbox.";
        assert_eq!(lines[2..], [digest; 4]);
    }

    #[test]
    fn a_notice_with_no_first_push_arrives_once_as_full_text() {
        let tmp = tempfile::tempdir().unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let texts = ["[horch] NOTE: Tasks can start: T-35".to_string()];
        let messages = orchestrator_mail(tmp.path(), &texts, at, false);
        let mut push = super::OrchestratorPush::default();
        push.observe(&messages, false);
        let sent = an_hour_of_pushes(&mut push, &messages, at, |_| true);
        assert_eq!(
            sent,
            [(
                30,
                format!(
                    "[horch #{}] NOTE: Tasks can start: T-35 (redelivery 1: not acknowledged; run horch inbox)",
                    messages[0].id.short()
                )
            )]
        );
    }

    #[test]
    fn told_mail_first_seen_late_gets_the_digest_only() {
        let tmp = tempfile::tempdir().unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let texts = ["[sonnet-2] DONE: all of it".to_string()];
        let messages = orchestrator_mail(tmp.path(), &texts, at, true);
        let mut push = super::OrchestratorPush::default();
        push.observe(&messages, false);
        let late = at + Duration::from_secs(120);
        let sent = an_hour_of_pushes(&mut push, &messages, late, |_| true);
        let digest = "[horch] NOTE: 1 unread message (sonnet-2 DONE). Run horch inbox.";
        assert_eq!(sent.len(), 4, "{sent:?}");
        assert!(sent.iter().all(|(_, line)| line == digest), "{sent:?}");
    }

    #[test]
    fn no_digest_comes_after_the_mail_is_acknowledged() {
        let tmp = tempfile::tempdir().unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let texts = ["[sonnet-2] DONE: all of it".to_string()];
        let messages = orchestrator_mail(tmp.path(), &texts, at, true);
        let mut push = super::OrchestratorPush::default();
        push.observe(&messages, false);
        let mut sent = 0;
        push.push(&messages, at + Duration::from_secs(300), |_, _| {
            sent += 1;
            true
        });
        assert_eq!(sent, 1);
        for second in (300..=7200).step_by(5) {
            push.push(&[], at + Duration::from_secs(second), |_, line| {
                panic!("no mail is unread: {line}")
            });
        }
    }

    #[test]
    fn a_worker_pane_keeps_its_per_message_redelivery() {
        use horch_core::runtime::{MapEnv, RuntimeContext};
        let tmp = tempfile::tempdir().unwrap();
        let clock = tmp.path().join("clock");
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/").with("HORCH_TEST_MAIL_CLOCK_FILE", clock.to_str().unwrap()),
        )
        .unwrap();
        let mail = Mail::to_role(tmp.path(), "w1", "sonnet-1");
        mail.enqueue("horch", "[horch] NOTE: the reviews are in")
            .unwrap();
        let messages = mail.pending().unwrap();
        let created: chrono::DateTime<chrono::Utc> = messages[0].created_at.into();
        let mut attempts = super::Attempts::new();
        let mut sent = Vec::new();
        for second in (0..=3600).step_by(5) {
            let now = created + chrono::Duration::seconds(second);
            std::fs::write(&clock, horch_core::clock::stamp(now)).unwrap();
            super::redeliver_due(&ctx, &mut attempts, &messages, |_, line| {
                sent.push(line.to_string());
                true
            });
        }
        assert_eq!(sent.len(), 14, "{sent:?}");
        let tag = format!(
            "[horch #{}] NOTE: the reviews are in",
            messages[0].id.short()
        );
        assert!(
            sent[13].starts_with(&format!("{tag} (redelivery 14:")),
            "{sent:?}"
        );
    }

    #[test]
    fn trf_03_a_redelivery_repeats_the_same_mail_id() {
        use horch_core::runtime::{MapEnv, RuntimeContext};
        let tmp = tempfile::tempdir().unwrap();
        let clock = tmp.path().join("clock");
        let ctx = RuntimeContext::from_env(
            &MapEnv::new("/").with("HORCH_TEST_MAIL_CLOCK_FILE", clock.to_str().unwrap()),
        )
        .unwrap();
        let mail = Mail::under(tmp.path(), "w1");
        let id = mail.enqueue("sonnet-1", "[sonnet-1] NOTE: x").unwrap();
        let messages = mail.pending().unwrap();
        let created: chrono::DateTime<chrono::Utc> = messages[0].created_at.into();
        let mut attempts = super::Attempts::new();
        let mut sent = Vec::new();
        for seconds in [0, 31, 91] {
            let at = created + chrono::Duration::seconds(seconds);
            std::fs::write(&clock, horch_core::clock::stamp(at)).unwrap();
            super::redeliver_due(&ctx, &mut attempts, &messages, |_, line| {
                sent.push(line.to_string());
                true
            });
        }

        let tag = format!("[sonnet-1 #{}] NOTE: x (redelivery ", id.short());
        assert_eq!(sent.len(), 2, "{sent:?}");
        assert!(sent[0].starts_with(&format!("{tag}1:")), "{sent:?}");
        assert!(sent[1].starts_with(&format!("{tag}2:")), "{sent:?}");
    }

    #[test]
    fn trf_04_a_courier_with_pending_mail_and_no_target_does_not_exit() {
        let tmp = tempfile::tempdir().unwrap();
        let (ctx, mail) = courier_for(tmp.path());
        let lock = super::acquire_lock(&mail).unwrap().unwrap();
        let mut courier = super::Courier::new(&ctx, "w1", mail.clone(), lock);
        mail.enqueue("sonnet-1", "[sonnet-1] NOTE: x").unwrap();
        let pending = mail.pending().unwrap();
        for _ in 0..3 {
            let step = courier.next_step(&pending, &Vec::new()).unwrap();
            assert!(matches!(step, super::Step::Wait));
        }

        mail.ack_all().unwrap();
        let step = courier.next_step(&[], &Vec::new()).unwrap();
        assert!(matches!(step, super::Step::Stop));
    }
}
