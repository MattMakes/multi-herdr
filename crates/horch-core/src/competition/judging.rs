//! The coordinator's judging step (B4): JUDGING_BACKGROUND to a decision.
//!
//! The coordinator is the single authority (CMP-16, JDG-04). The detached
//! judge job writes only its job dir; everything durable is written here:
//! every `judge.*` event, `judgements/<round>.json` (through
//! `create_immutable`, once), the winner events, and the `kind: judge`
//! execution record (JDG-09).
//!
//! [`start`] schedules attempt 1. [`poll`] runs each tick and after a
//! restart: it reads the projection and the job dir, so every step is
//! idempotent on re-entry. A crashed, timed-out, lost or malformed attempt
//! is recorded as `judge.failed` and retried once; a second failure leaves
//! the round in NEEDS_INTERVENTION (JDG-08).

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};

use crate::competition::model::RoundState;
use crate::evaluation::judge_input::{blind_labels, build_judge_input, JudgeInput};
use crate::evaluation::judgment::{Judgment, JudgmentRecord};
use crate::evaluation::parser::{parse_judgment, ParseError};
use crate::evaluation::rubric::{judge_policy_digest, rubric_text, schema_text};
use crate::evaluation::scheduler::{
    discover, job_facts, kill_job, ExitReason, JobState, JudgeJobSpec, OUTPUT_CAP_BYTES,
};
use crate::evaluation::validator::ValidationReport;
use crate::evaluation::winner::{decide_winner, WinnerOutcome, WinnerPolicy};
use crate::execution::legacy::LedgerRecordV1;
use crate::execution::model::{ExecutionStatus, FailureKind};
use crate::execution::store::ExecutionStore;
use crate::fsx;
use crate::ids::{ExecutionId, JudgmentId, RoundId, SessionId};
use crate::measure::digest::{sha256_bytes, Digest};
use crate::measure::event::{
    Actor, EventKind, InterventionSource, JudgeCompleted, JudgeFailed, JudgeFailure,
    JudgeScheduled, JudgeStarted, PromotionIntent, RoundNeedsIntervention, WinnerRejected,
    WinnerSelected,
};
use crate::measure::paths::DatasetPaths;
use crate::measure::projection::{fold, RoundView, MAX_JUDGE_ATTEMPTS};
use crate::measure::recorder::{JsonlRecorder, NewEvent, Recorder};
use crate::runtime::RuntimeContext;
use crate::teammates::Teammate;
use crate::vcs::git::GitClient;
use crate::vcs::worktree::FrozenCandidate;

/// After `judge.scheduled` and the spawn of that attempt.
pub const ABORT_AFTER_JUDGE_SCHEDULED: &str = "abort-after-judge-scheduled";
/// After `judgements/<round>.json` is written.
pub const ABORT_AFTER_JUDGMENT_WRITTEN: &str = "abort-after-judgment-written";
/// After `winner.selected`.
pub const ABORT_AFTER_WINNER_SELECTED: &str = "abort-after-winner-selected";

/// The exit code of a process a fault point stopped (as `Faults::abort_if`).
const ABORT_EXIT_CODE: i32 = 86;

/// The longest one judge attempt may run, unless the config says otherwise.
pub const DEFAULT_JUDGE_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// The `label` of a judge record: `judge:<attempt>` (the A6 ledger shape of
/// `ExecutionKind::Judge`).
pub fn judge_record_label(attempt: u32) -> String {
    format!("judge:{attempt}")
}

/// Starts one judge attempt. The real one is [`DetachedLauncher`]; tests
/// give a fake that writes the job files itself.
pub trait JobLauncher {
    /// Start the job for `spec`; returns its pid.
    fn launch(&self, spec: &JudgeJobSpec) -> Result<u32>;
}

/// `evaluation::scheduler::schedule`: the detached `judge-job` process.
pub struct DetachedLauncher<'a>(pub &'a RuntimeContext);

impl JobLauncher for DetachedLauncher<'_> {
    fn launch(&self, spec: &JudgeJobSpec) -> Result<u32> {
        crate::evaluation::scheduler::schedule(self.0, spec)
    }
}

