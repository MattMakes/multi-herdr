use std::path::Path;

use chrono::{DateTime, Utc};

use crate::execution::legacy::STATUS_WORKING;
use crate::execution::legacy::{HistoryEntry, LedgerRecordV1};
use crate::execution::model::{
    Execution, ExecutionPlan, ExecutionStatus, LaunchPlan, SessionMode, SessionState, SpawnRequest,
    Task, WorkspacePlan,
};
use crate::execution::store::{to_execution, PANE_CLOSED};
use crate::harness::HarnessKind;
use crate::ids::{ExecutionId, IdError, RoleName, SessionId, WorkerId, WorkspaceId};
use crate::roster::validation::model_agent_problem;
use crate::roster::{effort_problem, ModelSource, Phase, Roster, Teammate};
use crate::routing::decision::{self, Decision, RoutingDecision, RoutingMode, RoutingProvenance};
use crate::routing::policy::BalanceMode;
use crate::routing::quota::QuotaView;
use crate::skills::{SkillActivationPlan, SkillResolution};

pub const IDLE_TASK: &str = "(idle - awaiting assignment)";

#[derive(Debug, Clone, Copy)]
pub struct GateInputs<'a> {
    pub view: &'a QuotaView,
    pub balance: BalanceMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedIds {
    pub execution: ExecutionId,
    pub session: SessionId,
}

#[derive(Debug, Clone, Copy)]
pub struct PlanInputs<'a> {
    pub roster: &'a Roster,
    pub skills: &'a SkillResolution,
    pub gate: Option<GateInputs<'a>>,
    pub existing: Option<&'a LedgerRecordV1>,
    pub now: DateTime<Utc>,
    pub ids: &'a MintedIds,
    pub project: &'a Path,
}

