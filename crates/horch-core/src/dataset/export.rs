//! System-One/Laya shaped export rows (dataset design §4.10, EXP-03,
//! EXP-04).
//!
//! [`export`] regenerates one [`ExportRow`] per decided round only from the
//! recorded data: the event log, `judgements/<round>.json`,
//! `promotions/<round>.json` and the execution facts. No agent runs. The rows
//! are sorted by (experiment_id, round_id) and [`render_jsonl`] writes each
//! one as canonical JSON, so two exports of the same data are byte-identical.
//!
//! The execution store arrives in A6 (U19 `a6a-store`); until then the facts
//! come through [`ExecutionFactsSource`], which the CLI adapts to the store.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::competition::model::RoundState;
use crate::evaluation::judgment::{Judgment, JudgmentRecord};
use crate::fsx;
use crate::ids::{ExecutionId, ExperimentId, JudgmentId, RoundId, TaskId};
use crate::measure::digest::{canonical_json, Digest};
use crate::measure::event::{EventEnvelope, OutcomeRecorded};
use crate::measure::paths::DatasetPaths;
use crate::measure::projection::{CandidateView, Projection, RoundView};
use crate::measure::store;
use crate::measure::worker_run::{ExecutionFacts, WorkerRun};
use crate::routing::eligible::EligibleEntry;
use crate::teacher::system_one::{Answer, Question, QuestionKind, API};
use crate::teacher::TeacherRef;
use crate::usage::money::{MicroUsd, NanoUsd};

pub const EXPORT_SCHEMA: &str = "mh.export/1.0.0";

/// The question key of the round's empirical winner.
pub(crate) const BEST_WORKER: &str = "best_worker";

/// The prefix of the per-config quality question keys: `quality:<config_id>`.
pub(crate) const QUALITY_PREFIX: &str = "quality:";

/// The probability key of a `score` answer: the summed judge score goes
/// under it (dataset design 4.10.1).
pub(crate) const SCORE_KEY: &str = "score";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportRow {
    /// [`EXPORT_SCHEMA`].
    pub schema: String,
    /// `systemone/v1`.
    pub api: String,
    pub label_policy_version: String,
    pub experiment_id: ExperimentId,
    pub round_id: RoundId,
    pub task_id: TaskId,
    pub state: ExportState,
    /// `BEST_WORKER`: a choice over the validated candidates' config ids;
    /// `quality:<config_id>`: a score per candidate config.
    pub questions: BTreeMap<String, Question>,
    /// The empirical answers; `null` where the round gives none (abstain,
    /// tie, reject, needs-intervention, no judgment).
    pub answers: BTreeMap<String, Option<Answer>>,
    pub eligible_set: Vec<EligibleEntry>,
    pub planner_propensities: BTreeMap<String, f64>,
    /// `{"id":"none","probabilities":null}` while Clef and Laya are inert.
    pub teacher: TeacherRef,
    /// One per candidate, in label order.
    pub worker_runs: Vec<WorkerRun>,
    /// The winner's config id.
    pub winner: Option<String>,
    /// config id → rubric component → score, as the judge gave them.
    pub component_quality: BTreeMap<String, BTreeMap<String, f64>>,
    /// The candidates' cost, summed in nano-dollars and rounded once.
    pub cost_microusd: MicroUsd,
    /// `round.created` → the event that decided the round.
    pub latency_ms: u64,
    pub outcomes: Vec<OutcomeRecorded>,
    /// From `promotions/<round>.json`, when the round was promoted.
    pub promotion: Option<ExportPromotion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportState {
    /// The experiment's recorded settings, known before the round is
    /// decided: exactly the 4 keys of dataset design 4.10.1.
    pub task_features: BTreeMap<String, Value>,
    pub task_digest: Digest,
    pub repo_digest: Digest,
    pub environment_digest: Digest,
    pub base_sha: String,
}

// B5: typed once PromotionReceipt lands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportPromotion {
    pub dest_after: String,
}

/// Where [`export`] finds the execution part of each WorkerRun. The CLI
/// adapts the execution store to it.
pub trait ExecutionFactsSource {
    fn facts(&self, execution_id: &ExecutionId) -> Option<ExecutionFacts>;
}

/// An [`ExecutionFactsSource`] held in memory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InMemoryFacts {
    facts: BTreeMap<ExecutionId, ExecutionFacts>,
}