/// What the judging step needs from the coordinator.
pub struct JudgeEnv<'a> {
    pub ctx: &'a RuntimeContext,
    pub recorder: &'a JsonlRecorder,
    pub paths: &'a DatasetPaths,
    pub git: &'a dyn GitClient,
    /// The main repository: patches are read here, never in a worktree.
    pub repo: &'a Path,
    /// The project's execution store; the judge record goes here.
    pub store: &'a ExecutionStore,
    /// The hidden `judge` teammate.
    pub judge: &'a Teammate,
    /// The task text every candidate worked on (`task.md` of the bundle).
    pub task_text: &'a str,
    pub policy: &'a WinnerPolicy,
    /// What `winner.selected` says about promotion.
    pub promotion: PromotionIntent,
    pub timeout: Duration,
    /// See `evaluation::scheduler::DEFAULT_STALE_AFTER`.
    pub stale_after: Duration,
    pub launcher: &'a dyn JobLauncher,
    /// A monotonic clock for `occurred_at`.
    pub clock: &'a dyn Fn() -> DateTime<Utc>,
}

/// What [`poll`] found.
#[derive(Debug, Clone, PartialEq)]
pub enum JudgingStatus {
    /// The job runs, or a retry was just scheduled.
    Waiting,
    /// `winner.selected` (`Winner`) or `winner.rejected` (`Rejected`).
    Decided(WinnerOutcome),
    NeedsIntervention,
}

/// Schedule the first judge attempt of `round`.
///
/// CMP-12: the judge runs only on a terminal set. The round must be in
/// JUDGING_BACKGROUND, which the fold reaches only after every candidate is
/// validated (the round deadline turns every live candidate into a
/// validated `TimedOut` first). Zero eligible candidates never get here: the
/// coordinator records `winner.rejected{no_eligible}` and skips the judge.
/// A round that already has an attempt or a decision is left to [`poll`].
pub fn start(env: &JudgeEnv, round_id: &RoundId) -> Result<()> {
    let round = view(env, round_id)?;
    if settled(&round).is_some() || round.judge.attempt > 0 {
        return Ok(());
    }
    if round.state != RoundState::JudgingBackground {
        bail!(
            "round {round_id} is {}; the judge waits until every candidate is validated",
            round.state
        );
    }
    if let Some((label, _)) = round
        .candidates
        .iter()
        .find(|(_, c)| c.validation.is_none())
    {
        bail!("round {round_id}: candidate {label} is not validated yet");
    }
    schedule_attempt(env, round_id, &round, 1)
}

/// One tick of the judging step. Safe to call again at any point, also
/// after a crash at any fault point.
pub fn poll(env: &JudgeEnv, round_id: &RoundId, now: DateTime<Utc>) -> Result<JudgingStatus> {
    let round = view(env, round_id)?;
    if let Some(done) = settled(&round) {
        return Ok(done);
    }
    if round.state != RoundState::JudgingBackground {
        bail!("round {round_id} is {}, not judging", round.state);
    }
    if let Some(judgment_id) = round.judge.judgment_id.clone() {
        // A restart after `judge.completed`: finish the decision.
        return decide(env, round_id, &round, round.judge.attempt, &judgment_id);
    }
    let attempt = round.judge.attempt;
    if attempt == 0 {
        start(env, round_id)?;
        return Ok(JudgingStatus::Waiting);
    }
    if round.judge.failures >= attempt {
        // A restart after `judge.failed`, before the retry was scheduled.
        return retry_or_stop(env, round_id, attempt);
    }
    let job_dir = env.paths.job_dir(round_id, attempt)?;
    match discover(&job_dir, now, env.stale_after) {
        JobState::NotStarted => {
            // A restart between `judge.scheduled` and the spawn.
            schedule_attempt(env, round_id, &round, attempt)?;
            Ok(JudgingStatus::Waiting)
        }
        JobState::Running { pid } => {
            if pid != 0 && !round.judge.started {
                mark_started(env, round_id, &round, attempt, pid)?;
            }
            Ok(JudgingStatus::Waiting)
        }
        JobState::Exited(exit) => {
            let cause = match exit.reason {
                ExitReason::Timeout => JudgeFailure::TimedOut,
                ExitReason::TooLarge => JudgeFailure::OverCap,
                ExitReason::Crash | ExitReason::Ok => JudgeFailure::Crashed { code: exit.code },
            };
            fail(env, round_id, &round, attempt, cause, exit.code)
        }
        JobState::Lost => {
            if let Some(hb) = job_facts(&job_dir).heartbeat {
                kill_job(hb.pid);
            }
            fail(env, round_id, &round, attempt, JudgeFailure::Lost, None)
        }
        JobState::OutputPresent(path) => {
            let raw = read_capped(&path)?;
            let labels = bundle_pairs(round_id, &round)
                .into_iter()
                .map(|(b, _)| b)
                .collect::<Vec<_>>();
            match parse_judgment(&raw, &labels, OUTPUT_CAP_BYTES) {
                Ok(_) => complete(env, round_id, &round, attempt, &job_dir, &raw),
                Err(ParseError::TooLarge(_)) => fail(
                    env,
                    round_id,
                    &round,
                    attempt,
                    JudgeFailure::OverCap,
                    Some(0),
                ),
                Err(e) => fail(
                    env,
                    round_id,
                    &round,
                    attempt,
                    JudgeFailure::Malformed {
                        error: e.to_string(),
                    },
                    Some(0),
                ),
            }
        }
    }
}

