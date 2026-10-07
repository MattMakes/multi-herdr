//! Fleet context policy, the core (W1 u2): windows and thresholds (CTX-07),
//! row states and the warn rule (CTX-10, CTX-11), handoffs (CTX-12,
//! CTX-14), the compact line (CTX-20), the job (CTX-13, CTX-15, CTX-22,
//! CTX-23) and the job dir (CTX-15).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use horch_core::compaction::job::{run, JobFailure, JobPlan, JobPorts, Step};
use horch_core::compaction::jobfile::{
    self, claim_lost, create_job, start_locked, JobRecord, Started, JOB_FILE, LOG_FILE, LOST_FILE,
    SPAWNED_FILE,
};
use horch_core::compaction::policy::{
    already_asked, asked_at, classify, compact_line, count_compactions, handoff_fresh,
    handoff_path, should_warn, watch_base, HandoffProblem, JobLiveness, RowInputs, RowState,
};
use horch_core::compaction::window::{
    decide, fleet_window, headroom, native_trigger, threshold, OperatorWindow, WindowSource,
    BASE_THRESHOLD,
};
use horch_core::evaluation::scheduler::{self, JobFacts, JobState};
use horch_core::execution::legacy::HistoryEntry;
use horch_core::harness::HarnessKind;
use horch_core::heartbeat::{self, Heartbeat, Liveness, HEARTBEAT_FILE, STALE_AFTER};
use horch_core::telemetry::context::{CompactionMark, Reading};
use horch_core::telemetry::readers::Unreadable;

fn at(s: &str) -> DateTime<Utc> {
    horch_core::clock::parse(s).unwrap()
}

fn sys(s: &str) -> SystemTime {
    SystemTime::from(at(s))
}

fn ev(at: &str, event: &str) -> HistoryEntry {
    HistoryEntry {
        at: at.into(),
        event: event.into(),
        text: String::new(),
    }
}

fn mark(at: &str, offset: u64, post: Option<u64>) -> CompactionMark {
    CompactionMark {
        at: at.into(),
        offset,
        trigger: Some("manual".into()),
        pre_tokens: Some(311_225),
        post_tokens: post,
    }
}

fn reading(tokens: Option<u64>) -> Reading {
    Reading {
        tokens,
        provisional: false,
        pending: false,
        window: None,
        model: None,
        last_compaction: None,
        marks: 0,
        marks_at: Vec::new(),
        transcript: PathBuf::from("/t/rollout.jsonl"),
    }
}

fn caps(kind: HarnessKind) -> horch_core::harness::capabilities::CompactionCaps {
    kind.capabilities().compaction
}

// ─── CTX-07: windows, triggers, thresholds ──────────────────────────────────

#[test]
fn ctx_07_threshold_is_min_of_base_and_eight_tenths() {
    assert_eq!(threshold(300_000, Some(467_000)), 300_000);
    assert_eq!(threshold(300_000, Some(117_000)), 93_600);
    assert_eq!(threshold(300_000, Some(200_000)), 160_000);
    assert_eq!(threshold(300_000, Some(183_616)), 146_892);
}

#[test]
fn ctx_07_unknown_native_uses_base() {
    assert_eq!(threshold(300_000, None), 300_000);
    assert_eq!(threshold(BASE_THRESHOLD, None), BASE_THRESHOLD);
}