#[derive(Debug)]
pub enum PlanError {
    NothingToSpawn,
    Refused {
        decision: RoutingDecision,
        line: String,
    },
    UnknownTeammate(String),
    Unspawnable(String),
    NotResumable {
        id: String,
        reason: String,
    },
    BadEffort(String),
    BadModel(String),
    SkillUnsupported(String),
    MissingGate,
    BadId(IdError),
    RoleTaken(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NothingToSpawn => {
                f.write_str("give a teammate (e.g. `horch spawn sonnet`) or --resume <id>")
            }
            Self::Refused { .. } => f.write_str("spawn refused over usage limits"),
            Self::UnknownTeammate(m)
            | Self::Unspawnable(m)
            | Self::BadEffort(m)
            | Self::BadModel(m)
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

pub fn needs_gate(req: &SpawnRequest, roster: &Roster) -> bool {
    let teammate = req.teammate.as_ref().and_then(|n| roster.get(n.as_str()));
    !req.pinned && req.resume.is_none() && teammate.is_some_and(spends)
}

fn spends(t: &Teammate) -> bool {
    t.agent != HarnessKind::None && Roster::is_spawnable(t).is_ok()
}

pub(crate) fn resolve_phase(
    explicit: Option<Phase>,
    recorded: Option<Phase>,
    default: Option<Phase>,
) -> Option<Phase> {
    explicit.or(recorded).or(default)
}

fn fresh_session(agent: HarnessKind, minted: &SessionId) -> SessionMode {
    SessionMode::Fresh(
        agent
            .capabilities()
            .caller_minted_session
            .then(|| minted.clone()),
    )
}

struct Draft {
    teammate: Teammate,
    model: String,
    model_source: ModelSource,
    session: SessionMode,
    via: Option<String>,
    substitution_reason: Option<String>,
    routing: RoutingProvenance,
    resumed: Option<Execution>,
}

fn refusal(record: &LedgerRecordV1, reason: String) -> PlanError {
    PlanError::NotResumable {
        id: record.record_id.clone(),
        reason,
    }
}

fn check_resumable(record: &LedgerRecordV1) -> Result<String, PlanError> {
    if record.is_orchestrator() {
        return Err(refusal(
            record,
            format!(
                "record {} is an orchestrator; restart it with horch fleet",
                record.record_id
            ),
        ));
    }
    let session_id = record.session_id.clone().unwrap_or_default();
    if session_id.is_empty() {
        return Err(refusal(
            record,
            format!(
                "record {} has no captured session id; spawn a fresh {} instead",
                record.record_id, record.tier
            ),
        ));
    }
    if record.status == STATUS_WORKING {
        let pane = record
            .pane_id
            .as_deref()
            .map(|p| format!(" in pane {p}"))
            .unwrap_or_default();
        return Err(refusal(
            record,
            format!(
                "session {session_id} is still marked working{pane} (a live worker may own it); \
                 refusing to resume. If that pane is open, tell its worker instead. \
                 If you know the pane is gone, run: horch ledger legacy done {} \"{PANE_CLOSED}\"",
                record.record_id
            ),
        ));
    }
    Ok(session_id)
}

fn resume_routing(
    record: &LedgerRecordV1,
    roster: &Roster,
) -> Result<RoutingProvenance, PlanError> {
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
    };
    Ok(routing.resumed())
}

fn resume_teammate(
    record: &LedgerRecordV1,
    req: &SpawnRequest,
    roster: &Roster,
) -> Result<Teammate, PlanError> {
    let mut teammate = require(roster, &record.tier)?.clone();
    if let Some(via) = &record.via {
        teammate = decision::merge(&teammate, require(roster, via)?);
    }
    teammate.phase = resolve_phase(req.phase, record.phase, teammate.phase);
    if record.effort.is_some() {
        teammate.effort = record.effort.clone();
    }
    if record.model_override {
        teammate.model = Some(record.model.clone());
    } else {
        Roster::is_spawnable(&teammate).map_err(unspawnable)?;
    }
    Ok(teammate)
}

fn draft_resume(key: &str, req: &SpawnRequest, inputs: &PlanInputs) -> Result<Draft, PlanError> {
    let record = inputs.existing.ok_or_else(|| PlanError::NotResumable {
        id: key.to_string(),
        reason: format!("no record matches {key}"),
    })?;
    let session_id = check_resumable(record)?;
    let teammate = resume_teammate(record, req, inputs.roster)?;
    let routing = resume_routing(record, inputs.roster)?;
    let resumed = to_execution(record).map_err(|e| refusal(record, e.to_string()))?;
    Ok(Draft {
        model: record.model.clone(),
        model_source: if record.model_override {
            ModelSource::Operator
        } else {
            ModelSource::Teammate
        },
        session: SessionMode::Resume(SessionId::new(session_id)?),
        via: record.via.clone(),
        substitution_reason: record.substitution_reason.clone(),
        teammate,
        routing,
        resumed: Some(resumed),
    })
}

fn apply_model(teammate: &mut Teammate, req: &SpawnRequest) -> Result<ModelSource, PlanError> {
    let Some(model) = &req.model else {
        return Ok(ModelSource::Teammate);
    };
    if let Some(why) = model_agent_problem(teammate.agent, model) {
        return Err(PlanError::BadModel(format!("{}: {why}", teammate.name)));
    }
    teammate.model = Some(model.clone());
    Ok(ModelSource::Operator)
}

fn draft_fresh(name: &str, req: &SpawnRequest, inputs: &PlanInputs) -> Result<Draft, PlanError> {
    let mut teammate = require(inputs.roster, name)?.clone();
    teammate.phase = resolve_phase(req.phase, None, teammate.phase);
    let model_source = apply_model(&mut teammate, req)?;
    if model_source == ModelSource::Teammate {
        Roster::is_spawnable(&teammate).map_err(unspawnable)?;
    }
    let mut routing = RoutingProvenance::ungated(&teammate)?;
    if req.pinned {
        routing.mode = RoutingMode::Pinned;
    }
    Ok(Draft {
        model: teammate.model.clone().unwrap_or_default(),
        model_source,
        session: fresh_session(teammate.agent, &inputs.ids.session),
        teammate,
        via: None,
        substitution_reason: None,
        routing,
        resumed: None,
    })
}

fn gate(
    draft: &mut Draft,
    req: &SpawnRequest,
    inputs: &PlanInputs,
) -> Result<Option<String>, PlanError> {
    let g = inputs.gate.ok_or(PlanError::MissingGate)?;
    let roster = inputs.roster;
    let mut flags = req.flags;
    flags.exact |= draft.model_source == ModelSource::Operator;
    let decision = decision::decide(&draft.teammate, roster, g.view, g.balance, flags);
    if let Some(routing) = RoutingProvenance::from_decision(
        &draft.teammate,
        roster,
        g.view,
        &decision,
        RoutingMode::for_gate(g.balance, flags),
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
            Roster::model_is_spawnable(&draft.model, &draft.teammate.name, draft.model_source)
                .map_err(unspawnable)?;
        }
        Decision::Spawn { .. } => {}
    }
    Ok(line)
}

