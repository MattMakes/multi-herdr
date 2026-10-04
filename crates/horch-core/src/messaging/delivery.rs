//! How `horch tell` types a line into another pane, as opposed to what the line
//! says ([`crate::messaging::message`]).
//!
//! Moved out of `workspace/herdr.rs` in A7, so it runs over any
//! [`WorkspaceClient`]. The order of the calls and their timing are unchanged.

use std::time::{Duration, Instant};

use anyhow::Result;

use crate::workspace::client::WorkspaceClient;

/// The waits [`send_line`] makes. [`Timing::DEFAULT`] is the real one; a test
/// shortens it so a pane that never shows the text does not cost 15 seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How long to poll the pane for the end of the message.
    pub tail_timeout: Duration,
    /// The first poll interval; it doubles up to `poll_max`.
    pub poll_start: Duration,
    pub poll_max: Duration,
    /// The settle before the one Enter when the tail never showed.
    pub settle_short: Duration,
    /// The same settle for a message over 1500 bytes.
    pub settle_long: Duration,
    /// The wait before the retry Enter when the tail did show.
    pub second_enter: Duration,
}

impl Timing {
    pub const DEFAULT: Timing = Timing {
        tail_timeout: Duration::from_secs(15),
        poll_start: Duration::from_millis(100),
        poll_max: Duration::from_secs(1),
        settle_short: Duration::from_secs(1),
        settle_long: Duration::from_secs(2),
        second_enter: Duration::from_secs(1),
    };
}

/// Type a line into another pane's terminal and submit it.
///
/// The first choice is `herdr agent prompt`, which submits text and Enter as
/// one atomic write. Separate `send-text` and `send-keys enter` calls race a
/// busy TUI: an Enter that lands while the text is still arriving submits a
/// head, and the tail becomes a second, untagged message that the receiver
/// reads as human input. That was the truncated-`DONE` bug.
///
/// When herdr detects no agent in the pane, fall back to the two-call path,
/// but press Enter only once the end of the message shows in the pane, not
/// after a fixed sleep. The second Enter is a retry that is safe only
/// because the whole text has landed (Enter on an empty input does nothing).
pub fn send_line(ws: &dyn WorkspaceClient, pane: &str, message: &str) -> Result<()> {
    send_line_with(ws, pane, message, &Timing::DEFAULT)
}

/// [`send_line`] with explicit waits.
pub fn send_line_with(
    ws: &dyn WorkspaceClient,
    pane: &str,
    message: &str,
    timing: &Timing,
) -> Result<()> {
    if ws.agent_prompt(pane, message).is_ok() {
        return Ok(());
    }
    ws.pane_send_text(pane, message)?;
    let landed = wait_for_tail(ws, pane, message, timing);
    if !landed {
        // Could not see it land (wrapped past recognition, or the pane
        // redraws oddly). Keep the old length-scaled settle as a last resort.
        let settle = if message.len() > 1500 {
            timing.settle_long
        } else {
            timing.settle_short
        };
        std::thread::sleep(settle);
    }
    ws.pane_send_keys(pane, "enter")?;
    if landed {
        std::thread::sleep(timing.second_enter);
        ws.pane_send_keys(pane, "enter")?;
    }
    Ok(())
}

/// Poll the pane until the last characters of `message` show in it.
fn wait_for_tail(ws: &dyn WorkspaceClient, pane: &str, message: &str, timing: &Timing) -> bool {
    let needle = tail_needle(message);
    if needle.is_empty() {
        return true;
    }
    let deadline = Instant::now() + timing.tail_timeout;
    let mut delay = timing.poll_start;
    loop {
        if let Ok(screen) = ws.pane_read(pane, "visible") {
            if squash(&screen).contains(&needle) {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(delay);
        delay = (delay * 2).min(timing.poll_max);
    }
}

/// Drop what a TUI adds or moves when it draws an input box: whitespace (soft
/// wraps) and box-drawing borders. What is left compares across a wrap.
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && !('\u{2500}'..='\u{257F}').contains(c))
        .collect()
}

/// The last 16 significant characters of `message`, in squashed form.
fn tail_needle(message: &str) -> String {
    let squashed: Vec<char> = squash(message).chars().collect();
    let start = squashed.len().saturating_sub(16);
    squashed[start..].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_needle_survives_a_soft_wrap_inside_a_box() {
        let msg = "[sonnet-1] DONE: No file was unclassifiable.";
        let needle = tail_needle(msg);
        assert_eq!(needle, "sunclassifiable.");
        let screen = "\u{2502} > [sonnet-1] DONE: No file was uncl \u{2502}\n\u{2502}   assifiable.      \u{2502}\n";
        assert!(squash(screen).contains(&needle));
        assert_eq!(tail_needle("   "), "");
    }
}
