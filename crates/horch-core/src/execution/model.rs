//! The typed states and modes of an execution.
//!
//! The ledger still stores a bare `status` string, `"working"` or `"done"`.
//! [`ExecutionStatus`] says more, and maps onto that string so a ledger an old
//! binary reads never shows a failed launch as live.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::execution::legacy::HistoryEntry;
use crate::execution::lifecycle::ReportTarget;
use crate::harness::HarnessKind;
use crate::ids::{
    ExecutionId, ExperimentId, PaneId, RoleName, RoundId, SessionId, TaskId, TeammateName,
    WorkerId, WorkspaceId,
};
use crate::roster::{Phase, Teammate};
use crate::routing::decision::{GateFlags, RoutingProvenance};
use crate::skills::activation::{ResolvedSkillRef, SkillActivationPlan};
use crate::workspace::model::Direction;

/// The ledger's legacy status for a live execution.
const LEGACY_WORKING: &str = "working";
/// The ledger's legacy status for every terminal execution.
const LEGACY_DONE: &str = "done";

/// Where an execution is in its life.
///
/// Serialized with an internal `state` tag in snake_case. A failure nests its
/// own `kind` tag under `failure`:
///
/// ```json
/// {"state":"running"}
/// {"state":"failed","failure":{"kind":"agent_exited","code":1}}
/// {"state":"failed","failure":{"kind":"cancelled","reason":"operator"}}
/// {"state":"launch_failed","stage":"split","reason":"no such pane"}
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ExecutionStatus {
    Planned,
    Starting,
    Running,
    Done,
    Failed { failure: FailureKind },
    LaunchFailed { stage: LaunchStage, reason: String },
}

/// Why a started execution ended without finishing its work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FailureKind {
    AgentExited { code: Option<i32> },
    PaneVanished,
    TimedOut,
    Cancelled { reason: String },
    Crashed,
}

/// The launch step that failed before the agent ever ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStage {
    /// Writing the worker's brief.
    Brief,
    /// Splitting a pane for the worker.
    Split,
    /// Running the worker command in the new pane.
    Run,
}

impl ExecutionStatus {
    /// Whether an agent may be running for this execution right now.
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }

    /// Whether this execution has ended, for good or ill.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed { .. } | Self::LaunchFailed { .. }
        )
    }

    /// The ledger's `status` string for this state. Every terminal state is
    /// `"done"`, so an old reader never mistakes a failure for a live worker.
    pub(crate) fn legacy_status(&self) -> &'static str {
        if self.is_terminal() {
            LEGACY_DONE
        } else {
            LEGACY_WORKING
        }
    }

    /// The best reading of a record that has only the legacy `status`.
    /// Anything but `"working"` is treated as finished.
    pub fn from_legacy(status: &str) -> Self {
        if status == LEGACY_WORKING {
            Self::Running
        } else {
            Self::Done
        }
    }

    /// A record's status: the typed `state` when it has one, else derived from
    /// the legacy `status`.
    pub fn resolve(state: Option<&ExecutionStatus>, status: &str) -> Self {
        match state {
            Some(s) => s.clone(),
            None => Self::from_legacy(status),
        }
    }
}

/// What an execution is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionKind {
    Worker,
    Orchestrator,
    /// One competing attempt in an experiment round.
    Candidate {
        experiment: ExperimentId,
        round: RoundId,
        label: String,
    },
    /// A judge run over a round's candidates.
    Judge {
        round: RoundId,
        attempt: u32,
    },
}

impl ExecutionKind {
    /// The ledger's `kind` string. Candidates and judges are workers to every
    /// reader that predates them.
    pub(crate) fn legacy_kind(&self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Worker | Self::Candidate { .. } | Self::Judge { .. } => "worker",
        }
    }
}

/// How an agent session starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMode {
    /// A new session. The id is set when horch mints it before launch, and
    /// `None` when the agent mints its own.
    Fresh(Option<SessionId>),
    /// Continue an earlier session.
    Resume(SessionId),
}

impl SessionMode {
    pub(crate) fn is_resume(&self) -> bool {
        matches!(self, Self::Resume(_))
    }

    /// The session id, when horch knows it before launch.
    pub fn id(&self) -> Option<&SessionId> {
        match self {
            Self::Fresh(id) => id.as_ref(),
            Self::Resume(id) => Some(id),
        }
    }
}

/// What horch knows about an execution's agent session id.
///
/// Serialized as `{"session":"pending"}` or `{"session":"known","id":"..."}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "session", content = "id", rename_all = "snake_case")]
pub enum SessionState {
    /// The agent mints it; it is not harvested yet.
    Pending,
    Known(SessionId),
    /// The agent never revealed one, or it could not be recovered.
    Unavailable,
}

/// Whether horch lays the grid out after a pane change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TilingMode {
    Automatic,
    Disabled,
}

impl TilingMode {
    /// The mode `--no-tile` asks for.
    pub fn from_no_tile(flag: bool) -> Self {
        if flag {
            Self::Disabled
        } else {
            Self::Automatic
        }
    }
}