#[test]
fn ctx_07_native_trigger_per_rule() {
    let native = |kind: HarnessKind, model: &str, setting, window| {
        native_trigger(caps(kind).trigger, kind.as_str(), model, setting, window)
    };
    // Claude: min(setting, model window) - 33,000 (design §8.1, §8.2).
    let claude = |setting| native(HarnessKind::Claude, "claude-opus-5-5", setting, None);
    assert_eq!(claude(Some(500_000)), Some(467_000));
    assert_eq!(claude(Some(150_000)), Some(117_000));
    assert_eq!(claude(None), Some(967_000));
    assert_eq!(
        native(HarnessKind::Claude, "claude-haiku-4-5", None, None),
        Some(167_000)
    );
    // Codex: min(limit, floor(window x 18 / 19)); the limit alone without a
    // window; nothing without either.
    let codex = |limit, window| native(HarnessKind::Codex, "gpt-5.6-sol", limit, window);
    assert_eq!(codex(Some(200_000), Some(258_400)), Some(200_000));
    assert_eq!(codex(None, Some(258_400)), Some(244_800));
    assert_eq!(codex(None, None), None);
    assert_eq!(codex(Some(150_000), None), Some(150_000));
    // pi and Prime: min(setting, window) - 16,384, window from the table.
    assert_eq!(
        native(
            HarnessKind::Prime,
            "anthropic/claude-opus-5-5",
            Some(200_000),
            None
        ),
        Some(183_616)
    );
    assert_eq!(
        native(HarnessKind::Pi, "ollama/qwen3.8", None, None),
        Some(245_760)
    );
    // OpenCode: a fixed trigger per model.
    assert_eq!(
        native(HarnessKind::OpenCode, "opencode/big-pickle", None, None),
        Some(140_000)
    );
    // A model no table knows: unknown.
    assert_eq!(
        native(HarnessKind::Prime, "no/such-model", None, None),
        None
    );
    assert_eq!(
        native(HarnessKind::OpenCode, "no/such-model", None, None),
        None
    );
    assert_eq!(native(HarnessKind::Antigravity, "gemini", None, None), None);
    assert_eq!(headroom(117_000, 93_600), 23_400);

    // fleet_window: teammate override, then <harness>/<teammate>, then
    // <harness>/<model>.
    let windows: BTreeMap<String, u64> = [
        ("claude/orchestrator".to_string(), 300_000),
        ("claude/opus".to_string(), 200_000),
    ]
    .into();
    assert_eq!(
        fleet_window(&windows, "claude", "orchestrator", "opus", Some(123_000)),
        Some((123_000, "teammate orchestrator compact_window".into()))
    );
    assert_eq!(
        fleet_window(&windows, "claude", "orchestrator", "opus", None),
        Some((300_000, "context-policy windows claude/orchestrator".into()))
    );
    assert_eq!(
        fleet_window(&windows, "claude", "opus-architect", "opus", None),
        Some((200_000, "context-policy windows claude/opus".into()))
    );
    assert_eq!(
        fleet_window(&windows, "codex", "orchestrator", "opus", None),
        None
    );

    // decide: operator first and never applied; fleet applied; else the
    // harness default with no tokens.
    let op = OperatorWindow {
        tokens: Some(500_000),
        detail: "~/.claude/settings.json".into(),
    };
    let d = decide(Some(op), Some((150_000, "fleet".into())));
    assert_eq!(
        (d.tokens, d.source, d.applied),
        (Some(500_000), WindowSource::Operator, false)
    );
    assert_eq!(d.detail, "~/.claude/settings.json");
    let d = decide(
        None,
        Some((150_000, "context-policy windows claude/sonnet".into())),
    );
    assert_eq!(
        (d.tokens, d.source, d.applied),
        (Some(150_000), WindowSource::Fleet, true)
    );
    let d = decide(None, None);
    assert_eq!(
        (d.tokens, d.source, d.applied),
        (None, WindowSource::Harness, false)
    );
    assert_eq!(d.detail, "harness default");
}

// ─── CTX-10, CTX-11: row states and the warn rule ───────────────────────────

/// A named row input and the state it gives.
type RowCase = (
    String,
    Result<Reading, Unreadable>,
    Vec<HistoryEntry>,
    JobLiveness,
    RowState,
);

