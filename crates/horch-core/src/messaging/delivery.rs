//! How `horch tell` types a line into another pane, as opposed to what the line
//! says ([`crate::messaging::message`]).
//!
//! Moved out of `workspace/herdr.rs` in A7, so it runs over any
//! [`WorkspaceClient`]. The order of the calls and their timing are unchanged.

use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use crate::workspace::client::WorkspaceClient;
use crate::workspace::model::Pane;

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
    let settle = if message.len() > 1500 {
        timing.settle_long
    } else {
        timing.settle_short
    };
    // Typing needs its full settle, or a busy TUI splits the line, so the
    // push budget keeps the longer of the two waits after the tail poll,
    // and time for the Enter calls. Without that much left, type nothing:
    // the line waits in the mail.
    let reserve = settle.max(timing.second_enter) + ENTER_CALLS;
    if push_time_left().is_some_and(|left| left < reserve) {
        bail!("the push budget is spent, so the line was not typed");
    }
    ws.pane_send_text(pane, message)?;
    // Typing takes time too, so the tail poll gets only what is left now.
    let mut timing = *timing;
    if let Some(left) = push_time_left() {
        timing.tail_timeout = timing.tail_timeout.min(left.saturating_sub(reserve));
    }
    let landed = wait_for_tail(ws, pane, message, &timing);
    if !landed {
        // Could not see it land (wrapped past recognition, or the pane
        // redraws oddly). Keep the old length-scaled settle as a last resort.
        std::thread::sleep(before_enter(settle));
    }
    ws.pane_send_keys(pane, "enter")?;
    if landed {
        std::thread::sleep(before_enter(timing.second_enter));
        ws.pane_send_keys(pane, "enter")?;
    }
    Ok(())
}

/// The time a push budget keeps for the herdr Enter calls after typing. The
/// push deadline ends a herdr call, and an Enter cut off leaves typed text
/// in the pane, so this covers 2 calls on a loaded machine, not a quick one.
const ENTER_CALLS: Duration = Duration::from_secs(1);

/// `wait`, cut so the push budget still has [`ENTER_CALLS`] after it: typed
/// text that is not submitted would join the next line typed into the pane.
fn before_enter(wait: Duration) -> Duration {
    push_time_left().map_or(wait, |left| wait.min(left.saturating_sub(ENTER_CALLS)))
}

/// How long a delivery waits for an agent that is still starting, and how
/// often it asks herdr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Readiness {
    pub timeout: Duration,
    pub poll: Duration,
}

impl Readiness {
    /// An opencode resume took 5 s to reach idle on the operator's Mac
    /// (2026-10-04); a loaded machine gets 24 times that.
    pub const DEFAULT: Readiness = Readiness {
        timeout: Duration::from_secs(120),
        poll: Duration::from_millis(250),
    };
}

/// herdr detects an agent in the pane but does not know its state yet: the
/// agent is starting. `herdr agent prompt` still succeeds then, and the text
/// is lost (opencode 1.18.34, LA-3).
fn starting(pane: &Pane) -> bool {
    pane.agent.as_deref().is_some_and(|a| !a.is_empty())
        && matches!(pane.agent_status.as_deref(), None | Some("unknown"))
}