fn draft_for(req: &SpawnRequest, inputs: &PlanInputs) -> Result<Draft, PlanError> {
    match (&req.resume, &req.teammate) {
        (Some(key), _) => draft_resume(key, req, inputs),
        (None, Some(name)) => draft_fresh(name.as_str(), req, inputs),
        (None, None) => Err(PlanError::NothingToSpawn),
    }
}

fn gate_if_needed(
    draft: &mut Draft,
    req: &SpawnRequest,
    inputs: &PlanInputs,
) -> Result<Option<String>, PlanError> {
    if needs_gate(req, inputs.roster) {
        gate(draft, req, inputs)
    } else {
        Ok(None)
    }
}

fn launch_skills(draft: &mut Draft, inputs: &PlanInputs) -> Result<SkillActivationPlan, PlanError> {
    draft.teammate = inputs.roster.for_launch(draft.teammate.clone());
    inputs
        .skills
        .plan(&draft.teammate)
        .map_err(|e| PlanError::SkillUnsupported(format!("{e:#}")))
}

fn apply_effort(draft: &mut Draft, req: &SpawnRequest) -> Result<(), PlanError> {
    draft.teammate.effort = req.effort.clone().or(draft.teammate.effort.take());
    let Some(effort) = &draft.teammate.effort else {
        return Ok(());
    };
    match effort_problem(draft.teammate.agent, Some(&draft.model), effort) {
        Some(why) => Err(PlanError::BadEffort(format!(
            "{}: {why}",
            draft.teammate.name
        ))),
        None => Ok(()),
    }
}

