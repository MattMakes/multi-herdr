//! The fleet's compaction rules (CTX-10 to CTX-14, CTX-20). Pure: the
//! callers read the transcript, the ledger and the job dir, and pass what
//! they found.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};

use super::window::BASE_THRESHOLD;
use crate::execution::legacy::HistoryEntry;
use crate::harness::capabilities::CompactionCaps;
use crate::telemetry::context::{CompactionMark, Reading};
use crate::telemetry::readers::Unreadable;

/// Ledger event: the orchestrator asked a worker to prepare (CTX-11).
pub const EVENT_REQUESTED: &str = "compact-requested";
/// Ledger event: `horch note` warned a session over its threshold (CTX-10).
pub const EVENT_WARNED: &str = "context-warned";
/// Ledger event: the job confirmed a compaction (CTX-22).
pub const EVENT_COMPACTED: &str = "compacted";
/// Ledger event: a compaction failed or was lost (CTX-15).
pub const EVENT_FAILED: &str = "compact-failed";

/// A `compacted` event this long after a marker, or less, confirms that
/// horch drove the compaction (CTX-25).
pub const HORCH_MATCH_WINDOW: Duration = Duration::from_secs(15 * 60);

/// The newest ask of a cycle older than this, with no new ask, `compacted`
/// event or transcript marker after it, is stale (#75).
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(20 * 60);

/// `--request` types the line this many times in 1 cycle. The next stale or
/// failed row uses the fresh route (#75).
pub const MAX_REQUESTS: usize = 2;

/// Without an ask, a handoff older than this is stale (CTX-14).
pub const HANDOFF_MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// The state of 1 row of `horch context`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    CompactLost,
    NoSession,
    NotRead,
    NoTranscript,
    Unknown,
    Compacting,
    Pending,
    CompactFailed,
    RequestStale,
    Requested,
    Over,
    Ok,
}

/// The names `horch context` prints, in declaration order of [`RowState`].
const ROW_STATE_NAMES: [&str; 12] = [
    "compact-lost",
    "no-session",
    "not-read",
    "no-transcript",
    "unknown",
    "compacting",
    "pending",
    "compact-failed",
    "request-stale",
    "requested",
    "over",
    "ok",
];

impl RowState {
    /// The kebab-case name `horch context` prints.
    pub fn as_str(self) -> &'static str {
        ROW_STATE_NAMES[self as usize]
    }
}

impl From<&Unreadable> for RowState {
    /// The state of a row whose transcript does not read.
    fn from(why: &Unreadable) -> Self {
        match why {
            Unreadable::NoSessionId => RowState::NoSession,
            Unreadable::NotRead(_) => RowState::NotRead,
            Unreadable::NoTranscript(_) => RowState::NoTranscript,
            Unreadable::Failed(_) => RowState::Unknown,
        }
    }
}

/// What the job dir of the role says (`jobfile::liveness`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobLiveness {
    /// No job and no starting job.
    None,
    /// A job runs, or has just been spawned.
    Live,
    /// `job.json` exists and its process is gone: killed, or its heartbeat
    /// stopped. `step` is the step `job.json` names.
    Lost { step: String },
}

/// What 1 row is classified from.
#[derive(Debug, Clone, Copy)]
pub struct RowInputs<'a> {
    pub reading: Result<&'a Reading, &'a Unreadable>,
    pub threshold: u64,
    pub history: &'a [HistoryEntry],
    pub job: &'a JobLiveness,
    pub now: DateTime<Utc>,
}

/// First match: compact-lost (job Lost), no-session, not-read,
/// no-transcript, unknown (Failed, or tokens None and not pending),
/// compacting (job Live), pending, compact-failed, request-stale,
/// requested, over, ok.
///
/// `compact-lost` is first: a lost job is a failure the orchestrator must
/// see whatever the reading says (review finding 1). `requested` means
/// asked in this compaction cycle, whatever the tokens. `compact-failed`
/// means a job failed after the newest ask of the cycle; `request-stale`
/// means the newest ask is older than [`REQUEST_DEADLINE`] (#75).
pub fn classify(i: &RowInputs) -> RowState {
    if matches!(i.job, JobLiveness::Lost { .. }) {
        return RowState::CompactLost;
    }
    match i.reading {
        Err(why) => RowState::from(why),
        Ok(r) => classify_reading(r, i),
    }
}

