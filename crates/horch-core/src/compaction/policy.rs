use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};

use super::window::BASE_THRESHOLD;
use crate::execution::legacy::HistoryEntry;
use crate::harness::capabilities::CompactionCaps;
use crate::telemetry::context::{CompactionMark, Reading};
use crate::telemetry::readers::Unreadable;

pub const EVENT_REQUESTED: &str = "compact-requested";
pub const EVENT_WARNED: &str = "context-warned";
pub const EVENT_COMPACTED: &str = "compacted";
pub const EVENT_FAILED: &str = "compact-failed";

pub const HORCH_MATCH_WINDOW: Duration = Duration::from_secs(15 * 60);

pub const REQUEST_DEADLINE: Duration = Duration::from_secs(20 * 60);

pub const MAX_REQUESTS: usize = 2;

pub const HANDOFF_MAX_AGE: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    CompactLost,
    NoSession,
    NotRead,
    NoTranscript,
    Unknown,
    Compacting,
    Pending,
    Handoff,
    CompactFailed,
    RequestStale,
    Requested,
    Over,
    Ok,
}

const ROW_STATE_NAMES: [&str; 13] = [
    "compact-lost",
    "no-session",
    "not-read",
    "no-transcript",
    "unknown",
    "compacting",
    "pending",
    "handoff",
    "compact-failed",
    "request-stale",
    "requested",
    "over",
    "ok",
];

const INSTRUCTION_IDS: [Option<&str>; 13] = [
    Some("ctx-compact-lost"),
    Some("ctx-no-session"),
    Some("ctx-not-read"),
    Some("ctx-no-transcript"),
    Some("ctx-unknown"),
    Some("ctx-compacting"),
    Some("ctx-pending"),
    Some("ctx-handoff"),
    Some("ctx-compact-failed"),
    Some("ctx-request-stale"),
    Some("ctx-requested"),
    Some("ctx-over"),
    None,
];

impl RowState {
    pub const ALL: [RowState; 13] = [
        RowState::CompactLost,
        RowState::NoSession,
        RowState::NotRead,
        RowState::NoTranscript,
        RowState::Unknown,
        RowState::Compacting,
        RowState::Pending,
        RowState::Handoff,
        RowState::CompactFailed,
        RowState::RequestStale,
        RowState::Requested,
        RowState::Over,
        RowState::Ok,
    ];

    pub fn as_str(self) -> &'static str {
        ROW_STATE_NAMES[self as usize]
    }

    pub fn instruction_id(self) -> Option<&'static str> {
        INSTRUCTION_IDS[self as usize]
    }
}