impl InMemoryFacts {
    pub fn insert(&mut self, facts: ExecutionFacts) {
        self.facts.insert(facts.execution_id.clone(), facts);
    }
}

impl FromIterator<ExecutionFacts> for InMemoryFacts {
    fn from_iter<I: IntoIterator<Item = ExecutionFacts>>(iter: I) -> Self {
        let mut out = InMemoryFacts::default();
        for f in iter {
            out.insert(f);
        }
        out
    }
}

impl ExecutionFactsSource for InMemoryFacts {
    fn facts(&self, execution_id: &ExecutionId) -> Option<ExecutionFacts> {
        self.facts.get(execution_id).cloned()
    }
}

/// One row per round of `label_policy_version` that reached DECIDED,
/// REJECTED, NEEDS_INTERVENTION or a later state. Only from events,
/// judgments, promotion receipts, the execution facts and outcomes.
/// Sorted by (experiment_id, round_id).
pub fn export(
    paths: &DatasetPaths,
    facts: &dyn ExecutionFactsSource,
    label_policy_version: &str,
) -> Result<Vec<ExportRow>> {
    let events = store::read_all(paths)?.events;
    let Replay {
        projection,
        created_at,
        decided_at,
    } = replay(&events)?;
    let mut rows = projection
        .rounds
        .iter()
        .filter(|(_, r)| {
            r.created.label_policy_version == label_policy_version && is_decided(r.state)
        })
        .map(|(round_id, r)| {
            let latency = decided_at[round_id] - created_at[round_id];
            build_row(
                paths,
                facts,
                &projection,
                &events,
                round_id,
                r,
                u64::try_from(latency.num_milliseconds()).unwrap_or(0),
            )
            .with_context(|| format!("exporting round {round_id}"))
        })
        .collect::<Result<Vec<_>>>()?;
    rows.sort_by(|a, b| (&a.experiment_id, &a.round_id).cmp(&(&b.experiment_id, &b.round_id)));
    Ok(rows)
}

/// The projection after every event, and when each round was created and
/// decided.
struct Replay {
    projection: Projection,
    created_at: BTreeMap<RoundId, DateTime<Utc>>,
    decided_at: BTreeMap<RoundId, DateTime<Utc>>,
}

/// Fold one event at a time to see when each round was decided.
fn replay(events: &[EventEnvelope]) -> Result<Replay> {
    let mut out = Replay {
        projection: Projection::default(),
        created_at: BTreeMap::new(),
        decided_at: BTreeMap::new(),
    };
    for env in events {
        out.projection.apply(env);
        let known = env
            .round_id
            .as_ref()
            .and_then(|id| out.projection.rounds.get(id).map(|r| (id, r.state)));
        if let Some((round, state)) = known {
            if env.kind == "round.created" && !out.created_at.contains_key(round) {
                out.created_at.insert(round.clone(), occurred_at(env)?);
            }
            if is_decided(state) && !out.decided_at.contains_key(round) {
                out.decided_at.insert(round.clone(), occurred_at(env)?);
            }
        }
    }
    Ok(out)
}

/// The rows as JSONL: one canonical JSON object (sorted keys, no
/// whitespace) per line.
pub fn render_jsonl(rows: &[ExportRow]) -> Result<String> {
    let mut out = String::new();
    for row in rows {
        out.push_str(&canonical_json(&serde_json::to_value(row)?));
        out.push('\n');
    }
    Ok(out)
}

/// Write `exports/<label_policy_version>/<ts>.jsonl` once (0600). Every row
/// must carry `label_policy_version`. Returns the file's path.
pub fn write_export(
    paths: &DatasetPaths,
    label_policy_version: &str,
    rows: &[ExportRow],
    ts: DateTime<Utc>,
) -> Result<PathBuf> {
    check_path_component("label policy version", label_policy_version)?;
    if let Some(row) = rows
        .iter()
        .find(|r| r.label_policy_version != label_policy_version)
    {
        bail!(
            "round {} has label policy version '{}', not '{label_policy_version}'",
            row.round_id,
            row.label_policy_version
        );
    }
    let dir = paths.exports_dir(label_policy_version)?;
    fsx::ensure_private_dir(&paths.exports_root())?;
    fsx::ensure_private_dir(&dir)?;
    let file = dir.join(format!("{}.jsonl", ts.format("%Y%m%dT%H%M%S%.3fZ")));
    fsx::create_immutable(&file, render_jsonl(rows)?.as_bytes(), fsx::PRIVATE_FILE)?;
    Ok(file)
}