/// The state of a row whose transcript reads, after the lost-job check.
fn classify_reading(r: &Reading, i: &RowInputs) -> RowState {
    if r.tokens.is_none() && !r.pending {
        RowState::Unknown
    } else if *i.job == JobLiveness::Live {
        RowState::Compacting
    } else if r.pending {
        RowState::Pending
    } else if let Some(state) = ask_state(i, r.last_compaction.as_ref()) {
        state
    } else {
        match r.tokens {
            Some(t) if t >= i.threshold => RowState::Over,
            _ => RowState::Ok,
        }
    }
}

/// `compact-failed`, `request-stale` or `requested` when the cycle holds an
/// ask (#75); None when it holds none.
fn ask_state(i: &RowInputs, last: Option<&CompactionMark>) -> Option<RowState> {
    if failed_after_ask(i.history, last).is_some() {
        return Some(RowState::CompactFailed);
    }
    if !already_asked(i.history, last) {
        return None;
    }
    Some(if ask_is_stale(i.history, last, i.now) {
        RowState::RequestStale
    } else {
        RowState::Requested
    })
}

/// The `horch note` warning rule (CTX-10), in core so it cannot drift from
/// [`classify`] (review finding 8). `Over` already means: tokens known, not
/// pending, no live or lost job, and not asked in this cycle.
pub fn should_warn(i: &RowInputs) -> bool {
    classify(i) == RowState::Over
}

/// The watch base for a teammate: `compact_at`, else [`BASE_THRESHOLD`].
/// It takes the value, not the teammate, so `compaction` does not import
/// `roster`.
pub fn watch_base(compact_at: Option<u64>) -> u64 {
    compact_at.unwrap_or(BASE_THRESHOLD)
}

/// CTX-25: how many of the markers at `marks_at` were native and how many
/// horch drove, as `(native, horch)`. A marker is horch-driven when a
/// `compacted` event is at or after it and at most [`HORCH_MATCH_WINDOW`]
/// after it; each event matches 1 marker. Times compare as UTC instants. A
/// marker with no readable time is native.
pub fn count_compactions(marks_at: &[String], history: &[HistoryEntry]) -> (usize, usize) {
    let mut marks: Vec<Option<DateTime<Utc>>> =
        marks_at.iter().map(|m| crate::clock::parse(m)).collect();
    // `None` first: unreadable marks never match, so their place is free.
    marks.sort();
    let mut events: Vec<DateTime<Utc>> = history
        .iter()
        .filter(|e| e.event == EVENT_COMPACTED)
        .filter_map(|e| crate::clock::parse(&e.at))
        .collect();
    events.sort();
    let window = chrono::Duration::from_std(HORCH_MATCH_WINDOW).expect("15 minutes");
    // Oldest mark first, each takes the oldest free event in its window: no
    // other order matches more marks, because every window has one length.
    let mut next = 0;
    let mut horch = 0;
    for mark in marks.iter().flatten() {
        while next < events.len() && events[next] < *mark {
            next += 1;
        }
        if next < events.len() && events[next] <= *mark + window {
            horch += 1;
            next += 1;
        }
    }
    (marks_at.len() - horch, horch)
}

/// Rule A (CTX-10, CTX-11.2): the cycle starts at the later of the newest
/// `compacted` event and `last.at`, compared as UTC instants, never as
/// strings. Asked = a `compact-requested` or `context-warned` event at or
/// after the cycle start (any, when there is no start).
pub fn already_asked(history: &[HistoryEntry], last: Option<&CompactionMark>) -> bool {
    let start = cycle_start(history, last);
    history
        .iter()
        .filter(|e| e.event == EVENT_REQUESTED || e.event == EVENT_WARNED)
        .any(|e| match start {
            None => true,
            Some(start) => crate::clock::parse(&e.at).is_some_and(|at| at >= start),
        })
}

