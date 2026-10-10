//! `horch mode <role> write|plan`: switch a read-only Codex pane to write and
//! back (X6, design `ai_docs/plans/wave3/x6-design.md`).
//!
//! A Codex teammate with `permission_mode: plan` runs read-only: a plain
//! `horch note` records through the execpolicy rule, but a compound one and
//! the handoff file fail. Codex 0.160.0 switches its sandbox at run time
//! with `/permissions`, so the orchestrator can open the pane for a handoff
//! and close it again:
//!
//! 1. wait for the pane at its prompt (2 polls in a row);
//! 2. type `/permissions`, find the target item by label (`fleet_write`
//!    for write, `Read Only` for plan) and the `›` cursor, press `down` to
//!    it, check the cursor line, press Enter;
//! 3. type `/status` and check its `Permissions:` line (`Profile
//!    fleet_write`, or `Read Only ...`);
//! 4. record `mode-changed` (`write` or `plan`) on the record.
//!
//! [`switch`] is the keys and the check alone; `horch compact --request` and
//! the compaction job call it too. Both `/permissions` and `/status` are
//! local TUI commands: no model turn.

use std::process::ExitCode;
use std::time::Duration;

use anyhow::Result;
use horch_core::execution::legacy::{HistoryEntry, Record};
use horch_core::execution::records::Ledger;
use horch_core::harness::codex::{
    parse_permissions_menu, status_permissions, status_shows, PaneMode,
};
use horch_core::harness::HarnessKind;
use horch_core::messaging::delivery;
use horch_core::messaging::mailbox::Mailbox;
use horch_core::roster::{PermissionMode, Roster};
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::client::WorkspaceClient;
use horch_core::workspace::herdr::Herdr;
use horch_core::workspace::model::at_prompt;

use super::context::record_kind;

/// The event of a verified switch. Its text is the new mode.
pub const EVENT_MODE_CHANGED: &str = "mode-changed";
/// The event of a switch that failed after keys were typed:
/// `<mode>: <reason>`.
pub const EVENT_MODE_FAILED: &str = "mode-failed";

/// The command line of `horch mode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeArgs {
    pub role: String,
    pub mode: PaneMode,
    /// `--timeout`: the wait for the pane at its prompt, in seconds.
    pub timeout: Option<u64>,
}

/// The waits of 1 switch.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    pub poll: Duration,
    /// Polls before the pane must be at its prompt.
    pub idle_polls: u32,
    /// Polls for the menu to open or close, and for the `/status` block.
    pub screen_polls: u32,
}

impl Timing {
    /// 0.5 s polls; 120 s for the prompt, 10 s for a screen.
    pub const DEFAULT: Timing = Timing {
        poll: Duration::from_millis(500),
        idle_polls: 240,
        screen_polls: 20,
    };

    /// [`Timing::DEFAULT`] with a `--timeout` in seconds for the prompt.
    pub fn with_timeout(timeout: Option<u64>) -> Timing {
        let mut t = Timing::DEFAULT;
        if let Some(s) = timeout {
            let polls = Duration::from_secs(s).as_millis() / t.poll.as_millis();
            t.idle_polls = u32::try_from(polls).unwrap_or(u32::MAX).max(1);
        }
        t
    }
}

/// Why a switch did not happen.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SwitchError {
    /// Nothing was typed.
    Refused(String),
    /// Keys were typed; the pane's mode is unknown.
    Failed(String),
}

impl std::fmt::Display for SwitchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SwitchError::Refused(r) | SwitchError::Failed(r) => f.write_str(r),
        }
    }
}