/// Every input of `ctx_11_requested_state_after_ask`, with the state it
/// gives.
fn row_cases() -> Vec<RowCase> {
    let over = || Ok(reading(Some(311_225)));
    let asked = || vec![ev("2026-09-20T10:06:00Z", "compact-requested")];
    let pending = Reading {
        pending: true,
        tokens: Some(18_207),
        provisional: true,
        ..reading(None)
    };
    let lost = || JobLiveness::Lost {
        step: "wait-marker".into(),
    };
    vec![
        (
            "over, asked".into(),
            over(),
            asked(),
            JobLiveness::None,
            RowState::Requested,
        ),
        (
            "over, not asked".into(),
            over(),
            vec![],
            JobLiveness::None,
            RowState::Over,
        ),
        (
            "1 token".into(),
            Ok(reading(Some(1))),
            vec![],
            JobLiveness::None,
            RowState::Ok,
        ),
        (
            "job live".into(),
            over(),
            vec![],
            JobLiveness::Live,
            RowState::Compacting,
        ),
        (
            "pending".into(),
            Ok(pending),
            vec![],
            JobLiveness::None,
            RowState::Pending,
        ),
        (
            "no session".into(),
            Err(Unreadable::NoSessionId),
            vec![],
            JobLiveness::None,
            RowState::NoSession,
        ),
        (
            "not read".into(),
            Err(Unreadable::NotRead("slice 2".into())),
            vec![],
            JobLiveness::None,
            RowState::NotRead,
        ),
        (
            "no transcript".into(),
            Err(Unreadable::NoTranscript("/x".into())),
            vec![],
            JobLiveness::None,
            RowState::NoTranscript,
        ),
        (
            "failed".into(),
            Err(Unreadable::Failed("bad".into())),
            vec![],
            JobLiveness::None,
            RowState::Unknown,
        ),
        (
            "no tokens".into(),
            Ok(reading(None)),
            vec![],
            JobLiveness::None,
            RowState::Unknown,
        ),
        (
            "job lost".into(),
            over(),
            vec![],
            lost(),
            RowState::CompactLost,
        ),
        (
            "job lost, unreadable".into(),
            Err(Unreadable::NoTranscript("/x".into())),
            vec![],
            lost(),
            RowState::CompactLost,
        ),
    ]
}

#[test]
fn ctx_11_requested_state_after_ask() {
    for (name, reading, history, job, want) in row_cases() {
        let i = RowInputs {
            reading: reading.as_ref(),
            threshold: 300_000,
            history: &history,
            job: &job,
        };
        assert_eq!(classify(&i), want, "{name}");
    }
    assert_eq!(RowState::CompactLost.as_str(), "compact-lost");
    assert_eq!(RowState::NoTranscript.as_str(), "no-transcript");
}

/// Review finding 8: the note rule is `classify == Over`, nothing else.
#[test]
fn ctx_10_should_warn_is_classify_over() {
    let mut warned = 0;
    for (name, reading, history, job, _) in row_cases() {
        let i = RowInputs {
            reading: reading.as_ref(),
            threshold: 300_000,
            history: &history,
            job: &job,
        };
        assert_eq!(should_warn(&i), classify(&i) == RowState::Over, "{name}");
        warned += usize::from(should_warn(&i));
    }
    assert_eq!(warned, 1, "only the over, not asked row warns");
    assert_eq!(watch_base(None), BASE_THRESHOLD);
    assert_eq!(watch_base(Some(120_000)), 120_000);
}

#[test]
fn ctx_10_rule_a_cycle_starts_at_newest_compaction() {
    let m = mark("2026-09-20T10:05:00.000Z", 900, Some(18_207));
    let warned = ev("2026-09-20T10:00:00Z", "context-warned");
    // Warned before the marker: a new cycle, not asked.
    assert!(!already_asked(std::slice::from_ref(&warned), Some(&m)));
    // No compaction at all: any ask counts.
    assert!(already_asked(std::slice::from_ref(&warned), None));
    // Asked after the marker. `Z` and `.000Z` compare as instants.
    let asked = ev("2026-09-20T10:06:00Z", "compact-requested");
    assert!(already_asked(&[warned.clone(), asked.clone()], Some(&m)));
    let same = ev("2026-09-20T10:05:00Z", "compact-requested");
    assert!(
        already_asked(&[same], Some(&m)),
        "at the cycle start counts"
    );
    // A `compacted` event later than the marker starts the cycle there.
    let compacted = ev("2026-09-20T10:10:00Z", "compacted");
    assert!(!already_asked(
        &[warned.clone(), asked.clone(), compacted.clone()],
        Some(&m)
    ));
    assert!(!already_asked(&[asked, compacted.clone()], None));
    let later = ev("2026-09-20T10:11:00.000Z", "context-warned");
    assert!(already_asked(&[compacted, later], Some(&m)));
    // A note is not an ask.
    assert!(!already_asked(
        &[ev("2026-09-20T10:06:00Z", "note")],
        Some(&m)
    ));
}

// ─── CTX-25: native and horch compactions ───────────────────────────────────

fn marks_at(at: &[&str]) -> Vec<String> {
    at.iter().map(|s| s.to_string()).collect()
}