/// The start of the current compaction cycle: the later of the newest
/// `compacted` event and `last.at`.
fn cycle_start(history: &[HistoryEntry], last: Option<&CompactionMark>) -> Option<DateTime<Utc>> {
    let marked = last.and_then(|m| crate::clock::parse(&m.at));
    newest(history, &[EVENT_COMPACTED]).max(marked)
}

/// The newest ask (`compact-requested` or `context-warned`) of the current
/// cycle, as an instant.
pub fn cycle_ask(history: &[HistoryEntry], last: Option<&CompactionMark>) -> Option<DateTime<Utc>> {
    let start = cycle_start(history, last);
    newest(history, &[EVENT_REQUESTED, EVENT_WARNED]).filter(|ask| start.is_none_or(|s| *ask >= s))
}

/// The newest `compact-failed` event of the current cycle that is later than
/// the cycle's newest ask. With no ask in the cycle there is none: the
/// failure of a compaction nobody asked for does not change the row. A new
/// ask or a marker ends it (#75).
pub fn failed_after_ask<'a>(
    history: &'a [HistoryEntry],
    last: Option<&CompactionMark>,
) -> Option<&'a HistoryEntry> {
    let start = cycle_start(history, last);
    let ask = cycle_ask(history, last)?;
    history
        .iter()
        .filter(|e| e.event == EVENT_FAILED)
        .filter_map(|e| Some((crate::clock::parse(&e.at)?, e)))
        .filter(|(at, _)| start.is_none_or(|s| *at >= s) && *at > ask)
        .max_by_key(|(at, _)| *at)
        .map(|(_, e)| e)
}

/// How long ago the newest ask of the cycle was made, when there is one.
pub fn ask_age(
    history: &[HistoryEntry],
    last: Option<&CompactionMark>,
    now: DateTime<Utc>,
) -> Option<Duration> {
    let ask = cycle_ask(history, last)?;
    // An ask stamped after `now` (clock skew) has age 0.
    Some((now - ask).to_std().unwrap_or_default())
}

/// The newest ask of the cycle is older than [`REQUEST_DEADLINE`].
fn ask_is_stale(
    history: &[HistoryEntry],
    last: Option<&CompactionMark>,
    now: DateTime<Utc>,
) -> bool {
    ask_age(history, last, now).is_some_and(|age| age > REQUEST_DEADLINE)
}

/// How many times `--request` asked in the current cycle.
pub fn request_count(history: &[HistoryEntry], last: Option<&CompactionMark>) -> usize {
    let start = cycle_start(history, last);
    history
        .iter()
        .filter(|e| e.event == EVENT_REQUESTED)
        .filter(|e| match start {
            None => true,
            Some(start) => crate::clock::parse(&e.at).is_some_and(|at| at >= start),
        })
        .count()
}

/// The newest time of the `events` in `history`.
fn newest(history: &[HistoryEntry], events: &[&str]) -> Option<DateTime<Utc>> {
    history
        .iter()
        .filter(|e| events.contains(&e.event.as_str()))
        .filter_map(|e| crate::clock::parse(&e.at))
        .max()
}

/// `ai_docs/handoffs/<role>-whats-next.md` (CTX-12). Role characters
/// outside `[A-Za-z0-9._-]` become `-`.
pub fn handoff_path(role: &str) -> String {
    let safe: String = role
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("ai_docs/handoffs/{safe}-whats-next.md")
}

/// Why a handoff file is not accepted (CTX-14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffProblem {
    Missing,
    /// No ask, and the file is older than [`HANDOFF_MAX_AGE`].
    Stale {
        minutes: u64,
    },
    /// The file is not later than the newest ask of the record.
    BeforeAsk {
        asked_at: String,
    },
}