/// Switch the Codex TUI in `pane` to `mode` and check it with `/status`.
pub(crate) fn switch(
    ws: &dyn WorkspaceClient,
    pane: &str,
    mode: PaneMode,
    timing: &Timing,
) -> Result<(), SwitchError> {
    let mut idle = 0;
    let mut polls = 0;
    while idle < 2 {
        if polls >= timing.idle_polls {
            return Err(SwitchError::Refused(format!(
                "the pane {pane} is not at its prompt"
            )));
        }
        let status = ws
            .pane_get(pane)
            .map_err(|e| SwitchError::Refused(format!("{e:#}")))?
            .agent_status;
        idle = if at_prompt(status.as_deref()) {
            idle + 1
        } else {
            0
        };
        polls += 1;
        if idle < 2 {
            std::thread::sleep(timing.poll);
        }
    }
    let failed = |r: String| SwitchError::Failed(r);
    let label = mode.menu_label();

    type_line(ws, pane, "/permissions").map_err(failed)?;
    let menu = poll_screen(ws, pane, "visible", timing, parse_permissions_menu)
        .ok_or_else(|| failed("the /permissions menu did not open".into()))?;
    let Some(downs) = menu.downs_to(label) else {
        let _ = ws.pane_send_keys(pane, "esc");
        return Err(failed(format!(
            "the /permissions menu has no item {label:?}; it has {}",
            menu.items.join(", ")
        )));
    };
    for _ in 0..downs {
        key(ws, pane, "down").map_err(failed)?;
    }
    let at = ws
        .pane_read(pane, "visible")
        .ok()
        .and_then(|s| parse_permissions_menu(&s));
    let under = at.as_ref().and_then(|m| m.cursor_label()).unwrap_or("");
    if under != label {
        let _ = ws.pane_send_keys(pane, "esc");
        return Err(failed(format!(
            "the /permissions cursor is on {under:?}, not {label:?}"
        )));
    }
    key(ws, pane, "enter").map_err(failed)?;
    poll_screen(ws, pane, "visible", timing, |s| {
        parse_permissions_menu(s).is_none().then_some(())
    })
    .ok_or_else(|| failed("the /permissions menu did not close".into()))?;

    let seen = ws
        .pane_read(pane, "recent")
        .map(|s| s.matches("Permissions:").count())
        .unwrap_or(0);
    type_line(ws, pane, "/status").map_err(failed)?;
    let value = poll_screen(ws, pane, "recent", timing, |s| {
        (s.matches("Permissions:").count() > seen)
            .then(|| status_permissions(s))
            .flatten()
    })
    .ok_or_else(|| failed("/status printed no Permissions line".into()))?;
    if !status_shows(mode, &value) {
        return Err(failed(format!("/status shows Permissions: {value}")));
    }
    Ok(())
}

fn type_line(ws: &dyn WorkspaceClient, pane: &str, line: &str) -> Result<(), String> {
    delivery::send_line(ws, pane, line).map_err(|e| format!("typing {line}: {e:#}"))
}

fn key(ws: &dyn WorkspaceClient, pane: &str, key: &str) -> Result<(), String> {
    ws.pane_send_keys(pane, key)
        .map_err(|e| format!("pressing {key}: {e:#}"))
}

/// Read `pane` until `found` gives a value, at most `screen_polls` times.
fn poll_screen<T>(
    ws: &dyn WorkspaceClient,
    pane: &str,
    source: &str,
    timing: &Timing,
    found: impl Fn(&str) -> Option<T>,
) -> Option<T> {
    for i in 0..timing.screen_polls.max(1) {
        if i > 0 {
            std::thread::sleep(timing.poll);
        }
        if let Some(v) = ws.pane_read(pane, source).ok().as_deref().and_then(&found) {
            return Some(v);
        }
    }
    None
}

/// The mode of a record: the text of its newest `mode-changed` event, else
/// `plan` (the launch mode).
pub(crate) fn current_mode(history: &[HistoryEntry]) -> PaneMode {
    history
        .iter()
        .rev()
        .find(|e| e.event == EVENT_MODE_CHANGED)
        .and_then(|e| PaneMode::parse(&e.text))
        .unwrap_or(PaneMode::Plan)
}

/// Whether `record` is a Codex pane whose teammate launches read-only.
pub(crate) fn is_plan_pane(roster: &Roster, record: &Record) -> bool {
    record_kind(record) == HarnessKind::Codex
        && roster
            .get(&record.tier)
            .is_some_and(|t| t.permission_mode == Some(PermissionMode::Plan))
}

/// [`switch`] on a record, with its event: `mode-changed` on success,
/// `mode-failed` when keys were typed and the check failed.
pub(crate) fn switch_record(
    ledger: &Ledger,
    ws: &dyn WorkspaceClient,
    record: &Record,
    pane: &str,
    mode: PaneMode,
    timing: &Timing,
) -> Result<(), SwitchError> {
    match switch(ws, pane, mode, timing) {
        Ok(()) => ledger
            .record_event(&record.record_id, EVENT_MODE_CHANGED, mode.as_str())
            .map_err(|e| SwitchError::Failed(format!("{e:#}"))),
        Err(SwitchError::Failed(r)) => {
            let _ = ledger.record_event(
                &record.record_id,
                EVENT_MODE_FAILED,
                &format!("{}: {r}", mode.as_str()),
            );
            Err(SwitchError::Failed(r))
        }
        refused => refused,
    }
}