/// A round in one of these states has its answer, or knows it has none.
fn is_decided(state: RoundState) -> bool {
    use RoundState as S;
    matches!(
        state,
        S::Decided
            | S::Revalidating
            | S::Promoting
            | S::Promoted
            | S::NeedsIntervention
            | S::Rejected
            | S::Cleanup
            | S::Complete
    )
}

fn occurred_at(env: &EventEnvelope) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&env.occurred_at)
        .map(|t| t.with_timezone(&Utc))
        .with_context(|| format!("event {} has a bad occurred_at", env.event_id))
}

/// A value that names a file or a directory must be one plain path
/// component. `RoundId` itself allows `/`, so a forged log could otherwise
/// point the judgment read outside the dataset root.
fn check_path_component(what: &str, s: &str) -> Result<()> {
    let plain = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if s.is_empty() || s == "." || s == ".." || !plain {
        bail!("{what} '{s}' is not a plain name");
    }
    Ok(())
}

/// The judgment of `round_id`, read from its file when the log names one.
fn read_judgment(
    paths: &DatasetPaths,
    round_id: &RoundId,
    id: Option<&JudgmentId>,
) -> Result<Option<Judgment>> {
    let Some(id) = id else { return Ok(None) };
    let file = paths.judgement(round_id)?;
    let text =
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    let record: JudgmentRecord =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", file.display()))?;
    if &record.judgment_id != id || &record.round_id != round_id {
        bail!(
            "{} holds judgment {} of round {}; the log names judgment {id}",
            file.display(),
            record.judgment_id,
            record.round_id
        );
    }
    Ok(Some(record.judgment))
}

/// What the candidates of one round add up to.
#[derive(Default)]
struct RowParts {
    questions: BTreeMap<String, Question>,
    answers: BTreeMap<String, Option<Answer>>,
    worker_runs: Vec<WorkerRun>,
    component_quality: BTreeMap<String, BTreeMap<String, f64>>,
    options: Vec<String>,
    cost: NanoUsd,
}

impl RowParts {
    fn add_candidate(
        &mut self,
        label: &str,
        c: &CandidateView,
        facts: &dyn ExecutionFactsSource,
        judgment: Option<&Judgment>,
        round_events: &[EventEnvelope],
    ) -> Result<()> {
        let planned = c
            .planned
            .as_ref()
            .with_context(|| format!("candidate {label} was never planned"))?;
        let config_id = &planned.config_id;
        let execution_id = c
            .execution_id
            .as_ref()
            .with_context(|| format!("candidate {label} has no execution id"))?;
        let run_facts = facts
            .facts(execution_id)
            .with_context(|| format!("no execution facts for {execution_id}"))?;
        let components = judgment
            .and_then(|j| j.candidates.get(label))
            .map(|a| a.scores.clone());
        let run = WorkerRun::project(&run_facts, round_events, components.clone())?;
        self.cost += NanoUsd(i128::from(run.facts.cost_microusd.0) * 1000);
        self.worker_runs.push(run);

        let key = format!("{QUALITY_PREFIX}{config_id}");
        if self.questions.contains_key(&key) {
            bail!("config {config_id} appears twice in the round");
        }
        self.questions.insert(
            key.clone(),
            Question {
                kind: QuestionKind::Score,
                instructions: format!("How good is the result of {config_id} on this task?"),
                options: Vec::new(),
                criteria: None,
            },
        );
        // The sum exists only in this answer; the components stay separate.
        let answer = components.as_ref().map(|scores| Answer {
            choice: None,
            probabilities: BTreeMap::from([(SCORE_KEY.to_string(), scores.values().sum())]),
            confidence: None,
        });
        self.answers.insert(key, answer);
        if let Some(scores) = components {
            self.component_quality.insert(config_id.clone(), scores);
        }
        if c.is_eligible() {
            self.options.push(config_id.clone());
        }
        Ok(())
    }

