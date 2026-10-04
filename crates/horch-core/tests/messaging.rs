//! ARC-20 (delivery is unchanged) and ARC-21 (`done` runs in a fixed order),
//! over the in-memory workspace.

use std::cell::RefCell;
use std::time::Duration;

use anyhow::{anyhow, Result};
use horch_core::execution::lifecycle::{self, DoneRequest, DoneSteps, ReportTarget};
use horch_core::messaging::delivery::{self, Timing};
use horch_core::workspace::client::WorkspaceClient;
use horch_core::workspace::testing::{FakeCall, FakeWorkspace};

/// A workspace with one pane, `p1`.
fn one_pane() -> (FakeWorkspace, String) {
    let ws = FakeWorkspace::new();
    let created = ws.workspace_create("fleet", None, false).unwrap();
    (ws, created.root_pane_id)
}

/// The calls after the setup call, as `method arg arg`.
fn trace(ws: &FakeWorkspace) -> Vec<String> {
    ws.calls()
        .into_iter()
        .filter(|c| c.method != "workspace_create")
        .map(|FakeCall { method, args }| format!("{method} {}", args.join(" ")))
        .collect()
}

/// The real waits, cut to milliseconds.
const FAST: Timing = Timing {
    tail_timeout: Duration::from_millis(50),
    poll_start: Duration::from_millis(1),
    poll_max: Duration::from_millis(5),
    settle_short: Duration::from_millis(1),
    settle_long: Duration::from_millis(1),
    second_enter: Duration::from_millis(1),
};

#[test]
fn arc_20_send_line_prompt_first() {
    let (ws, pane) = one_pane();
    delivery::send_line_with(&ws, &pane, "[sonnet-1] DONE: x", &FAST).unwrap();
    assert_eq!(
        trace(&ws),
        vec![format!("agent_prompt {pane} [sonnet-1] DONE: x")],
        "an accepted agent prompt is the whole delivery"
    );
}

#[test]
fn arc_20_fallback_waits_tail() {
    let message = "[sonnet-1] DONE: No file was unclassifiable.";

    // The tail shows, soft-wrapped inside a box: text, poll, Enter, retry Enter.
    let (ws, pane) = one_pane();
    ws.fail_next("agent_prompt", "no agent detected");
    ws.set_screen(
        &pane,
        "\u{2502} > [sonnet-1] DONE: No file was uncl \u{2502}\n\u{2502}   assifiable.      \u{2502}\n",
    );
    delivery::send_line_with(&ws, &pane, message, &FAST).unwrap();
    assert_eq!(
        trace(&ws),
        vec![
            format!("agent_prompt {pane} {message}"),
            format!("pane_send_text {pane} {message}"),
            format!("pane_read {pane} visible"),
            format!("pane_send_keys {pane} enter"),
            format!("pane_send_keys {pane} enter"),
        ]
    );

    // The tail never shows: the poll times out, and one Enter goes after the
    // settle. As today, the timeout is not an error.
    let (ws, pane) = one_pane();
    ws.fail_next("agent_prompt", "no agent detected");
    ws.set_screen(&pane, "something else entirely");
    delivery::send_line_with(&ws, &pane, message, &FAST).unwrap();
    let calls = trace(&ws);
    assert_eq!(calls[0], format!("agent_prompt {pane} {message}"));
    assert_eq!(calls[1], format!("pane_send_text {pane} {message}"));
    assert_eq!(
        calls.last().unwrap(),
        &format!("pane_send_keys {pane} enter")
    );
    let reads = &calls[2..calls.len() - 1];
    assert!(reads.len() > 1, "it polled more than once: {calls:?}");
    assert!(reads
        .iter()
        .all(|c| c == &format!("pane_read {pane} visible")));

    // A failed send_text is the error, and nothing is pressed after it.
    let (ws, pane) = one_pane();
    ws.fail_next("agent_prompt", "no agent detected");
    ws.fail_next("pane_send_text", "herdr pane send-text failed");
    let err = delivery::send_line_with(&ws, &pane, message, &FAST).unwrap_err();
    assert_eq!(err.to_string(), "herdr pane send-text failed");
    assert_eq!(trace(&ws).len(), 2);
}