impl From<&Unreadable> for RowState {
    fn from(why: &Unreadable) -> Self {
        match why {
            Unreadable::NoSessionId => RowState::NoSession,
            Unreadable::NotRead(_) => RowState::NotRead,
            Unreadable::NoTranscript(_) => RowState::NoTranscript,
            Unreadable::Failed(_) => RowState::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobLiveness {
    None,
    Live,
    Lost { step: String },
}

#[derive(Debug, Clone, Copy)]
pub struct RowInputs<'a> {
    pub reading: Result<&'a Reading, &'a Unreadable>,
    pub threshold: u64,
    pub history: &'a [HistoryEntry],
    pub job: &'a JobLiveness,
    pub now: DateTime<Utc>,
}

pub fn classify(i: &RowInputs) -> RowState {
    if matches!(i.job, JobLiveness::Lost { .. }) {
        return RowState::CompactLost;
    }
    match i.reading {
        Err(why) => RowState::from(why),
        Ok(r) => classify_reading(r, i),
    }
}

fn classify_reading(r: &Reading, i: &RowInputs) -> RowState {
    if r.tokens.is_none() && !r.pending {
        RowState::Unknown
    } else if *i.job == JobLiveness::Live {
        RowState::Compacting
    } else if r.pending {
        RowState::Pending
    } else if still_over_after_compaction(i, r) {
        RowState::Handoff
    } else if let Some(state) = ask_state(i, r.last_compaction.as_ref()) {
        state
    } else {
        match r.tokens {
            Some(t) if t >= i.threshold => RowState::Over,
            _ => RowState::Ok,
        }
    }
}

fn still_over_after_compaction(i: &RowInputs, r: &Reading) -> bool {
    let over = |n: Option<u64>| n.is_some_and(|n| n >= i.threshold);
    over(r.tokens)
        && over(post_compaction_tokens(
            i.history,
            r.last_compaction.as_ref(),
        ))
}

pub fn post_compaction_tokens(
    history: &[HistoryEntry],
    last: Option<&CompactionMark>,
) -> Option<u64> {
    let marked = last.and_then(|m| crate::clock::parse(&m.at));
    let event = history
        .iter()
        .filter(|e| e.event == EVENT_COMPACTED)
        .filter_map(|e| Some((crate::clock::parse(&e.at)?, e)))
        .max_by_key(|(at, _)| *at)
        .filter(|(at, _)| marked.is_none_or(|m| *at >= m));
    event
        .and_then(|(_, e)| compacted_post(&e.text))
        .or_else(|| last.and_then(|m| m.post_tokens))
}

pub fn compacted_text(pre: Option<u64>, post: Option<u64>, handoff: &str) -> String {
    let n = |n: Option<u64>| n.map_or_else(|| "unknown".into(), |n| n.to_string());
    format!("{} -> {} tokens; handoff {handoff}", n(pre), n(post))
}

fn compacted_post(text: &str) -> Option<u64> {
    let (_, rest) = text.split_once(" -> ")?;
    rest.split_once(" tokens")?.0.parse().ok()
}

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

pub fn should_warn(i: &RowInputs) -> bool {
    classify(i) == RowState::Over
}

pub fn watch_base(compact_at: Option<u64>) -> u64 {
    compact_at.unwrap_or(BASE_THRESHOLD)
}

pub fn count_compactions(marks_at: &[String], history: &[HistoryEntry]) -> (usize, usize) {
    let mut marks: Vec<Option<DateTime<Utc>>> =
        marks_at.iter().map(|m| crate::clock::parse(m)).collect();
    marks.sort();
    let mut events: Vec<DateTime<Utc>> = history
        .iter()
        .filter(|e| e.event == EVENT_COMPACTED)
        .filter_map(|e| crate::clock::parse(&e.at))
        .collect();
    events.sort();
    let window = chrono::Duration::from_std(HORCH_MATCH_WINDOW).expect("15 minutes");
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

fn cycle_start(history: &[HistoryEntry], last: Option<&CompactionMark>) -> Option<DateTime<Utc>> {
    let marked = last.and_then(|m| crate::clock::parse(&m.at));
    newest(history, &[EVENT_COMPACTED]).max(marked)
}

pub fn cycle_ask(history: &[HistoryEntry], last: Option<&CompactionMark>) -> Option<DateTime<Utc>> {
    let start = cycle_start(history, last);
    newest(history, &[EVENT_REQUESTED, EVENT_WARNED]).filter(|ask| start.is_none_or(|s| *ask >= s))
}

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

pub fn ask_age(
    history: &[HistoryEntry],
    last: Option<&CompactionMark>,
    now: DateTime<Utc>,
) -> Option<Duration> {
    let ask = cycle_ask(history, last)?;
    Some((now - ask).to_std().unwrap_or_default())
}

fn ask_is_stale(
    history: &[HistoryEntry],
    last: Option<&CompactionMark>,
    now: DateTime<Utc>,
) -> bool {
    ask_age(history, last, now).is_some_and(|age| age > REQUEST_DEADLINE)
}

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

fn newest(history: &[HistoryEntry], events: &[&str]) -> Option<DateTime<Utc>> {
    history
        .iter()
        .filter(|e| events.contains(&e.event.as_str()))
        .filter_map(|e| crate::clock::parse(&e.at))
        .max()
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffProblem {
    Missing,
    Stale { minutes: u64 },
    BeforeAsk { asked_at: String },
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

pub fn asked_at(history: &[HistoryEntry]) -> Option<SystemTime> {
    newest(history, &[EVENT_REQUESTED, EVENT_WARNED]).map(SystemTime::from)
}

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
        assert_eq!(row(Ok(&blank), &none, &[]), RowState::Unknown);
        assert_eq!(row(Ok(&blank), &live, &[]), RowState::Unknown);
        assert_eq!(row(Ok(&pending_blank), &live, &[]), RowState::Compacting);
        assert_eq!(row(Ok(&over), &live, &failed), RowState::Compacting);
        assert_eq!(row(Ok(&pending), &none, &failed), RowState::Pending);
        assert_eq!(row(Ok(&pending_blank), &none, &[]), RowState::Pending);
        assert_eq!(row(Ok(&over), &none, &failed), RowState::CompactFailed);
        assert_eq!(row(Ok(&ok), &none, &asked), RowState::Requested);
        assert_eq!(row(Ok(&over), &none, &asked), RowState::Requested);
        assert_eq!(row(Ok(&over), &none, &[]), RowState::Over);
        assert_eq!(row(Ok(&ok), &none, &[]), RowState::Ok);
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
            (RowState::Handoff, "handoff"),
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

    #[test]
    fn trf_08_every_non_ok_state_has_a_stable_instruction_id() {
        let want = [
            (RowState::CompactLost, Some("ctx-compact-lost")),
            (RowState::NoSession, Some("ctx-no-session")),
            (RowState::NotRead, Some("ctx-not-read")),
            (RowState::NoTranscript, Some("ctx-no-transcript")),
            (RowState::Unknown, Some("ctx-unknown")),
            (RowState::Compacting, Some("ctx-compacting")),
            (RowState::Pending, Some("ctx-pending")),
            (RowState::Handoff, Some("ctx-handoff")),
            (RowState::CompactFailed, Some("ctx-compact-failed")),
            (RowState::RequestStale, Some("ctx-request-stale")),
            (RowState::Requested, Some("ctx-requested")),
            (RowState::Over, Some("ctx-over")),
            (RowState::Ok, None),
        ];
        assert_eq!(want.len(), RowState::ALL.len());
        for ((state, id), listed) in want.iter().zip(RowState::ALL) {
            assert_eq!(*state, listed, "ALL is in declaration order");
            assert_eq!(state.instruction_id(), *id, "{}", state.as_str());
        }
    }

    fn compacted_at(at: &str, pre: u64, post: u64) -> HistoryEntry {
        ev(
            at,
            EVENT_COMPACTED,
            &compacted_text(Some(pre), Some(post), "h.md"),
        )
    }

    fn mark(at: &str, post: Option<u64>) -> CompactionMark {
        CompactionMark {
            at: at.into(),
            offset: 10,
            trigger: Some("manual".into()),
            pre_tokens: Some(311_225),
            post_tokens: post,
        }
    }

    fn with_mark(history: &[HistoryEntry], m: Option<CompactionMark>, now: &str) -> RowState {
        let mut reading = over_reading();
        reading.last_compaction = m;
        classify(&RowInputs {
            reading: Ok(&reading),
            threshold: 300_000,
            history,
            job: &JobLiveness::None,
            now: at(now),
        })
    }

    #[test]
    fn trf_09_still_over_after_1_compaction_is_handoff_not_a_new_cycle() {
        let history = [
            ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, ""),
            compacted_at("2026-09-20T10:05:00Z", 330_000, 305_000),
        ];
        let m = Some(mark("2026-09-20T10:04:00Z", Some(305_000)));
        assert_eq!(
            with_mark(&history, m.clone(), "2026-09-20T10:06:00Z"),
            RowState::Handoff
        );
        let asked_again = [
            history[0].clone(),
            history[1].clone(),
            ev("2026-09-20T10:07:00Z", EVENT_REQUESTED, ""),
        ];
        assert_eq!(
            with_mark(&asked_again, m, "2026-09-20T10:08:00Z"),
            RowState::Handoff
        );
    }

    #[test]
    fn trf_09_l3_repro_over_after_compaction_starts_no_new_cycle() {
        let history = [
            ev("2026-09-20T10:00:00Z", EVENT_REQUESTED, ""),
            ev("2026-09-20T10:03:00Z", EVENT_COMPACTED, ""),
            ev("2026-09-20T10:10:00Z", EVENT_REQUESTED, ""),
            ev("2026-09-20T10:13:00Z", EVENT_COMPACTED, ""),
        ];
        let mut reading = over_reading();
        reading.tokens = Some(305_000);
        reading.last_compaction = Some(mark("2026-09-20T10:12:59Z", Some(305_000)));
        let i = RowInputs {
            reading: Ok(&reading),
            threshold: 300_000,
            history: &history,
            job: &JobLiveness::None,
            now: at("2026-09-20T10:20:00Z"),
        };
        assert_eq!(classify(&i), RowState::Handoff);
        assert!(!should_warn(&i), "no 3rd cycle starts");
    }

    #[test]
    fn trf_09_a_native_compaction_that_leaves_the_row_over_is_handoff() {
        let m = Some(mark("2026-09-20T10:04:00Z", Some(301_000)));
        assert_eq!(with_mark(&[], m, "2026-09-20T10:06:00Z"), RowState::Handoff);
    }

    #[test]
    fn trf_09_a_row_that_grew_back_after_a_good_compaction_is_over() {
        let history = [compacted_at("2026-09-20T10:05:00Z", 330_000, 9_000)];
        let m = Some(mark("2026-09-20T10:04:00Z", Some(9_000)));
        assert_eq!(
            with_mark(&history, m, "2026-09-20T12:00:00Z"),
            RowState::Over
        );
        let unknown = [ev(
            "2026-09-20T10:05:00Z",
            EVENT_COMPACTED,
            &compacted_text(Some(330_000), None, "h.md"),
        )];
        assert_eq!(
            with_mark(&unknown, None, "2026-09-20T10:06:00Z"),
            RowState::Over
        );
    }

    #[test]
    fn trf_09_a_newer_native_marker_wins_over_an_older_compacted_event() {
        let history = [compacted_at("2026-09-20T10:05:00Z", 330_000, 305_000)];
        let m = Some(mark("2026-09-20T11:00:00Z", Some(9_000)));
        assert_eq!(post_compaction_tokens(&history, m.as_ref()), Some(9_000));
        assert_eq!(
            with_mark(&history, m, "2026-09-20T11:30:00Z"),
            RowState::Over
        );
    }

    #[test]
    fn trf_09_handoff_needs_the_current_tokens_over_too() {
        let history = [compacted_at("2026-09-20T10:05:00Z", 330_000, 305_000)];
        let mut reading = over_reading();
        reading.tokens = Some(200_000);
        let got = classify(&RowInputs {
            reading: Ok(&reading),
            threshold: 300_000,
            history: &history,
            job: &JobLiveness::None,
            now: at("2026-09-20T10:06:00Z"),
        });
        assert_eq!(got, RowState::Ok);
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
        assert_eq!(
            compact_line(caps(CompactCommand::Unknown, 1), "keep x"),
            None
        );
        assert_eq!(compact_line(caps(CompactCommand::Bare, 0), "keep x"), None);
        assert_eq!(
            compact_line(caps(CompactCommand::Bare, 1), "keep x").as_deref(),
            Some("/compact")
        );
        assert_eq!(compact_line(with, "a\nb"), None);
        assert_eq!(compact_line(with, "a\rb"), None);
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
