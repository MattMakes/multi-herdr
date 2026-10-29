//! Detached delivery of durable worker mail.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use horch_core::messaging::channel;
use horch_core::messaging::delivery;
use horch_core::messaging::mail::{redelivery_due, redelivery_offset, Id, Mail};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::runtime::{process, RuntimeContext};
use horch_core::workspace::herdr::Herdr;

const POLL: Duration = Duration::from_secs(5);
const NO_ORCHESTRATOR_LIMIT: Duration = Duration::from_secs(10 * 60);

pub fn start_courier(ctx: &RuntimeContext, workspace: &str) -> Result<()> {
    let Some(exe) = ctx.bins.current_exe.clone() else {
        anyhow::bail!("the mail courier cannot start: the horch binary path is unknown");
    };
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(["mail", "courier", "--workspace", workspace])
        .env("HORCH_STATE_DIR", &ctx.paths.state_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    process::spawn_detached(&mut cmd)
        .with_context(|| format!("starting the mail courier {}", exe.display()))?;
    Ok(())
}

pub fn courier(ctx: &RuntimeContext, workspace: &str) -> Result<()> {
    let mail = Mail::under(&ctx.paths.state_root, workspace);
    std::fs::create_dir_all(mail.dir())
        .with_context(|| format!("creating {}", mail.dir().display()))?;
    let lock_path = mail.dir().join("courier.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("opening {}", lock_path.display()))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Ok(()),
        Err(error) => {
            return Err(error).with_context(|| format!("locking {}", lock_path.display()))
        }
    }

    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr).with_timeout(Duration::from_secs(5));
    let mailbox = Mailbox::in_context(ctx, workspace);
    let socket = channel::socket_path(&ctx.paths.state_root, workspace);
    let poll = test_duration("HORCH_TEST_MAIL_POLL_MS", POLL);
    let no_orchestrator_limit =
        test_duration("HORCH_TEST_MAIL_NO_ORCHESTRATOR_MS", NO_ORCHESTRATOR_LIMIT);
    let mut attempts: HashMap<Id, (usize, Option<SystemTime>)> = HashMap::new();
    let mut no_orchestrator_since = None;

    loop {
        let pending = mail.pending()?;
        if pending.is_empty() {
            // A message enqueued while this courier was deciding to exit
            // found the lock held, so its own courier exited at once. Look
            // once more with the lock released, and take it back for it.
            let _ = lock.unlock();
            if mail.pending()?.is_empty() || lock.try_lock().is_err() {
                return Ok(());
            }
            continue;
        }
        // A pane id file can outlive its pane: a pane herdr does not answer
        // for is no orchestrator.
        let pane = mailbox
            .pane_for("orchestrator")
            .and_then(|id| herdr.pane_get(&id).ok().map(|state| (id, state)));
        let relay_live = pane.is_none() && channel::is_live(&socket);
        if pane.is_none() && !relay_live {
            let since = no_orchestrator_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= no_orchestrator_limit {
                return Ok(());
            }
            std::thread::sleep(poll);
            continue;
        }
        no_orchestrator_since = None;
        if check_login(&herdr, &mailbox, &mail)? {
            std::thread::sleep(poll);
            continue;
        }
        let idle = pane.as_ref().is_some_and(|(_, state)| state.at_prompt());

        for message in pending {
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
            let attempted = if let Some((id, _)) = &pane {
                if idle {
                    let _ = delivery::send_line(&herdr, id, &line);
                    true
                } else {
                    false
                }
            } else if relay_live {
                let _ = channel::send(&socket, &message.from, &line, channel::SEND_TIMEOUT);
                true
            } else {
                false
            };
            if attempted {
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
        std::thread::sleep(poll);
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
}
