use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use crate::fleetd::task_execution::domain::stall::DOORBELL;
use crate::messaging::mail::{Id, Mail};
use crate::runtime::Faults;
use crate::workspace::client::WorkspaceClient;
use crate::workspace::model::Pane;
use crate::workspace::paneshell::PaneShell;

pub const MAIL_LINE: &str = "You have new mail. Run horch inbox.";

pub const DELIVERY_ECHO_POSIX: &str = "echo HORCH_DELIVERY_$((70+3))";
pub const DELIVERY_ECHO_POWERSHELL: &str = "Write-Output \"HORCH_DELIVERY_$(70+3)\"";
pub const SMOKE_ECHO_POSIX: &str = "echo SMOKE_TEST_$((40+2))";
pub const SMOKE_ECHO_POWERSHELL: &str = "Write-Output \"SMOKE_TEST_$(40+2)\"";

pub const TYPED_LINES: [&str; 9] = [
    DOORBELL,
    MAIL_LINE,
    "/permissions",
    "/status",
    "/compact",
    DELIVERY_ECHO_POSIX,
    DELIVERY_ECHO_POWERSHELL,
    SMOKE_ECHO_POSIX,
    SMOKE_ECHO_POWERSHELL,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedLine {
    Doorbell,
    Mail,
    Permissions,
    Status,
    Compact,
    DeliveryEchoPosix,
    DeliveryEchoPowerShell,
    SmokeEchoPosix,
    SmokeEchoPowerShell,
}

impl TypedLine {
    pub const ALL: [TypedLine; 9] = [
        TypedLine::Doorbell,
        TypedLine::Mail,
        TypedLine::Permissions,
        TypedLine::Status,
        TypedLine::Compact,
        TypedLine::DeliveryEchoPosix,
        TypedLine::DeliveryEchoPowerShell,
        TypedLine::SmokeEchoPosix,
        TypedLine::SmokeEchoPowerShell,
    ];

    pub const fn text(self) -> &'static str {
        match self {
            TypedLine::Doorbell => DOORBELL,
            TypedLine::Mail => MAIL_LINE,
            TypedLine::Permissions => "/permissions",
            TypedLine::Status => "/status",
            TypedLine::Compact => "/compact",
            TypedLine::DeliveryEchoPosix => DELIVERY_ECHO_POSIX,
            TypedLine::DeliveryEchoPowerShell => DELIVERY_ECHO_POWERSHELL,
            TypedLine::SmokeEchoPosix => SMOKE_ECHO_POSIX,
            TypedLine::SmokeEchoPowerShell => SMOKE_ECHO_POWERSHELL,
        }
    }

    pub const fn delivery_echo(shell: PaneShell) -> Self {
        match shell {
            PaneShell::Posix => TypedLine::DeliveryEchoPosix,
            PaneShell::PowerShell => TypedLine::DeliveryEchoPowerShell,
        }
    }

    pub const fn smoke_echo(shell: PaneShell) -> Self {
        match shell {
            PaneShell::Posix => TypedLine::SmokeEchoPosix,
            PaneShell::PowerShell => TypedLine::SmokeEchoPowerShell,
        }
    }

    pub fn find(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.text() == text)
    }
}

pub fn post(
    ws: &dyn WorkspaceClient,
    pane: &str,
    mail: &Mail,
    from: &str,
    text: &str,
) -> Result<Id> {
    let id = mail.enqueue(from, text)?;
    send_line_when_ready(
        ws,
        pane,
        TypedLine::Mail,
        Duration::ZERO,
        &Readiness::DEFAULT,
        &Timing::DEFAULT,
    )?;
    mail.mark_pushed(&id)?;
    Ok(id)
}

