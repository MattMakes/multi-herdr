//! The typed states and modes of an execution.
//!
//! The ledger still stores a bare `status` string, `"working"` or `"done"`.
//! [`ExecutionStatus`] says more, and maps onto that string so a ledger an old
//! binary reads never shows a failed launch as live.

use serde::{Deserialize, Serialize};

use crate::ids::{ExperimentId, RoundId, SessionId};

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
    pub fn legacy_status(&self) -> &'static str {
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
    pub fn legacy_kind(&self) -> &'static str {
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
    pub fn is_resume(&self) -> bool {
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