/// Everything `horch mode` works with. Tests pass a simulated pane.
pub(crate) struct Env<'a> {
    pub ledger: &'a Ledger,
    pub roster: &'a Roster,
    pub ws: &'a dyn WorkspaceClient,
    pub mailbox: &'a Mailbox,
    pub timing: Timing,
}

/// 1 run: the exit code and the lines for stdout and stderr.
pub(crate) fn run(env: &Env, args: &ModeArgs) -> (u8, String) {
    let role = &args.role;
    let workspace = env.mailbox.workspace_id();
    let refused = |r: String| (1, format!("horch mode: refused: {r}"));
    let record = match env.ledger.live_for_role(role, Some(workspace)) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return refused(format!(
                "no live record for role {role} in workspace {workspace}"
            ))
        }
        Err(e) => return refused(format!("{e:#}")),
    };
    if !is_plan_pane(env.roster, &record) {
        return refused(format!(
            "role {role} is not a Codex pane with permission_mode plan"
        ));
    }
    let Some(pane) = env.mailbox.pane_for(role) else {
        return refused(format!("role {role} has no pane in workspace {workspace}"));
    };
    match switch_record(env.ledger, env.ws, &record, &pane, args.mode, &env.timing) {
        Ok(()) => (0, format!("{role} is in {} mode", args.mode.as_str())),
        Err(SwitchError::Refused(r)) => refused(r),
        Err(SwitchError::Failed(r)) => (1, format!("horch mode: {r}")),
    }
}

/// `horch mode`.
pub fn mode(ctx: &RuntimeContext, args: &ModeArgs) -> Result<ExitCode> {
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let mailbox = match Mailbox::resolve_in(&herdr, ctx) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("horch mode: refused: {e:#}");
            return Ok(ExitCode::from(1));
        }
    };
    let ledger = Ledger::open_in(ctx)?;
    let roster = match super::load_roster_unwarned(ctx, None) {
        Ok(r) => r,
        Err(_) => Roster::builtin()?,
    };
    let env = Env {
        ledger: &ledger,
        roster: &roster,
        ws: &herdr,
        mailbox: &mailbox,
        timing: Timing::with_timeout(args.timeout),
    };
    let (code, line) = run(&env, args);
    if code == 0 {
        println!("{line}");
    } else {
        eprintln!("{line}");
    }
    Ok(ExitCode::from(code))
}

#[cfg(test)]
pub(crate) mod sim {
    //! A simulated Codex 0.160.0 TUI behind [`WorkspaceClient`]: it opens
    //! the `/permissions` menu, moves its cursor, applies a selection and
    //! prints `/status`, as the research pane showed (X6).

    use std::cell::RefCell;

    use super::*;
    use horch_core::workspace::model::{NewWorkspace, Pane};

    pub struct SimCodex {
        pub state: RefCell<State>,
    }

    pub struct State {
        pub pane: String,
        pub status: Vec<&'static str>,
        pub items: Vec<&'static str>,
        pub mode: PaneMode,
        pub menu: Option<usize>,
        pub lines: Vec<String>,
        /// Every call: `prompt <text>`, `key <key>`, `read <source>`.
        pub calls: Vec<String>,
        /// The `/status` value after `fleet_write` is selected.
        pub write_status: String,
        /// When set, the cursor skips this many extra items on `down`.
        pub drift: usize,
    }

    impl SimCodex {
        /// A pane `p1` at its prompt in `mode`, with the 5-item menu of the
        /// profile launch (X6 repro 5b).
        pub fn new(mode: PaneMode) -> SimCodex {
            SimCodex {
                state: RefCell::new(State {
                    pane: "p1".into(),
                    status: vec![],
                    items: vec![
                        "Ask for approval",
                        "Approve for me",
                        "Full Access",
                        "Read Only",
                        "fleet_write",
                    ],
                    mode,
                    menu: None,
                    lines: vec!["› Ask Codex to do anything".into()],
                    calls: vec![],
                    write_status: "Profile fleet_write".into(),
                    drift: 0,
                }),
            }
        }

