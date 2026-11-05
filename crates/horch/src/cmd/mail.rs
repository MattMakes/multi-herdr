//! Detached delivery of durable worker mail.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use horch_core::messaging::channel;
use horch_core::messaging::delivery;
use horch_core::messaging::mail::{
    digest_line, is_note, redelivery_due, Id, Mail, Message, DIGEST_FIRST, DIGEST_GAP,
};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::herdr::Herdr;

const POLL: Duration = Duration::from_secs(5);
const NO_ORCHESTRATOR_LIMIT: Duration = Duration::from_secs(10 * 60);

pub fn start_courier(ctx: &RuntimeContext, workspace: &str) -> Result<()> {
    horch_core::messaging::mail::start_courier(ctx, workspace)
}

/// What one pass of the courier loop decides.
enum Step {
    /// Leave the loop.
    Stop,
    /// Look again at once.
    Retry,
    /// Sleep one poll, then look again.
    Wait,
    /// Redeliver the due orchestrator mail to `pane` or the relay.
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
    herdr: Herdr,
    mailbox: Mailbox,
    socket: PathBuf,
    poll: Duration,
    no_orchestrator_limit: Duration,
    attempts: Attempts,
    orchestrator: OrchestratorPush,
    no_orchestrator_since: Option<Instant>,
}

pub fn courier(ctx: &RuntimeContext, workspace: &str) -> Result<()> {
    let mail = Mail::under(&ctx.paths.state_root, workspace);
    mail.ensure_dir()?;
    let Some(lock) = acquire_lock(&mail)? else {
        return Ok(());
    };
    Courier::new(ctx, workspace, mail, lock).run()
}

/// Take the courier lock of `mail`. `None` means another courier holds it.
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