pub fn prompt(ws: &dyn WorkspaceClient, pane: &str, line: TypedLine) -> Result<()> {
    ws.agent_prompt(pane, line.text())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub tail_timeout: Duration,
    pub poll_start: Duration,
    pub poll_max: Duration,
    pub settle_short: Duration,
    pub settle_long: Duration,
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

pub fn send_line(ws: &dyn WorkspaceClient, pane: &str, line: TypedLine) -> Result<()> {
    send_line_with(ws, pane, line, &Timing::DEFAULT)
}

pub fn send_line_with(
    ws: &dyn WorkspaceClient,
    pane: &str,
    line: TypedLine,
    timing: &Timing,
) -> Result<()> {
    let message = line.text();
    let prompt_err = match ws.agent_prompt(pane, line.text()) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    let settle = if message.len() > 1500 {
        timing.settle_long
    } else {
        timing.settle_short
    };
    let reserve = settle.max(timing.second_enter) + ENTER_CALLS;
    if push_time_left().is_some_and(|left| left < reserve) {
        bail!("the push budget is spent, so the line was not typed");
    }
    if let Err(type_err) = ws.pane_send_text(pane, line.text()) {
        if type_err.is::<crate::workspace::Unsupported>() {
            return Err(prompt_err);
        }
        return Err(type_err);
    }
    let mut timing = *timing;
    if let Some(left) = push_time_left() {
        timing.tail_timeout = timing.tail_timeout.min(left.saturating_sub(reserve));
    }
    let landed = wait_for_tail(ws, pane, message, &timing);
    if !landed {
        std::thread::sleep(before_enter(settle));
    }
    ws.pane_send_keys(pane, "enter")?;
    if landed {
        std::thread::sleep(before_enter(timing.second_enter));
        ws.pane_send_keys(pane, "enter")?;
    }
    Ok(())
}

const ENTER_CALLS: Duration = Duration::from_secs(1);

fn before_enter(wait: Duration) -> Duration {
    push_time_left().map_or(wait, |left| wait.min(left.saturating_sub(ENTER_CALLS)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Readiness {
    pub timeout: Duration,
    pub poll: Duration,
}

impl Readiness {
    pub const DEFAULT: Readiness = Readiness {
        timeout: Duration::from_secs(120),
        poll: Duration::from_millis(250),
    };
}

fn starting(pane: &Pane) -> bool {
    pane.agent.as_deref().is_some_and(|a| !a.is_empty())
        && matches!(pane.agent_status.as_deref(), None | Some("unknown"))
}

fn wait_for(
    ws: &dyn WorkspaceClient,
    pane: &str,
    wait: &Readiness,
    ready: impl Fn(&Pane) -> bool,
) -> bool {
    let deadline = within_budget(Instant::now() + wait.timeout);
    poll_until(deadline, wait.poll, wait.poll, || {
        ws.pane_get(pane).is_ok_and(|p| ready(&p))
    })
}

fn poll_until(
    deadline: Instant,
    mut delay: Duration,
    max_delay: Duration,
    mut done: impl FnMut() -> bool,
) -> bool {
    loop {
        if done() {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(delay.min(deadline.saturating_duration_since(Instant::now())));
        delay = (delay * 2).min(max_delay);
    }
}

pub fn deliver_when_idle(
    ws: &dyn WorkspaceClient,
    pane: &str,
    line: TypedLine,
    wait: &Readiness,
    timing: &Timing,
) -> Result<()> {
    let waited = capped(wait.timeout);
    if !wait_for(ws, pane, wait, Pane::at_prompt) {
        bail!(
            "the agent in pane {pane} was not idle after {} s, so its task was not typed",
            waited.as_secs()
        );
    }
    send_line_with(ws, pane, line, timing)
}

pub const TELL_GRACE: Duration = Duration::from_secs(30);

pub fn send_line_when_ready(
    ws: &dyn WorkspaceClient,
    pane: &str,
    line: TypedLine,
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
    send_line_with(ws, pane, line, timing)
}

pub const DONE_PUSH_BUDGET: Duration = Duration::from_secs(20);

pub fn done_push_budget(faults: &Faults) -> Duration {
    faults
        .indexed("done-push-budget-ms")
        .and_then(|ms| ms.parse::<u64>().ok())
        .map_or(DONE_PUSH_BUDGET, |ms| {
            Duration::from_millis(ms.clamp(1, 30_000))
        })
}

thread_local! {
    static PUSH_DEADLINE: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

#[must_use = "the budget ends when this is dropped"]
pub struct PushBudget(std::marker::PhantomData<*const ()>);

impl Drop for PushBudget {
    fn drop(&mut self) {
        PUSH_DEADLINE.with(|deadline| deadline.set(None));
    }
}

pub fn start_push_budget(total: Duration) -> Option<PushBudget> {
    PUSH_DEADLINE.with(|deadline| {
        if deadline.get().is_some() {
            return None;
        }
        deadline.set(Some(Instant::now() + total));
        Some(PushBudget(std::marker::PhantomData))
    })
}

pub fn push_deadline() -> Option<Instant> {
    PUSH_DEADLINE.with(|deadline| deadline.get())
}

pub fn push_time_left() -> Option<Duration> {
    PUSH_DEADLINE.with(|deadline| {
        deadline
            .get()
            .map(|at| at.saturating_duration_since(Instant::now()))
    })
}

pub fn capped(wait: Duration) -> Duration {
    push_time_left().map_or(wait, |left| wait.min(left))
}

fn within_budget(deadline: Instant) -> Instant {
    PUSH_DEADLINE.with(|budget| budget.get().map_or(deadline, |end| deadline.min(end)))
}

fn wait_for_tail(ws: &dyn WorkspaceClient, pane: &str, message: &str, timing: &Timing) -> bool {
    let needle = tail_needle(message);
    if needle.is_empty() {
        return true;
    }
    let deadline = within_budget(Instant::now() + timing.tail_timeout);
    poll_until(deadline, timing.poll_start, timing.poll_max, || {
        ws.pane_read(pane, "visible")
            .is_ok_and(|screen| squash(&screen).contains(&needle))
    })
}

fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && !('\u{2500}'..='\u{257F}').contains(c))
        .collect()
}

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

    #[test]
    fn poll_until_stops_at_the_first_true_and_sleeps_between_tests() {
        let mut calls = 0;
        let started = Instant::now();
        let delay = Duration::from_millis(20);
        let done = poll_until(started + Duration::from_secs(30), delay, delay, || {
            calls += 1;
            calls == 3
        });
        assert!(done);
        assert_eq!(calls, 3);
        assert!(
            started.elapsed() >= 2 * delay,
            "2 sleeps came before the 3rd test"
        );
        assert!(started.elapsed() < Duration::from_secs(30));
    }

    #[test]
    fn poll_until_gives_up_at_the_deadline_and_tests_once_more_first() {
        let mut calls = 0;
        let started = Instant::now();
        let done = poll_until(
            started + Duration::from_millis(60),
            Duration::from_millis(10),
            Duration::from_millis(20),
            || {
                calls += 1;
                false
            },
        );
        assert!(!done);
        assert!(started.elapsed() >= Duration::from_millis(60));
        assert!((2..=5).contains(&calls), "{calls} polls");
    }

    #[test]
    fn wait_for_tail_sees_the_message_tail_across_a_wrap_and_a_border() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        ws.set_screen(&pane, "│ hello wor │\n│ ld, again │");
        let timing = Timing {
            tail_timeout: Duration::from_millis(100),
            poll_start: Duration::from_millis(5),
            poll_max: Duration::from_millis(10),
            ..Timing::DEFAULT
        };
        assert!(wait_for_tail(&ws, &pane, "hello world, again", &timing));
        assert!(!wait_for_tail(&ws, &pane, "never shown anywhere", &timing));
        assert!(wait_for_tail(&ws, &pane, "", &timing));
    }

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
        ws.set_agent_states(&pane, &[(None, None), starting]);
        let started = Instant::now();
        let guard = start_push_budget(budget).expect("no budget runs yet");
        assert!(
            start_push_budget(Duration::from_secs(60)).is_none(),
            "the first budget stays"
        );
        let grace = Duration::from_secs(30);
        let result =
            send_line_when_ready(&ws, &pane, TypedLine::Mail, grace, &wait, &Timing::DEFAULT);
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

        ws.fail_next("agent_prompt", "no agent");
        let spent = Instant::now();
        let typed = send_line_with(&ws, &pane, TypedLine::Mail, &Timing::DEFAULT);
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

    #[test]
    fn a_typed_line_keeps_its_settle_inside_the_budget() {
        let ws = FakeWorkspace::new();
        let pane = ws.workspace_create("w", None, false).unwrap().root_pane_id;
        let timing = Timing {
            settle_short: Duration::from_millis(100),
            second_enter: Duration::from_millis(100),
            ..Timing::DEFAULT
        };
        let budget = Duration::from_millis(350) + ENTER_CALLS;
        let started = Instant::now();
        let guard = start_push_budget(budget).expect("no budget runs yet");
        ws.fail_next("agent_prompt", "no agent");
        send_line_with(&ws, &pane, TypedLine::Mail, &timing).unwrap();
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
        assert!(send_line_with(&ws, &pane, TypedLine::Mail, &timing).is_err());
        let after = ws.calls();
        assert!(
            !after[before..].iter().any(|c| c.method == "pane_send_text"),
            "nothing is typed"
        );
        drop(short);
    }

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
        let budget = Duration::from_millis(300) + ENTER_CALLS;
        let guard = start_push_budget(budget).expect("no budget runs yet");
        let deadline = push_deadline().expect("the budget runs");
        send_line_with(&ws, &pane, TypedLine::Mail, &timing).unwrap();
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

    #[cfg(unix)]
    #[test]
    fn hcb_06_send_line_uses_only_the_prompt_on_hc() {
        let dir = tempfile::tempdir().unwrap();
        let hc_bin = dir.path().join("hc");
        let app_bin = dir.path().join("herdr-app");
        let sock = dir.path().join("default.sock");
        std::fs::write(&sock, b"").unwrap();
        let body = r#"#!/bin/sh
if [ "$1" = warm ]; then exit 0; fi
d=$(dirname "$0")
printf '%s\n' "$*" >> "$d/calls"
case "$3" in
t1)
  printf '{"started":true,"status":"idle","turn":1}\n'
  exit 0;;
t2)
  printf '{"error":{"hint":null,"kind":"blocked","message":"terminal t2 is blocked"}}\n' >&2
  exit 9;;
esac
exit 1
"#;
        assert!(crate::test_support::write_warmed(&hc_bin, body, &["warm"])
            .status
            .success());
        assert!(
            crate::test_support::write_warmed(&app_bin, "#!/bin/sh\nexit 0\n", &["warm"])
                .status
                .success()
        );
        let host = crate::workspace::PaneHost::hc_app(crate::workspace::hc::HcApp::new(
            &hc_bin, &app_bin, &sock,
        ));
        send_line(&host, "t1", TypedLine::Mail).unwrap();

        let blocked = send_line(&host, "t2", TypedLine::Doorbell).unwrap_err();
        assert_eq!(
            blocked
                .downcast_ref::<crate::workspace::HcFailed>()
                .map(|f| f.kind.as_str()),
            Some("blocked"),
            "send_line must keep the prompt failure and never enter the 2-call fallback: {blocked:#}"
        );

        let calls: Vec<String> = std::fs::read_to_string(dir.path().join("calls"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            calls,
            [
                format!("send --json t1 {MAIL_LINE}"),
                format!("send --json t2 {DOORBELL}"),
            ]
        );
    }
}