/// CTX-25: a mark is horch-driven when a `compacted` event is at or after it
/// and within 15 minutes of it; each event matches 1 mark; the rest are
/// native.
#[test]
fn ctx_25_counts_native_and_horch_compactions() {
    // 2 marks, 1 matched `compacted` event: 1 native, 1 horch.
    let marks = marks_at(&["2026-09-20T09:00:00Z", "2026-09-20T10:05:00Z"]);
    let history = vec![
        ev("2026-09-20T10:04:00Z", "compact-requested"),
        ev("2026-09-20T10:06:00Z", "compacted"),
    ];
    assert_eq!(count_compactions(&marks, &history), (1, 1));

    // No marks: nothing, whatever the ledger says.
    assert_eq!(count_compactions(&[], &history), (0, 0));

    // The window: at the mark and at 15 minutes match; before the mark and
    // after 15 minutes do not.
    let mark = marks_at(&["2026-09-20T10:00:00Z"]);
    for (event_at, want) in [
        ("2026-09-20T10:00:00Z", (0, 1)),
        ("2026-09-20T10:15:00Z", (0, 1)),
        ("2026-09-20T10:15:01Z", (1, 0)),
        ("2026-09-20T09:59:59Z", (1, 0)),
    ] {
        let history = vec![ev(event_at, "compacted")];
        assert_eq!(count_compactions(&mark, &history), want, "{event_at}");
    }

    // 1 event matches 1 mark, not 2.
    let close = marks_at(&["2026-09-20T10:00:00Z", "2026-09-20T10:01:00Z"]);
    let one = vec![ev("2026-09-20T10:02:00Z", "compacted")];
    assert_eq!(count_compactions(&close, &one), (1, 1));
    let two = vec![
        ev("2026-09-20T10:02:00Z", "compacted"),
        ev("2026-09-20T10:03:00Z", "compacted"),
    ];
    assert_eq!(count_compactions(&close, &two), (0, 2));

    // Only `compacted` counts; mark times compare as instants, not strings.
    let other = vec![
        ev("2026-09-20T10:01:00Z", "compact-failed"),
        ev("2026-09-20T10:01:00Z", "context-warned"),
    ];
    assert_eq!(count_compactions(&mark, &other), (1, 0));
    let offset = marks_at(&["2026-09-20T12:00:00+02:00"]);
    let utc = vec![ev("2026-09-20T10:05:00.000Z", "compacted")];
    assert_eq!(count_compactions(&offset, &utc), (0, 1));

    // A mark with no readable time is native.
    let bad = marks_at(&[""]);
    assert_eq!(count_compactions(&bad, &utc), (1, 0));
}

// ─── CTX-12, CTX-14: handoffs ───────────────────────────────────────────────

#[test]
fn ctx_12_handoff_path_per_role_is_distinct() {
    assert_eq!(
        handoff_path("sonnet-1"),
        "ai_docs/handoffs/sonnet-1-whats-next.md"
    );
    assert_eq!(
        handoff_path("orchestrator"),
        "ai_docs/handoffs/orchestrator-whats-next.md"
    );
    assert_ne!(handoff_path("sonnet-1"), handoff_path("sonnet-2"));
    assert_eq!(handoff_path("a/b"), "ai_docs/handoffs/a-b-whats-next.md");
    assert_eq!(
        handoff_path("x y.z_1"),
        "ai_docs/handoffs/x-y.z_1-whats-next.md"
    );
}

#[test]
fn ctx_14_handoff_freshness_rule() {
    let now = sys("2026-10-06T11:00:00Z");
    // No ask: the 60-minute rule.
    assert_eq!(handoff_fresh(None, None, now), Err(HandoffProblem::Missing));
    assert_eq!(
        handoff_fresh(Some(sys("2026-10-06T10:01:00Z")), None, now),
        Ok(())
    );
    assert_eq!(
        handoff_fresh(Some(sys("2026-10-06T09:59:00Z")), None, now),
        Err(HandoffProblem::Stale { minutes: 61 })
    );
    // With an ask at 10:00: later than the ask, at any age.
    let asked = Some(sys("2026-10-06T10:00:00Z"));
    let later = sys("2026-10-06T12:01:00Z");
    assert_eq!(
        handoff_fresh(Some(sys("2026-10-06T10:01:00Z")), asked, later),
        Ok(())
    );
    assert_eq!(
        handoff_fresh(Some(sys("2026-10-06T09:59:00Z")), asked, now),
        Err(HandoffProblem::BeforeAsk {
            asked_at: "2026-10-06T10:00:00Z".into()
        })
    );
    assert_eq!(
        handoff_fresh(None, asked, now),
        Err(HandoffProblem::Missing)
    );
    // asked_at: the newest request or warning; notes do not count.
    let history = [
        ev("2026-10-06T09:00:00Z", "compact-requested"),
        ev("2026-10-06T10:00:00.000Z", "context-warned"),
        ev("2026-10-06T10:30:00Z", "note"),
    ];
    assert_eq!(asked_at(&history), asked);
    assert_eq!(asked_at(&history[2..]), None);
}