#[test]
fn arc_20_default_timing_is_unchanged() {
    assert_eq!(
        Timing::DEFAULT,
        Timing {
            tail_timeout: Duration::from_secs(15),
            poll_start: Duration::from_millis(100),
            poll_max: Duration::from_secs(1),
            settle_short: Duration::from_secs(1),
            settle_long: Duration::from_secs(2),
            second_enter: Duration::from_secs(1),
        }
    );
}

/// Records every step, and the workspace calls made so far at each one, into
/// one log.
struct Recorder<'a> {
    ws: &'a FakeWorkspace,
    log: RefCell<Vec<String>>,
    fail_report: bool,
}

impl Recorder<'_> {
    fn new(ws: &FakeWorkspace, fail_report: bool) -> Recorder<'_> {
        Recorder {
            ws,
            log: RefCell::new(Vec::new()),
            fail_report,
        }
    }

    fn push(&self, step: String) {
        // Every workspace call made since the last step goes in first, so the
        // log is one ordered story.
        let seen = self
            .log
            .borrow()
            .iter()
            .filter(|l| l.starts_with("ws "))
            .count();
        for call in trace(self.ws).into_iter().skip(seen) {
            self.log.borrow_mut().push(format!("ws {call}"));
        }
        self.log.borrow_mut().push(step);
    }

    fn finish(&self) -> Vec<String> {
        self.push("end".into());
        let mut log = self.log.borrow().clone();
        log.pop();
        log
    }
}

impl DoneSteps for Recorder<'_> {
    fn mark_done(&self, record_id: &str, summary: &str) -> Result<()> {
        self.push(format!("ledger done {record_id} {summary}"));
        Ok(())
    }
    fn report(&self, line: &str) -> Result<()> {
        self.push(format!("report {line}"));
        if self.fail_report {
            return Err(anyhow!("orchestrator unreachable"));
        }
        Ok(())
    }
    fn unregister(&self, workspace: &str, role: &str) {
        self.push(format!("unregister {workspace} {role}"));
    }
    fn settle(&self, workspace: &str) {
        self.push(format!("settle {workspace}"));
    }
}

fn request<'a>(pane: &'a str, report_to: ReportTarget) -> DoneRequest<'a> {
    DoneRequest {
        record_id: "r1",
        role: "sonnet-1",
        pane,
        workspace: None,
        summary: "[sonnet-1] DONE: The report is written.",
        report_to,
    }
}

#[test]
fn arc_21_done_order() {
    let full = |pane: &str| {
        vec![
            "ledger done r1 The report is written.".to_string(),
            "report [sonnet-1] DONE: The report is written.".to_string(),
            format!("ws pane_get {pane}"),
            "unregister w1 sonnet-1".to_string(),
            "settle w1".to_string(),
            format!("ws pane_close {pane}"),
        ]
    };

    let (ws, pane) = one_pane();
    let rec = Recorder::new(&ws, false);
    lifecycle::done(&ws, &rec, &request(&pane, ReportTarget::Orchestrator)).unwrap();
    assert_eq!(rec.finish(), full(&pane));
    assert!(ws.pane_ids().is_empty(), "the pane is closed");

    // No report target: the report step is absent, the rest is unchanged.
    let (ws, pane) = one_pane();
    let rec = Recorder::new(&ws, false);
    lifecycle::done(&ws, &rec, &request(&pane, ReportTarget::None)).unwrap();
    let mut expected = full(&pane);
    expected.remove(1);
    assert_eq!(rec.finish(), expected);

    // A failed report does not stop the shutdown.
    let (ws, pane) = one_pane();
    let rec = Recorder::new(&ws, true);
    lifecycle::done(&ws, &rec, &request(&pane, ReportTarget::Orchestrator)).unwrap();
    assert_eq!(rec.finish(), full(&pane));

    // An explicit workspace wins over the pane's own.
    let (ws, pane) = one_pane();
    let rec = Recorder::new(&ws, false);
    let mut req = request(&pane, ReportTarget::None);
    req.workspace = Some("w9");
    lifecycle::done(&ws, &rec, &req).unwrap();
    assert!(rec.finish().contains(&"settle w9".to_string()));
}

/// Readiness waits cut to milliseconds.
const READY_FAST: delivery::Readiness = delivery::Readiness {
    timeout: Duration::from_millis(40),
    poll: Duration::from_millis(1),
};

