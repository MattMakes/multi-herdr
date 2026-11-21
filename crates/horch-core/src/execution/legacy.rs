use serde::{Deserialize, Serialize};

use crate::execution::model::ExecutionStatus;
use crate::roster::Phase;
use crate::skills::activation::ResolvedSkillRef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub at: String,
    pub event: String,
    pub text: String,
}

pub const STATUS_WORKING: &str = "working";
pub const STATUS_DONE: &str = "done";

pub const KIND_WORKER: &str = "worker";
pub const KIND_ORCHESTRATOR: &str = "orchestrator";

fn worker_kind() -> String {
    KIND_WORKER.to_string()
}

fn is_worker(kind: &str) -> bool {
    kind == KIND_WORKER
}

fn is_herdr(backend: &crate::runtime::Backend) -> bool {
    *backend == crate::runtime::Backend::Herdr
}

pub type Record = LedgerRecordV1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRecordV1 {
    pub record_id: String,
    pub session_id: Option<String>,
    pub agent: String,
    pub tier: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<Phase>,
    pub role: String,
    pub status: String,
    pub task: String,
    pub history: Vec<HistoryEntry>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default = "worker_kind", skip_serializing_if = "is_worker")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub substitution_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing: Option<crate::routing::decision::RoutingProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experiment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ExecutionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<ResolvedSkillRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_window: Option<LedgerWindow>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub model_override: bool,
    #[serde(default, skip_serializing_if = "is_herdr")]
    pub backend: crate::runtime::Backend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerWindow {
    #[serde(rename = "tokens", default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(rename = "source")]
    pub source: LedgerWindowSource,
    #[serde(rename = "detail")]
    pub detail: String,
    #[serde(rename = "applied")]
    pub applied: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerWindowSource {
    #[serde(rename = "operator")]
    Operator,
    #[serde(rename = "fleet")]
    Fleet,
    #[serde(rename = "harness")]
    Harness,
    #[serde(other)]
    Unknown,
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
            compact_window: None,
            model_override: false,
            backend: crate::runtime::Backend::Herdr,
        }
    }
}

impl LedgerRecordV1 {
    pub fn is_orchestrator(&self) -> bool {
        self.kind == KIND_ORCHESTRATOR
    }

    pub fn matches(&self, key: &str) -> bool {
        self.record_id == key || self.session_id.as_deref() == Some(key)
    }

    pub fn execution_status(&self) -> ExecutionStatus {
        ExecutionStatus::resolve(self.state.as_ref(), &self.status)
    }

    pub fn set_state(&mut self, state: ExecutionStatus) {
        self.status = state.legacy_status().to_string();
        self.state = Some(state);
    }

    pub(crate) fn set_legacy_status(&mut self, status: &str) {
        self.status = status.to_string();
        if let Some(state) = &self.state {
            let working = status == ExecutionStatus::Running.legacy_status();
            if working == state.is_terminal() {
                self.state = Some(ExecutionStatus::from_legacy(status));
            }
        }
    }

    pub(crate) fn sync_legacy_status(&mut self) {
        if let Some(state) = &self.state {
            self.status = state.legacy_status().to_string();
        }
    }
}