        pub fn calls(&self) -> Vec<String> {
            self.state.borrow().calls.clone()
        }

        /// The calls that type or press, without the reads.
        pub fn typed(&self) -> Vec<String> {
            self.calls()
                .into_iter()
                .filter(|c| !c.starts_with("read") && !c.starts_with("get"))
                .collect()
        }

        fn label(mode: PaneMode) -> &'static str {
            mode.menu_label()
        }

        fn render(st: &State) -> String {
            let mut out = st.lines.join("\n");
            if let Some(cursor) = st.menu {
                out.push_str("\n  Update Model Permissions\n\n");
                for (i, item) in st.items.iter().enumerate() {
                    let mark = if i == cursor { "›" } else { " " };
                    out.push_str(&format!("{mark} {}. {item}  Some text\n", i + 1));
                }
                out.push_str("\n  enter select · esc back\n");
            }
            out
        }
    }

    impl WorkspaceClient for SimCodex {
        fn pane_get(&self, pane: &str) -> Result<Pane> {
            let mut st = self.state.borrow_mut();
            st.calls.push(format!("get {pane}"));
            let status = if st.status.is_empty() {
                "idle"
            } else {
                st.status.remove(0)
            };
            Ok(serde_json::from_value(serde_json::json!({
                "pane_id": pane, "agent_status": status
            }))?)
        }
        fn pane_list(&self, _: &str) -> Result<Vec<Pane>> {
            Ok(vec![])
        }
        fn pane_split(
            &self,
            _: &str,
            _: horch_core::workspace::model::Direction,
        ) -> Result<String> {
            anyhow::bail!("not simulated")
        }
        fn pane_run(&self, _: &str, _: &str) -> Result<()> {
            anyhow::bail!("not simulated")
        }
        fn pane_close(&self, _: &str) -> Result<()> {
            anyhow::bail!("not simulated")
        }
        fn agent_prompt(&self, pane: &str, text: &str) -> Result<()> {
            let mut st = self.state.borrow_mut();
            st.calls.push(format!("prompt {text}"));
            anyhow::ensure!(pane == st.pane, "no pane {pane}");
            match text {
                "/permissions" => {
                    let current = Self::label(st.mode);
                    st.menu = Some(st.items.iter().position(|i| *i == current).unwrap_or(0));
                }
                "/status" => {
                    let value = match st.mode {
                        PaneMode::Plan => "Read Only (Ask for approval)".to_string(),
                        PaneMode::Write => st.write_status.clone(),
                    };
                    st.lines.push(format!("  Permissions:         {value}"));
                    st.lines.push("  Agents.md:           <none>".into());
                }
                other => st.lines.push(format!("› {other}")),
            }
            Ok(())
        }
        fn pane_send_text(&self, _: &str, _: &str) -> Result<()> {
            anyhow::bail!("not simulated")
        }
        fn pane_send_keys(&self, _: &str, key: &str) -> Result<()> {
            let mut st = self.state.borrow_mut();
            st.calls.push(format!("key {key}"));
            let n = st.items.len();
            match (key, st.menu) {
                ("down", Some(c)) => st.menu = Some((c + 1 + st.drift) % n),
                ("esc", Some(_)) => st.menu = None,
                ("enter", Some(c)) => {
                    let item = st.items[c];
                    st.mode = match item {
                        "Read Only" => PaneMode::Plan,
                        _ => PaneMode::Write,
                    };
                    st.menu = None;
                    st.lines
                        .push(format!("• Permission selection requested: {item}"));
                }
                _ => {}
            }
            Ok(())
        }
        fn pane_read(&self, _: &str, source: &str) -> Result<String> {
            let mut st = self.state.borrow_mut();
            st.calls.push(format!("read {source}"));
            Ok(Self::render(&st))
        }
        fn workspace_create(&self, _: &str, _: Option<&str>, _: bool) -> Result<NewWorkspace> {
            anyhow::bail!("not simulated")
        }
        fn workspace_close(&self, _: &str) -> Result<()> {
            anyhow::bail!("not simulated")
        }
        fn server_reachable(&self) -> bool {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::context::testkit::World;
    use super::sim::SimCodex;
    use super::*;
    use horch_core::roster::Teammate;

    const FAST: Timing = Timing {
        poll: Duration::ZERO,
        idle_polls: 6,
        screen_polls: 3,
    };

    /// A world with a plan Codex teammate `codex-plan` and its live record.
    struct Fleet {
        w: World,
        mailbox: Mailbox,
    }

    impl Fleet {
        fn new() -> Fleet {
            let mut w = World::new();
            w.roster.insert_for_test(Teammate {
                name: "codex-plan".into(),
                agent: HarnessKind::Codex,
                permission_mode: Some(PermissionMode::Plan),
                ..Default::default()
            });
            w.roster.insert_for_test(Teammate {
                name: "codex-auto".into(),
                agent: HarnessKind::Codex,
                permission_mode: Some(PermissionMode::Auto),
                ..Default::default()
            });
            let mailbox = Mailbox::under(w.tmp.path().join("mail"), "w1");
            std::fs::create_dir_all(mailbox.dir()).unwrap();
            Fleet { w, mailbox }
        }

        fn worker(&self, role: &str, agent: &str, tier: &str) {
            self.w
                .worker(&format!("rec-{role}"), role, agent, tier, None);
            std::fs::write(self.mailbox.dir().join(format!("{role}.id")), "p1").unwrap();
        }

        fn run(&self, sim: &SimCodex, role: &str, mode: PaneMode) -> (u8, String) {
            let env = Env {
                ledger: &self.w.ledger,
                roster: &self.w.roster,
                ws: sim,
                mailbox: &self.mailbox,
                timing: FAST,
            };
            run(
                &env,
                &ModeArgs {
                    role: role.into(),
                    mode,
                    timeout: None,
                },
            )
        }

        fn events(&self, role: &str) -> Vec<(String, String)> {
            self.w
                .ledger
                .get(&format!("rec-{role}"))
                .unwrap()
                .history
                .into_iter()
                .filter(|e| e.event.starts_with("mode-"))
                .map(|e| (e.event, e.text))
                .collect()
        }
    }

    /// The exact keys of a switch to write: the menu opens on Read Only
    /// (item 4), 1 down to fleet_write (item 5), Enter, then `/status`.
    #[test]
    fn write_types_the_menu_keys_checks_status_and_records_the_event() {
        let f = Fleet::new();
        f.worker("codex-plan-1", "codex", "codex-plan");
        let sim = SimCodex::new(PaneMode::Plan);
        let (code, line) = f.run(&sim, "codex-plan-1", PaneMode::Write);
        assert_eq!((code, line.as_str()), (0, "codex-plan-1 is in write mode"));
        assert_eq!(
            sim.typed(),
            [
                "prompt /permissions",
                "key down",
                "key enter",
                "prompt /status"
            ]
        );
        assert_eq!(
            f.events("codex-plan-1"),
            [("mode-changed".to_string(), "write".to_string())]
        );
        assert_eq!(
            current_mode(&f.w.ledger.get("rec-codex-plan-1").unwrap().history),
            PaneMode::Write
        );
    }

    /// Back to plan: the menu opens on fleet_write (item 5); 4 downs wrap
    /// to Read Only (item 4).
    #[test]
    fn plan_selects_read_only_and_records_the_event() {
        let f = Fleet::new();
        f.worker("codex-plan-1", "codex", "codex-plan");
        let sim = SimCodex::new(PaneMode::Write);
        let (code, _) = f.run(&sim, "codex-plan-1", PaneMode::Plan);
        assert_eq!(code, 0);
        assert_eq!(
            sim.typed(),
            [
                "prompt /permissions",
                "key down",
                "key down",
                "key down",
                "key down",
                "key enter",
                "prompt /status"
            ]
        );
        assert_eq!(
            f.events("codex-plan-1"),
            [("mode-changed".to_string(), "plan".to_string())]
        );
    }

    /// Only a Codex pane whose teammate has `permission_mode: plan`: the
    /// others type nothing and record nothing.
    #[test]
    fn other_panes_are_refused_without_a_key() {
        let f = Fleet::new();
        f.worker("claude-1", "claude", "codex-plan");
        f.worker("codex-auto-1", "codex", "codex-auto");
        let sim = SimCodex::new(PaneMode::Plan);
        for role in ["claude-1", "codex-auto-1", "nobody"] {
            let (code, line) = f.run(&sim, role, PaneMode::Write);
            assert_eq!(code, 1, "{role}");
            assert!(line.starts_with("horch mode: refused: "), "{line}");
        }
        assert!(sim.typed().is_empty());
        assert!(f.events("claude-1").is_empty());
    }

    /// A pane that stays busy is refused before any key.
    #[test]
    fn a_busy_pane_is_refused_before_any_key() {
        let f = Fleet::new();
        f.worker("codex-plan-1", "codex", "codex-plan");
        let sim = SimCodex::new(PaneMode::Plan);
        sim.state.borrow_mut().status = vec!["working"; 10];
        let (code, line) = f.run(&sim, "codex-plan-1", PaneMode::Write);
        assert_eq!(code, 1);
        assert!(line.contains("not at its prompt"), "{line}");
        assert!(sim.typed().is_empty());
        assert!(f.events("codex-plan-1").is_empty());
    }

    /// The cursor is checked before Enter: on a mismatch the menu closes
    /// with Esc, Enter is never pressed, and `mode-failed` is recorded.
    #[test]
    fn a_cursor_on_the_wrong_item_presses_esc_never_enter() {
        let f = Fleet::new();
        f.worker("codex-plan-1", "codex", "codex-plan");
        let sim = SimCodex::new(PaneMode::Plan);
        sim.state.borrow_mut().drift = 1;
        let (code, line) = f.run(&sim, "codex-plan-1", PaneMode::Write);
        assert_eq!(code, 1);
        assert!(line.contains("cursor is on"), "{line}");
        assert_eq!(sim.typed(), ["prompt /permissions", "key down", "key esc"]);
        let events = f.events("codex-plan-1");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "mode-failed");
        assert!(events[0].1.starts_with("write: "), "{:?}", events[0]);
    }

    /// A menu without the target item (the legacy 3-item menu has neither
    /// Read Only nor fleet_write) closes with Esc.
    #[test]
    fn a_menu_without_the_item_presses_esc() {
        let f = Fleet::new();
        f.worker("codex-plan-1", "codex", "codex-plan");
        let sim = SimCodex::new(PaneMode::Write);
        sim.state.borrow_mut().items = vec!["Ask for approval", "Approve for me", "Full Access"];
        let (code, line) = f.run(&sim, "codex-plan-1", PaneMode::Plan);
        assert_eq!(code, 1);
        assert!(line.contains("has no item \"Read Only\""), "{line}");
        assert_eq!(sim.typed(), ["prompt /permissions", "key esc"]);
    }

    /// A `/status` that is not `Profile fleet_write` fails the write
    /// switch: a plain workspace lacks the state root (X6 repro 4).
    #[test]
    fn a_status_without_the_fleet_profile_fails_the_write_check() {
        let f = Fleet::new();
        f.worker("codex-plan-1", "codex", "codex-plan");
        let sim = SimCodex::new(PaneMode::Plan);
        sim.state.borrow_mut().write_status = "Workspace (Ask for approval)".into();
        let (code, line) = f.run(&sim, "codex-plan-1", PaneMode::Write);
        assert_eq!(code, 1);
        assert!(
            line.contains("/status shows Permissions: Workspace (Ask for approval)"),
            "{line}"
        );
        assert_eq!(f.events("codex-plan-1")[0].0, "mode-failed");
    }

    #[test]
    fn the_current_mode_is_plan_until_a_mode_changed_event() {
        let ev = |event: &str, text: &str| HistoryEntry {
            at: "2026-10-07T00:00:00Z".into(),
            event: event.into(),
            text: text.into(),
        };
        assert_eq!(current_mode(&[]), PaneMode::Plan);
        assert_eq!(
            current_mode(&[ev("mode-changed", "write"), ev("mode-failed", "plan: x")]),
            PaneMode::Write
        );
        assert_eq!(
            current_mode(&[ev("mode-changed", "write"), ev("mode-changed", "plan")]),
            PaneMode::Plan
        );
    }

    #[test]
    fn a_timeout_sets_the_prompt_polls() {
        assert_eq!(Timing::with_timeout(Some(30)).idle_polls, 60);
        assert_eq!(Timing::with_timeout(Some(0)).idle_polls, 1);
        assert_eq!(Timing::with_timeout(None).idle_polls, 240);
    }
}