/// LA-3: opencode drops text typed while it starts (status `unknown`), so a
/// resumed prompt is typed only once herdr reports the agent idle.
#[test]
fn deliver_when_idle_waits_for_idle() {
    let (ws, pane) = one_pane();
    ws.set_agent_states(
        &pane,
        &[
            (None, None),
            (Some("opencode"), Some("unknown")),
            (Some("opencode"), Some("idle")),
        ],
    );
    delivery::deliver_when_idle(&ws, &pane, "task", &READY_FAST, &FAST).unwrap();
    assert_eq!(
        trace(&ws),
        vec![
            format!("pane_get {pane}"),
            format!("pane_get {pane}"),
            format!("pane_get {pane}"),
            format!("agent_prompt {pane} task"),
        ]
    );
}

#[test]
fn deliver_when_idle_times_out_without_typing() {
    let (ws, pane) = one_pane();
    ws.set_agent_states(&pane, &[(Some("opencode"), Some("unknown"))]);
    let err = delivery::deliver_when_idle(&ws, &pane, "task", &READY_FAST, &FAST).unwrap_err();
    assert!(err.to_string().contains("was not idle"), "{err}");
    assert!(
        trace(&ws).iter().all(|c| c.starts_with("pane_get ")),
        "nothing typed: {:?}",
        trace(&ws)
    );
}

/// `horch tell` and `horch assign`: a starting agent is waited out; a pane
/// with no agent, or a busy agent, gets the line at once, as before.
#[test]
fn send_line_when_ready_waits_only_for_a_starting_agent() {
    let (ws, pane) = one_pane();
    ws.set_agent_states(
        &pane,
        &[
            (Some("opencode"), Some("unknown")),
            (Some("opencode"), Some("unknown")),
            (Some("opencode"), Some("idle")),
        ],
    );
    delivery::send_line_when_ready(&ws, &pane, "msg", Duration::ZERO, &READY_FAST, &FAST).unwrap();
    assert_eq!(
        trace(&ws),
        vec![
            format!("pane_get {pane}"),
            format!("pane_get {pane}"),
            format!("pane_get {pane}"),
            format!("agent_prompt {pane} msg"),
        ]
    );

    for (agent, status) in [(None, None), (Some("claude"), Some("working"))] {
        let (ws, pane) = one_pane();
        ws.set_agent_states(&pane, &[(agent, status)]);
        delivery::send_line_when_ready(&ws, &pane, "msg", Duration::ZERO, &READY_FAST, &FAST)
            .unwrap();
        assert_eq!(
            trace(&ws),
            vec![
                format!("pane_get {pane}"),
                format!("agent_prompt {pane} msg")
            ],
            "{agent:?} {status:?}"
        );
    }

    let (ws, pane) = one_pane();
    ws.set_agent_states(&pane, &[(Some("opencode"), Some("unknown"))]);
    let err = delivery::send_line_when_ready(&ws, &pane, "msg", Duration::ZERO, &READY_FAST, &FAST)
        .unwrap_err();
    assert!(err.to_string().contains("still starting"), "{err}");
    assert!(trace(&ws).iter().all(|c| c.starts_with("pane_get ")));
}

/// A role that registered just now: herdr shows no agent at first, then one
/// that is starting, then idle. The line waits for all of it (LA-3: a tell
/// 3 s after spawn was lost while herdr showed no agent yet).
#[test]
fn send_line_when_ready_gives_a_fresh_role_time_to_show_its_agent() {
    let grace = Duration::from_millis(40);
    let (ws, pane) = one_pane();
    ws.set_agent_states(
        &pane,
        &[
            (None, None),
            (None, None),
            (Some("opencode"), Some("unknown")),
            (Some("opencode"), Some("idle")),
        ],
    );
    delivery::send_line_when_ready(&ws, &pane, "msg", grace, &READY_FAST, &FAST).unwrap();
    let calls = trace(&ws);
    assert_eq!(calls.last().unwrap(), &format!("agent_prompt {pane} msg"));
    let gets = calls.iter().filter(|c| c.starts_with("pane_get ")).count();
    assert_eq!(gets, 4, "{calls:?}");
    assert_eq!(calls.len(), 5, "only the gets, then the prompt: {calls:?}");

    // A pane that never shows an agent gets the line after the grace, on
    // the old path.
    let (ws, pane) = one_pane();
    ws.set_agent_states(&pane, &[(None, None)]);
    let started = std::time::Instant::now();
    delivery::send_line_when_ready(&ws, &pane, "msg", grace, &READY_FAST, &FAST).unwrap();
    assert!(started.elapsed() >= grace, "it waited out the grace");
    let calls = trace(&ws);
    assert!(calls.len() > 3, "it polled during the grace: {calls:?}");
    assert_eq!(calls.last().unwrap(), &format!("agent_prompt {pane} msg"));
}

