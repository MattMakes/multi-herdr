//! WorkerRun 1.0.0 (dataset design §4.3, MEA-06).
//!
//! A WorkerRun is a projection of one execution plus the measure events of
//! its candidate. `worker_run_id ≡ execution_id`. Facts (what happened) and
//! scores (how it was rated) are separate objects. There is no winner field:
//! the winner is a property of the round, not of a run.
//!
//! The `Execution` struct arrives in A6; until then the execution's part
//! comes in as [`ExecutionFacts`].
//!
//! SPEC-TODO(Spec B WorkerRun): the field list verbatim. The golden
//! `tests/golden/worker-run-1.0.0.json` (MEA-11) freezes this shape; a
//! change bumps [`WORKER_RUN_SCHEMA_VERSION`].

use std::collections::BTreeMap;
use std::fmt;

use chrono::DateTime;
use serde::{Deserialize, Serialize};

use crate::evaluation::validator::GateResult;
use crate::execution::ExecutionStatus;
use crate::harness::HarnessKind;
use crate::ids::{ExecutionId, ExperimentId, ModelId, RoundId, SessionId, TaskId, TeammateName};
use crate::measure::digest::Digest;
use crate::measure::event::{EventEnvelope, SlotKind};
use crate::measure::projection::fold;
use crate::measure::NumstatLine;
use crate::routing::decision::RoutingProvenance;
use crate::skills::activation::ResolvedSkillRef;
use crate::usage::money::{CostSource, MicroUsd};
use crate::usage::Tokens;

pub const WORKER_RUN_SCHEMA_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRun {
    pub schema_version: String,
    pub worker_run_id: ExecutionId,
    pub experiment_id: ExperimentId,
    pub round_id: RoundId,
    pub label: String,
    pub task_id: TaskId,
    pub task_digest: Digest,
    pub config: RunConfig,
    pub facts: RunFacts,
    pub scores: RunScores,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    /// `<teammate>|<harness>|<model>|<effort>`, as the planner wrote it.
    pub config_id: String,
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: ModelId,
    pub effort: Option<String>,
    pub slot: SlotKind,
    pub propensity: f64,
    pub skills: Vec<ResolvedSkillRef>,
    pub routing: RoutingProvenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunFacts {
    pub status: ExecutionStatus,
    pub base_sha: String,
    pub head_sha: Option<String>,
    pub numstat: Vec<NumstatLine>,
    pub diff_digest: Option<Digest>,
    pub session_id: Option<SessionId>,
    /// A path, never a copy (SEC-03).
    pub transcript_ref: Option<String>,
    pub transcript_digest: Option<Digest>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub latency_ms: Option<u64>,
    pub tokens: Tokens,
    pub cost_microusd: MicroUsd,
    pub cost_source: CostSource,
    pub environment_digest: Digest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunScores {
    pub gates: Vec<GateResult>,
    /// Passed gates / total gates.
    pub mechanical_score: Option<f64>,
    /// From the Judgment, for this candidate's label.
    pub judge_components: Option<BTreeMap<String, f64>>,
}

/// The fields of a WorkerRun that come from the execution record, not from
/// measure events. A6/B3 build this from `execution::Execution`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionFacts {
    pub execution_id: ExecutionId,
    pub status: ExecutionStatus,
    pub session_id: Option<SessionId>,
    pub transcript_ref: Option<String>,
    pub transcript_digest: Option<Digest>,
    /// RFC 3339.
    pub started_at: String,
    pub finished_at: Option<String>,
    pub tokens: Tokens,
    pub cost_microusd: MicroUsd,
    pub cost_source: CostSource,
    pub skills: Vec<ResolvedSkillRef>,
    /// Used when the log has no `candidate.spawned` for this run.
    pub routing: Option<RoutingProvenance>,
}

/// Why a WorkerRun cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerRunError {
    /// No candidate in the log carries this execution id.
    NoCandidate { execution_id: ExecutionId },
    /// The candidate has no `candidate.planned` event.
    NotPlanned { label: String },
    /// Neither `candidate.spawned` nor the facts give a routing.
    NoRouting { label: String },
}

