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

const LEGACY_WORKING: &str = "working";
const LEGACY_DONE: &str = "done";

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FailureKind {
    AgentExited { code: Option<i32> },
    PaneVanished,
    TimedOut,
    Cancelled { reason: String },
    Crashed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStage {
    Brief,
    Split,
    Run,
}

impl ExecutionStatus {
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed { .. } | Self::LaunchFailed { .. }
        )
    }

    pub(crate) fn legacy_status(&self) -> &'static str {
        if self.is_terminal() {
            LEGACY_DONE
        } else {
            LEGACY_WORKING
        }
    }

    pub fn from_legacy(status: &str) -> Self {
        if status == LEGACY_WORKING {
            Self::Running
        } else {
            Self::Done
        }
    }

    pub fn resolve(state: Option<&ExecutionStatus>, status: &str) -> Self {
        match state {
            Some(s) => s.clone(),
            None => Self::from_legacy(status),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionKind {
    Worker,
    Orchestrator,
    Candidate {
        experiment: ExperimentId,
        round: RoundId,
        label: String,
    },
    Judge {
        round: RoundId,
        attempt: u32,
    },
}

impl ExecutionKind {
    pub(crate) fn legacy_kind(&self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Worker | Self::Candidate { .. } | Self::Judge { .. } => "worker",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMode {
    Fresh(Option<SessionId>),
    Resume(SessionId),
}

impl SessionMode {
    pub(crate) fn is_resume(&self) -> bool {
        matches!(self, Self::Resume(_))
    }

    pub fn id(&self) -> Option<&SessionId> {
        match self {
            Self::Fresh(id) => id.as_ref(),
            Self::Resume(id) => Some(id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "session", content = "id", rename_all = "snake_case")]
pub enum SessionState {
    Pending,
    Known(SessionId),
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TilingMode {
    Automatic,
    Disabled,
}

impl TilingMode {
    pub fn from_no_tile(flag: bool) -> Self {
        if flag {
            Self::Disabled
        } else {
            Self::Automatic
        }
    }
}

impl PartialEq for HistoryEntry {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at && self.event == other.event && self.text == other.text
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: Option<TaskId>,
    pub text: String,
    pub plan: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Execution {
    pub id: ExecutionId,
    pub kind: ExecutionKind,
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: String,
    pub model_override: bool,
    pub effort: Option<String>,
    pub phase: Option<Phase>,
    pub role: RoleName,
    pub task: Task,
    pub status: ExecutionStatus,
    pub typed_status: bool,
    pub session: SessionState,
    pub exit_code: Option<i32>,
    pub project: Option<PathBuf>,
    pub workdir: Option<PathBuf>,
    pub workspace: Option<WorkspaceId>,
    pub pane: Option<PaneId>,
    pub backend: crate::runtime::Backend,
    pub skills: Vec<ResolvedSkillRef>,
    pub routing: Option<RoutingProvenance>,
    pub via: Option<String>,
    pub substitution_reason: Option<String>,
    pub history: Vec<HistoryEntry>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
}

impl Execution {
    pub fn idempotency_key(&self) -> Option<String> {
        match &self.kind {
            ExecutionKind::Candidate { round, label, .. } => Some(format!("spawn:{round}:{label}")),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpawnRequest {
    pub teammate: Option<TeammateName>,
    pub resume: Option<String>,
    pub task: String,
    pub phase: Option<Phase>,
    pub effort: Option<String>,
    pub model: Option<String>,
    pub from_pane: Option<String>,
    pub direction: Direction,
    pub tiling: TilingMode,
    pub flags: GateFlags,
    pub pinned: bool,
    pub workdir: Option<PathBuf>,
    pub kind: ExecutionKind,
    pub report_to: ReportTarget,
}

impl SpawnRequest {
    pub fn worker(teammate: Option<TeammateName>, task: impl Into<String>) -> Self {
        SpawnRequest {
            teammate,
            resume: None,
            task: task.into(),
            phase: None,
            effort: None,
            model: None,
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

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchPlan {
    pub teammate: Teammate,
    pub model: String,
    pub model_alias: Option<String>,
    pub session: SessionMode,
    pub task: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspacePlan {
    pub workspace: Option<WorkspaceId>,
    pub from_pane: Option<String>,
    pub direction: Direction,
    pub tiling: TilingMode,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionPlan {
    pub execution: Execution,
    pub worker: Option<WorkerId>,
    pub launch: LaunchPlan,
    pub skills: SkillActivationPlan,
    pub workspace: WorkspacePlan,
    pub gate_line: Option<String>,
    pub resumed: bool,
    pub report_to: ReportTarget,
    pub controller: Option<String>,
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
            assert_eq!(ExecutionStatus::resolve(Some(&s), "working"), s);
            assert_eq!(ExecutionStatus::resolve(Some(&s), "done"), s);
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