/// Send `line` to the pane `id` when it is idle. Returns whether it tried.
fn send_if_idle(herdr: &Herdr, id: &str, idle: bool, line: &str) -> bool {
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
            herdr: Herdr::with_bin(&ctx.bins.harness.herdr).with_timeout(Duration::from_secs(5)),
            mailbox: Mailbox::in_context(ctx, workspace),
            socket: channel::socket_path(&ctx.paths.state_root, workspace),
            poll: test_duration("HORCH_TEST_MAIL_POLL_MS", POLL),
            no_orchestrator_limit: test_duration(
                "HORCH_TEST_MAIL_NO_ORCHESTRATOR_MS",
                NO_ORCHESTRATOR_LIMIT,
            ),
            attempts: HashMap::new(),
            orchestrator: OrchestratorPush::default(),
            no_orchestrator_since: None,
        }
    }

    fn run(&mut self) -> Result<()> {
        loop {
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
        let now = mail_now(self.ctx);
        if !(roles_reachable || has_target) {
            self.orchestrator.observe(pending, true, now);
            return Ok(self.wait_for_orchestrator());
        }
        self.no_orchestrator_since = None;
        let missed = !has_target || check_login(&self.herdr, &self.mailbox, &self.mail)?;
        self.orchestrator.observe(pending, missed, now);
        if missed {
            return Ok(Step::Wait);
        }
        Ok(Step::Deliver { pane, relay_live })
    }

    /// The orchestrator's pane, else whether its relay socket is live.
    fn orchestrator_target(&self, pending: &[Message]) -> (Option<(String, bool)>, bool) {
        if pending.is_empty() {
            return (None, false);
        }
        // A pane id file can outlive its pane: a pane herdr does not answer
        // for is no target.
        let pane = self.mailbox.pane_for("orchestrator").and_then(|id| {
            self.herdr
                .pane_get(&id)
                .ok()
                .map(|state| (id, state.at_prompt()))
        });
        let relay_live = pane.is_none() && channel::is_live(&self.socket);
        (pane, relay_live)
    }

    /// The queue is empty. A message enqueued while this courier was deciding
    /// to exit found the lock held, so its own courier exited at once. Look
    /// once more with the lock released, and take it back for it.
    fn settle_empty(&self) -> Result<Step> {
        let _ = self.lock.unlock();
        let still = !self.mail.pending()?.is_empty() || !pending_role_mail(&self.mail).is_empty();
        if !still || self.lock.try_lock().is_err() {
            return Ok(Step::Stop);
        }
        Ok(Step::Retry)
    }

    /// No target is reachable: stop once that has lasted past the limit.
    fn wait_for_orchestrator(&mut self) -> Step {
        let since = self.no_orchestrator_since.get_or_insert_with(Instant::now);
        if since.elapsed() >= self.no_orchestrator_limit {
            Step::Stop
        } else {
            Step::Wait
        }
    }

    /// Redeliver the mail of each worker role. Returns whether a role pane
    /// answered. A worker pane gets its mail only when it is idle: no relay.
    fn deliver_roles(&mut self, role_mail: &RoleMail) -> bool {
        // A role's mail dir name is file safe, not the role: match the dirs.
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
            // The role is gone (`done` freed it): no one can answer its
            // notices, so they must not keep this courier alive.
            if let Err(error) = role_box.ack_all() {
                eprintln!("horch: acknowledging the mail of {dir_name} failed: {error:#}");
            }
            return false;
        };
        // A pane id file can outlive its pane.
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

/// The pending mail of each worker role that has some. A role whose mail
/// cannot be read is printed and left out: it must not stop the delivery
/// of the others or of the orchestrator's mail.
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

/// The courier's pushes to the orchestrator. Unread mail does not flood its
/// window: the full text goes again 1 time only when the first push can
/// have missed, and the rest is 1 digest line now and then.
#[derive(Default)]
struct OrchestratorPush {
    /// Whether each message the courier saw still owes its 1 full-text resend.
    owes: HashMap<Id, bool>,
    /// When the last digest went.
    last_digest: Option<SystemTime>,
}

impl OrchestratorPush {
    /// Record the mail the courier sees for the first time. Its first push
    /// can have missed when `missed` (no reachable pane, or /login), or when
    /// the mail is older than its first redelivery slot: no courier saw it
    /// in time, or no one pushed it.
    fn observe(&mut self, messages: &[Message], missed: bool, now: SystemTime) {
        self.owes
            .retain(|id, _| messages.iter().any(|m| &m.id == id));
        for message in messages {
            self.owes.entry(message.id.clone()).or_insert_with(|| {
                missed || due_at(message.created_at, 1).is_some_and(|due| due <= now)
            });
        }
    }

    /// Send the due full-text resends, then the due digest, with `send`
    /// (sender, line), which returns whether it attempted the delivery. A
    /// digest is a line only: it is not mail, so it is never redelivered.
    fn push(
        &mut self,
        messages: &[Message],
        now: SystemTime,
        mut send: impl FnMut(&str, &str) -> bool,
    ) {
        for message in messages {
            let Some(owes) = self.owes.get_mut(&message.id) else {
                continue;
            };
            let due = due_at(message.created_at, 1).is_some_and(|due| due <= now);
            if *owes && due {
                let line = format!(
                    "{} (redelivery 1: not acknowledged; run horch inbox)",
                    message.text
                );
                *owes = !send(&message.from, &line);
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

/// Send each message whose redelivery is due with `send`, which returns
/// whether it attempted the delivery, and schedule its next one.
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
        // A held pane may miss hours of scheduled retries. Skip those slots
        // rather than replaying them on consecutive courier polls.
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
            message.text
        );
        if send(message, &line) {
            let following = attempt.saturating_add(1);
            let gap = due_at(message.created_at, following)
                .zip(due_at(message.created_at, attempt))
                .and_then(|(after, before)| after.duration_since(before).ok());
            // Anchor the next retry to this actual attempt, including failed
            // pushes. A slow call or a suppressed pane cannot create a backlog.
            *next = (
                following,
                gap.and_then(|gap| mail_now(ctx).checked_add(gap)),
            );
        }
    }
}

/// Skip missed early retries; after the early schedule, retry the most recent
/// fixed-interval attempt immediately, then resume five-minute intervals.
fn first_attempt_after(created_at: SystemTime, now: SystemTime) -> usize {
    let age = now.duration_since(created_at).unwrap_or_default();
    // At most six early probes; the fixed tail is computed directly even for years-old mail.
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
    // A test sets the first gap; the rest keep the real schedule's shape.
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

/// `duration` on the scale a test sets for the first redelivery gap.
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

/// Read the recent screen on every pass, retaining the previous state on read failure.
/// The persisted create-new marker deduplicates notifications across tell and courier.
pub(super) fn check_login(herdr: &Herdr, mailbox: &Mailbox, mail: &Mail) -> Result<bool> {
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

    /// A courier that cannot start is an error, so `done` stops before it
    /// marks the record done (#67).
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

    /// A busy worker pane gets no line, and its redelivery stays due: the
    /// next poll that sends it sends redelivery 1, and then none is due.
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
        // The courier sees the mail when it is new: redelivery 1 is 30 s on.
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
            ["the reviews are in (redelivery 1: not acknowledged; run horch inbox)"]
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
        // 100 s old: attempts 1 (30 s) and 2 (90 s) fell due; 3 (210 s) is next.
        assert_eq!(first_attempt_after(at, at + Duration::from_secs(100)), 3);
    }

    #[test]
    fn a_restarted_courier_continues_on_the_fixed_interval() {
        let at = SystemTime::UNIX_EPOCH;
        // After the early schedule the fixed retry continues indefinitely.
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
        // The fork window: a process that another test thread spawns at the
        // drop holds a copy of the lock file until it execs, and the flock
        // stays held while any copy is open. The release comes, ms late.
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
        // The courier holds the lock again for the mail that arrived.
        assert!(super::acquire_lock(&mail).unwrap().is_none());
    }

    /// `texts` as orchestrator mail, all created at `at`.
    fn orchestrator_mail(
        tmp: &std::path::Path,
        texts: &[String],
        at: SystemTime,
    ) -> Vec<horch_core::messaging::mail::Message> {
        let mail = Mail::under(tmp, "w1");
        for text in texts {
            let from = text[1..].split_once(']').unwrap().0;
            mail.enqueue(from, text).unwrap();
        }
        let mut messages = mail.pending().unwrap();
        for message in &mut messages {
            message.created_at = at;
        }
        messages
    }

    /// Poll every 5 s for 1 hour from `start`. Returns each sent line with
    /// its age in seconds.
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

    /// The incident of 2026-11-05: a reachable orchestrator holds 25 unread
    /// lines. It gets digests only: no full text again, no NOTE.
    #[test]
    fn a_reachable_orchestrator_with_25_unread_lines_gets_digests_only() {
        let tmp = tempfile::tempdir().unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let kinds = ["DONE", "QUESTION", "BLOCKED", "NOTE", "NOTE"];
        let texts: Vec<String> = (0..25)
            .map(|n| format!("[worker-{n}] {}: report text {n}", kinds[n % 5]))
            .collect();
        let messages = orchestrator_mail(tmp.path(), &texts, at);
        let mut push = super::OrchestratorPush::default();
        push.observe(&messages, false, at + Duration::from_secs(1));

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

    /// Mail that can have missed its first push gets its full text 1 time.
    /// After that, a NOTE gets nothing, and other mail gets the digest. A
    /// busy pane keeps the resend due.
    #[test]
    fn mail_that_missed_its_first_push_gets_its_full_text_once() {
        let tmp = tempfile::tempdir().unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let texts = [
            "[sonnet-1] NOTE: progress".to_string(),
            "[sonnet-2] DONE: all of it".to_string(),
        ];
        let messages = orchestrator_mail(tmp.path(), &texts, at);
        let mut push = super::OrchestratorPush::default();
        push.observe(&messages, true, at);

        let sent = an_hour_of_pushes(&mut push, &messages, at, |second| second >= 60);

        let lines: Vec<&str> = sent.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            lines[..2],
            [
                "[sonnet-1] NOTE: progress (redelivery 1: not acknowledged; run horch inbox)",
                "[sonnet-2] DONE: all of it (redelivery 1: not acknowledged; run horch inbox)",
            ]
        );
        assert_eq!(sent[0].0, 60, "the busy pane kept the resend due");
        let digest = "[horch] NOTE: 1 unread message (sonnet-2 DONE). Run horch inbox.";
        assert_eq!(lines[2..], [digest; 4]);
    }

    /// Mail older than its first redelivery slot when the courier first sees
    /// it gets the full text 1 time, even with a reachable pane.
    #[test]
    fn mail_older_than_30_s_at_first_sight_gets_its_full_text_once() {
        let tmp = tempfile::tempdir().unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let texts = ["[horch] NOTE: All tasks are done.".to_string()];
        let messages = orchestrator_mail(tmp.path(), &texts, at);
        let mut push = super::OrchestratorPush::default();
        push.observe(&messages, false, at + Duration::from_secs(31));
        let sent = an_hour_of_pushes(&mut push, &messages, at, |_| true);
        assert_eq!(
            sent,
            [(
                30,
                "[horch] NOTE: All tasks are done. (redelivery 1: not acknowledged; run horch inbox)"
                    .to_string()
            )]
        );
    }

    /// After `horch inbox` acknowledges the mail, no digest comes.
    #[test]
    fn no_digest_comes_after_the_mail_is_acknowledged() {
        let tmp = tempfile::tempdir().unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let texts = ["[sonnet-2] DONE: all of it".to_string()];
        let messages = orchestrator_mail(tmp.path(), &texts, at);
        let mut push = super::OrchestratorPush::default();
        push.observe(&messages, false, at);
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

    /// A worker pane keeps its per-message redelivery: 30 s, 1 m, 2 m, 4 m,
    /// 5 m, 5 m, then every 5 min. That is 14 lines in 1 hour.
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
        assert!(sent[13].starts_with("[horch] NOTE: the reviews are in (redelivery 14:"));
    }

    #[test]
    fn a_courier_with_no_target_waits_until_the_limit_then_stops() {
        let tmp = tempfile::tempdir().unwrap();
        let (ctx, mail) = courier_for(tmp.path());
        let lock = super::acquire_lock(&mail).unwrap().unwrap();
        let mut courier = super::Courier::new(&ctx, "w1", mail, lock);
        courier.no_orchestrator_limit = Duration::from_secs(3600);
        assert!(matches!(courier.wait_for_orchestrator(), super::Step::Wait));
        assert!(courier.no_orchestrator_since.is_some());
        courier.no_orchestrator_limit = Duration::ZERO;
        assert!(matches!(courier.wait_for_orchestrator(), super::Step::Stop));
    }
}