impl fmt::Display for WorkerRunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkerRunError::NoCandidate { execution_id } => {
                write!(
                    f,
                    "no candidate in the event log has execution {execution_id}"
                )
            }
            WorkerRunError::NotPlanned { label } => {
                write!(f, "candidate {label} has no candidate.planned event")
            }
            WorkerRunError::NoRouting { label } => {
                write!(f, "candidate {label} has no routing provenance")
            }
        }
    }
}

impl std::error::Error for WorkerRunError {}

impl WorkerRun {
    /// Project the run of `facts.execution_id` from the event log.
    /// `judge_components` are the judgment's component scores for the
    /// run's label, when a judgment exists.
    pub fn project(
        facts: &ExecutionFacts,
        events: &[EventEnvelope],
        judge_components: Option<BTreeMap<String, f64>>,
    ) -> Result<WorkerRun, WorkerRunError> {
        let projection = fold(events);
        let (round_id, round, label, cand) = projection
            .rounds
            .iter()
            .find_map(|(id, r)| {
                r.candidates
                    .iter()
                    .find(|(_, c)| c.execution_id.as_ref() == Some(&facts.execution_id))
                    .map(|(label, c)| (id, r, label, c))
            })
            .ok_or_else(|| WorkerRunError::NoCandidate {
                execution_id: facts.execution_id.clone(),
            })?;
        let experiment = &projection.experiments[&round.experiment_id].created;
        let planned = cand
            .planned
            .as_ref()
            .ok_or_else(|| WorkerRunError::NotPlanned {
                label: label.clone(),
            })?;
        let routing = cand
            .spawned
            .as_ref()
            .map(|s| s.routing.clone())
            .or_else(|| facts.routing.clone())
            .ok_or_else(|| WorkerRunError::NoRouting {
                label: label.clone(),
            })?;
        let base_sha = cand
            .worktree
            .as_ref()
            .map_or_else(|| round.created.base_sha.clone(), |w| w.base_sha.clone());
        let frozen = cand.frozen.as_ref();
        let validation = cand.validation.as_ref();

        Ok(WorkerRun {
            schema_version: WORKER_RUN_SCHEMA_VERSION.to_string(),
            worker_run_id: facts.execution_id.clone(),
            experiment_id: round.experiment_id.clone(),
            round_id: round_id.clone(),
            label: label.clone(),
            task_id: experiment.task_id.clone(),
            task_digest: experiment.task_digest,
            config: RunConfig {
                config_id: planned.config_id.clone(),
                teammate: planned.teammate.clone(),
                harness: planned.harness,
                model: planned.model.clone(),
                effort: planned.effort.clone(),
                slot: planned.slot,
                propensity: planned.propensity,
                skills: facts.skills.clone(),
                routing,
            },
            facts: RunFacts {
                status: facts.status.clone(),
                base_sha,
                head_sha: frozen.map(|f| f.head_sha.clone()),
                numstat: frozen.map(|f| f.numstat.clone()).unwrap_or_default(),
                diff_digest: frozen.map(|f| f.diff_digest),
                session_id: facts.session_id.clone(),
                transcript_ref: facts.transcript_ref.clone(),
                transcript_digest: facts.transcript_digest,
                started_at: facts.started_at.clone(),
                finished_at: facts.finished_at.clone(),
                latency_ms: latency_ms(&facts.started_at, facts.finished_at.as_deref()),
                tokens: facts.tokens,
                cost_microusd: facts.cost_microusd,
                cost_source: facts.cost_source.clone(),
                environment_digest: experiment.environment_digest,
            },
            scores: RunScores {
                gates: validation.map(|v| v.gates.clone()).unwrap_or_default(),
                mechanical_score: validation.map(|v| v.mechanical_score),
                judge_components,
            },
        })
    }
}

/// Milliseconds from `started` to `finished`; `None` when either is missing
/// or unparsable, or the clock went backwards.
fn latency_ms(started: &str, finished: Option<&str>) -> Option<u64> {
    let start = DateTime::parse_from_rfc3339(started).ok()?;
    let end = DateTime::parse_from_rfc3339(finished?).ok()?;
    u64::try_from((end - start).num_milliseconds()).ok()
}
