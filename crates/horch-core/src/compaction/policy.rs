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
    Requested,
    Over,
    Ok,
}

impl RowState {
    /// The kebab-case name `horch context` prints.
    pub fn as_str(self) -> &'static str {
        match self {
            RowState::CompactLost => "compact-lost",
            RowState::NoSession => "no-session",
            RowState::NotRead => "not-read",
            RowState::NoTranscript => "no-transcript",
            RowState::Unknown => "unknown",
            RowState::Compacting => "compacting",
            RowState::Pending => "pending",
            RowState::Requested => "requested",
            RowState::Over => "over",
            RowState::Ok => "ok",
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
}

/// First match: compact-lost (job Lost), no-session, not-read,
/// no-transcript, unknown (Failed, or tokens None and not pending),
/// compacting (job Live), pending, requested, over, ok.
///
/// `compact-lost` is first: a lost job is a failure the orchestrator must
/// see whatever the reading says (review finding 1). `requested` means
/// asked in this compaction cycle, whatever the tokens.
pub fn classify(i: &RowInputs) -> RowState {
    if matches!(i.job, JobLiveness::Lost { .. }) {
        return RowState::CompactLost;
    }
    let reading = match i.reading {
        Err(Unreadable::NoSessionId) => return RowState::NoSession,
        Err(Unreadable::NotRead(_)) => return RowState::NotRead,
        Err(Unreadable::NoTranscript(_)) => return RowState::NoTranscript,
        Err(Unreadable::Failed(_)) => return RowState::Unknown,
        Ok(r) => r,
    };
    if reading.tokens.is_none() && !reading.pending {
        return RowState::Unknown;
    }
    if *i.job == JobLiveness::Live {
        return RowState::Compacting;
    }
    if reading.pending {
        return RowState::Pending;
    }
    if already_asked(i.history, reading.last_compaction.as_ref()) {
        return RowState::Requested;
    }
    match reading.tokens {
        Some(t) if t >= i.threshold => RowState::Over,
        _ => RowState::Ok,
    }
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

/// Rule A (CTX-10, CTX-11.2): the cycle starts at the later of the newest
/// `compacted` event and `last.at`, compared as UTC instants, never as
/// strings. Asked = a `compact-requested` or `context-warned` event at or
/// after the cycle start (any, when there is no start).
pub fn already_asked(history: &[HistoryEntry], last: Option<&CompactionMark>) -> bool {
    let compacted = newest(history, &[EVENT_COMPACTED]);
    let marked = last.and_then(|m| crate::clock::parse(&m.at));
    let start = compacted.max(marked);
    history
        .iter()
        .filter(|e| e.event == EVENT_REQUESTED || e.event == EVENT_WARNED)
        .any(|e| match start {
            None => true,
            Some(start) => crate::clock::parse(&e.at).is_some_and(|at| at >= start),
        })
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
    let Some(mtime) = mtime else {
        return Err(HandoffProblem::Missing);
    };
    if let Some(asked) = asked_at {
        return if mtime > asked {
            Ok(())
        } else {
            Err(HandoffProblem::BeforeAsk {
                asked_at: crate::clock::stamp(DateTime::<Utc>::from(asked)),
            })
        };
    }
    let age = now.duration_since(mtime).unwrap_or_default();
    if age <= HANDOFF_MAX_AGE {
        Ok(())
    } else {
        Err(HandoffProblem::Stale {
            minutes: age.as_secs() / 60,
        })
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
    if !caps.has_command() {
        return None;
    }
    if !caps.takes_instructions() {
        return Some("/compact".into());
    }
    if instructions.contains(['\n', '\r']) {
        return None;
    }
    let keep = instructions.trim();
    Some(if keep.is_empty() {
        "/compact".into()
    } else {
        format!("/compact {keep}")
    })
}
