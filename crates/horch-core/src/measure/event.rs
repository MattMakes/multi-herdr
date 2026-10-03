//! The dataset event log's vocabulary (dataset design §4.1, MEA-02).
//!
//! An [`EventEnvelope`] is one line of `events/YYYY-MM-DD.jsonl`. Its
//! `payload` is plain JSON on disk; [`EventKind`] is the typed view of a
//! `(kind, payload)` pair. A kind this binary does not know becomes
//! [`EventKind::Unknown`] and keeps its payload, so a newer writer's events
//! survive a rebuild by an older reader.
//!
//! `PromotionStarted.strategy` is `serde_json::Value` until B5 builds
//! `PromotionStrategy`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::competition::preflight::PreflightReport;
use crate::evaluation::validator::ValidationReport;
use crate::evaluation::winner::RejectReason;
use crate::execution::FailureKind;
use crate::harness::HarnessKind;
use crate::ids::{
    EventId, ExecutionId, ExperimentId, JudgmentId, ModelId, PaneId, RoundId, TaskId, TeammateName,
};
use crate::measure::digest::Digest;
use crate::measure::NumstatLine;
use crate::routing::decision::RoutingProvenance;
use crate::routing::eligible::EligibleEntry;
use crate::teacher::TeacherRef;

pub const EVENT_SCHEMA_VERSION: &str = "1.0.0";

/// One event, as stored. Field order is the on-disk key order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub schema_version: String,
    /// UUIDv7, minted at `occurred_at`.
    pub event_id: EventId,
    /// The dotted name of the kind, such as `candidate.spawned`.
    pub kind: String,
    /// RFC 3339 UTC with exactly 3 fractional digits (see [`format_occurred_at`]).
    pub occurred_at: String,
    pub actor: Actor,
    pub experiment_id: ExperimentId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_id: Option<RoundId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<ExecutionId>,
    pub idempotency_key: String,
    pub payload: Value,
}

impl EventEnvelope {
    /// The typed view of this envelope's kind and payload.
    pub fn event(&self) -> Result<EventKind, EventError> {
        EventKind::from_parts(&self.kind, &self.payload)
    }
}

/// Who wrote an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Coordinator,
    Worker,
    JudgeJob,
    Operator,
}

/// `…T12:00:00.123Z`: millisecond precision, always 3 digits, always `Z`.
pub fn format_occurred_at(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Why a payload does not fit its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventError {
    /// A known kind whose payload does not deserialize.
    BadPayload { kind: String, error: String },
    /// A payload that does not serialize, such as a non-UTF-8 path.
    Unserializable { kind: String, error: String },
}

impl fmt::Display for EventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventError::BadPayload { kind, error } => {
                write!(f, "bad payload for event kind '{kind}': {error}")
            }
            EventError::Unserializable { kind, error } => {
                write!(
                    f,
                    "cannot serialize payload of event kind '{kind}': {error}"
                )
            }
        }
    }
}

impl std::error::Error for EventError {}