fn resumed_execution(
    mut e: Execution,
    draft: &Draft,
    req: &SpawnRequest,
    skills: &SkillActivationPlan,
    now: String,
) -> Execution {
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

fn fresh_execution(
    draft: &Draft,
    req: &SpawnRequest,
    inputs: &PlanInputs,
    skills: &SkillActivationPlan,
    now: String,
) -> Result<Execution, PlanError> {
    Ok(Execution {
        id: inputs.ids.execution.clone(),
        kind: req.kind.clone(),
        teammate: draft.teammate.name.parse()?,
        harness: draft.teammate.agent,
        model: draft.model.clone(),
        model_override: draft.model_source == ModelSource::Operator,
        effort: draft.teammate.effort.clone(),
        phase: draft.teammate.phase,
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
        session: draft
            .session
            .id()
            .map_or(SessionState::Pending, |id| SessionState::Known(id.clone())),
        exit_code: None,
        project: Some(inputs.project.to_path_buf()),
        workdir: req.workdir.clone(),
        workspace: None,
        pane: None,
        backend: crate::runtime::Backend::Herdr,
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
    })
}

pub fn plan_launch(req: &SpawnRequest, inputs: &PlanInputs) -> Result<ExecutionPlan, PlanError> {
    let mut draft = draft_for(req, inputs)?;

    Roster::model_is_spawnable(&draft.model, &draft.teammate.name, draft.model_source)
        .map_err(unspawnable)?;

    let gate_line = gate_if_needed(&mut draft, req, inputs)?;
    apply_effort(&mut draft, req)?;
    let skills = launch_skills(&mut draft, inputs)?;

    let now = crate::clock::stamp(inputs.now);
    let execution = match draft.resumed.take() {
        Some(e) => resumed_execution(e, &draft, req, &skills, now),
        None => fresh_execution(&draft, req, inputs, &skills, now)?,
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
        controller: None,
    })
}

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
    use crate::skills::{harness_support, SkillCatalog};

    fn bundled_skills() -> SkillResolution {
        SkillResolution::read(
            SkillCatalog::bundled().unwrap(),
            None,
            None,
            harness_support,
        )
        .unwrap()
    }

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

    #[test]
    fn working_refusal_names_the_fix() {
        let roster = Roster::builtin().unwrap();
        let skills = bundled_skills();
        let ids = MintedIds {
            execution: ExecutionId::new("e1").unwrap(),
            session: SessionId::new("s9").unwrap(),
        };
        let record = LedgerRecordV1 {
            record_id: "r1".into(),
            session_id: Some("s1".into()),
            tier: "sonnet".into(),
            agent: "claude".into(),
            model: "sonnet".into(),
            status: STATUS_WORKING.into(),
            pane_id: Some("w1:p5".into()),
            ..LedgerRecordV1::default()
        };
        let inputs = PlanInputs {
            roster: &roster,
            skills: &skills,
            gate: None,
            existing: Some(&record),
            now: crate::clock::parse("2026-10-04T00:00:00Z").unwrap(),
            ids: &ids,
            project: Path::new("/work/alpha"),
        };
        let mut req = SpawnRequest::worker(None, "");
        req.resume = Some("r1".into());
        let err = draft_resume("r1", &req, &inputs).err().unwrap().to_string();
        assert!(err.contains("in pane w1:p5"), "{err}");
        assert!(
            err.contains("horch ledger legacy done r1 \"pane closed without horch done\""),
            "{err}"
        );
    }

    #[test]
    fn skills_when_reaches_the_record_and_the_launch_teammate() {
        use crate::roster::ProjectFacts;
        use crate::skills::InvocationPolicy;

        let mut roster = Roster::builtin().unwrap();
        let mut sonnet = roster.require("sonnet").unwrap().clone();
        sonnet.skills_when = [(
            "*.csproj".to_string(),
            vec!["code-review".into(), "document".into()],
        )]
        .into();
        roster.insert_for_test(sonnet);
        let skills = bundled_skills();
        let ids = MintedIds {
            execution: ExecutionId::new("e1").unwrap(),
            session: SessionId::new("s1").unwrap(),
        };
        let mut req = SpawnRequest::worker(Some("sonnet".parse().unwrap()), "build");
        req.pinned = true;
        let plan_in = |roster: &Roster| {
            plan_launch(
                &req,
                &PlanInputs {
                    roster,
                    skills: &skills,
                    gate: None,
                    existing: None,
                    now: crate::clock::parse("2026-10-04T00:00:00Z").unwrap(),
                    ids: &ids,
                    project: Path::new("/work/alpha"),
                },
            )
            .unwrap()
        };
        let added = ["code-review", "document"];

        let csharp = roster
            .clone()
            .with_project_facts(ProjectFacts::from_names(["Game.csproj"]));
        let plan = plan_in(&csharp);
        for name in added {
            let r = plan
                .execution
                .skills
                .iter()
                .find(|r| r.id.as_str() == name)
                .unwrap_or_else(|| panic!("{name} not recorded"));
            assert_eq!(r.policy, InvocationPolicy::Explicit, "{name}");
            assert!(plan.launch.teammate.skills.iter().any(|s| s == name));
        }
        assert_eq!(plan.execution.skills, plan.skills.activated);

        let gdscript = roster
            .clone()
            .with_project_facts(ProjectFacts::from_names(["player.gd"]));
        for plan in [plan_in(&gdscript), plan_in(&roster)] {
            let ids = plan.skills.activated_ids();
            assert!(added.iter().all(|n| !ids.contains(n)), "{ids:?}");
        }
    }

    #[test]
    fn arc_15_plan_is_pure() {
        let source = include_str!("plan.rs");
        let code = &source[..source.find("#[cfg(test)]").unwrap()];
        let forbidden = [
            "std::fs",
            "std::process",
            "Command::new",
            "std::env",
            "workspace::",
            "herdr",
            "Utc::now",
            "clock::now",
        ];
        let found: Vec<&str> = code
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .filter(|line| forbidden.iter().any(|f| line.contains(f)))
            .collect();
        assert!(found.is_empty(), "plan.rs is not pure: {found:#?}");
    }
}