impl std::fmt::Display for HandoffProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandoffProblem::Missing => f.write_str("the handoff file does not exist"),
            HandoffProblem::Stale { minutes } => {
                write!(f, "the handoff file is {minutes} minutes old")
            }
            HandoffProblem::BeforeAsk { asked_at } => {
                write!(f, "the handoff file is older than the ask at {asked_at}")
            }
        }
    }
}

/// CTX-14 (review finding 14). The caller resolves [`handoff_path`] against
/// the record's `workdir` (else the project dir), never its own cwd.
/// `asked_at` is [`asked_at`] of that record's history. With it: Ok only
/// when the file exists and its mtime is later than `asked_at`. Without it:
/// Ok when the file exists and is at most 60 minutes old.
pub fn handoff_fresh(
    mtime: Option<SystemTime>,
    asked_at: Option<SystemTime>,
    now: SystemTime,
) -> Result<(), HandoffProblem> {
    match (mtime, asked_at) {
        (None, _) => Err(HandoffProblem::Missing),
        (Some(m), Some(asked)) if m > asked => Ok(()),
        (Some(_), Some(asked)) => Err(HandoffProblem::BeforeAsk {
            asked_at: crate::clock::stamp(DateTime::<Utc>::from(asked)),
        }),
        (Some(m), None) => {
            let age = now.duration_since(m).unwrap_or_default();
            if age <= HANDOFF_MAX_AGE {
                Ok(())
            } else {
                Err(HandoffProblem::Stale {
                    minutes: age.as_secs() / 60,
                })
            }
        }
    }
}

/// The newest `compact-requested` or `context-warned` event of a record, as
/// an instant. `note` and other events do not count.
pub fn asked_at(history: &[HistoryEntry]) -> Option<SystemTime> {
    newest(history, &[EVENT_REQUESTED, EVENT_WARNED]).map(SystemTime::from)
}