// ─── CTX-20: the compact line ───────────────────────────────────────────────

#[test]
fn ctx_20_compact_line_has_keep_list_only_where_accepted() {
    assert_eq!(
        compact_line(caps(HarnessKind::Claude), "keep X"),
        Some("/compact keep X".into())
    );
    assert_eq!(
        compact_line(caps(HarnessKind::Codex), "keep X"),
        Some("/compact".into())
    );
    assert_eq!(compact_line(caps(HarnessKind::Antigravity), "keep X"), None);
    assert_eq!(compact_line(caps(HarnessKind::None), "keep X"), None);
    assert_eq!(
        compact_line(caps(HarnessKind::Claude), "keep X\nthen Y"),
        None,
        "a second line would be typed as a prompt"
    );
}

// ─── CTX-13, CTX-15, CTX-22, CTX-23: the job ────────────────────────────────

const OLD_MARK: u64 = 100;
const NEW_MARK: u64 = 5_000;
const FAILED: &str =
    "[horch] BLOCKED: compaction of sonnet-1 failed at step {step}: {reason}. log /s/job.log";
const REPORTED: &str = "[horch] NOTE: sonnet-1 compacted. {pre} -> {post} tokens.";

/// A scripted pane and transcript, on a fake clock.
struct Fake {
    statuses: RefCell<VecDeque<&'static str>>,
    /// The new marker shows from this send on. None: never.
    marker_after: Option<usize>,
    /// The tokens after the marker; None keeps the reading pending.
    post: Option<u64>,
    mark_post: Option<u64>,
    clock: Cell<Duration>,
    /// What happened, in order: `status <s>`, `send <line>`, `marker`.
    trace: RefCell<Vec<String>>,
    sends: RefCell<Vec<String>>,
    events: RefCell<Vec<(String, String)>>,
    reports: RefCell<Vec<String>>,
}

impl Fake {
    fn new(statuses: &[&'static str], marker_after: Option<usize>) -> Fake {
        Fake {
            statuses: RefCell::new(statuses.iter().copied().collect()),
            marker_after,
            post: Some(18_207),
            mark_post: Some(18_207),
            clock: Cell::new(Duration::ZERO),
            trace: RefCell::default(),
            sends: RefCell::default(),
            events: RefCell::default(),
            reports: RefCell::default(),
        }
    }

    fn marker_seen(&self) -> bool {
        self.marker_after
            .is_some_and(|n| self.sends.borrow().len() >= n)
    }
}

impl JobPorts for Fake {
    fn pane_status(&self) -> anyhow::Result<Option<String>> {
        let mut q = self.statuses.borrow_mut();
        let s = if q.len() > 1 {
            q.pop_front().unwrap()
        } else {
            q.front().copied().unwrap_or("idle")
        };
        self.trace.borrow_mut().push(format!("status {s}"));
        Ok(Some(s.into()))
    }

    fn send(&self, line: &str) -> anyhow::Result<()> {
        self.trace.borrow_mut().push(format!("send {line}"));
        self.sends.borrow_mut().push(line.into());
        Ok(())
    }

    fn reading(&self) -> Result<Reading, Unreadable> {
        let old = mark("2026-09-20T09:00:00Z", OLD_MARK, Some(9_000));
        if !self.marker_seen() {
            return Ok(Reading {
                last_compaction: Some(old),
                marks: 1,
                ..reading(Some(311_225))
            });
        }
        self.trace.borrow_mut().push("marker".into());
        Ok(Reading {
            pending: self.post.is_none(),
            last_compaction: Some(mark("2026-09-20T10:05:00Z", NEW_MARK, self.mark_post)),
            marks: 2,
            ..reading(self.post)
        })
    }

