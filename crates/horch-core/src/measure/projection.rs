//! Rebuildable views of the event log (dataset design §4.2, MEA-05).
//!
//! [`fold`] replays events into a [`Projection`]. It is deterministic: the
//! same events in the same order give the same projection, and folding a
//! prefix then applying the rest equals folding everything. An event that
//! does not fit the state it arrives in (design §5) is recorded as an
//! [`Anomaly`] and never applied.
//!
//! Every state change goes through `competition::state::transition`
//! (CMP-03). The fold maps each event to a `RoundEvent`, asks the table for
//! the target state, then checks what the table cannot see (such as a judge
//! attempt number). `round.needs_intervention`, `round.cleanup_started` and
//! `round.completed` reach NEEDS_INTERVENTION, CLEANUP and COMPLETE
//! (SPEC-TODO(Spec B event list): the orchestrator added these 3 kinds).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::competition::model::RoundState;
use crate::competition::preflight::PreflightReport;
use crate::competition::state::{transition, RoundEvent};
use crate::evaluation::validator::ValidationReport;
use crate::evaluation::winner::RejectReason;
use crate::ids::{EventId, ExecutionId, ExperimentId, JudgmentId, RoundId};
use crate::measure::event::{
    CandidateCompleted, CandidateFailed, CandidateFrozen, CandidatePlanned, CandidateSpawned,
    EventEnvelope, EventKind, ExperimentAborted, ExperimentCreated, FinalOutcome,
    InterventionSource, OutcomeRecorded, PromotionCompleted, PromotionConflicted, PromotionIntent,
    PromotionRolledBack, PromotionStarted, RoundCreated, RoundNeedsIntervention, WinnerSelected,
    WorktreeCleanupFailed, WorktreeCreated,
};

/// A failed judge attempt with this number ends the judging (design §5).
pub const MAX_JUDGE_ATTEMPTS: u32 = 2;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    pub experiments: BTreeMap<ExperimentId, ExperimentView>,
    pub rounds: BTreeMap<RoundId, RoundView>,
    pub anomalies: Vec<Anomaly>,
}

/// An event that was not applied, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anomaly {
    pub event_id: EventId,
    pub reason: String,
}