// `legacy.rs` derives no `PartialEq` for history entries; plans compare them.
impl PartialEq for HistoryEntry {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at && self.event == other.event && self.text == other.text
    }
}

/// The work an execution was given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// Set by callers that track tasks (B3); absent for an ad hoc task.
    pub id: Option<TaskId>,
    /// The task text, or the idle placeholder for a worker spawned without one.
    pub text: String,
    /// The plan-file slug the task names (`ai_docs/plans/<slug>.md`).
    pub plan: Option<String>,
}

/// One run of one worker, orchestrator, candidate or judge: the typed view of
/// a ledger record. [`crate::execution::store::to_execution`] and
/// [`crate::execution::store::from_execution`] convert, losing nothing.
///
/// The field list is Spec A §4 (architecture design §4.3). The timestamps
/// stay the ledger's text, so a record written by any earlier version keeps
/// its bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Execution {
    pub id: ExecutionId,
    pub kind: ExecutionKind,
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: String,
    pub effort: Option<String>,
    pub phase: Option<Phase>,
    pub role: RoleName,
    pub task: Task,
    pub status: ExecutionStatus,
    /// Whether the ledger carries `status` as a typed `state`. False for a
    /// record written before A6, which keeps only the legacy string.
    pub typed_status: bool,
    pub session: SessionState,
    pub exit_code: Option<i32>,
    pub project: Option<PathBuf>,
    /// The directory the agent runs in, when it is not the project.
    pub workdir: Option<PathBuf>,
    pub workspace: Option<WorkspaceId>,
    pub pane: Option<PaneId>,
    pub skills: Vec<ResolvedSkillRef>,
    pub routing: Option<RoutingProvenance>,
    /// The fallback teammate whose launch settings ran, and why (legacy keys).
    pub via: Option<String>,
    pub substitution_reason: Option<String>,
    pub history: Vec<HistoryEntry>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
}

impl Execution {
    /// The key a repeated spawn of the same candidate finds this execution
    /// by: `spawn:<round>:<label>`. Other kinds have none.
    pub fn idempotency_key(&self) -> Option<String> {
        match &self.kind {
            ExecutionKind::Candidate { round, label, .. } => Some(format!("spawn:{round}:{label}")),
            _ => None,
        }
    }
}

/// What a caller asks `horch spawn` (or the B3 coordinator) to start.
///
/// The field list is Spec A §8 (architecture design §4.5).
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    /// The teammate to start. `None` with `resume`: the record names it.
    pub teammate: Option<TeammateName>,
    /// A record id or session id to resume.
    pub resume: Option<String>,
    /// Empty for an idle worker.
    pub task: String,
    pub phase: Option<Phase>,
    /// Effort for this spawn only, over the teammate's (or the record's).
    pub effort: Option<String>,
    /// An explicit role; `None` allocates `<teammate>-<n>`.
    pub role: Option<String>,
    /// The pane to split; `None` splits the caller's own pane.
    pub from_pane: Option<String>,
    pub direction: Direction,
    pub tiling: TilingMode,
    /// `exact`: never substitute; `force`: never refuse.
    pub flags: GateFlags,
    /// Skip the usage-limit gate: the teammate is fixed (B3 candidates).
    /// The provenance says `pinned`.
    pub pinned: bool,
    /// `None`: the agent runs in the project dir.
    pub workdir: Option<PathBuf>,
    pub kind: ExecutionKind,
    pub report_to: ReportTarget,
}

impl SpawnRequest {
    /// A worker spawn of `teammate` with every option at its default.
    pub fn worker(teammate: Option<TeammateName>, task: impl Into<String>) -> Self {
        SpawnRequest {
            teammate,
            resume: None,
            task: task.into(),
            phase: None,
            effort: None,
            role: None,
            from_pane: None,
            direction: Direction::Right,
            tiling: TilingMode::Automatic,
            flags: GateFlags::default(),
            pinned: false,
            workdir: None,
            kind: ExecutionKind::Worker,
            report_to: ReportTarget::Orchestrator,
        }
    }
}

/// What the worker will launch.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchPlan {
    /// Resolved: merged with the fallback when the gate substituted.
    pub teammate: Teammate,
    pub model: String,
    pub session: SessionMode,
    /// The task the worker is briefed with, as asked: empty for an idle
    /// worker or a resume without a new task.
    pub task: String,
}

/// Where the worker's pane goes.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspacePlan {
    /// Set by [`crate::execution::plan::finish_plan`].
    pub workspace: Option<WorkspaceId>,
    pub from_pane: Option<String>,
    pub direction: Direction,
    pub tiling: TilingMode,
}

