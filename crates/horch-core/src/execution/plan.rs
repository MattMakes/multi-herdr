//! Spawn planning: a [`SpawnRequest`] and what the shell read become an
//! [`ExecutionPlan`], with no I/O.
//!
//! [`plan_launch`] holds every rule `horch spawn` applies before it touches
//! the ledger or a pane: the resume rules, the usage-limit gate, the
//! reserved-tier check, effort and skill support. The shell reads the
//! roster, the quota view and the record to resume, mints the ids, and passes
//! them in [`PlanInputs`]. The same inputs always give the same plan (ARC-15).
//! [`finish_plan`] adds the role once the service allocated it under the
//! ledger lock.

use std::path::Path;

use chrono::{DateTime, Utc};

use crate::execution::legacy::{HistoryEntry, LedgerRecordV1};
use crate::execution::model::{
    Execution, ExecutionPlan, ExecutionStatus, LaunchPlan, SessionMode, SessionState, SpawnRequest,
    Task, WorkspacePlan,
};
use crate::execution::store::to_execution;
use crate::harness::HarnessKind;
use crate::ids::{ExecutionId, IdError, RoleName, SessionId, WorkerId, WorkspaceId};
use crate::ledger::STATUS_WORKING;
use crate::roster::{effort_problem, Phase, Roster, Teammate};
use crate::routing::decision::{self, Decision, RoutingDecision, RoutingMode, RoutingProvenance};
use crate::routing::policy::BalanceMode;
use crate::routing::quota::QuotaView;
use crate::skills::{plan_activation, SkillCatalog};

/// The task a worker spawned without one records. The same text as the
/// ledger facade's placeholder.
pub const IDLE_TASK: &str = "(idle - awaiting assignment)";

/// The usage-limit gate's inputs: read by the shell, only for a gated spawn
/// ([`needs_gate`]).
#[derive(Debug, Clone, Copy)]
pub struct GateInputs<'a> {
    pub view: &'a QuotaView,
    pub balance: BalanceMode,
}

/// Ids the shell minted for this spawn. A resume uses neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedIds {
    pub execution: ExecutionId,
    /// Used only when the harness accepts a caller-minted session id.
    pub session: SessionId,
}

/// Everything [`plan_launch`] reads. Pure data.
#[derive(Debug, Clone, Copy)]
pub struct PlanInputs<'a> {
    pub roster: &'a Roster,
    /// The roster's catalog (`Roster::skill_catalog`): bundled skills plus
    /// the installed marketplace lock.
    pub catalog: &'a SkillCatalog,
    /// `Some` when [`needs_gate`] said so.
    pub gate: Option<GateInputs<'a>>,
    /// The newest record the resume key matches, if any.
    pub existing: Option<&'a LedgerRecordV1>,
    pub now: DateTime<Utc>,
    pub ids: &'a MintedIds,
    pub project: &'a Path,
}

/// Why a spawn cannot be planned. Each message is the one `horch spawn`
/// printed before A6.
#[derive(Debug)]
pub enum PlanError {
    NothingToSpawn,
    /// The gate refused. `line` is the REFUSED line for stdout.
    Refused {
        decision: RoutingDecision,
        line: String,
    },
    UnknownTeammate(String),
    /// A reserved model tier, or a headless-only teammate.
    Unspawnable(String),
    NotResumable {
        id: String,
        reason: String,
    },
    BadEffort(String),
    SkillUnsupported(String),
    /// A gated spawn was planned without [`GateInputs`]: a caller bug.
    MissingGate,
    BadId(IdError),
    /// The role is registered, briefed or planned in this workspace already.
    RoleTaken(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NothingToSpawn => {
                f.write_str("give a teammate (e.g. `horch spawn sonnet \"task\"`) or --resume <id>")
            }
            Self::Refused { .. } => f.write_str("spawn refused over usage limits"),
            Self::UnknownTeammate(m)
            | Self::Unspawnable(m)
            | Self::BadEffort(m)
            | Self::SkillUnsupported(m) => f.write_str(m),
            Self::NotResumable { reason, .. } => f.write_str(reason),
            Self::MissingGate => f.write_str("the spawn gate ran without a quota view"),
            Self::BadId(e) => write!(f, "{e}"),
            Self::RoleTaken(role) => write!(f, "role '{role}' already exists in this workspace"),
        }
    }
}

impl std::error::Error for PlanError {}