/// An experiment before and around its rounds. `state` is one of CREATED,
/// PREFLIGHT, PLANNED or ABORTED.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExperimentView {
    pub state: RoundState,
    pub created: ExperimentCreated,
    pub preflight: Option<PreflightReport>,
    pub aborted: Option<ExperimentAborted>,
    pub rounds: Vec<RoundId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoundView {
    pub experiment_id: ExperimentId,
    pub state: RoundState,
    pub created: RoundCreated,
    /// One entry per label of `created.labels`.
    pub candidates: BTreeMap<String, CandidateView>,
    pub judge: JudgeView,
    pub winner: Option<WinnerSelected>,
    pub rejected: Option<RejectReason>,
    pub promotion: PromotionView,
    pub needs_intervention: Option<RoundNeedsIntervention>,
    /// The state the round held when cleanup started.
    pub cleaned_from: Option<RoundState>,
    pub final_outcome: Option<FinalOutcome>,
    pub cleanup_failures: Vec<WorktreeCleanupFailed>,
    pub outcomes: Vec<OutcomeRecorded>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CandidateView {
    pub execution_id: Option<ExecutionId>,
    pub planned: Option<CandidatePlanned>,
    pub worktree: Option<WorktreeCreated>,
    pub spawned: Option<CandidateSpawned>,
    pub completed: Option<CandidateCompleted>,
    pub failed: Option<CandidateFailed>,
    pub frozen: Option<CandidateFrozen>,
    pub validation: Option<ValidationReport>,
}

impl CandidateView {
    pub fn is_terminal(&self) -> bool {
        self.completed.is_some() || self.failed.is_some()
    }

    /// Validated, and every gate passed.
    pub fn is_eligible(&self) -> bool {
        self.validation.as_ref().is_some_and(|v| v.eligible)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JudgeView {
    /// The latest scheduled attempt; 0 before the first.
    pub attempt: u32,
    pub started: bool,
    pub failures: u32,
    pub judgment_id: Option<JudgmentId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PromotionView {
    pub started: Option<PromotionStarted>,
    pub completed: Option<PromotionCompleted>,
    pub conflicted: Option<PromotionConflicted>,
    pub rolled_back: Vec<PromotionRolledBack>,
}

/// Fold every event, in order, into a fresh projection.
pub fn fold(events: &[EventEnvelope]) -> Projection {
    let mut p = Projection::default();
    for e in events {
        p.apply(e);
    }
    p
}

impl Projection {
    /// Apply one event, or record why it does not apply.
    pub fn apply(&mut self, env: &EventEnvelope) {
        if let Err(reason) = self.try_apply(env) {
            self.anomalies.push(Anomaly {
                event_id: env.event_id.clone(),
                reason,
            });
        }
    }

    fn try_apply(&mut self, env: &EventEnvelope) -> Result<(), String> {
        let kind = env.event().map_err(|e| e.to_string())?;
        let exp = &env.experiment_id;
        match kind {
            // A newer writer's kind: kept in the log, nothing to project.
            EventKind::Unknown { .. } => Ok(()),
            EventKind::ExperimentCreated(created) => {
                if self.experiments.contains_key(exp) {
                    return Err(format!("experiment {exp} already created"));
                }
                // CREATED → PREFLIGHT on the event itself.
                let state = step(RoundState::Created, RoundEvent::ExperimentCreated)?;
                self.experiments.insert(
                    exp.clone(),
                    ExperimentView {
                        state,
                        created,
                        preflight: None,
                        aborted: None,
                        rounds: Vec::new(),
                    },
                );
                Ok(())
            }
            EventKind::PreflightCompleted(p) => {
                let x = self.experiment(exp)?;
                // A failed preflight waits for experiment.aborted.
                let ev = if p.report.passed {
                    RoundEvent::PreflightPassed
                } else {
                    RoundEvent::PreflightFailed
                };
                x.state = step(x.state, ev)?;
                x.preflight = Some(p.report);
                Ok(())
            }
            EventKind::ExperimentAborted(a) => {
                let x = self.experiment(exp)?;
                x.state = step(x.state, RoundEvent::ExperimentAborted)?;
                x.aborted = Some(a);
                Ok(())
            }
            EventKind::RoundCreated(created) => {
                let round = round_id(env)?;
                let state = step(self.experiment(exp)?.state, RoundEvent::RoundCreated)?;
                if self.rounds.contains_key(&round) {
                    return Err(format!("round {round} already created"));
                }
                let mut candidates = BTreeMap::new();
                for label in &created.labels {
                    if candidates
                        .insert(label.clone(), CandidateView::default())
                        .is_some()
                    {
                        return Err(format!("label {label} appears twice"));
                    }
                }
                self.experiment(exp)?.rounds.push(round.clone());
                self.rounds.insert(
                    round,
                    RoundView {
                        experiment_id: exp.clone(),
                        state,
                        created,
                        candidates,
                        judge: JudgeView::default(),
                        winner: None,
                        rejected: None,
                        promotion: PromotionView::default(),
                        needs_intervention: None,
                        cleaned_from: None,
                        final_outcome: None,
                        cleanup_failures: Vec::new(),
                        outcomes: Vec::new(),
                    },
                );
                Ok(())
            }
            other => {
                let r = self.round(env)?;
                apply_round(r, env, other)
            }
        }
    }

    fn experiment(&mut self, exp: &ExperimentId) -> Result<&mut ExperimentView, String> {
        self.experiments
            .get_mut(exp)
            .ok_or_else(|| format!("unknown experiment {exp}"))
    }

    fn round(&mut self, env: &EventEnvelope) -> Result<&mut RoundView, String> {
        let round = round_id(env)?;
        let r = self
            .rounds
            .get_mut(&round)
            .ok_or_else(|| format!("unknown round {round}"))?;
        if r.experiment_id != env.experiment_id {
            return Err(format!(
                "round {round} belongs to experiment {}",
                r.experiment_id
            ));
        }
        Ok(r)
    }
}

fn round_id(env: &EventEnvelope) -> Result<RoundId, String> {
    env.round_id
        .clone()
        .ok_or_else(|| format!("{} without a round_id", env.kind))
}

/// The state `event` moves `from` to, or why the table has no such row.
fn step(from: RoundState, event: RoundEvent) -> Result<RoundState, String> {
    transition(from, &event).map_err(|e| e.to_string())
}

/// Whether every candidate other than `label` satisfies `pred`.
fn others_all(r: &RoundView, label: &str, pred: impl Fn(&CandidateView) -> bool) -> bool {
    r.candidates
        .iter()
        .filter(|(l, _)| l.as_str() != label)
        .all(|(_, c)| pred(c))
}

fn candidate<'a>(r: &'a mut RoundView, label: &str) -> Result<&'a mut CandidateView, String> {
    r.candidates
        .get_mut(label)
        .ok_or_else(|| format!("unknown label {label}"))
}

fn once<T>(slot: &Option<T>, what: &str, label: &str) -> Result<(), String> {
    match slot {
        Some(_) => Err(format!("{what} repeated for {label}")),
        None => Ok(()),
    }
}

/// Apply a round-level event to `r`. The target state comes from the table
/// first, so nothing changes when the event is invalid.
fn apply_round(r: &mut RoundView, env: &EventEnvelope, kind: EventKind) -> Result<(), String> {
    use RoundState as S;
    let name = kind.name().to_string();
    match kind {
        EventKind::CandidatePlanned(p) => {
            let last = others_all(r, &p.label, |c| c.planned.is_some());
            let to = step(r.state, RoundEvent::CandidatePlanned { last })?;
            let c = candidate(r, &p.label)?;
            once(&c.planned, &name, &p.label)?;
            if env.execution_id.is_some() {
                c.execution_id = env.execution_id.clone();
            }
            c.planned = Some(p);
            r.state = to;
        }
        EventKind::WorktreeCreated(w) => {
            let last = others_all(r, &w.label, |c| c.worktree.is_some());
            let to = step(r.state, RoundEvent::WorktreeCreated { last })?;
            let c = candidate(r, &w.label)?;
            once(&c.worktree, &name, &w.label)?;
            c.worktree = Some(w);
            r.state = to;
        }
        EventKind::CandidateSpawned(s) => {
            let to = step(r.state, RoundEvent::CandidateSpawned)?;
            let c = candidate(r, &s.label)?;
            once(&c.spawned, &name, &s.label)?;
            if c.is_terminal() {
                return Err(format!("{name} after {} ended", s.label));
            }
            if env.execution_id.is_some() {
                c.execution_id = env.execution_id.clone();
            }
            c.spawned = Some(s);
            r.state = to;
        }
        EventKind::CandidateCompleted(done) => {
            let last = others_all(r, &done.label, CandidateView::is_terminal);
            let to = step(r.state, RoundEvent::CandidateEnded { last })?;
            let c = candidate(r, &done.label)?;
            if c.spawned.is_none() {
                return Err(format!("{name} before {} was spawned", done.label));
            }
            if c.is_terminal() {
                return Err(format!("{name} after {} ended", done.label));
            }
            c.completed = Some(done);
            r.state = to;
        }
        EventKind::CandidateFailed(failed) => {
            // A launch can fail before the spawn event.
            let last = others_all(r, &failed.label, CandidateView::is_terminal);
            let to = step(r.state, RoundEvent::CandidateEnded { last })?;
            let c = candidate(r, &failed.label)?;
            if c.is_terminal() {
                return Err(format!("{name} after {} ended", failed.label));
            }
            c.failed = Some(failed);
            r.state = to;
        }
        EventKind::CandidateFrozen(f) => {
            let to = step(r.state, RoundEvent::CandidateFrozen)?;
            let c = candidate(r, &f.label)?;
            once(&c.frozen, &name, &f.label)?;
            c.frozen = Some(f);
            r.state = to;
        }
        EventKind::ValidationCompleted(v) => {
            let last = others_all(r, &v.label, |c| c.validation.is_some());
            let any_eligible = v.report.eligible
                || r.candidates
                    .iter()
                    .any(|(l, c)| l != &v.label && c.is_eligible());
            let to = step(
                r.state,
                RoundEvent::ValidationCompleted { last, any_eligible },
            )?;
            let c = candidate(r, &v.label)?;
            if c.frozen.is_none() {
                return Err(format!("{name} before {} was frozen", v.label));
            }
            once(&c.validation, &name, &v.label)?;
            c.validation = Some(v.report);
            r.state = to;
        }
        EventKind::JudgeScheduled(j) => {
            let to = step(r.state, RoundEvent::JudgeScheduled)?;
            if r.judge.judgment_id.is_some() {
                return Err(format!("{name} after a judgment"));
            }
            if j.attempt != r.judge.attempt + 1 {
                return Err(format!(
                    "{name} attempt {} after attempt {}",
                    j.attempt, r.judge.attempt
                ));
            }
            r.judge.attempt = j.attempt;
            r.judge.started = false;
            r.state = to;
        }
        EventKind::JudgeStarted(j) => {
            let to = step(r.state, RoundEvent::JudgeStarted)?;
            expect_judge_attempt(r, j.attempt, &name)?;
            if r.judge.started {
                return Err(format!("{name} repeated for attempt {}", j.attempt));
            }
            r.judge.started = true;
            r.state = to;
        }
        EventKind::JudgeCompleted(j) => {
            let to = step(r.state, RoundEvent::JudgeCompleted)?;
            expect_judge_attempt(r, j.attempt, &name)?;
            r.judge.judgment_id = Some(j.judgment_id);
            r.state = to;
        }
        EventKind::JudgeFailed(j) => {
            let last_attempt = j.attempt >= MAX_JUDGE_ATTEMPTS;
            let to = step(r.state, RoundEvent::JudgeFailed { last_attempt })?;
            expect_judge_attempt(r, j.attempt, &name)?;
            r.judge.failures += 1;
            r.state = to;
        }
        EventKind::WinnerSelected(w) => {
            // DECIDED; a requested promotion moves on to revalidate at once.
            let mut to = step(r.state, RoundEvent::WinnerSelected)?;
            if let PromotionIntent::Requested { .. } = w.promotion {
                to = step(to, RoundEvent::PromotionRequested)?;
            }
            let Some(judgment) = &r.judge.judgment_id else {
                return Err(format!("{name} before a judgment"));
            };
            if &w.judgment_id != judgment {
                return Err(format!("{name} cites judgment {}", w.judgment_id));
            }
            if !candidate(r, &w.label)?.is_eligible() {
                return Err(format!("{name}: {} is not eligible", w.label));
            }
            r.winner = Some(w);
            r.state = to;
        }
        EventKind::WinnerRejected(rej) => {
            let to = step(r.state, RoundEvent::WinnerRejected)?;
            let allowed = match r.state {
                S::Validating => {
                    all_validated(r) && !r.candidates.values().any(CandidateView::is_eligible)
                }
                S::JudgingBackground => r.judge.judgment_id.is_some(),
                _ => true,
            };
            if !allowed {
                return Err(format!("{name} is invalid in state {}", r.state));
            }
            r.rejected = Some(rej.reason);
            r.state = to;
        }
        EventKind::PromotionStarted(p) => {
            let to = step(r.state, RoundEvent::PromotionStarted)?;
            r.promotion.started = Some(p);
            r.state = to;
        }
        EventKind::PromotionConflicted(c) => {
            let to = step(r.state, RoundEvent::PromotionConflicted)?;
            r.promotion.conflicted = Some(c);
            r.state = to;
        }
        EventKind::PromotionCompleted(c) => {
            let to = step(r.state, RoundEvent::PromotionCompleted)?;
            r.promotion.completed = Some(c);
            r.state = to;
        }
        EventKind::OperatorPromote(_) => {
            // Back to DECIDED, and on to revalidate at once, as a
            // `winner.selected` with a requested promotion does.
            let to = step(r.state, RoundEvent::OperatorPromote)?;
            let to = step(to, RoundEvent::PromotionRequested)?;
            if r.winner.is_none() {
                return Err(format!("{name} for a round without a winner"));
            }
            r.state = to;
        }
        EventKind::PromotionRolledBack(rb) => {
            let to = step(r.state, RoundEvent::PromotionRolledBack)?;
            r.promotion.rolled_back.push(rb);
            r.state = to;
        }
        // Neither moves the round.
        EventKind::WorktreeCleanupFailed(f) => {
            candidate(r, &f.label)?;
            r.cleanup_failures.push(f);
        }
        EventKind::OutcomeRecorded(o) => {
            if !matches!(r.state, S::Decided | S::Promoted | S::Cleanup | S::Complete) {
                return Err(format!("{name} is invalid in state {}", r.state));
            }
            r.outcomes.push(o);
        }
        EventKind::RoundNeedsIntervention(n) => {
            let to = step(r.state, RoundEvent::NeedsIntervention(n.source))?;
            if n.source == InterventionSource::Judge && r.judge.judgment_id.is_none() {
                return Err(format!("{name} from the judge before a judgment"));
            }
            r.needs_intervention = Some(n);
            r.state = to;
        }
        EventKind::RoundCleanupStarted(_) => {
            let to = step(r.state, RoundEvent::CleanupStarted)?;
            r.cleaned_from = Some(r.state);
            r.state = to;
        }
        EventKind::RoundCompleted(c) => {
            let to = step(r.state, RoundEvent::RoundCompleted)?;
            let want = match r.cleaned_from {
                Some(S::Decided) => FinalOutcome::Winner,
                Some(S::Rejected) => FinalOutcome::Rejected,
                Some(S::Promoted) => FinalOutcome::Promoted,
                _ => FinalOutcome::NeedsIntervention,
            };
            if c.final_outcome != want {
                return Err(format!(
                    "{name} says {:?}, cleanup started from {:?}",
                    c.final_outcome, r.cleaned_from
                ));
            }
            r.final_outcome = Some(c.final_outcome);
            r.state = to;
        }
        EventKind::ExperimentCreated(_)
        | EventKind::PreflightCompleted(_)
        | EventKind::ExperimentAborted(_)
        | EventKind::RoundCreated(_)
        | EventKind::Unknown { .. } => unreachable!("handled by Projection::try_apply"),
    }
    Ok(())
}

fn all_validated(r: &RoundView) -> bool {
    r.candidates.values().all(|c| c.validation.is_some())
}

fn expect_judge_attempt(r: &RoundView, attempt: u32, what: &str) -> Result<(), String> {
    if r.judge.judgment_id.is_some() {
        return Err(format!("{what} after a judgment"));
    }
    if attempt == 0 || attempt != r.judge.attempt {
        return Err(format!(
            "{what} for attempt {attempt}, scheduled {}",
            r.judge.attempt
        ));
    }
    if r.judge.failures >= attempt {
        return Err(format!("{what} after attempt {attempt} failed"));
    }
    Ok(())
}