/// The line typed into the pane: `/compact <instructions>` where the
/// harness takes them, `/compact` where it does not, None where it has no
/// command. Instructions with a line break are refused (None): a second
/// line would be typed as a prompt.
pub fn compact_line(caps: CompactionCaps, instructions: &str) -> Option<String> {
    let keep = instructions.trim();
    if !caps.has_command() {
        None
    } else if !caps.takes_instructions() {
        Some("/compact".into())
    } else if instructions.contains(['\n', '\r']) {
        None
    } else if keep.is_empty() {
        Some("/compact".into())
    } else {
        Some(format!("/compact {keep}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::capabilities::CompactCommand;
    use crate::telemetry::context::Reading;
    use std::path::PathBuf;

    fn at(raw: &str) -> DateTime<Utc> {
        crate::clock::parse(raw).expect("test time")
    }

    fn ev(at: &str, event: &str, text: &str) -> HistoryEntry {
        HistoryEntry {
            at: at.into(),
            event: event.into(),
            text: text.into(),
        }
    }

    fn over_reading() -> Reading {
        Reading {
            tokens: Some(311_225),
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

    fn state(history: &[HistoryEntry], now: &str) -> RowState {
        let reading = over_reading();
        classify(&RowInputs {
            reading: Ok(&reading),
            threshold: 300_000,
            history,
            job: &JobLiveness::None,
            now: at(now),
        })
    }

    #[test]
    fn an_ask_past_the_deadline_with_no_marker_is_request_stale() {
        let history = [ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, "")];
        assert_eq!(state(&history, "2026-09-20T10:19:00Z"), RowState::Requested);
        assert_eq!(
            state(&history, "2026-09-20T10:21:00Z"),
            RowState::RequestStale
        );
    }

    #[test]
    fn a_stale_warning_is_request_stale_too() {
        let history = [ev("2026-09-20T10:00:00Z", EVENT_WARNED, "")];
        assert_eq!(
            state(&history, "2026-09-20T11:00:00Z"),
            RowState::RequestStale
        );
    }

    #[test]
    fn a_new_ask_after_a_stale_one_is_requested_again() {
        let history = [
            ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, ""),
            ev("2026-09-20T10:30:00Z", EVENT_REQUESTED, ""),
        ];
        assert_eq!(state(&history, "2026-09-20T10:35:00Z"), RowState::Requested);
    }

    #[test]
    fn a_failure_after_the_ask_is_compact_failed_until_a_new_ask() {
        let history = [
            ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, ""),
            ev(
                "2026-09-20T10:05:00Z",
                EVENT_FAILED,
                "step wait-idle: timed out",
            ),
        ];
        assert_eq!(
            state(&history, "2026-09-20T10:06:00Z"),
            RowState::CompactFailed
        );
        // The failure outranks the deadline.
        assert_eq!(
            state(&history, "2026-09-20T12:00:00Z"),
            RowState::CompactFailed
        );
        let asked_again = [
            history[0].clone(),
            history[1].clone(),
            ev("2026-09-20T10:07:00Z", EVENT_REQUESTED, ""),
        ];
        assert_eq!(
            state(&asked_again, "2026-09-20T10:08:00Z"),
            RowState::Requested
        );
    }

    #[test]
    fn a_failure_before_the_ask_does_not_count() {
        let history = [
            ev("2026-09-20T09:00:00Z", EVENT_FAILED, "step start: boom"),
            ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, ""),
        ];
        assert_eq!(state(&history, "2026-09-20T10:01:00Z"), RowState::Requested);
    }

    #[test]
    fn a_marker_ends_a_failure_and_a_stale_ask() {
        let history = [
            ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, ""),
            ev("2026-09-20T10:05:00Z", EVENT_FAILED, "step start: boom"),
        ];
        let mut reading = over_reading();
        reading.last_compaction = Some(CompactionMark {
            at: "2026-09-20T10:30:00Z".into(),
            offset: 10,
            trigger: Some("manual".into()),
            pre_tokens: Some(311_225),
            post_tokens: Some(9_000),
        });
        let got = classify(&RowInputs {
            reading: Ok(&reading),
            threshold: 300_000,
            history: &history,
            job: &JobLiveness::None,
            now: at("2026-09-20T11:00:00Z"),
        });
        assert_eq!(got, RowState::Over);
    }

    #[test]
    fn a_failure_with_no_ask_in_the_cycle_leaves_the_row_over() {
        let history = [ev("2026-09-20T10:05:00Z", EVENT_FAILED, "step start: boom")];
        assert_eq!(state(&history, "2026-09-20T10:06:00Z"), RowState::Over);
        let reading = over_reading();
        let inputs = RowInputs {
            reading: Ok(&reading),
            threshold: 300_000,
            history: &history,
            job: &JobLiveness::None,
            now: at("2026-09-20T10:06:00Z"),
        };
        assert!(should_warn(&inputs), "the note warning still fires");
    }

    #[test]
    fn the_deadline_is_20_minutes_exactly() {
        let history = [ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, "")];
        assert_eq!(state(&history, "2026-09-20T10:20:00Z"), RowState::Requested);
        assert_eq!(
            state(&history, "2026-09-20T10:20:01Z"),
            RowState::RequestStale
        );
    }

    #[test]
    fn a_failure_in_the_same_second_as_the_ask_does_not_count() {
        let history = [
            ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, ""),
            ev("2026-09-20T10:00:00Z", EVENT_FAILED, "step start: boom"),
        ];
        assert_eq!(state(&history, "2026-09-20T10:01:00Z"), RowState::Requested);
    }

    #[test]
    fn a_compacted_event_ends_a_failure() {
        let history = [
            ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, ""),
            ev("2026-09-20T10:05:00Z", EVENT_FAILED, "step start: boom"),
            ev(
                "2026-09-20T10:10:00Z",
                EVENT_COMPACTED,
                "311225 -> 9000 tokens",
            ),
        ];
        assert_eq!(state(&history, "2026-09-20T10:11:00Z"), RowState::Over);
    }

    #[test]
    fn request_count_counts_only_requests_of_the_cycle() {
        let history = [
            ev("2026-09-20T09:00:00Z", EVENT_REQUESTED, ""),
            ev("2026-09-20T09:30:00Z", EVENT_COMPACTED, ""),
            ev("2026-09-20T10:00:00Z", EVENT_WARNED, ""),
            ev("2026-09-20T10:25:00Z", EVENT_REQUESTED, ""),
            ev("2026-09-20T10:50:00Z", EVENT_REQUESTED, ""),
        ];
        assert_eq!(request_count(&history, None), 2);
        assert_eq!(request_count(&[], None), 0);
    }

    #[test]
    fn ask_age_is_the_time_since_the_newest_ask() {
        let history = [
            ev("2026-09-20T10:00:00Z", EVENT_WARNED, ""),
            ev("2026-09-20T10:10:00Z", EVENT_REQUESTED, ""),
        ];
        assert_eq!(
            ask_age(&history, None, at("2026-09-20T10:25:00Z")),
            Some(Duration::from_secs(15 * 60))
        );
        assert_eq!(ask_age(&[], None, at("2026-09-20T10:25:00Z")), None);
        // An ask after `now` (clock skew) has age 0.
        assert_eq!(
            ask_age(&history, None, at("2026-09-20T10:05:00Z")),
            Some(Duration::ZERO)
        );
    }

    fn row(
        reading: Result<&Reading, &Unreadable>,
        job: &JobLiveness,
        history: &[HistoryEntry],
    ) -> RowState {
        classify(&RowInputs {
            reading,
            threshold: 300_000,
            history,
            job,
            now: at("2026-09-20T10:01:00Z"),
        })
    }

    #[test]
    fn classify_returns_each_state_in_policy_order() {
        let lost = JobLiveness::Lost {
            step: "send".into(),
        };
        let asked = [ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, "")];
        let failed = [
            ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, ""),
            ev("2026-09-20T10:00:30Z", EVENT_FAILED, "boom"),
        ];
        let over = over_reading();
        let mut ok = over_reading();
        ok.tokens = Some(1_000);
        let mut blank = over_reading();
        blank.tokens = None;
        let mut pending = over_reading();
        pending.pending = true;
        let mut pending_blank = blank.clone();
        pending_blank.pending = true;
        let none = JobLiveness::None;
        let live = JobLiveness::Live;

        // A lost job outranks every reading, even an unreadable one.
        assert_eq!(
            row(Err(&Unreadable::NoSessionId), &lost, &[]),
            RowState::CompactLost
        );
        assert_eq!(row(Ok(&over), &lost, &asked), RowState::CompactLost);
        assert_eq!(
            row(Err(&Unreadable::NoSessionId), &none, &[]),
            RowState::NoSession
        );
        assert_eq!(
            row(Err(&Unreadable::NotRead("x".into())), &live, &[]),
            RowState::NotRead
        );
        assert_eq!(
            row(Err(&Unreadable::NoTranscript("x".into())), &none, &[]),
            RowState::NoTranscript
        );
        assert_eq!(
            row(Err(&Unreadable::Failed("x".into())), &none, &[]),
            RowState::Unknown
        );
        // No tokens and not pending is unknown, even with a live job.
        assert_eq!(row(Ok(&blank), &none, &[]), RowState::Unknown);
        assert_eq!(row(Ok(&blank), &live, &[]), RowState::Unknown);
        // A live job outranks pending and every ask.
        assert_eq!(row(Ok(&pending_blank), &live, &[]), RowState::Compacting);
        assert_eq!(row(Ok(&over), &live, &failed), RowState::Compacting);
        // Pending outranks an ask.
        assert_eq!(row(Ok(&pending), &none, &failed), RowState::Pending);
        assert_eq!(row(Ok(&pending_blank), &none, &[]), RowState::Pending);
        // An ask outranks the threshold, in both directions.
        assert_eq!(row(Ok(&over), &none, &failed), RowState::CompactFailed);
        assert_eq!(row(Ok(&ok), &none, &asked), RowState::Requested);
        assert_eq!(row(Ok(&over), &none, &asked), RowState::Requested);
        assert_eq!(row(Ok(&over), &none, &[]), RowState::Over);
        assert_eq!(row(Ok(&ok), &none, &[]), RowState::Ok);
        // The threshold is inclusive.
        let mut edge = over_reading();
        edge.tokens = Some(300_000);
        assert_eq!(row(Ok(&edge), &none, &[]), RowState::Over);
        edge.tokens = Some(299_999);
        assert_eq!(row(Ok(&edge), &none, &[]), RowState::Ok);
    }

    #[test]
    fn classify_reports_request_stale_past_the_deadline() {
        let asked = [ev("2026-09-20T09:00:00Z", EVENT_REQUESTED, "")];
        let reading = over_reading();
        assert_eq!(
            row(Ok(&reading), &JobLiveness::None, &asked),
            RowState::RequestStale
        );
    }

    #[test]
    fn as_str_names_every_state() {
        let names = [
            (RowState::CompactLost, "compact-lost"),
            (RowState::NoSession, "no-session"),
            (RowState::NotRead, "not-read"),
            (RowState::NoTranscript, "no-transcript"),
            (RowState::Unknown, "unknown"),
            (RowState::Compacting, "compacting"),
            (RowState::Pending, "pending"),
            (RowState::CompactFailed, "compact-failed"),
            (RowState::RequestStale, "request-stale"),
            (RowState::Requested, "requested"),
            (RowState::Over, "over"),
            (RowState::Ok, "ok"),
        ];
        for (state, name) in names {
            assert_eq!(state.as_str(), name);
        }
    }

    fn caps(command: CompactCommand, max_sends: u8) -> CompactionCaps {
        CompactionCaps {
            command,
            max_sends,
            trigger: crate::harness::capabilities::TriggerRule::CodexLimit,
            failure_lines: &[],
        }
    }

    #[test]
    fn compact_line_covers_its_four_outcomes() {
        let with = caps(CompactCommand::WithInstructions, 1);
        // No command: none.
        assert_eq!(
            compact_line(caps(CompactCommand::Unknown, 1), "keep x"),
            None
        );
        assert_eq!(compact_line(caps(CompactCommand::Bare, 0), "keep x"), None);
        // A command without instructions types the bare line.
        assert_eq!(
            compact_line(caps(CompactCommand::Bare, 1), "keep x").as_deref(),
            Some("/compact")
        );
        // A line break is refused.
        assert_eq!(compact_line(with, "a\nb"), None);
        assert_eq!(compact_line(with, "a\rb"), None);
        // Instructions are trimmed; blank ones give the bare line.
        assert_eq!(
            compact_line(with, "  keep x  ").as_deref(),
            Some("/compact keep x")
        );
        assert_eq!(compact_line(with, "   ").as_deref(), Some("/compact"));
    }

    #[test]
    fn handoff_fresh_covers_its_branches() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let ago = |s: u64| Some(now - Duration::from_secs(s));
        assert_eq!(handoff_fresh(None, None, now), Err(HandoffProblem::Missing));
        assert_eq!(
            handoff_fresh(None, ago(5), now),
            Err(HandoffProblem::Missing)
        );
        assert_eq!(handoff_fresh(ago(10), ago(20), now), Ok(()));
        assert!(matches!(
            handoff_fresh(ago(20), ago(10), now),
            Err(HandoffProblem::BeforeAsk { .. })
        ));
        // Equal times are not later than the ask.
        assert!(matches!(
            handoff_fresh(ago(10), ago(10), now),
            Err(HandoffProblem::BeforeAsk { .. })
        ));
        assert_eq!(handoff_fresh(ago(3_600), None, now), Ok(()));
        assert_eq!(
            handoff_fresh(ago(3_660), None, now),
            Err(HandoffProblem::Stale { minutes: 61 })
        );
    }
}