impl From<IdError> for PlanError {
    fn from(e: IdError) -> Self {
        Self::BadId(e)
    }
}

fn unspawnable(e: anyhow::Error) -> PlanError {
    PlanError::Unspawnable(format!("{e:#}"))
}

fn require<'r>(roster: &'r Roster, name: &str) -> Result<&'r Teammate, PlanError> {
    roster
        .require(name)
        .map_err(|e| PlanError::UnknownTeammate(format!("{e:#}")))
}

/// Whether the spawn passes the usage-limit gate, so the shell must read the
/// quota view first. Only a fresh, unpinned spawn of a spawnable teammate
/// that spends something is gated; a resume keeps the harness it ran on.
pub fn needs_gate(req: &SpawnRequest, roster: &Roster) -> bool {
    if req.pinned || req.resume.is_some() {
        return false;
    }
    let Some(t) = req.teammate.as_ref().and_then(|n| roster.get(n.as_str())) else {
        return false;
    };
    t.agent != HarnessKind::None && Roster::is_spawnable(t).is_ok()
}

/// Explicit task selection wins over the recorded phase and roster default.
pub fn resolve_phase(
    explicit: Option<Phase>,
    recorded: Option<Phase>,
    default: Option<Phase>,
) -> Option<Phase> {
    explicit.or(recorded).or(default)
}

/// A fresh session: the minted id when the harness accepts one, else `None`
/// (codex and OpenCode reveal theirs after launch; the worker discovers it).
fn fresh_session(agent: HarnessKind, minted: &SessionId) -> SessionMode {
    SessionMode::Fresh(
        agent
            .capabilities()
            .caller_minted_session
            .then(|| minted.clone()),
    )
}

/// The launch as decided so far, before the gate.
struct Draft {
    teammate: Teammate,
    model: String,
    session: SessionMode,
    via: Option<String>,
    substitution_reason: Option<String>,
    routing: RoutingProvenance,
    /// The record a resume reopens.
    resumed: Option<Execution>,
}

/// Resume: teammate, model and session all come from the record.
fn draft_resume(key: &str, req: &SpawnRequest, inputs: &PlanInputs) -> Result<Draft, PlanError> {
    let record = inputs.existing.ok_or_else(|| PlanError::NotResumable {
        id: key.to_string(),
        reason: format!("no record matches {key}"),
    })?;
    let refuse = |reason: String| PlanError::NotResumable {
        id: record.record_id.clone(),
        reason,
    };
    if record.is_orchestrator() {
        return Err(refuse(format!(
            "record {} is an orchestrator; restart it with horch fleet",
            record.record_id
        )));
    }
    let session_id = record.session_id.clone().unwrap_or_default();
    if session_id.is_empty() {
        return Err(refuse(format!(
            "record {} has no captured session id; spawn a fresh {} instead",
            record.record_id, record.tier
        )));
    }
    // The legacy status: a `Planned` record is not live, but its spawner
    // may still be on its way to a pane.
    if record.status == STATUS_WORKING {
        return Err(refuse(format!(
            "session {session_id} is still marked working (a live worker may own it); \
             refusing to resume"
        )));
    }
    let roster = inputs.roster;
    let mut teammate = require(roster, &record.tier)?.clone();
    // A substituted session resumes on the harness it ran on (BAL-05): the
    // same fallback's launch settings, never a new gate decision.
    if let Some(via) = &record.via {
        teammate = decision::merge(&teammate, require(roster, via)?);
    }
    teammate.phase = resolve_phase(req.phase, record.phase, teammate.phase);
    // A resume keeps the level it ran at, as it keeps its model.
    if record.effort.is_some() {
        teammate.effort = record.effort.clone();
    }
    Roster::is_spawnable(&teammate).map_err(unspawnable)?;
    // A record from before A5 has no routing; rebuild it from `via`.
    let routing = match &record.routing {
        Some(r) => r.clone(),
        None => RoutingProvenance::legacy(
            &record.tier,
            record.via.as_deref(),
            &record.agent,
            &record.model,
            record.substitution_reason.as_deref(),
            roster.get(&record.tier),
        )?,
    }
    .resumed();
    let resumed = to_execution(record).map_err(|e| refuse(format!("{e}")))?;
    Ok(Draft {
        model: record.model.clone(),
        session: SessionMode::Resume(SessionId::new(session_id)?),
        via: record.via.clone(),
        substitution_reason: record.substitution_reason.clone(),
        teammate,
        routing,
        resumed: Some(resumed),
    })
}