macro_rules! event_kinds {
    ($($(#[$meta:meta])* $variant:ident => $name:literal,)*) => {
        /// Every event kind, plus [`EventKind::Unknown`].
        ///
        /// SPEC-TODO(Spec B event list): the master plan names these kinds;
        /// confirm the list and every payload against Spec B verbatim.
        #[derive(Debug, Clone, PartialEq)]
        pub enum EventKind {
            $($(#[$meta])* $variant($variant),)*
            /// An unknown kind, kept as read so a newer writer's events
            /// survive a rebuild by an older reader (MEA-02).
            Unknown { kind: String, payload: Value },
        }

        impl EventKind {
            /// Every known dotted name, in declaration order.
            pub const KNOWN: &'static [&'static str] = &[$($name,)*];

            /// The dotted name.
            pub fn name(&self) -> &str {
                match self {
                    $(EventKind::$variant(_) => $name,)*
                    EventKind::Unknown { kind, .. } => kind,
                }
            }

            /// The payload as JSON.
            pub fn try_payload(&self) -> Result<Value, EventError> {
                let value = match self {
                    $(EventKind::$variant(p) => serde_json::to_value(p),)*
                    EventKind::Unknown { payload, .. } => return Ok(payload.clone()),
                };
                value.map_err(|e| EventError::Unserializable {
                    kind: self.name().to_string(),
                    error: e.to_string(),
                })
            }

            /// The payload as JSON. Panics only on a payload that cannot be
            /// JSON, such as a non-UTF-8 path; writers use [`Self::try_payload`].
            pub fn payload(&self) -> Value {
                self.try_payload().expect("event payload serializes")
            }

            /// Type a `(kind, payload)` pair. An unknown kind is `Unknown`
            /// with the payload kept as given.
            pub fn from_parts(kind: &str, payload: &Value) -> Result<EventKind, EventError> {
                match kind {
                    $($name => serde_json::from_value(payload.clone())
                        .map(EventKind::$variant)
                        .map_err(|e| EventError::BadPayload {
                            kind: kind.to_string(),
                            error: e.to_string(),
                        }),)*
                    _ => Ok(EventKind::Unknown {
                        kind: kind.to_string(),
                        payload: payload.clone(),
                    }),
                }
            }
        }
    };
}

event_kinds! {
    ExperimentCreated => "experiment.created",
    PreflightCompleted => "preflight.completed",
    ExperimentAborted => "experiment.aborted",
    RoundCreated => "round.created",
    CandidatePlanned => "candidate.planned",
    WorktreeCreated => "worktree.created",
    CandidateSpawned => "candidate.spawned",
    CandidateCompleted => "candidate.completed",
    CandidateFailed => "candidate.failed",
    CandidateFrozen => "candidate.frozen",
    ValidationCompleted => "validation.completed",
    JudgeScheduled => "judge.scheduled",
    JudgeStarted => "judge.started",
    JudgeCompleted => "judge.completed",
    JudgeFailed => "judge.failed",
    WinnerSelected => "winner.selected",
    WinnerRejected => "winner.rejected",
    PromotionStarted => "promotion.started",
    PromotionCompleted => "promotion.completed",
    PromotionConflicted => "promotion.conflicted",
    PromotionRolledBack => "promotion.rolled_back",
    WorktreeCleanupFailed => "worktree.cleanup_failed",
    OutcomeRecorded => "outcome.recorded",
    /// SPEC-TODO(Spec B event list): added by the orchestrator so a rebuild
    /// reaches NEEDS_INTERVENTION without a failed judge or promotion event.
    RoundNeedsIntervention => "round.needs_intervention",
    /// SPEC-TODO(Spec B event list): added so a rebuild reaches CLEANUP.
    RoundCleanupStarted => "round.cleanup_started",
    /// SPEC-TODO(Spec B event list): added so a rebuild reaches COMPLETE.
    RoundCompleted => "round.completed",
}

// ─── payloads ───────────────────────────────────────────────────────────────
// No payload sets `deny_unknown_fields`, so payloads stay forward-compatible.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExperimentCreated {
    pub task_id: TaskId,
    pub task_digest: Digest,
    pub config_digest: Digest,
    pub base_sha: String,
    pub repo_digest: Digest,
    pub environment_digest: Digest,
    pub candidates: u32,
    pub strategy: String,
    pub budget_usd_micro: i64,
    pub promote_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreflightCompleted {
    pub report: PreflightReport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExperimentAborted {
    pub reason: String,
    /// Requirement ids of the failed checks, such as `PRE-02`.
    pub failed_checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundCreated {
    pub index: u32,
    pub base_sha: String,
    pub labels: Vec<String>,
    pub eligible_set: Vec<EligibleEntry>,
    pub propensities: BTreeMap<String, f64>,
    pub teacher: TeacherRef,
    pub seed: u64,
    pub label_policy_version: String,
}

/// Why the planner chose a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Baseline,
    Diversity,
    Exploration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidatePlanned {
    pub label: String,
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: ModelId,
    pub effort: Option<String>,
    pub slot: SlotKind,
    pub propensity: f64,
    pub config_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorktreeCreated {
    pub label: String,
    pub path: PathBuf,
    pub branch: String,
    pub base_sha: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateSpawned {
    pub label: String,
    pub pane: PaneId,
    pub routing: RoutingProvenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateCompleted {
    pub label: String,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateFailed {
    pub label: String,
    pub failure: FailureKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateFrozen {
    pub label: String,
    pub head_sha: String,
    pub numstat: Vec<NumstatLine>,
    pub diff_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationCompleted {
    pub label: String,
    pub report: ValidationReport,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeScheduled {
    pub attempt: u32,
    pub input_digest: Digest,
    pub judge_policy_digest: Digest,
    pub job_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeStarted {
    pub attempt: u32,
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeCompleted {
    pub attempt: u32,
    pub judgment_id: JudgmentId,
    pub output_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeFailed {
    pub attempt: u32,
    pub cause: JudgeFailure,
}

/// Why a judge attempt produced no judgment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JudgeFailure {
    Crashed { code: Option<i32> },
    TimedOut,
    Lost,
    Malformed { error: String },
    OverCap,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WinnerSelected {
    pub label: String,
    pub execution_id: ExecutionId,
    pub head_sha: String,
    pub judgment_id: JudgmentId,
    pub promotion: PromotionIntent,
}

/// `"not_requested"` or `{"requested":{"target":"…"}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionIntent {
    NotRequested,
    Requested { target: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WinnerRejected {
    pub reason: RejectReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromotionStarted {
    pub target: String,
    pub dest_before: String,
    pub planned_after: String,
    // B5: typed once PromotionStrategy lands.
    pub strategy: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromotionCompleted {
    pub receipt_digest: Digest,
    pub dest_after: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromotionConflicted {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromotionRolledBack {
    pub target: String,
    pub restored: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorktreeCleanupFailed {
    pub label: String,
    pub path: PathBuf,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutcomeRecorded {
    pub kind: OutcomeKind,
    pub post_merge_score: f64,
    pub note: Option<String>,
}

/// What happened to a promoted change after the merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Regression,
    Revert,
    Verified,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundNeedsIntervention {
    pub reason: String,
    pub source: InterventionSource,
}

/// Who stopped the round for the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionSource {
    Judge,
    Promotion,
    Operator,
}

/// Cleanup begins: worktrees are removed, branches are kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundCleanupStarted {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundCompleted {
    pub final_outcome: FinalOutcome,
}

/// How a completed round ended: the state it held when cleanup started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalOutcome {
    Winner,
    Rejected,
    NeedsIntervention,
    Promoted,
}