    fn record_event(&self, event: &str, text: &str) -> anyhow::Result<()> {
        self.events.borrow_mut().push((event.into(), text.into()));
        Ok(())
    }

    fn report(&self, line: &str) -> anyhow::Result<()> {
        self.reports.borrow_mut().push(line.into());
        Ok(())
    }

    fn log(&self, _step: Step, _text: &str) {}

    fn sleep(&self, d: Duration) {
        self.clock.set(self.clock.get() + d);
    }

    fn elapsed(&self) -> Duration {
        self.clock.get()
    }
}

fn plan(max_sends: u8) -> JobPlan {
    JobPlan {
        compact_line: "/compact keep X".into(),
        max_sends,
        resume_line: "Resume from ai_docs/handoffs/sonnet-1-whats-next.md".into(),
        reported_tmpl: REPORTED.into(),
        failed_tmpl: FAILED.into(),
        handoff: "ai_docs/handoffs/sonnet-1-whats-next.md".into(),
        role: "sonnet-1".into(),
        log_path: "/s/job.log".into(),
        poll: Duration::from_secs(2),
        idle_polls: 2,
        idle_timeout: Duration::from_secs(1800),
        resend_after: Duration::from_secs(30),
        marker_timeout: Duration::from_secs(600),
        settle_timeout: Duration::from_secs(600),
        reading_timeout: Duration::from_secs(180),
    }
}

fn compact_sends(f: &Fake) -> usize {
    f.sends
        .borrow()
        .iter()
        .filter(|s| s.starts_with("/compact"))
        .count()
}

#[test]
fn ctx_13_job_waits_for_two_idle_polls() {
    let f = Fake::new(&["working", "idle", "working", "idle", "idle"], Some(1));
    run(&f, &plan(1)).unwrap();
    let trace = f.trace.borrow();
    let first_send = trace.iter().position(|t| t.starts_with("send")).unwrap();
    let polls = trace[..first_send]
        .iter()
        .filter(|t| t.starts_with("status"))
        .count();
    assert_eq!(polls, 5, "the send follows the 5th poll: {trace:?}");
    assert_eq!(compact_sends(&f), 1);
}

#[test]
fn ctx_13_job_confirms_by_marker_then_types_resume() {
    let f = Fake::new(&["idle"], Some(1));
    let done = run(&f, &plan(1)).unwrap();
    assert_eq!(done.mark.offset, NEW_MARK);
    let trace = f.trace.borrow();
    let resume = trace
        .iter()
        .position(|t| t.starts_with("send Resume"))
        .unwrap();
    let marker = trace.iter().position(|t| t == "marker").unwrap();
    assert!(marker < resume, "{trace:?}");
    assert_eq!(
        trace[marker + 1..resume],
        ["status idle".to_string(), "status idle".to_string()],
        "2 idle polls between the marker and the resume"
    );
    assert_eq!(f.sends.borrow().len(), 2, "the compact line and the resume");
}

#[test]
fn ctx_13_codex_resends_at_most_three_times() {
    let f = Fake::new(&["idle"], None);
    let err = run(&f, &plan(3)).unwrap_err();
    assert_eq!(compact_sends(&f), 3);
    assert_eq!(
        err,
        JobFailure {
            step: Step::WaitMarker,
            reason: "no compaction marker after 3 sends".into()
        }
    );
}

#[test]
fn ctx_13_claude_gets_one_send() {
    let f = Fake::new(&["idle"], None);
    let err = run(&f, &plan(1)).unwrap_err();
    assert_eq!(compact_sends(&f), 1);
    assert_eq!(err.step, Step::WaitMarker);
    assert_eq!(err.reason, "no compaction marker after 1 sends");
    assert!(f.elapsed() >= Duration::from_secs(600), "{:?}", f.elapsed());
    assert!(f.elapsed() < Duration::from_secs(610), "{:?}", f.elapsed());
}

#[test]
fn ctx_15_idle_timeout_fails_with_step_and_reason() {
    let f = Fake::new(&["working"], Some(1));
    let err = run(&f, &plan(1)).unwrap_err();
    assert_eq!(
        err,
        JobFailure {
            step: Step::WaitIdle,
            reason: "no idle status in 1800 s".into()
        }
    );
    assert!(f.sends.borrow().is_empty());
}

#[test]
fn ctx_15_failure_reports_blocked_line_and_event() {
    let f = Fake::new(&["working"], Some(1));
    run(&f, &plan(1)).unwrap_err();
    assert_eq!(
        *f.events.borrow(),
        [(
            "compact-failed".to_string(),
            "step wait-idle: no idle status in 1800 s".to_string()
        )]
    );
    assert_eq!(
        *f.reports.borrow(),
        [
            "[horch] BLOCKED: compaction of sonnet-1 failed at step wait-idle: \
          no idle status in 1800 s. log /s/job.log"
                .to_string()
        ]
    );
}

#[test]
fn ctx_22_job_records_compacted_with_exact_text() {
    let f = Fake::new(&["idle"], Some(1));
    let done = run(&f, &plan(1)).unwrap();
    assert_eq!((done.pre, done.post), (Some(311_225), Some(18_207)));
    assert_eq!(
        *f.events.borrow(),
        [(
            "compacted".to_string(),
            "311225 -> 18207 tokens; handoff ai_docs/handoffs/sonnet-1-whats-next.md".to_string()
        )]
    );

    // No response after the marker and no post figure in it: unknown.
    let mut f = Fake::new(&["idle"], Some(1));
    f.post = None;
    f.mark_post = None;
    let done = run(&f, &plan(1)).unwrap();
    assert_eq!(done.post, None);
    assert_eq!(
        f.events.borrow()[0].1,
        "311225 -> unknown tokens; handoff ai_docs/handoffs/sonnet-1-whats-next.md"
    );
}

#[test]
fn ctx_23_job_reports_success_once() {
    let f = Fake::new(&["idle"], Some(1));
    run(&f, &plan(1)).unwrap();
    assert_eq!(
        *f.reports.borrow(),
        ["[horch] NOTE: sonnet-1 compacted. 311225 -> 18207 tokens.".to_string()]
    );
}

// ─── CTX-15: the job dir and the shared heartbeat ───────────────────────────

fn write_json(dir: &Path, name: &str, value: &impl serde::Serialize) {
    std::fs::write(dir.join(name), serde_json::to_vec(value).unwrap()).unwrap();
}

fn job(step: &str) -> JobRecord {
    JobRecord {
        v: 1,
        role: "sonnet-1".into(),
        record_id: "rec-1".into(),
        started_at: "2026-10-06T10:00:00Z".into(),
        step: step.into(),
    }
}

/// A heartbeat written now by this test's own process.
fn own_heartbeat() -> Heartbeat {
    let pid = std::process::id();
    Heartbeat {
        pid,
        at: horch_core::clock::stamp(Utc::now()),
        started: horch_core::procid::start_time(pid),
    }
}

/// A fresh heartbeat whose process has ended.
fn dead_heartbeat() -> Heartbeat {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    Heartbeat {
        pid,
        at: horch_core::clock::stamp(Utc::now()),
        // No process has this start time.
        started: Some(1),
    }
}

#[test]
fn ctx_15_lost_job_classifies_compact_lost() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = jobfile::job_dir(tmp.path(), "ws1", "sonnet-1");
    assert_eq!(dir, tmp.path().join("compact/ws1-sonnet-1"));
    std::fs::create_dir_all(&dir).unwrap();
    let now = Utc::now();