/// The answer a round already has, if any.
fn settled(round: &RoundView) -> Option<JudgingStatus> {
    if let Some(w) = &round.winner {
        return Some(JudgingStatus::Decided(WinnerOutcome::Winner {
            label: w.label.clone(),
        }));
    }
    if let Some(reason) = &round.rejected {
        return Some(JudgingStatus::Decided(WinnerOutcome::Rejected {
            reason: reason.clone(),
        }));
    }
    if round.needs_intervention.is_some() || round.state == RoundState::NeedsIntervention {
        return Some(JudgingStatus::NeedsIntervention);
    }
    None
}

fn view(env: &JudgeEnv, round_id: &RoundId) -> Result<RoundView> {
    let events = env.recorder.read_all()?.events;
    fold(&events)
        .rounds
        .remove(round_id)
        .with_context(|| format!("no round {round_id} in the event log"))
}

fn abort_if(env: &JudgeEnv, point: &str) {
    if env.ctx.settings.faults.has(point) {
        eprintln!("multi-herdr-dataset: HORCH_FAULT {point}: aborting");
        std::process::exit(ABORT_EXIT_CODE);
    }
}

fn emit(
    env: &JudgeEnv,
    round_id: &RoundId,
    round: &RoundView,
    execution: Option<&ExecutionId>,
    key: String,
    kind: EventKind,
) -> Result<()> {
    env.recorder.append(NewEvent {
        kind,
        actor: Actor::Coordinator,
        experiment_id: round.experiment_id.clone(),
        round_id: Some(round_id.clone()),
        execution_id: execution.cloned(),
        idempotency_key: key,
        occurred_at: (env.clock)(),
    })?;
    Ok(())
}

/// The round's labels that go into the bundle (every validated candidate),
/// paired with their bundle labels: `(bundle, original)`. The same pairs as
/// `build_judge_input` makes.
fn bundle_pairs(round_id: &RoundId, round: &RoundView) -> Vec<(String, String)> {
    let originals: Vec<String> = round
        .created
        .labels
        .iter()
        .filter(|l| {
            round
                .candidates
                .get(*l)
                .is_some_and(|c| c.frozen.is_some() && c.validation.is_some())
        })
        .cloned()
        .collect();
    blind_labels(round_id, &originals)
}

/// Build the bundle, or verify the sealed one. A partial bundle with
/// different bytes is a `Conflict` error and is never retried.
fn bundle(env: &JudgeEnv, round_id: &RoundId, round: &RoundView) -> Result<JudgeInput> {
    let mut inputs: Vec<(String, FrozenCandidate, ValidationReport)> = Vec::new();
    for (_, original) in bundle_pairs(round_id, round) {
        let c = &round.candidates[&original];
        let (Some(frozen), Some(report)) = (&c.frozen, &c.validation) else {
            continue;
        };
        let worktree = c.worktree.as_ref();
        inputs.push((
            original.clone(),
            FrozenCandidate {
                label: original.clone(),
                execution_id: c
                    .execution_id
                    .clone()
                    .with_context(|| format!("candidate {original} has no execution"))?,
                worktree: worktree.map(|w| w.path.clone()).unwrap_or_default(),
                branch: worktree.map(|w| w.branch.clone()).unwrap_or_default(),
                base_sha: worktree
                    .map(|w| w.base_sha.clone())
                    .unwrap_or_else(|| round.created.base_sha.clone()),
                head_sha: frozen.head_sha.clone(),
                numstat: frozen.numstat.clone(),
                diff_digest: frozen.diff_digest.clone(),
                frozen_at: String::new(),
            },
            report.clone(),
        ));
    }
    build_judge_input(
        round_id,
        round,
        env.task_text,
        &inputs,
        env.paths,
        env.git,
        env.repo,
    )
}