fn draft_fresh(name: &str, req: &SpawnRequest, inputs: &PlanInputs) -> Result<Draft, PlanError> {
    let mut teammate = require(inputs.roster, name)?.clone();
    teammate.phase = resolve_phase(req.phase, None, teammate.phase);
    Roster::is_spawnable(&teammate).map_err(unspawnable)?;
    let mut routing = RoutingProvenance::ungated(&teammate)?;
    if req.pinned {
        routing.mode = RoutingMode::Pinned;
    }
    Ok(Draft {
        model: teammate.model.clone().unwrap_or_default(),
        session: fresh_session(teammate.agent, &inputs.ids.session),
        teammate,
        via: None,
        substitution_reason: None,
        routing,
        resumed: None,
    })
}

/// The usage-limit gate (design 13.5): after the top-tier gate, before any
/// role, ledger or pane side effect. Returns the gate's NOTE or SUBSTITUTED
/// line.
fn gate(
    draft: &mut Draft,
    req: &SpawnRequest,
    inputs: &PlanInputs,
) -> Result<Option<String>, PlanError> {
    let g = inputs.gate.ok_or(PlanError::MissingGate)?;
    let roster = inputs.roster;
    let decision = decision::decide(&draft.teammate, roster, g.view, g.balance, req.flags);
    if let Some(routing) = RoutingProvenance::from_decision(
        &draft.teammate,
        roster,
        g.view,
        &decision,
        RoutingMode::for_gate(g.balance, req.flags),
    )? {
        draft.routing = routing;
    }
    let line = decision.line();
    match &decision {
        Decision::Refuse { .. } => {
            return Err(PlanError::Refused {
                decision: RoutingDecision::from(&decision),
                line: line.unwrap_or_default(),
            })
        }
        Decision::Substitute { via, reason, .. } => {
            let merged =
                decision::resolve(&draft.teammate, roster, &decision).ok_or_else(|| {
                    PlanError::UnknownTeammate(format!("fallback '{via}' vanished from the roster"))
                })?;
            draft.model = merged.model.clone().unwrap_or_default();
            draft.session = fresh_session(merged.agent, &inputs.ids.session);
            draft.teammate = merged;
            draft.via = Some(via.clone());
            draft.substitution_reason = Some(reason.clone());
            Roster::model_is_spawnable(&draft.model, &draft.teammate.name).map_err(unspawnable)?;
        }
        Decision::Spawn { .. } => {}
    }
    Ok(line)
}

