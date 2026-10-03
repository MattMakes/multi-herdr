//! Rebuildable views of the event log (dataset design §4.2, MEA-05).
//!
//! [`fold`] replays events into a [`Projection`]. It is deterministic: the
//! same events in the same order give the same projection, and folding a
//! prefix then applying the rest equals folding everything. An event that
//! does not fit the state it arrives in (design §5) is recorded as an
//! [`Anomaly`] and never applied.
//!
//! The full transition table is CMP-03 (B3). This fold applies only what the
//! events themselves say. `round.needs_intervention`, `round.cleanup_started`
//! and `round.completed` reach NEEDS_INTERVENTION, CLEANUP and COMPLETE
//! (SPEC-TODO(Spec B event list): the orchestrator added these 3 kinds).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::competition::model::RoundState;
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
    // B2: typed once PreflightReport lands.
    pub preflight: Option<Value>,
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
    // B3: typed once ValidationReport lands.
    pub validation: Option<Value>,
}

impl CandidateView {
    pub fn is_terminal(&self) -> bool {
        self.completed.is_some() || self.failed.is_some()
    }

    /// The validation report says `eligible: true`.
    pub fn is_eligible(&self) -> bool {
        self.validation
            .as_ref()
            .and_then(|v| v.get("eligible"))
            .and_then(Value::as_bool)
            == Some(true)
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
                self.experiments.insert(
                    exp.clone(),
                    ExperimentView {
                        state: RoundState::Preflight,
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
                expect_state(x.state, &[RoundState::Preflight], "preflight.completed")?;
                // A failed preflight waits for experiment.aborted.
                if p.report.get("passed").and_then(Value::as_bool) != Some(false) {
                    x.state = RoundState::Planned;
                }
                x.preflight = Some(p.report);
                Ok(())
            }
            EventKind::ExperimentAborted(a) => {
                let x = self.experiment(exp)?;
                expect_state(
                    x.state,
                    &[RoundState::Created, RoundState::Preflight],
                    "experiment.aborted",
                )?;
                x.state = RoundState::Aborted;
                x.aborted = Some(a);
                Ok(())
            }
            EventKind::RoundCreated(created) => {
                let round = round_id(env)?;
                let state = self.experiment(exp)?.state;
                expect_state(state, &[RoundState::Planned], "round.created")?;
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
                        state: RoundState::Planned,
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

fn expect_state(state: RoundState, allowed: &[RoundState], what: &str) -> Result<(), String> {
    if allowed.contains(&state) {
        Ok(())
    } else {
        Err(format!("{what} is invalid in state {state}"))
    }
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

/// Apply a round-level event to `r`.
fn apply_round(r: &mut RoundView, env: &EventEnvelope, kind: EventKind) -> Result<(), String> {
    use RoundState as S;
    let name = kind.name().to_string();
    match kind {
        EventKind::CandidatePlanned(p) => {
            expect_state(r.state, &[S::Planned], &name)?;
            let c = candidate(r, &p.label)?;
            once(&c.planned, &name, &p.label)?;
            if env.execution_id.is_some() {
                c.execution_id = env.execution_id.clone();
            }
            c.planned = Some(p);
            if r.candidates.values().all(|c| c.planned.is_some()) {
                r.state = S::Provisioning;
            }
        }
        EventKind::WorktreeCreated(w) => {
            expect_state(r.state, &[S::Provisioning], &name)?;
            let c = candidate(r, &w.label)?;
            once(&c.worktree, &name, &w.label)?;
            c.worktree = Some(w);
            if r.candidates.values().all(|c| c.worktree.is_some()) {
                r.state = S::Running;
            }
        }
        EventKind::CandidateSpawned(s) => {
            expect_state(r.state, &[S::Running], &name)?;
            let c = candidate(r, &s.label)?;
            once(&c.spawned, &name, &s.label)?;
            if c.is_terminal() {
                return Err(format!("{name} after {} ended", s.label));
            }
            if env.execution_id.is_some() {
                c.execution_id = env.execution_id.clone();
            }
            c.spawned = Some(s);
        }
        EventKind::CandidateCompleted(done) => {
            expect_state(r.state, &[S::Running], &name)?;
            let c = candidate(r, &done.label)?;
            if c.spawned.is_none() {
                return Err(format!("{name} before {} was spawned", done.label));
            }
            if c.is_terminal() {
                return Err(format!("{name} after {} ended", done.label));
            }
            c.completed = Some(done);
            finish_running(r);
        }
        EventKind::CandidateFailed(failed) => {
            // A launch can fail before the spawn event.
            expect_state(r.state, &[S::Running], &name)?;
            let c = candidate(r, &failed.label)?;
            if c.is_terminal() {
                return Err(format!("{name} after {} ended", failed.label));
            }
            c.failed = Some(failed);
            finish_running(r);
        }
        EventKind::CandidateFrozen(f) => {
            expect_state(r.state, &[S::Validating], &name)?;
            let c = candidate(r, &f.label)?;
            once(&c.frozen, &name, &f.label)?;
            c.frozen = Some(f);
        }
        EventKind::ValidationCompleted(v) => {
            expect_state(r.state, &[S::Validating], &name)?;
            let c = candidate(r, &v.label)?;
            if c.frozen.is_none() {
                return Err(format!("{name} before {} was frozen", v.label));
            }
            once(&c.validation, &name, &v.label)?;
            c.validation = Some(v.report);
            if all_validated(r) && r.candidates.values().any(CandidateView::is_eligible) {
                r.state = S::JudgingBackground;
            }
        }
        EventKind::JudgeScheduled(j) => {
            expect_state(r.state, &[S::JudgingBackground], &name)?;
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
        }
        EventKind::JudgeStarted(j) => {
            expect_judge_attempt(r, j.attempt, &name)?;
            if r.judge.started {
                return Err(format!("{name} repeated for attempt {}", j.attempt));
            }
            r.judge.started = true;
        }
        EventKind::JudgeCompleted(j) => {
            expect_judge_attempt(r, j.attempt, &name)?;
            r.judge.judgment_id = Some(j.judgment_id);
        }
        EventKind::JudgeFailed(j) => {
            expect_judge_attempt(r, j.attempt, &name)?;
            r.judge.failures += 1;
            if j.attempt >= MAX_JUDGE_ATTEMPTS {
                r.state = S::NeedsIntervention;
            }
        }
        EventKind::WinnerSelected(w) => {
            expect_state(r.state, &[S::JudgingBackground], &name)?;
            let Some(judgment) = &r.judge.judgment_id else {
                return Err(format!("{name} before a judgment"));
            };
            if &w.judgment_id != judgment {
                return Err(format!("{name} cites judgment {}", w.judgment_id));
            }
            if !candidate(r, &w.label)?.is_eligible() {
                return Err(format!("{name}: {} is not eligible", w.label));
            }
            // DECIDED; a requested promotion moves straight on to revalidate.
            r.state = match w.promotion {
                PromotionIntent::NotRequested => S::Decided,
                PromotionIntent::Requested { .. } => S::Revalidating,
            };
            r.winner = Some(w);
        }
        EventKind::WinnerRejected(rej) => {
            let allowed = match r.state {
                S::Validating => {
                    all_validated(r) && !r.candidates.values().any(CandidateView::is_eligible)
                }
                S::JudgingBackground => r.judge.judgment_id.is_some(),
                S::Revalidating => true,
                _ => false,
            };
            if !allowed {
                return Err(format!("{name} is invalid in state {}", r.state));
            }
            r.rejected = Some(rej.reason);
            r.state = S::Rejected;
        }
        EventKind::PromotionStarted(p) => {
            expect_state(r.state, &[S::Revalidating], &name)?;
            r.promotion.started = Some(p);
            r.state = S::Promoting;
        }
        EventKind::PromotionConflicted(c) => {
            expect_state(r.state, &[S::Revalidating, S::Promoting], &name)?;
            r.promotion.conflicted = Some(c);
            r.state = S::NeedsIntervention;
        }
        EventKind::PromotionCompleted(c) => {
            expect_state(r.state, &[S::Promoting], &name)?;
            r.promotion.completed = Some(c);
            r.state = S::Promoted;
        }
        EventKind::PromotionRolledBack(rb) => {
            expect_state(r.state, &[S::Promoted, S::Complete], &name)?;
            r.promotion.rolled_back.push(rb);
        }
        EventKind::WorktreeCleanupFailed(f) => {
            candidate(r, &f.label)?;
            r.cleanup_failures.push(f);
        }
        EventKind::OutcomeRecorded(o) => {
            expect_state(
                r.state,
                &[S::Decided, S::Promoted, S::Cleanup, S::Complete],
                &name,
            )?;
            r.outcomes.push(o);
        }
        EventKind::RoundNeedsIntervention(n) => {
            let allowed: &[S] = match n.source {
                InterventionSource::Judge => &[S::JudgingBackground],
                InterventionSource::Promotion => &[S::Revalidating, S::Promoting],
                InterventionSource::Operator => &[
                    S::Running,
                    S::Validating,
                    S::JudgingBackground,
                    S::Decided,
                    S::Revalidating,
                    S::Promoting,
                ],
            };
            expect_state(r.state, allowed, &name)?;
            if n.source == InterventionSource::Judge && r.judge.judgment_id.is_none() {
                return Err(format!("{name} from the judge before a judgment"));
            }
            r.needs_intervention = Some(n);
            r.state = S::NeedsIntervention;
        }
        EventKind::RoundCleanupStarted(_) => {
            expect_state(
                r.state,
                &[S::Decided, S::Rejected, S::Promoted, S::NeedsIntervention],
                &name,
            )?;
            r.cleaned_from = Some(r.state);
            r.state = S::Cleanup;
        }
        EventKind::RoundCompleted(c) => {
            expect_state(r.state, &[S::Cleanup], &name)?;
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
            r.state = S::Complete;
        }
        EventKind::ExperimentCreated(_)
        | EventKind::PreflightCompleted(_)
        | EventKind::ExperimentAborted(_)
        | EventKind::RoundCreated(_)
        | EventKind::Unknown { .. } => unreachable!("handled by Projection::try_apply"),
    }
    Ok(())
}

/// RUNNING → VALIDATING once every candidate is terminal.
fn finish_running(r: &mut RoundView) {
    if r.candidates.values().all(CandidateView::is_terminal) {
        r.state = RoundState::Validating;
    }
}

fn all_validated(r: &RoundView) -> bool {
    r.candidates.values().all(|c| c.validation.is_some())
}

fn expect_judge_attempt(r: &RoundView, attempt: u32, what: &str) -> Result<(), String> {
    expect_state(r.state, &[RoundState::JudgingBackground], what)?;
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