/// A spawn, decided: everything the service writes and starts. Built with no
/// I/O by [`crate::execution::plan::plan_launch`].
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionPlan {
    /// Status `Planned`. A resume holds the record as it will be written.
    pub execution: Execution,
    /// Set by `finish_plan`, with the role.
    pub worker: Option<WorkerId>,
    pub launch: LaunchPlan,
    pub skills: SkillActivationPlan,
    pub workspace: WorkspacePlan,
    /// The NOTE or SUBSTITUTED line printed before the pane id.
    pub gate_line: Option<String>,
    /// True when the plan reopens an existing record.
    pub resumed: bool,
    pub report_to: ReportTarget,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_status() -> Vec<ExecutionStatus> {
        vec![
            ExecutionStatus::Planned,
            ExecutionStatus::Starting,
            ExecutionStatus::Running,
            ExecutionStatus::Done,
            ExecutionStatus::Failed {
                failure: FailureKind::AgentExited { code: Some(1) },
            },
            ExecutionStatus::Failed {
                failure: FailureKind::AgentExited { code: None },
            },
            ExecutionStatus::Failed {
                failure: FailureKind::PaneVanished,
            },
            ExecutionStatus::Failed {
                failure: FailureKind::TimedOut,
            },
            ExecutionStatus::Failed {
                failure: FailureKind::Cancelled {
                    reason: "operator".into(),
                },
            },
            ExecutionStatus::Failed {
                failure: FailureKind::Crashed,
            },
            ExecutionStatus::LaunchFailed {
                stage: LaunchStage::Brief,
                reason: "disk full".into(),
            },
            ExecutionStatus::LaunchFailed {
                stage: LaunchStage::Split,
                reason: "no such pane".into(),
            },
            ExecutionStatus::LaunchFailed {
                stage: LaunchStage::Run,
                reason: "pane closed".into(),
            },
        ]
    }

    #[test]
    fn arc_04_status_legacy_roundtrip() {
        for s in every_status() {
            let live = matches!(
                s,
                ExecutionStatus::Planned | ExecutionStatus::Starting | ExecutionStatus::Running
            );
            assert_eq!(
                s.legacy_status(),
                if live { "working" } else { "done" },
                "{s:?}"
            );
            assert_eq!(s.is_terminal(), !live, "{s:?}");
            assert_eq!(
                s.is_live(),
                matches!(s, ExecutionStatus::Starting | ExecutionStatus::Running),
                "{s:?}"
            );
            // The typed state always wins over the legacy string.
            assert_eq!(ExecutionStatus::resolve(Some(&s), "working"), s);
            assert_eq!(ExecutionStatus::resolve(Some(&s), "done"), s);
            // Serde round-trips through the documented shape.
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(serde_json::from_str::<ExecutionStatus>(&json).unwrap(), s);
        }

        assert_eq!(
            ExecutionStatus::from_legacy("working"),
            ExecutionStatus::Running
        );
        assert_eq!(ExecutionStatus::from_legacy("done"), ExecutionStatus::Done);
        assert_eq!(
            ExecutionStatus::from_legacy("garbage"),
            ExecutionStatus::Done
        );
        assert_eq!(
            ExecutionStatus::resolve(None, "working"),
            ExecutionStatus::Running
        );
        assert_eq!(
            ExecutionStatus::resolve(None, "done"),
            ExecutionStatus::Done
        );

        assert_eq!(
            serde_json::to_string(&ExecutionStatus::Failed {
                failure: FailureKind::AgentExited { code: Some(1) }
            })
            .unwrap(),
            r#"{"state":"failed","failure":{"kind":"agent_exited","code":1}}"#
        );
        assert_eq!(
            serde_json::to_string(&ExecutionStatus::LaunchFailed {
                stage: LaunchStage::Split,
                reason: "x".into()
            })
            .unwrap(),
            r#"{"state":"launch_failed","stage":"split","reason":"x"}"#
        );
        assert_eq!(
            serde_json::to_string(&ExecutionStatus::Running).unwrap(),
            r#"{"state":"running"}"#
        );

        let round = RoundId::new("r1").unwrap();
        for (k, legacy) in [
            (ExecutionKind::Worker, "worker"),
            (ExecutionKind::Orchestrator, "orchestrator"),
            (
                ExecutionKind::Candidate {
                    experiment: ExperimentId::new("e1").unwrap(),
                    round: round.clone(),
                    label: "A".into(),
                },
                "worker",
            ),
            (ExecutionKind::Judge { round, attempt: 1 }, "worker"),
        ] {
            assert_eq!(k.legacy_kind(), legacy);
        }
    }

    #[test]
    fn arc_04_tiling_mode() {
        assert_eq!(TilingMode::from_no_tile(true), TilingMode::Disabled);
        assert_eq!(TilingMode::from_no_tile(false), TilingMode::Automatic);

        let id = SessionId::new("s1").unwrap();
        assert!(SessionMode::Resume(id.clone()).is_resume());
        assert!(!SessionMode::Fresh(Some(id.clone())).is_resume());
        assert_eq!(SessionMode::Fresh(Some(id.clone())).id(), Some(&id));
        assert_eq!(SessionMode::Fresh(None).id(), None);
        assert_eq!(SessionMode::Resume(id.clone()).id(), Some(&id));

        assert_eq!(
            serde_json::to_string(&SessionState::Known(id)).unwrap(),
            r#"{"session":"known","id":"s1"}"#
        );
        assert_eq!(
            serde_json::to_string(&SessionState::Pending).unwrap(),
            r#"{"session":"pending"}"#
        );
    }
}