/// Plan a spawn. Pure: the same request and inputs give an equal plan.
///
/// The order is the one `horch spawn` always had: the resume rules or the
/// roster lookup, the reserved-tier check on what will actually launch, the
/// gate (fresh spawns only), effort, then skill support. The plan's record is
/// `Planned`; [`finish_plan`] adds the role.
pub fn plan_launch(req: &SpawnRequest, inputs: &PlanInputs) -> Result<ExecutionPlan, PlanError> {
    let mut draft = match (&req.resume, &req.teammate) {
        (Some(key), _) => draft_resume(key, req, inputs)?,
        (None, Some(name)) => draft_fresh(name.as_str(), req, inputs)?,
        (None, None) => return Err(PlanError::NothingToSpawn),
    };

    // The last gate before launch, and the only one that sees what will
    // ACTUALLY start: a resume takes its model from the ledger record, so a
    // stale or hand-edited record could otherwise start a second top-tier
    // session behind an ordinary tier name.
    Roster::model_is_spawnable(&draft.model, &draft.teammate.name).map_err(unspawnable)?;

    let gate_line = if needs_gate(req, inputs.roster) && draft.teammate.agent != HarnessKind::None {
        gate(&mut draft, req, inputs)?
    } else {
        None
    };

    if let Some(effort) = &req.effort {
        draft.teammate.effort = Some(effort.clone());
    }
    // Checked against what will launch, so a `--effort` typo or a level this
    // agent and model cannot take fails here, not in an unwatched pane.
    if let Some(effort) = &draft.teammate.effort {
        if let Some(why) = effort_problem(draft.teammate.agent, Some(&draft.model), effort) {
            return Err(PlanError::BadEffort(format!(
                "{}: {why}",
                draft.teammate.name
            )));
        }
    }
    // Unusable catalogs fail before a role or a record exists.
    crate::skills::ensure_supported_in(&draft.teammate, inputs.catalog)
        .map_err(|e| PlanError::SkillUnsupported(format!("{e:#}")))?;
    let skills = plan_activation(&draft.teammate, draft.teammate.phase, inputs.catalog)
        .map_err(|e| PlanError::SkillUnsupported(format!("{e:#}")))?;

    let now = crate::clock::stamp(inputs.now);
    let execution = match draft.resumed.take() {
        Some(mut e) => {
            e.phase = draft.teammate.phase;
            if !req.task.is_empty() {
                e.task.text = req.task.clone();
                e.task.plan = crate::telemetry::plan_slug(&req.task);
            }
            e.effort = draft.teammate.effort.clone();
            e.routing = Some(draft.routing.clone());
            e.status = ExecutionStatus::Planned;
            e.typed_status = true;
            e.exit_code = None;
            e.pane = None;
            e.finished_at = None;
            e.updated_at = now.clone();
            e.skills = skills.activated.clone();
            e.history.push(HistoryEntry {
                at: now,
                event: "resumed".to_string(),
                text: req.task.clone(),
            });
            e
        }
        None => Execution {
            id: inputs.ids.execution.clone(),
            kind: req.kind.clone(),
            teammate: draft.teammate.name.parse()?,
            harness: draft.teammate.agent,
            model: draft.model.clone(),
            effort: draft.teammate.effort.clone(),
            phase: draft.teammate.phase,
            // A placeholder until `finish_plan`.
            role: draft.teammate.name.parse()?,
            task: Task {
                id: None,
                text: if req.task.is_empty() {
                    IDLE_TASK.to_string()
                } else {
                    req.task.clone()
                },
                plan: crate::telemetry::plan_slug(&req.task),
            },
            status: ExecutionStatus::Planned,
            typed_status: true,
            session: match draft.session.id() {
                Some(id) => SessionState::Known(id.clone()),
                None => SessionState::Pending,
            },
            exit_code: None,
            project: Some(inputs.project.to_path_buf()),
            workdir: req.workdir.clone(),
            workspace: None,
            pane: None,
            skills: skills.activated.clone(),
            routing: Some(draft.routing.clone()),
            via: draft.via.clone(),
            substitution_reason: draft.substitution_reason.clone(),
            history: vec![HistoryEntry {
                at: now.clone(),
                event: "spawned".to_string(),
                text: req.task.clone(),
            }],
            created_at: now.clone(),
            updated_at: now,
            finished_at: None,
        },
    };

    Ok(ExecutionPlan {
        resumed: req.resume.is_some(),
        execution,
        worker: None,
        launch: LaunchPlan {
            teammate: draft.teammate,
            model: draft.model,
            session: draft.session,
            task: req.task.clone(),
        },
        skills,
        workspace: WorkspacePlan {
            workspace: None,
            from_pane: req.from_pane.clone(),
            direction: req.direction,
            tiling: req.tiling,
        },
        gate_line,
        report_to: req.report_to,
    })
}

/// Complete a plan once its role is allocated. Pure.
///
/// A fresh record takes the workspace; a resumed one keeps the workspace it
/// was spawned in, as it always did. An empty history text becomes
/// `spawned idle as <role>` or `resumed as <role>`.
pub fn finish_plan(
    mut plan: ExecutionPlan,
    role: RoleName,
    workspace: &WorkspaceId,
) -> ExecutionPlan {
    if !plan.resumed {
        plan.execution.workspace = Some(workspace.clone());
    }
    if let Some(entry) = plan.execution.history.last_mut() {
        if entry.text.is_empty() {
            entry.text = match entry.event.as_str() {
                "resumed" => format!("resumed as {role}"),
                _ => format!("spawned idle as {role}"),
            };
        }
    }
    plan.worker = Some(WorkerId::new_for(workspace, &role));
    plan.workspace.workspace = Some(workspace.clone());
    plan.execution.role = role;
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_override_wins_and_resume_keeps_recorded_selection() {
        assert_eq!(
            resolve_phase(
                Some(Phase::Validation),
                Some(Phase::Research),
                Some(Phase::Plan)
            ),
            Some(Phase::Validation)
        );
        assert_eq!(
            resolve_phase(None, Some(Phase::Research), Some(Phase::Plan)),
            Some(Phase::Research)
        );
        assert_eq!(
            resolve_phase(None, None, Some(Phase::Plan)),
            Some(Phase::Plan)
        );
        assert_eq!(resolve_phase(None, None, None), None);
    }
}
