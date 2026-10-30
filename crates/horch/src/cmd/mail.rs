//! Detached delivery of durable worker mail.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use horch_core::messaging::channel;
use horch_core::messaging::delivery;
use horch_core::messaging::mail::{redelivery_due, redelivery_offset, Id, Mail, Message};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::herdr::Herdr;
use horch_core::workspace::model::Pane;

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
        pane: Option<(String, Pane)>,
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
        if !(roles_reachable || has_target) {
            return Ok(self.wait_for_orchestrator());
        }
        self.no_orchestrator_since = None;
        if !has_target || check_login(&self.herdr, &self.mailbox, &self.mail)? {
            return Ok(Step::Wait);
        }
        Ok(Step::Deliver { pane, relay_live })
    }

    /// The orchestrator's pane, else whether its relay socket is live.
    fn orchestrator_target(&self, pending: &[Message]) -> (Option<(String, Pane)>, bool) {
        if pending.is_empty() {
            return (None, false);
        }
        // A pane id file can outlive its pane: a pane herdr does not answer
        // for is no target.
        let pane = self
            .mailbox
            .pane_for("orchestrator")
            .and_then(|id| self.herdr.pane_get(&id).ok().map(|state| (id, state)));
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

    fn deliver_due(&mut self, pending: &[Message], pane: Option<(String, Pane)>, relay_live: bool) {
        let idle = pane.as_ref().is_some_and(|(_, state)| state.at_prompt());
        redeliver_due(
            self.ctx,
            &mut self.attempts,
            pending,
            |message, line| match &pane {
                Some((id, _)) => send_if_idle(&self.herdr, id, idle, line),
                None if relay_live => {
                    let _ = channel::send(&self.socket, &message.from, line, channel::SEND_TIMEOUT);
                    true
                }
                None => false,
            },
        );
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
            let offset = redelivery_offset(attempt)?;
            let scaled = offset.as_millis() as u64 * first / 30_000;
            return created_at.checked_add(Duration::from_millis(scaled));
        }
    }
    redelivery_due(created_at, attempt)
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
    #[cfg(debug_assertions)]
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
        assert!(super::acquire_lock(&mail).unwrap().is_some());
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