fn policy_digest(env: &JudgeEnv, round: &RoundView) -> Result<Digest> {
    let (model, effort) = model_effort(env.judge)?;
    Ok(judge_policy_digest(
        &env.judge.persona,
        rubric_text(),
        schema_text(),
        &model,
        &effort,
        env.policy,
        &round.created.label_policy_version,
    ))
}

fn model_effort(judge: &Teammate) -> Result<(String, String)> {
    let model = judge
        .model
        .clone()
        .context("the judge teammate names no model")?;
    Ok((model, judge.effort.clone().unwrap_or_default()))
}

/// The judge record of `attempt`, created on first use: `kind: judge`
/// (ledger `kind` worker, `round_id`, `label` `judge:<n>`), status Planned.
fn judge_record(
    env: &JudgeEnv,
    round_id: &RoundId,
    attempt: u32,
    input_dir: &Path,
) -> Result<LedgerRecordV1> {
    let label = judge_record_label(attempt);
    let round_text = round_id.to_string();
    let found = env.store.read()?.into_iter().rfind(|r| {
        r.round_id.as_deref() == Some(round_text.as_str()) && r.label.as_deref() == Some(&label)
    });
    if let Some(r) = found {
        return Ok(r);
    }
    let now = (env.clock)();
    let stamp = crate::clock::stamp(now);
    let (model, effort) = model_effort(env.judge)?;
    let session = SessionId::new(crate::ids::mint_v7(now).to_string())?;
    let mut r = LedgerRecordV1 {
        record_id: ExecutionId::mint(now).to_string(),
        session_id: Some(session.to_string()),
        agent: env.judge.agent.as_str().to_string(),
        tier: env.judge.name.clone(),
        model,
        effort: Some(effort).filter(|e| !e.is_empty()),
        role: format!("judge-{}-{attempt}", short(&round_text)),
        task: format!("judge round {round_id}, attempt {attempt}"),
        created_at: stamp.clone(),
        updated_at: stamp,
        project: Some(env.ctx.paths.project()?.to_string_lossy().into_owned()),
        workdir: Some(input_dir.to_string_lossy().into_owned()),
        round_id: Some(round_text),
        label: Some(label),
        ..LedgerRecordV1::default()
    };
    r.set_state(ExecutionStatus::Planned);
    env.store.insert(r.clone())?;
    Ok(r)
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

/// `judge.scheduled` for `attempt`, then the spawn unless the job dir shows
/// one already. Idempotent: the event key is per attempt, and the record is
/// found again.
fn schedule_attempt(
    env: &JudgeEnv,
    round_id: &RoundId,
    round: &RoundView,
    attempt: u32,
) -> Result<()> {
    let input = bundle(env, round_id, round)?;
    let record = judge_record(env, round_id, attempt, &input.dir)?;
    let execution: ExecutionId = record.record_id.parse()?;
    let job_dir = env.paths.job_dir(round_id, attempt)?;
    emit(
        env,
        round_id,
        round,
        Some(&execution),
        format!("judge.scheduled:{round_id}:{attempt}"),
        EventKind::JudgeScheduled(JudgeScheduled {
            attempt,
            input_digest: input.digest.clone(),
            judge_policy_digest: policy_digest(env, round)?,
            job_dir: job_dir.clone(),
        }),
    )?;
    if job_facts(&job_dir).spawned_at.is_none() {
        let (model, effort) = model_effort(env.judge)?;
        let session = record
            .session_id
            .clone()
            .context("the judge record has no session id")?;
        env.launcher.launch(&JudgeJobSpec {
            round: round_id.clone(),
            attempt,
            input_dir: input.dir.clone(),
            job_dir,
            session: SessionId::new(session)?,
            model,
            effort,
            timeout: env.timeout,
        })?;
    }
    abort_if(env, ABORT_AFTER_JUDGE_SCHEDULED);
    Ok(())
}

fn record_of(env: &JudgeEnv, round_id: &RoundId, attempt: u32) -> Result<ExecutionId> {
    let label = judge_record_label(attempt);
    let round_text = round_id.to_string();
    let r = env
        .store
        .read()?
        .into_iter()
        .rfind(|r| {
            r.round_id.as_deref() == Some(round_text.as_str()) && r.label.as_deref() == Some(&label)
        })
        .with_context(|| format!("no judge record for round {round_id} attempt {attempt}"))?;
    Ok(r.record_id.parse()?)
}

fn mark_started(
    env: &JudgeEnv,
    round_id: &RoundId,
    round: &RoundView,
    attempt: u32,
    pid: u32,
) -> Result<()> {
    let execution = record_of(env, round_id, attempt)?;
    emit(
        env,
        round_id,
        round,
        Some(&execution),
        format!("judge.started:{round_id}:{attempt}"),
        EventKind::JudgeStarted(JudgeStarted { attempt, pid }),
    )?;
    env.store
        .set_state(execution.as_str(), ExecutionStatus::Running)
}

/// `judge.failed`, the record Failed, then attempt 2 or NEEDS_INTERVENTION.
fn fail(
    env: &JudgeEnv,
    round_id: &RoundId,
    round: &RoundView,
    attempt: u32,
    cause: JudgeFailure,
    code: Option<i32>,
) -> Result<JudgingStatus> {
    let execution = record_of(env, round_id, attempt)?;
    let failure = match &cause {
        JudgeFailure::TimedOut => FailureKind::TimedOut,
        JudgeFailure::Lost => FailureKind::Crashed,
        JudgeFailure::Crashed { code } => FailureKind::AgentExited { code: *code },
        JudgeFailure::Malformed { .. } | JudgeFailure::OverCap => FailureKind::AgentExited { code },
    };
    emit(
        env,
        round_id,
        round,
        Some(&execution),
        format!("judge.failed:{round_id}:{attempt}"),
        EventKind::JudgeFailed(JudgeFailed { attempt, cause }),
    )?;
    env.store
        .set_state(execution.as_str(), ExecutionStatus::Failed { failure })?;
    retry_or_stop(env, round_id, attempt)
}

fn retry_or_stop(env: &JudgeEnv, round_id: &RoundId, attempt: u32) -> Result<JudgingStatus> {
    if attempt >= MAX_JUDGE_ATTEMPTS {
        return Ok(JudgingStatus::NeedsIntervention);
    }
    let round = view(env, round_id)?;
    schedule_attempt(env, round_id, &round, attempt + 1)?;
    Ok(JudgingStatus::Waiting)
}

/// A valid answer: `judge.started` if the job was never seen running,
/// `judge.completed`, the record Done, then the decision.
fn complete(
    env: &JudgeEnv,
    round_id: &RoundId,
    round: &RoundView,
    attempt: u32,
    job_dir: &Path,
    raw: &[u8],
) -> Result<JudgingStatus> {
    let execution = record_of(env, round_id, attempt)?;
    if !round.judge.started {
        if let Some(hb) = job_facts(job_dir).heartbeat {
            mark_started(env, round_id, round, attempt, hb.pid)?;
        }
    }
    let judgment_id = JudgmentId::mint((env.clock)());
    emit(
        env,
        round_id,
        round,
        Some(&execution),
        format!("judge.completed:{round_id}:{attempt}"),
        EventKind::JudgeCompleted(JudgeCompleted {
            attempt,
            judgment_id,
            output_digest: sha256_bytes(raw),
        }),
    )?;
    env.store
        .set_state(execution.as_str(), ExecutionStatus::Done)?;
    // The id the log holds wins (a duplicate append keeps the first).
    let round = view(env, round_id)?;
    let judgment_id = round
        .judge
        .judgment_id
        .clone()
        .context("judge.completed did not reach the projection")?;
    decide(env, round_id, &round, attempt, &judgment_id)
}

/// Write the judgment once, then `winner.selected`, `winner.rejected` or
/// `round.needs_intervention`. Re-entry rebuilds the same record bytes from
/// the same output and events, so `create_immutable` accepts them.
fn decide(
    env: &JudgeEnv,
    round_id: &RoundId,
    round: &RoundView,
    attempt: u32,
    judgment_id: &JudgmentId,
) -> Result<JudgingStatus> {
    let pairs = bundle_pairs(round_id, round);
    let labels: Vec<String> = pairs.iter().map(|(b, _)| b.clone()).collect();
    let to_original: BTreeMap<String, String> = pairs.into_iter().collect();
    let job_dir = env.paths.job_dir(round_id, attempt)?;
    let raw = read_capped(&job_dir.join(crate::evaluation::scheduler::OUTPUT_FILE))?;
    let judgment: Judgment = parse_judgment(&raw, &labels, OUTPUT_CAP_BYTES)
        .map_err(|e| anyhow::anyhow!("the judged output no longer parses: {e}"))?;
    let scheduled = scheduled_event(env, round_id, attempt)?;
    let execution = record_of(env, round_id, attempt)?;
    let record = JudgmentRecord {
        judgment_id: judgment_id.clone(),
        round_id: round_id.clone(),
        attempt,
        input_digest: scheduled.input_digest,
        judge_policy_digest: scheduled.judge_policy_digest,
        execution_id: execution.clone(),
        judgment: judgment.clone(),
    };
    let mut bytes = serde_json::to_vec_pretty(&record)?;
    bytes.push(b'\n');
    fsx::ensure_private_dir(&env.paths.judgements_dir())?;
    fsx::create_immutable(&env.paths.judgement(round_id)?, &bytes, fsx::PRIVATE_FILE)?;
    abort_if(env, ABORT_AFTER_JUDGMENT_WRITTEN);

    let eligible: BTreeSet<String> = to_original
        .iter()
        .filter(|(_, original)| round.candidates[*original].is_eligible())
        .map(|(bundle, _)| bundle.clone())
        .collect();
    match decide_winner(Some(&judgment), &eligible, env.policy) {
        WinnerOutcome::Winner { label } => {
            let original = to_original
                .get(&label)
                .with_context(|| format!("judge picked unknown label {label}"))?
                .clone();
            let c = &round.candidates[&original];
            emit(
                env,
                round_id,
                round,
                Some(&execution),
                format!("winner:{round_id}"),
                EventKind::WinnerSelected(WinnerSelected {
                    label: original.clone(),
                    execution_id: c
                        .execution_id
                        .clone()
                        .with_context(|| format!("candidate {original} has no execution"))?,
                    head_sha: c
                        .frozen
                        .as_ref()
                        .map(|f| f.head_sha.clone())
                        .unwrap_or_default(),
                    judgment_id: judgment_id.clone(),
                    promotion: env.promotion.clone(),
                }),
            )?;
            abort_if(env, ABORT_AFTER_WINNER_SELECTED);
            Ok(JudgingStatus::Decided(WinnerOutcome::Winner {
                label: original,
            }))
        }
        WinnerOutcome::Rejected { reason } => {
            emit(
                env,
                round_id,
                round,
                Some(&execution),
                format!("winner:{round_id}"),
                EventKind::WinnerRejected(WinnerRejected {
                    reason: reason.clone(),
                }),
            )?;
            Ok(JudgingStatus::Decided(WinnerOutcome::Rejected { reason }))
        }
        WinnerOutcome::NeedsIntervention { reason } => {
            emit(
                env,
                round_id,
                round,
                Some(&execution),
                format!("needs_intervention:{round_id}"),
                EventKind::RoundNeedsIntervention(RoundNeedsIntervention {
                    reason,
                    source: InterventionSource::Judge,
                }),
            )?;
            Ok(JudgingStatus::NeedsIntervention)
        }
    }
}

fn scheduled_event(env: &JudgeEnv, round_id: &RoundId, attempt: u32) -> Result<JudgeScheduled> {
    env.recorder
        .read_all()?
        .events
        .iter()
        .filter(|e| e.round_id.as_ref() == Some(round_id))
        .filter_map(|e| match e.event() {
            Ok(EventKind::JudgeScheduled(s)) if s.attempt == attempt => Some(s),
            _ => None,
        })
        .next()
        .with_context(|| format!("no judge.scheduled for round {round_id} attempt {attempt}"))
}

/// At most one byte over the cap, so the parser reports `TooLarge` without
/// the whole file in memory.
fn read_capped(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut raw = Vec::new();
    file.take(OUTPUT_CAP_BYTES as u64 + 1)
        .read_to_end(&mut raw)
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(raw)
}

/// For a test or a caller that needs the record shape: whether `r` is the
/// judge record of `round` and `attempt`.
pub fn is_judge_record(r: &LedgerRecordV1, round: &RoundId, attempt: u32) -> bool {
    r.round_id.as_deref() == Some(round.as_str())
        && r.label.as_deref() == Some(judge_record_label(attempt).as_str())
        && r.experiment_id.is_none()
}
