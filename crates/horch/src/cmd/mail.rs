//! Detached delivery of durable worker mail.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use horch_core::messaging::channel;
use horch_core::messaging::delivery;
use horch_core::messaging::mail::{redelivery_due, redelivery_offset, Id, Mail, Message};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::runtime::{process, RuntimeContext};
use horch_core::workspace::herdr::Herdr;

const POLL: Duration = Duration::from_secs(5);
const NO_ORCHESTRATOR_LIMIT: Duration = Duration::from_secs(10 * 60);

pub fn start_courier(ctx: &RuntimeContext, workspace: &str) {
    let Some(exe) = ctx.bins.current_exe.clone() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["mail", "courier", "--workspace", workspace])
        .env("HORCH_STATE_DIR", &ctx.paths.state_root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let _ = process::spawn_detached(&mut cmd);
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

    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let mailbox = Mailbox::in_context(ctx, workspace);
    let socket = channel::socket_path(&ctx.paths.state_root, workspace);
    let poll = test_duration("HORCH_TEST_MAIL_POLL_MS", POLL);
    let no_orchestrator_limit =
        test_duration("HORCH_TEST_MAIL_NO_ORCHESTRATOR_MS", NO_ORCHESTRATOR_LIMIT);
    let mut attempts: HashMap<Id, usize> = HashMap::new();
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
        let exhausted = pending.iter().all(|message| {
            let attempt = *attempts
                .entry(message.id.clone())
                .or_insert_with(|| attempt_for_age(message));
            due_at(message.created_at, attempt).is_none()
        });
        if exhausted {
            // Nothing left to push; the mail stays pending for horch inbox.
            return Ok(());
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
        let idle = pane.as_ref().is_some_and(|(_, state)| state.at_prompt());

        for message in pending {
            let next = attempts
                .entry(message.id.clone())
                .or_insert_with(|| attempt_for_age(&message));
            let attempt = *next;
            let Some(due) = due_at(message.created_at, attempt) else {
                continue;
            };
            if SystemTime::now() < due {
                continue;
            }
            let line = format!(
                "{} (redelivery {attempt}: not acknowledged; run horch inbox)",
                message.text
            );
            if let Some((id, _)) = &pane {
                if idle {
                    let _ = delivery::send_line(&herdr, id, &line);
                    *next += 1;
                }
            } else if relay_live {
                let _ = channel::send(&socket, &message.from, &line, channel::SEND_TIMEOUT);
                *next += 1;
            }
        }
        std::thread::sleep(poll);
    }
}

fn attempt_for_age(message: &Message) -> usize {
    first_attempt_after(message.created_at, SystemTime::now())
}

/// The redelivery a courier that starts at `now` owes a message: the first
/// one not yet due. An attempt that fell due before this courier started is
/// skipped, never replayed: a courier per enqueue would otherwise resend it
/// once for every later message. 7 means none is left.
fn first_attempt_after(created_at: SystemTime, now: SystemTime) -> usize {
    (1..=6)
        .find(|attempt| due_at(created_at, *attempt).is_some_and(|due| due > now))
        .unwrap_or(7)
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
    if let Ok(milliseconds) = std::env::var(name) {
        if let Ok(milliseconds) = milliseconds.parse() {
            return Duration::from_millis(milliseconds);
        }
    }
    let _ = name;
    default
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::first_attempt_after;

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
    fn a_restarted_courier_never_replays_the_last_redelivery() {
        let at = SystemTime::UNIX_EPOCH;
        // Past the 6th (1050 s): nothing left, on every restart.
        assert_eq!(first_attempt_after(at, at + Duration::from_secs(1050)), 7);
        assert_eq!(first_attempt_after(at, at + Duration::from_secs(86_400)), 7);
    }
}