    // No job.json and no spawn: no job.
    assert_eq!(jobfile::liveness(&dir, now), JobLiveness::None);

    // A fresh spawn and no heartbeat yet: live.
    write_json(
        &dir,
        SPAWNED_FILE,
        &jobfile::Spawned {
            pid: 1,
            at: horch_core::clock::stamp(now),
        },
    );
    assert_eq!(jobfile::liveness(&dir, now), JobLiveness::Live);
    std::fs::remove_file(dir.join(SPAWNED_FILE)).unwrap();

    // job.json, created once only.
    create_job(&dir, &job("wait-idle")).unwrap();
    let again = create_job(&dir, &job("wait-idle")).unwrap_err();
    assert_eq!(again.kind(), std::io::ErrorKind::AlreadyExists);

    // job.json with this process's fresh heartbeat: live.
    write_json(&dir, HEARTBEAT_FILE, &own_heartbeat());
    assert_eq!(jobfile::liveness(&dir, now), JobLiveness::Live);

    // job.json with a dead pid's heartbeat: lost, at the step of job.json.
    jobfile::write_step(&dir, &job("wait-marker")).unwrap();
    write_json(&dir, HEARTBEAT_FILE, &dead_heartbeat());
    assert_eq!(
        jobfile::liveness(&dir, now),
        JobLiveness::Lost {
            step: "wait-marker".into()
        }
    );