    fn add_best_worker(&mut self, winner: &Option<String>) {
        self.options.sort();
        self.questions.insert(
            BEST_WORKER.to_string(),
            Question {
                kind: QuestionKind::Choice,
                instructions: "Which worker configuration gives the best result for this task?"
                    .to_string(),
                options: std::mem::take(&mut self.options),
                criteria: None,
            },
        );
        self.answers.insert(
            BEST_WORKER.to_string(),
            winner.as_ref().map(|w| Answer {
                choice: Some(w.clone()),
                probabilities: BTreeMap::from([(w.clone(), 1.0)]),
                confidence: None,
            }),
        );
    }
}

/// The winner's config id. A later rejection (stale judgment, failed
/// revalidation) voids the winner.
fn winner_config(r: &RoundView) -> Result<Option<String>> {
    match (&r.winner, &r.rejected) {
        (Some(w), None) => Ok(Some(
            r.candidates[&w.label]
                .planned
                .as_ref()
                .with_context(|| format!("winner {} was never planned", w.label))?
                .config_id
                .clone(),
        )),
        _ => Ok(None),
    }
}

fn build_row(
    paths: &DatasetPaths,
    facts: &dyn ExecutionFactsSource,
    projection: &Projection,
    events: &[EventEnvelope],
    round_id: &RoundId,
    r: &RoundView,
    latency_ms: u64,
) -> Result<ExportRow> {
    check_path_component("round id", round_id.as_str())?;
    let experiment = &projection
        .experiments
        .get(&r.experiment_id)
        .with_context(|| format!("unknown experiment {}", r.experiment_id))?
        .created;
    let judgment = read_judgment(paths, round_id, r.judge.judgment_id.as_ref())?;

    // WorkerRun::project folds what it is given: only this round's events.
    let round_events: Vec<EventEnvelope> = events
        .iter()
        .filter(|e| {
            e.experiment_id == r.experiment_id
                && e.round_id.as_ref().is_none_or(|id| id == round_id)
        })
        .cloned()
        .collect();

    let mut parts = RowParts::default();
    for (label, c) in &r.candidates {
        parts.add_candidate(label, c, facts, judgment.as_ref(), &round_events)?;
    }
    let winner = winner_config(r)?;
    parts.add_best_worker(&winner);

    Ok(ExportRow {
        schema: EXPORT_SCHEMA.to_string(),
        api: API.to_string(),
        label_policy_version: r.created.label_policy_version.clone(),
        experiment_id: r.experiment_id.clone(),
        round_id: round_id.clone(),
        task_id: experiment.task_id.clone(),
        state: ExportState {
            task_features: BTreeMap::from([
                (
                    "budget_usd_micro".to_string(),
                    json!(experiment.budget_usd_micro),
                ),
                ("candidates".to_string(), json!(experiment.candidates)),
                ("round_index".to_string(), json!(r.created.index)),
                ("strategy".to_string(), json!(experiment.strategy)),
            ]),
            task_digest: experiment.task_digest,
            repo_digest: experiment.repo_digest,
            environment_digest: experiment.environment_digest,
            base_sha: r.created.base_sha.clone(),
        },
        questions: parts.questions,
        answers: parts.answers,
        eligible_set: r.created.eligible_set.clone(),
        planner_propensities: r.created.propensities.clone(),
        teacher: TeacherRef::none(),
        worker_runs: parts.worker_runs,
        winner,
        component_quality: parts.component_quality,
        cost_microusd: parts.cost.to_micro_half_even(),
        latency_ms,
        outcomes: r.outcomes.clone(),
        promotion: read_promotion(paths, round_id)?,
    })
}

/// `dest_after` of `promotions/<round>.json`, when the file exists.
fn read_promotion(paths: &DatasetPaths, round_id: &RoundId) -> Result<Option<ExportPromotion>> {
    let file = paths.promotion(round_id)?;
    match std::fs::read_to_string(&file) {
        Ok(text) => parse_promotion(&file, &text).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", file.display())),
    }
}

fn parse_promotion(file: &Path, text: &str) -> Result<ExportPromotion> {
    let receipt: Value =
        serde_json::from_str(text).with_context(|| format!("parsing {}", file.display()))?;
    let dest_after = receipt
        .get("dest_after")
        .and_then(Value::as_str)
        .with_context(|| format!("{} has no dest_after", file.display()))?;
    Ok(ExportPromotion {
        dest_after: dest_after.to_string(),
    })
}