/// Poll `pane` until `ready` holds. False when `wait.timeout` passes first.
/// A `pane_get` that fails counts as not ready.
fn wait_for(
    ws: &dyn WorkspaceClient,
    pane: &str,
    wait: &Readiness,
    ready: impl Fn(&Pane) -> bool,
) -> bool {
    let deadline = within_budget(Instant::now() + wait.timeout);
    loop {
        if ws.pane_get(pane).is_ok_and(|p| ready(&p)) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(
            wait.poll
                .min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

/// Type the first prompt of a launch into its pane once the agent there is
/// at its prompt ([`Pane::at_prompt`]: `idle` or `done`), then submit it
/// with [`send_line_with`].
///
/// For a harness that drops a prompt given on its command line, such as an
/// opencode resume. An error when the agent is not idle within
/// `wait.timeout`: nothing was typed, and the caller says so.
pub fn deliver_when_idle(
    ws: &dyn WorkspaceClient,
    pane: &str,
    text: &str,
    wait: &Readiness,
    timing: &Timing,
) -> Result<()> {
    // The push budget can cut the wait short: say how long it really was.
    let waited = capped(wait.timeout);
    if !wait_for(ws, pane, wait, Pane::at_prompt) {
        bail!(
            "the agent in pane {pane} was not idle after {} s, so its task was not typed",
            waited.as_secs()
        );
    }
    send_line_with(ws, pane, text, timing)
}

/// How long after a role registers `horch tell` treats a pane with no agent
/// as one whose agent has not shown yet. A worker registers before its agent
/// starts, and opencode took 1.6 s more to show (LA-3).
pub const TELL_GRACE: Duration = Duration::from_secs(30);

/// [`send_line_with`], but first wait out an agent that is still starting
/// in `pane`: what `horch tell` and `horch assign` do.
///
/// - herdr sees an agent that is starting (`starting`): wait until it is
///   not, up to `wait.timeout`, else an error and nothing typed.
/// - herdr sees no agent and `grace` is not zero (the role registered less
///   than [`TELL_GRACE`] ago): wait up to `grace` for an agent to show, then
///   as above. When none shows, the line goes as before.
/// - Otherwise (no agent and no grace, or an agent in a known state, a busy
///   one too) the line goes at once, exactly as before.
pub fn send_line_when_ready(
    ws: &dyn WorkspaceClient,
    pane: &str,
    message: &str,
    grace: Duration,
    wait: &Readiness,
    timing: &Timing,
) -> Result<()> {
    let has_agent = |p: &Pane| p.agent.as_deref().is_some_and(|a| !a.is_empty());
    let first = ws.pane_get(pane).ok();
    let mut booting = first.as_ref().is_some_and(starting);
    if !grace.is_zero() && first.as_ref().is_some_and(|p| !has_agent(p)) {
        let appear = Readiness {
            timeout: grace,
            poll: wait.poll,
        };
        booting = wait_for(ws, pane, &appear, has_agent);
    }
    let waited = capped(wait.timeout);
    if booting && !wait_for(ws, pane, wait, |p| !starting(p)) {
        bail!(
            "the agent in pane {pane} was still starting after {} s, so the message was not typed",
            waited.as_secs()
        );
    }
    send_line_with(ws, pane, message, timing)
}

/// The total time `horch done` lets all its pushes take: the report and the
/// ledger's wake lines. Claude Code kills a command after 120 s (P18).
pub const DONE_PUSH_BUDGET: Duration = Duration::from_secs(20);

thread_local! {
    static PUSH_DEADLINE: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

/// The push budget of one `horch done`, from [`start_push_budget`].
/// Dropping it ends the budget: later deliveries wait as they always have.
/// Not `Send`: the deadline it clears belongs to the thread that made it.
#[must_use = "the budget ends when this is dropped"]
pub struct PushBudget(std::marker::PhantomData<*const ()>);

impl Drop for PushBudget {
    fn drop(&mut self) {
        PUSH_DEADLINE.with(|deadline| deadline.set(None));
    }
}

/// Cap every push on this thread at `total` from now, while the returned
/// [`PushBudget`] lives: every wait of a delivery ends by then. `None` when
/// a budget runs already; the first one stays.
pub fn start_push_budget(total: Duration) -> Option<PushBudget> {
    PUSH_DEADLINE.with(|deadline| {
        if deadline.get().is_some() {
            return None;
        }
        deadline.set(Some(Instant::now() + total));
        Some(PushBudget(std::marker::PhantomData))
    })
}

/// When this thread's push budget ends. `None`: no budget started.
pub fn push_deadline() -> Option<Instant> {
    PUSH_DEADLINE.with(|deadline| deadline.get())
}

/// What is left of this thread's push budget. `None`: no budget started,
/// so a push waits as long as it always has.
pub fn push_time_left() -> Option<Duration> {
    PUSH_DEADLINE.with(|deadline| {
        deadline
            .get()
            .map(|at| at.saturating_duration_since(Instant::now()))
    })
}

/// `wait`, cut to what is left of the push budget.
pub fn capped(wait: Duration) -> Duration {
    push_time_left().map_or(wait, |left| wait.min(left))
}

/// `deadline`, moved up to the end of the push budget.
fn within_budget(deadline: Instant) -> Instant {
    PUSH_DEADLINE.with(|budget| budget.get().map_or(deadline, |end| deadline.min(end)))
}

/// Poll the pane until the last characters of `message` show in it.
fn wait_for_tail(ws: &dyn WorkspaceClient, pane: &str, message: &str, timing: &Timing) -> bool {
    let needle = tail_needle(message);
    if needle.is_empty() {
        return true;
    }
    let deadline = within_budget(Instant::now() + timing.tail_timeout);
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
        std::thread::sleep(delay.min(deadline.saturating_duration_since(Instant::now())));
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

    use crate::workspace::model::{Direction, NewWorkspace};
    use crate::workspace::testing::FakeWorkspace;

    /// #67: one budget bounds the grace and readiness waits together. An
    /// agent appears on the first grace poll and stays `unknown`; after
    /// readiness spends the budget, a fallback line must not be typed.
    #[test]
    fn the_push_budget_bounds_every_phase_together() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let budget = Duration::from_millis(600);
        let wait = Readiness {
            timeout: Duration::from_secs(120),
            poll: Duration::from_millis(200),
        };
        let starting = (Some("claude"), Some("unknown"));
        // Advance by calls, without sleeps before the agent appears. A
        // long no-agent script can spend the entire budget on a loaded
        // runner, never exercise readiness, and take the no-agent path.
        ws.set_agent_states(&pane, &[(None, None), starting]);
        let started = Instant::now();
        let guard = start_push_budget(budget).expect("no budget runs yet");
        assert!(
            start_push_budget(Duration::from_secs(60)).is_none(),
            "the first budget stays"
        );
        let grace = Duration::from_secs(30);
        let result =
            send_line_when_ready(&ws, &pane, "[r] DONE: x", grace, &wait, &Timing::DEFAULT);
        let error = format!("{:#}", result.unwrap_err());
        assert!(
            error.contains("still starting after 0 s"),
            "the wait it had: {error}"
        );
        assert!(
            started.elapsed() < budget + Duration::from_millis(300),
            "{:?}",
            started.elapsed()
        );

        // The budget is spent: a line that would need typing is not typed,
        // since a typed line needs its full settle.
        ws.fail_next("agent_prompt", "no agent");
        let spent = Instant::now();
        let typed = send_line_with(&ws, &pane, "[r] DONE: x", &Timing::DEFAULT);
        assert!(typed.is_err(), "nothing is typed");
        assert!(!ws.calls().iter().any(|c| c.method == "pane_send_text"));
        assert!(
            spent.elapsed() < Duration::from_millis(300),
            "{:?}",
            spent.elapsed()
        );

        drop(guard);
        assert_eq!(push_time_left(), None, "the budget ends with its guard");
    }

    /// #67: typed text keeps its full settle inside the budget. A tail that
    /// never shows ends early enough for the settle; a budget too short for
    /// the settle types nothing.
    #[test]
    fn a_typed_line_keeps_its_settle_inside_the_budget() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let timing = Timing {
            settle_short: Duration::from_millis(100),
            second_enter: Duration::from_millis(100),
            ..Timing::DEFAULT
        };
        // The settle and the Enter calls keep their part; the poll gets 250 ms.
        let budget = Duration::from_millis(350) + ENTER_CALLS;
        let started = Instant::now();
        let guard = start_push_budget(budget).expect("no budget runs yet");
        ws.fail_next("agent_prompt", "no agent");
        send_line_with(&ws, &pane, "[r] DONE: x", &timing).unwrap();
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(250),
            "the tail poll ran: {took:?}"
        );
        assert!(took < budget + Duration::from_millis(100), "{took:?}");
        drop(guard);

        let short = start_push_budget(Duration::from_millis(50)).expect("the guard ended");
        ws.fail_next("agent_prompt", "no agent");
        let before = ws.calls().len();
        assert!(send_line_with(&ws, &pane, "[r] DONE: y", &timing).is_err());
        let after = ws.calls();
        assert!(
            !after[before..].iter().any(|c| c.method == "pane_send_text"),
            "nothing is typed"
        );
        drop(short);
    }

    /// A workspace whose `pane_send_text` is slow, and that records when
    /// each Enter arrives. Every other call goes to the fake.
    struct SlowTyping {
        fake: FakeWorkspace,
        typing: Duration,
        enters: std::cell::RefCell<Vec<Instant>>,
    }

    impl WorkspaceClient for SlowTyping {
        fn pane_get(&self, pane: &str) -> Result<Pane> {
            self.fake.pane_get(pane)
        }
        fn pane_list(&self, workspace: &str) -> Result<Vec<Pane>> {
            self.fake.pane_list(workspace)
        }
        fn pane_split(&self, from: &str, direction: Direction) -> Result<String> {
            self.fake.pane_split(from, direction)
        }
        fn pane_run(&self, pane: &str, command: &str) -> Result<()> {
            self.fake.pane_run(pane, command)
        }
        fn pane_close(&self, pane: &str) -> Result<()> {
            self.fake.pane_close(pane)
        }
        fn agent_prompt(&self, _: &str, _: &str) -> Result<()> {
            bail!("no agent")
        }
        fn pane_send_text(&self, pane: &str, text: &str) -> Result<()> {
            std::thread::sleep(self.typing);
            self.fake.pane_send_text(pane, text)
        }
        fn pane_send_keys(&self, pane: &str, keys: &str) -> Result<()> {
            self.enters.borrow_mut().push(Instant::now());
            self.fake.pane_send_keys(pane, keys)
        }
        fn pane_read(&self, pane: &str, source: &str) -> Result<String> {
            self.fake.pane_read(pane, source)
        }
        fn workspace_create(
            &self,
            label: &str,
            cwd: Option<&str>,
            focus: bool,
        ) -> Result<NewWorkspace> {
            self.fake.workspace_create(label, cwd, focus)
        }
        fn workspace_close(&self, workspace: &str) -> Result<()> {
            self.fake.workspace_close(workspace)
        }
        fn server_reachable(&self) -> bool {
            self.fake.server_reachable()
        }
    }

    /// #67: typing that takes most of the budget still leaves time for the
    /// Enter calls, so typed text is never left unsubmitted at the deadline.
    #[test]
    fn slow_typing_still_presses_enter_inside_the_budget() {
        let ws = SlowTyping {
            fake: FakeWorkspace::new(),
            typing: Duration::from_millis(400),
            enters: Default::default(),
        };
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let timing = Timing {
            settle_short: Duration::from_millis(100),
            second_enter: Duration::from_millis(100),
            ..Timing::DEFAULT
        };
        // Enough to start typing, but typing eats into the reserve.
        let budget = Duration::from_millis(300) + ENTER_CALLS;
        let guard = start_push_budget(budget).expect("no budget runs yet");
        let deadline = push_deadline().expect("the budget runs");
        send_line_with(&ws, &pane, "[r] DONE: x", &timing).unwrap();
        drop(guard);
        let enters = ws.enters.borrow();
        assert!(!enters.is_empty(), "the typed line is submitted");
        for enter in enters.iter() {
            assert!(
                *enter + ENTER_CALLS / 2 <= deadline,
                "an Enter left too little time: {:?} before the deadline",
                deadline.saturating_duration_since(*enter)
            );
        }
    }

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
