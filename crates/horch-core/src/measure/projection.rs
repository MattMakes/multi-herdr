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
//! (design §4.1 says why these 3 kinds exist).

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
    InterventionSource, JudgeCompleted, JudgeFailed, JudgeScheduled, JudgeStarted, OutcomeRecorded,
    PreflightCompleted, PromotionCompleted, PromotionConflicted, PromotionIntent,
    PromotionRolledBack, PromotionStarted, RoundCreated, RoundNeedsIntervention,
    ValidationCompleted, WinnerSelected, WorktreeCleanupFailed, WorktreeCreated,
};

/// A failed judge attempt with this number ends the judging (design §5).
pub(crate) const MAX_JUDGE_ATTEMPTS: u32 = 2;

/// Return `Err(format!(..))` unless `ok`. A guard of the fold: the message
/// is built only when it fails.
macro_rules! require {
    ($ok:expr, $($why:tt)+) => {
        if !$ok {
            return Err(format!($($why)+));
        }
    };
}

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
        let kind = event_of(env)?;
        let exp = &env.experiment_id;
        match kind {
            // A newer writer's kind: kept in the log, nothing to project.
            EventKind::Unknown { .. } => Ok(()),
            EventKind::ExperimentCreated(created) => self.create_experiment(exp, created),
            EventKind::PreflightCompleted(p) => self.complete_preflight(exp, p),
            EventKind::ExperimentAborted(a) => self.abort_experiment(exp, a),
            EventKind::RoundCreated(created) => self.create_round(env, created),
            other => apply_round(self.round(env)?, env, other),
        }
    }

    fn complete_preflight(
        &mut self,
        exp: &ExperimentId,
        p: PreflightCompleted,
    ) -> Result<(), String> {
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

    fn abort_experiment(&mut self, exp: &ExperimentId, a: ExperimentAborted) -> Result<(), String> {
        let x = self.experiment(exp)?;
        x.state = step(x.state, RoundEvent::ExperimentAborted)?;
        x.aborted = Some(a);
        Ok(())
    }

    fn create_experiment(
        &mut self,
        exp: &ExperimentId,
        created: ExperimentCreated,
    ) -> Result<(), String> {
        require!(
            !self.experiments.contains_key(exp),
            "experiment {exp} already created"
        );
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

    fn create_round(&mut self, env: &EventEnvelope, created: RoundCreated) -> Result<(), String> {
        let exp = &env.experiment_id;
        let round = round_id(env)?;
        let state = step(self.experiment(exp)?.state, RoundEvent::RoundCreated)?;
        require!(
            !self.rounds.contains_key(&round),
            "round {round} already created"
        );
        let candidates = round_candidates(&created.labels)?;
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
        require!(
            r.experiment_id == env.experiment_id,
            "round {round} belongs to experiment {}",
            r.experiment_id
        );
        Ok(r)
    }
}

/// One empty view per label; a label may not repeat.
fn round_candidates(labels: &[String]) -> Result<BTreeMap<String, CandidateView>, String> {
    let mut candidates = BTreeMap::new();
    for label in labels {
        let fresh = candidates
            .insert(label.clone(), CandidateView::default())
            .is_none();
        require!(fresh, "label {label} appears twice");
    }
    Ok(candidates)
}

fn event_of(env: &EventEnvelope) -> Result<EventKind, String> {
    env.event().map_err(|e| e.to_string())
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
    let name = kind.name().to_string();
    match kind {
        EventKind::CandidatePlanned(_)
        | EventKind::WorktreeCreated(_)
        | EventKind::CandidateSpawned(_)
        | EventKind::CandidateCompleted(_)
        | EventKind::CandidateFailed(_)
        | EventKind::CandidateFrozen(_)
        | EventKind::ValidationCompleted(_) => apply_candidate_event(r, env, kind, &name),
        EventKind::JudgeScheduled(_)
        | EventKind::JudgeStarted(_)
        | EventKind::JudgeCompleted(_)
        | EventKind::JudgeFailed(_) => apply_judge_event(r, kind, &name),
        EventKind::WinnerSelected(_) | EventKind::WinnerRejected(_) => {
            apply_decision_event(r, kind, &name)
        }
        EventKind::PromotionStarted(_)
        | EventKind::PromotionConflicted(_)
        | EventKind::PromotionCompleted(_) => apply_promotion_event(r, kind),
        EventKind::PromotionRolledBack(_) | EventKind::OperatorPromote(_) => {
            apply_reversal_event(r, kind, &name)
        }
        EventKind::WorktreeCleanupFailed(_)
        | EventKind::OutcomeRecorded(_)
        | EventKind::RoundNeedsIntervention(_)
        | EventKind::RoundCleanupStarted(_)
        | EventKind::RoundCompleted(_) => apply_lifecycle_event(r, kind, &name),
        EventKind::ExperimentCreated(_)
        | EventKind::PreflightCompleted(_)
        | EventKind::ExperimentAborted(_)
        | EventKind::RoundCreated(_)
        | EventKind::Unknown { .. } => unreachable!("handled by Projection::try_apply"),
    }
}

/// `candidate.*`, `worktree.created` and `validation.completed`.
fn apply_candidate_event(
    r: &mut RoundView,
    env: &EventEnvelope,
    kind: EventKind,
    name: &str,
) -> Result<(), String> {
    match kind {
        EventKind::CandidatePlanned(p) => plan_candidate(r, env, p, name),
        EventKind::WorktreeCreated(w) => create_worktree(r, w, name),
        EventKind::CandidateSpawned(s) => spawn_candidate(r, env, s, name),
        EventKind::CandidateCompleted(done) => complete_candidate(r, done, name),
        EventKind::CandidateFailed(failed) => fail_candidate(r, failed, name),
        EventKind::CandidateFrozen(f) => freeze_candidate(r, f, name),
        EventKind::ValidationCompleted(v) => validate_candidate(r, v, name),
        _ => unreachable!("not a candidate event"),
    }
}

fn plan_candidate(
    r: &mut RoundView,
    env: &EventEnvelope,
    p: CandidatePlanned,
    name: &str,
) -> Result<(), String> {
    let last = others_all(r, &p.label, |c| c.planned.is_some());
    let to = step(r.state, RoundEvent::CandidatePlanned { last })?;
    let c = candidate(r, &p.label)?;
    once(&c.planned, name, &p.label)?;
    if env.execution_id.is_some() {
        c.execution_id = env.execution_id.clone();
    }
    c.planned = Some(p);
    r.state = to;
    Ok(())
}

fn create_worktree(r: &mut RoundView, w: WorktreeCreated, name: &str) -> Result<(), String> {
    let last = others_all(r, &w.label, |c| c.worktree.is_some());
    let to = step(r.state, RoundEvent::WorktreeCreated { last })?;
    let c = candidate(r, &w.label)?;
    once(&c.worktree, name, &w.label)?;
    c.worktree = Some(w);
    r.state = to;
    Ok(())
}

fn spawn_candidate(
    r: &mut RoundView,
    env: &EventEnvelope,
    s: CandidateSpawned,
    name: &str,
) -> Result<(), String> {
    let to = step(r.state, RoundEvent::CandidateSpawned)?;
    let c = candidate(r, &s.label)?;
    once(&c.spawned, name, &s.label)?;
    not_ended(c, name, &s.label)?;
    if env.execution_id.is_some() {
        c.execution_id = env.execution_id.clone();
    }
    c.spawned = Some(s);
    r.state = to;
    Ok(())
}

fn complete_candidate(
    r: &mut RoundView,
    done: CandidateCompleted,
    name: &str,
) -> Result<(), String> {
    let last = others_all(r, &done.label, CandidateView::is_terminal);
    let to = step(r.state, RoundEvent::CandidateEnded { last })?;
    let c = candidate(r, &done.label)?;
    require!(
        c.spawned.is_some(),
        "{name} before {} was spawned",
        done.label
    );
    not_ended(c, name, &done.label)?;
    c.completed = Some(done);
    r.state = to;
    Ok(())
}

fn fail_candidate(r: &mut RoundView, failed: CandidateFailed, name: &str) -> Result<(), String> {
    // A launch can fail before the spawn event.
    let last = others_all(r, &failed.label, CandidateView::is_terminal);
    let to = step(r.state, RoundEvent::CandidateEnded { last })?;
    let c = candidate(r, &failed.label)?;
    not_ended(c, name, &failed.label)?;
    c.failed = Some(failed);
    r.state = to;
    Ok(())
}

fn freeze_candidate(r: &mut RoundView, f: CandidateFrozen, name: &str) -> Result<(), String> {
    let to = step(r.state, RoundEvent::CandidateFrozen)?;
    let c = candidate(r, &f.label)?;
    once(&c.frozen, name, &f.label)?;
    c.frozen = Some(f);
    r.state = to;
    Ok(())
}

fn validate_candidate(r: &mut RoundView, v: ValidationCompleted, name: &str) -> Result<(), String> {
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
    require!(c.frozen.is_some(), "{name} before {} was frozen", v.label);
    once(&c.validation, name, &v.label)?;
    c.validation = Some(v.report);
    r.state = to;
    Ok(())
}

fn not_ended(c: &CandidateView, name: &str, label: &str) -> Result<(), String> {
    require!(!c.is_terminal(), "{name} after {label} ended");
    Ok(())
}

/// `judge.*`.
fn apply_judge_event(r: &mut RoundView, kind: EventKind, name: &str) -> Result<(), String> {
    match kind {
        EventKind::JudgeScheduled(j) => schedule_judge(r, j, name),
        EventKind::JudgeStarted(j) => start_judge(r, j, name),
        EventKind::JudgeCompleted(j) => complete_judge(r, j, name),
        EventKind::JudgeFailed(j) => fail_judge(r, j, name),
        _ => unreachable!("not a judge event"),
    }
}

fn schedule_judge(r: &mut RoundView, j: JudgeScheduled, name: &str) -> Result<(), String> {
    let to = step(r.state, RoundEvent::JudgeScheduled)?;
    require!(r.judge.judgment_id.is_none(), "{name} after a judgment");
    require!(
        j.attempt == r.judge.attempt + 1,
        "{name} attempt {} after attempt {}",
        j.attempt,
        r.judge.attempt
    );
    r.judge.attempt = j.attempt;
    r.judge.started = false;
    r.state = to;
    Ok(())
}

fn start_judge(r: &mut RoundView, j: JudgeStarted, name: &str) -> Result<(), String> {
    let to = step(r.state, RoundEvent::JudgeStarted)?;
    expect_judge_attempt(r, j.attempt, name)?;
    require!(
        !r.judge.started,
        "{name} repeated for attempt {}",
        j.attempt
    );
    r.judge.started = true;
    r.state = to;
    Ok(())
}

fn complete_judge(r: &mut RoundView, j: JudgeCompleted, name: &str) -> Result<(), String> {
    let to = step(r.state, RoundEvent::JudgeCompleted)?;
    expect_judge_attempt(r, j.attempt, name)?;
    r.judge.judgment_id = Some(j.judgment_id);
    r.state = to;
    Ok(())
}

fn fail_judge(r: &mut RoundView, j: JudgeFailed, name: &str) -> Result<(), String> {
    let last_attempt = j.attempt >= MAX_JUDGE_ATTEMPTS;
    let to = step(r.state, RoundEvent::JudgeFailed { last_attempt })?;
    expect_judge_attempt(r, j.attempt, name)?;
    r.judge.failures += 1;
    r.state = to;
    Ok(())
}

/// `winner.selected` and `winner.rejected`.
fn apply_decision_event(r: &mut RoundView, kind: EventKind, name: &str) -> Result<(), String> {
    match kind {
        EventKind::WinnerSelected(w) => select_winner(r, w, name),
        EventKind::WinnerRejected(rej) => reject_winner(r, rej.reason, name),
        _ => unreachable!("not a decision event"),
    }
}

fn select_winner(r: &mut RoundView, w: WinnerSelected, name: &str) -> Result<(), String> {
    // DECIDED; a requested promotion moves on to revalidate at once.
    let mut to = step(r.state, RoundEvent::WinnerSelected)?;
    if let PromotionIntent::Requested { .. } = w.promotion {
        to = step(to, RoundEvent::PromotionRequested)?;
    }
    let Some(judgment) = &r.judge.judgment_id else {
        return Err(format!("{name} before a judgment"));
    };
    require!(
        &w.judgment_id == judgment,
        "{name} cites judgment {}",
        w.judgment_id
    );
    require!(
        candidate(r, &w.label)?.is_eligible(),
        "{name}: {} is not eligible",
        w.label
    );
    r.winner = Some(w);
    r.state = to;
    Ok(())
}

fn reject_winner(r: &mut RoundView, reason: RejectReason, name: &str) -> Result<(), String> {
    let to = step(r.state, RoundEvent::WinnerRejected)?;
    let allowed = match r.state {
        RoundState::Validating => {
            all_validated(r) && !r.candidates.values().any(CandidateView::is_eligible)
        }
        RoundState::JudgingBackground => r.judge.judgment_id.is_some(),
        _ => true,
    };
    require!(allowed, "{name} is invalid in state {}", r.state);
    r.rejected = Some(reason);
    r.state = to;
    Ok(())
}

/// `promotion.started`, `promotion.conflicted` and `promotion.completed`.
fn apply_promotion_event(r: &mut RoundView, kind: EventKind) -> Result<(), String> {
    match kind {
        EventKind::PromotionStarted(p) => advance(r, RoundEvent::PromotionStarted, |r| {
            r.promotion.started = Some(p)
        }),
        EventKind::PromotionConflicted(c) => advance(r, RoundEvent::PromotionConflicted, |r| {
            r.promotion.conflicted = Some(c)
        }),
        EventKind::PromotionCompleted(c) => advance(r, RoundEvent::PromotionCompleted, |r| {
            r.promotion.completed = Some(c)
        }),
        _ => unreachable!("not a promotion event"),
    }
}

/// `promotion.rolled_back` and `operator.promote`: a promotion undone or
/// asked for again.
fn apply_reversal_event(r: &mut RoundView, kind: EventKind, name: &str) -> Result<(), String> {
    match kind {
        EventKind::PromotionRolledBack(rb) => advance(r, RoundEvent::PromotionRolledBack, |r| {
            r.promotion.rolled_back.push(rb)
        }),
        EventKind::OperatorPromote(_) => operator_promote(r, name),
        _ => unreachable!("not a reversal event"),
    }
}

/// Move the round along `event`, then record what the event carried. The
/// state changes only when the table allows the move.
fn advance(
    r: &mut RoundView,
    event: RoundEvent,
    record: impl FnOnce(&mut RoundView),
) -> Result<(), String> {
    let to = step(r.state, event)?;
    record(r);
    r.state = to;
    Ok(())
}

fn operator_promote(r: &mut RoundView, name: &str) -> Result<(), String> {
    // Back to DECIDED, and on to revalidate at once, as a
    // `winner.selected` with a requested promotion does.
    let to = step(r.state, RoundEvent::OperatorPromote)?;
    let to = step(to, RoundEvent::PromotionRequested)?;
    require!(r.winner.is_some(), "{name} for a round without a winner");
    r.state = to;
    Ok(())
}

/// Cleanup, outcome, intervention and completion events.
fn apply_lifecycle_event(r: &mut RoundView, kind: EventKind, name: &str) -> Result<(), String> {
    match kind {
        // Neither moves the round.
        EventKind::WorktreeCleanupFailed(f) => {
            candidate(r, &f.label)?;
            r.cleanup_failures.push(f);
            Ok(())
        }
        EventKind::OutcomeRecorded(o) => record_outcome(r, o, name),
        EventKind::RoundNeedsIntervention(n) => need_intervention(r, n, name),
        EventKind::RoundCleanupStarted(_) => {
            let from = r.state;
            advance(r, RoundEvent::CleanupStarted, |r| {
                r.cleaned_from = Some(from)
            })
        }
        EventKind::RoundCompleted(c) => complete_round(r, c.final_outcome, name),
        _ => unreachable!("not a lifecycle event"),
    }
}

fn record_outcome(r: &mut RoundView, o: OutcomeRecorded, name: &str) -> Result<(), String> {
    let open = matches!(
        r.state,
        RoundState::Decided | RoundState::Promoted | RoundState::Cleanup | RoundState::Complete
    );
    require!(open, "{name} is invalid in state {}", r.state);
    r.outcomes.push(o);
    Ok(())
}

fn need_intervention(
    r: &mut RoundView,
    n: RoundNeedsIntervention,
    name: &str,
) -> Result<(), String> {
    let to = step(r.state, RoundEvent::NeedsIntervention(n.source))?;
    require!(
        n.source != InterventionSource::Judge || r.judge.judgment_id.is_some(),
        "{name} from the judge before a judgment"
    );
    r.needs_intervention = Some(n);
    r.state = to;
    Ok(())
}

fn complete_round(r: &mut RoundView, outcome: FinalOutcome, name: &str) -> Result<(), String> {
    let to = step(r.state, RoundEvent::RoundCompleted)?;
    let want = match r.cleaned_from {
        Some(RoundState::Decided) => FinalOutcome::Winner,
        Some(RoundState::Rejected) => FinalOutcome::Rejected,
        Some(RoundState::Promoted) => FinalOutcome::Promoted,
        _ => FinalOutcome::NeedsIntervention,
    };
    require!(
        outcome == want,
        "{name} says {outcome:?}, cleanup started from {:?}",
        r.cleaned_from
    );
    r.final_outcome = Some(outcome);
    r.state = to;
    Ok(())
}

fn all_validated(r: &RoundView) -> bool {
    r.candidates.values().all(|c| c.validation.is_some())
}

fn expect_judge_attempt(r: &RoundView, attempt: u32, what: &str) -> Result<(), String> {
    require!(r.judge.judgment_id.is_none(), "{what} after a judgment");
    require!(
        attempt != 0 && attempt == r.judge.attempt,
        "{what} for attempt {attempt}, scheduled {}",
        r.judge.attempt
    );
    require!(
        r.judge.failures < attempt,
        "{what} after attempt {attempt} failed"
    );
    Ok(())
}
