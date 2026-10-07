//! The fleet row schemas (fleet-dataset §3.1-3.3).
//!
//! The fields are the spec's, in the spec's order. No type denies unknown
//! fields: a reader of an older version reads a newer row.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `schema` of a start row.
pub const START_SCHEMA: &str = "mh.fleet-start/1.0.0";
/// `schema` of a run row.
pub const RUN_SCHEMA: &str = "mh.fleet-run/1.0.0";
/// `schema` of a verdict row.
pub const VERDICT_SCHEMA: &str = "mh.fleet-verdict/1.0.0";

/// `mh.fleet-start/1.0.0`: what `horch spawn` knew when the record got its
/// pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartRow {
    pub schema: String,
    pub record_id: String,
    pub at: String,
    /// `git rev-parse HEAD` in the project; null outside a git repository.
    pub base_sha: Option<String>,
    /// The plan file the task names, relative to the project.
    pub plan_path: Option<String>,
    /// `sha256:<hex>` of the plan file bytes.
    pub plan_digest: Option<String>,
    /// The plan text, at most [`crate::fleet_runs::facts::PLAN_TEXT_CAP`]
    /// bytes.
    pub plan_text: Option<String>,
    pub plan_truncated: bool,
}

/// One skill a record was briefed with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillExpected {
    pub id: String,
    pub version: String,
    pub digest: String,
}

/// How a record ended: its ledger `status`, `state` and `exit_code`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunEnd {
    pub status: String,
    /// The ledger `state` object as is; null for a record without one.
    pub state: Option<Value>,
    pub exit_code: Option<i32>,
}

/// The tokens of a session transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunTokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
}

/// Which path wrote a run row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WrittenBy {
    Done,
    Sync,
}

/// The segment of a run row without one.
fn first_segment() -> u32 {
    1
}

/// `mh.fleet-run/1.0.0`: the facts of one finished segment of a ledger
/// record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRow {
    pub schema: String,
    pub record_id: String,
    pub session_id: Option<String>,
    pub role: String,
    /// 1 for the first run of the record, 1 more for each resume. A row
    /// written before segments existed reads as 1.
    #[serde(default = "first_segment")]
    pub segment: u32,
    /// `worker` or `orchestrator`.
    pub kind: String,
    pub project: String,
    pub project_slug: String,
    pub teammate: String,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub phase: Option<String>,
    /// The ledger `routing`, copied as is.
    pub routing: Option<Value>,
    pub substitution_reason: Option<String>,
    pub skills_expected: Vec<SkillExpected>,
    pub task: String,
    pub task_digest: String,
    pub plan_path: Option<String>,
    pub plan_digest: Option<String>,
    pub base_sha: Option<String>,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: i64,
    pub end: RunEnd,
    pub done_summary: Option<String>,
    pub notes: u32,
    pub resumed: bool,
    pub tokens: Option<RunTokens>,
    pub cost_microusd: Option<i64>,
    /// `price_table@<date>` or `unpriced`.
    pub cost_source: String,
    pub transcript_ref: Option<String>,
    pub transcript_digest: Option<String>,
    pub usage_at: String,
    pub head_sha: Option<String>,
    pub written_by: WrittenBy,
    pub horch_version: String,
}

/// `mh.fleet-verdict/1.0.0`: the orchestrator's check of one record's DONE.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerdictRow {
    pub schema: String,
    pub record_id: String,
    pub role: String,
    /// `accepted`, `rework` or `rejected`.
    pub verdict: String,
    pub note: Option<String>,
    pub at: String,
    /// The orchestrator's record id, when known.
    pub by: Option<String>,
}