    // The first reader claims the loss; the second does not.
    assert_eq!(claim_lost(&dir).unwrap(), Some(job("wait-marker")));
    assert_eq!(claim_lost(&dir).unwrap(), None);

    // start_locked on a lost dir: clears it (job.log stays), spawns once.
    std::fs::write(dir.join(LOG_FILE), "old log\n").unwrap();
    write_json(
        &dir,
        SPAWNED_FILE,
        &jobfile::Spawned {
            pid: 9,
            at: "2026-10-06T09:00:00Z".into(),
        },
    );
    let calls = Cell::new(0);
    let started = start_locked(&dir, || {
        calls.set(calls.get() + 1);
        Ok(4242)
    })
    .unwrap();
    assert_eq!(started, Started::Spawned { pid: 4242 });
    assert_eq!(calls.get(), 1);
    for gone in [JOB_FILE, HEARTBEAT_FILE, LOST_FILE] {
        assert!(!dir.join(gone).exists(), "{gone} stays");
    }
    assert_eq!(
        std::fs::read_to_string(dir.join(LOG_FILE)).unwrap(),
        "old log\n"
    );
    let spawned: jobfile::Spawned =
        serde_json::from_slice(&std::fs::read(dir.join(SPAWNED_FILE)).unwrap()).unwrap();
    assert_eq!(spawned.pid, 4242, "the new spawn replaces the old file");

    // start_locked on a live dir spawns nothing.
    create_job(&dir, &job("wait-idle")).unwrap();
    write_json(&dir, HEARTBEAT_FILE, &own_heartbeat());
    let started = start_locked(&dir, || {
        calls.set(calls.get() + 1);
        Ok(1)
    })
    .unwrap();
    assert_eq!(started, Started::AlreadyRunning);
    assert_eq!(calls.get(), 1);
    assert!(dir.join(JOB_FILE).exists());

    // The child's exit clears its files and keeps the log.
    jobfile::clear_on_exit(&dir);
    assert_eq!(jobfile::liveness(&dir, Utc::now()), JobLiveness::None);
    assert!(dir.join(LOG_FILE).exists());

    // The shared heartbeat says what the judge's decide says.
    let now = at("2026-10-02T12:00:30Z");
    let hb = |at: &str| Heartbeat {
        pid: 7,
        at: at.into(),
        started: None,
    };
    let cases = [
        (
            None,
            false,
            None,
            Liveness::NotStarted,
            JobState::NotStarted,
        ),
        (
            Some(hb("2026-10-02T12:00:25Z")),
            true,
            None,
            Liveness::Running { pid: 7 },
            JobState::Running { pid: 7 },
        ),
        (
            None,
            false,
            Some(at("2026-10-02T12:00:10Z")),
            Liveness::Running { pid: 0 },
            JobState::Running { pid: 0 },
        ),
        (
            Some(hb("2026-10-02T12:00:25Z")),
            false,
            None,
            Liveness::Lost,
            JobState::Lost,
        ),
        (
            Some(hb("2026-10-02T11:59:00Z")),
            true,
            None,
            Liveness::Lost,
            JobState::Lost,
        ),
        (
            None,
            false,
            Some(at("2026-10-02T11:59:00Z")),
            Liveness::Lost,
            JobState::Lost,
        ),
    ];
    for (hb, pid_alive, spawned_at, live, state) in cases {
        let got = heartbeat::liveness(hb.as_ref(), pid_alive, spawned_at, now, STALE_AFTER);
        assert_eq!(got, live, "{hb:?} {pid_alive} {spawned_at:?}");
        let facts = JobFacts {
            heartbeat: hb,
            pid_alive,
            spawned_at,
            ..JobFacts::default()
        };
        assert_eq!(scheduler::decide(&facts, now, STALE_AFTER), state);
    }
}