/// G5: `horch inbox` reads which registered panes herdr still has. A role
/// whose pane closed without `horch done` is `Closed`; a herdr that does not
/// answer leaves every pane `Unknown`.
#[test]
fn role_states_mark_a_closed_pane() {
    use horch_core::messaging::mailbox::{Mailbox, PaneState, RoleEntry};

    let ws = FakeWorkspace::new();
    let created = ws.workspace_create("fleet", None, false).unwrap();
    let worker = ws
        .pane_split(
            &created.root_pane_id,
            horch_core::workspace::model::Direction::Right,
        )
        .unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let mailbox = Mailbox::under(tmp.path(), &created.workspace_id);
    std::fs::create_dir_all(mailbox.dir()).unwrap();
    std::fs::write(mailbox.dir().join("orchestrator.id"), &created.root_pane_id).unwrap();
    std::fs::write(mailbox.dir().join("sonnet-1.id"), &worker).unwrap();
    ws.pane_close(&worker).unwrap();

    let entry = |role: &str, pane: &str, state| RoleEntry {
        role: role.into(),
        pane: pane.into(),
        state,
    };
    assert_eq!(
        mailbox.role_states(&ws),
        vec![
            entry("orchestrator", &created.root_pane_id, PaneState::Open),
            entry("sonnet-1", &worker, PaneState::Closed),
        ]
    );

    ws.set_reachable(false);
    assert!(mailbox
        .role_states(&ws)
        .iter()
        .all(|r| r.state == PaneState::Unknown));
}

/// G5: `horch spawn --resume` of a `working` record whose pane is gone ends
/// the record first, so it resumes. An open pane, or a herdr that does not
/// answer, leaves the record `working`, and the resume refuses.
#[test]
fn end_if_pane_closed_ends_only_a_vanished_pane() {
    use horch_core::execution::legacy::LedgerRecordV1;
    use horch_core::execution::store::{ExecutionStore, PANE_CLOSED};

    let working = |pane: &str| LedgerRecordV1 {
        record_id: "r1".into(),
        session_id: Some("s1".into()),
        role: "sonnet-1".into(),
        tier: "sonnet".into(),
        agent: "claude".into(),
        model: "sonnet".into(),
        status: "working".into(),
        pane_id: Some(pane.into()),
        ..LedgerRecordV1::default()
    };

    // The pane is open: the record stays working.
    let (ws, pane) = one_pane();
    let tmp = tempfile::tempdir().unwrap();
    let store = ExecutionStore::for_project(tmp.path(), "/work/alpha");
    store.insert(working(&pane)).unwrap();
    assert!(!store.end_if_pane_closed("r1", &ws).unwrap());
    assert_eq!(store.get("r1").unwrap().status, "working");

    // herdr does not answer: the pane may be open, so nothing changes.
    ws.pane_close(&pane).unwrap();
    ws.set_reachable(false);
    assert!(!store.end_if_pane_closed("s1", &ws).unwrap());
    assert_eq!(store.get("r1").unwrap().status, "working");

    // herdr answers without the pane: the record ends, with the reason.
    ws.set_reachable(true);
    assert!(store.end_if_pane_closed("s1", &ws).unwrap());
    let r = store.get("r1").unwrap();
    assert_eq!(r.status, "done");
    assert!(r.finished_at.is_some());
    let last = r.history.last().unwrap();
    assert_eq!(
        (last.event.as_str(), last.text.as_str()),
        ("ended", PANE_CLOSED)
    );
    assert_eq!(
        serde_json::to_value(r.execution_status()).unwrap()["failure"]["kind"],
        "pane_vanished"
    );

    // A second call finds nothing to end.
    assert!(!store.end_if_pane_closed("r1", &ws).unwrap());
}
