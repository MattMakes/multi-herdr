//! The on-disk ledger record, `LedgerRecordV1`.
//!
//! It is the pre-refactor `ledger::Record` serde, field for field and in the
//! same order, plus optional keys that are skipped when empty. A record
//! without the new keys serializes byte-identical to before, and the struct
//! has no `deny_unknown_fields`, so an old binary reads a new ledger and
//! ignores what it does not know.
//!
//! # `status` and `state`
//!
//! `status` stays the legacy `"working"` / `"done"` string. The typed
//! [`ExecutionStatus`] goes under the key `state`, as a nested object with
//! its own internal `state` tag:
//!
//! ```json
//! "status": "done",
//! "state": {"state": "launch_failed", "stage": "split", "reason": "no such pane"}
//! ```
//!
//! The nesting means the outer key and the inner tag never clash. When
//! `state` is set, the store writes `status` as `state.legacy_status()`, so
//! a failed record reads as `done` to an old binary, never as live. A reader
//! takes [`LedgerRecordV1::execution_status`]: `state` when present, else the
//! status derived from `status`.

use serde::{Deserialize, Serialize};

use crate::execution::model::ExecutionStatus;
use crate::roster::Phase;
use crate::skills::activation::ResolvedSkillRef;

/// One entry in a record's history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub at: String,
    pub event: String,
    pub text: String,
}

/// `Record::kind` of a worker pane.
pub const KIND_WORKER: &str = "worker";
/// `Record::kind` of a fleet orchestrator pane.
pub const KIND_ORCHESTRATOR: &str = "orchestrator";

fn worker_kind() -> String {
    KIND_WORKER.to_string()
}

fn is_worker(kind: &str) -> bool {
    kind == KIND_WORKER
}

/// The name every caller still uses for a ledger record.
pub type Record = LedgerRecordV1;

/// One agent session: a worker, or a fleet's orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRecordV1 {
    pub record_id: String,
    /// `null` until known: codex only reveals its session id after launch.
    pub session_id: Option<String>,
    pub agent: String,
    /// The teammate's name. Still spelled `tier` on disk: ledgers written by
    /// earlier versions carry this key, and the built-in teammates kept the old
    /// tier ids as their names precisely so those records still resolve.
    pub tier: String,
    pub model: String,
    /// The effort level actually passed to the agent CLI, if any. Recorded so
    /// a cost report can say what a session was tuned to, and so a resume
    /// keeps the level it ran at. Absent in ledgers written before effort was
    /// recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<Phase>,
    pub role: String,
    /// `"working"` or `"done"`, for old readers. See the module docs.
    pub status: String,
    pub task: String,
    pub history: Vec<HistoryEntry>,
    pub created_at: String,
    pub updated_at: String,
    /// `worker` or `orchestrator`. Absent in older ledgers, which only ever
    /// recorded workers.
    #[serde(default = "worker_kind", skip_serializing_if = "is_worker")]
    pub kind: String,
    /// The absolute project path. The ledger's file name is a lossy slug of
    /// it, so the path is kept here for the telemetry space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The plan-file slug the task names (`ai_docs/plans/<slug>.md`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    /// The herdr workspace the pane runs in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    /// The fallback teammate whose launch settings the spawn gate used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// Why the gate substituted, in one line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substitution_reason: Option<String>,
    /// What routing decided for this record (ARC-14). Absent before A5. It
    /// stays in the place A5 wrote it, so those ledgers keep their bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<crate::routing::decision::RoutingProvenance>,
    // Keys added in A6. Each is absent on disk until set.
    /// The herdr pane the execution runs in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// The directory the agent runs in, when it is not the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
    /// A candidate's or judge's experiment. Its `kind` stays `worker`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_id: Option<String>,
    /// A candidate's label within its round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The typed status. See the module docs for its shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ExecutionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    /// The exact skill versions the execution was briefed with (SKL-04).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<ResolvedSkillRef>,
}

impl Default for LedgerRecordV1 {
    fn default() -> Self {
        LedgerRecordV1 {
            record_id: String::new(),
            session_id: None,
            agent: String::new(),
            tier: String::new(),
            model: String::new(),
            effort: None,
            phase: None,
            role: String::new(),
            status: String::new(),
            task: String::new(),
            history: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
            kind: worker_kind(),
            project: None,
            plan: None,
            workspace_id: None,
            via: None,
            substitution_reason: None,
            routing: None,
            pane_id: None,
            task_id: None,
            workdir: None,
            experiment_id: None,
            round_id: None,
            label: None,
            state: None,
            exit_code: None,
            finished_at: None,
            skills: Vec::new(),
        }
    }
}

impl LedgerRecordV1 {
    pub fn is_orchestrator(&self) -> bool {
        self.kind == KIND_ORCHESTRATOR
    }

    /// Records match on either id so callers can address a session by whichever
    /// one they hold.
    pub fn matches(&self, key: &str) -> bool {
        self.record_id == key || self.session_id.as_deref() == Some(key)
    }

    /// The typed status: `state` when set, else derived from `status`.
    pub fn execution_status(&self) -> ExecutionStatus {
        ExecutionStatus::resolve(self.state.as_ref(), &self.status)
    }

    /// Set the typed status and the legacy `status` that goes with it.
    pub fn set_state(&mut self, state: ExecutionStatus) {
        self.status = state.legacy_status().to_string();
        self.state = Some(state);
    }

    /// Set the legacy `status` from a caller that only knows the string. A
    /// typed `state` follows it, so the two never disagree: `"working"`
    /// reopens a terminal state as running, and anything else closes a
    /// non-terminal one as done. A record without `state` keeps none.
    pub fn set_legacy_status(&mut self, status: &str) {
        self.status = status.to_string();
        if let Some(state) = &self.state {
            let working = status == ExecutionStatus::Running.legacy_status();
            if working == state.is_terminal() {
                self.state = Some(ExecutionStatus::from_legacy(status));
            }
        }
    }

    /// Make `status` agree with `state` before a write.
    pub fn sync_legacy_status(&mut self) {
        if let Some(state) = &self.state {
            self.status = state.legacy_status().to_string();
        }
    }
}
