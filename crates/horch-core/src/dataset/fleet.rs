//! Fleet rows in the export and the readiness report (fleet-dataset §6).
//!
//! [`fleet_rows`] turns `fleet/runs.jsonl` and `fleet/verdicts.jsonl` into
//! one System-One shaped [`FleetExportRow`] per worker record:
//! `exports/fleet-observed-1/<ts>.jsonl` next to the round export.
//! [`fleet_readiness`] counts them for the `fleet observed:` section that
//! `readiness` prints after its round report. The round rows, the readiness
//! verdict and the Clef gaps never read a fleet row.
//!
//! Recording informs, it never decides (FD5): nothing that routes or picks
//! a teammate reads this module.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::competition::planner::config_id;
use crate::execution::legacy::KIND_WORKER;
use crate::fleet_runs::rows::{RunRow, RunTokens, VerdictRow};
use crate::fleet_runs::store::{read_rows, FleetFile};
use crate::fsx;
use crate::measure::digest::canonical_json;
use crate::measure::paths::DatasetPaths;
use crate::teacher::system_one::{Answer, Question, QuestionKind, API};
use crate::teacher::TeacherRef;

/// `schema` of a fleet export row.
pub const FLEET_EXPORT_SCHEMA: &str = "mh.export-fleet/1.0.0";

/// The label policy of the fleet rows, and their export directory.
pub const FLEET_LABEL_POLICY: &str = "fleet-observed-1";

/// The question key of the orchestrator's verdict.
pub const VERDICT_QUESTION: &str = "verdict";

/// The 3 verdict words, in the order the question lists them.
pub const VERDICTS: [&str; 3] = ["accepted", "rework", "rejected"];

/// One worker record as a System-One row (§6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FleetExportRow {
    /// `systemone/v1`.
    pub api: String,
    /// [`FLEET_EXPORT_SCHEMA`].
    pub schema: String,
    /// [`FLEET_LABEL_POLICY`].
    pub label_policy_version: String,
    /// The record's `task_digest`.
    pub task_id: String,
    pub state: FleetState,
    /// [`VERDICT_QUESTION`]: a choice over [`VERDICTS`].
    pub questions: BTreeMap<String, Question>,
    /// The latest verdict; `null` when the record has none.
    pub answers: BTreeMap<String, Option<Answer>>,
    pub config: FleetConfig,
    pub facts: FleetFacts,
    /// `{"id":"none","probabilities":null}` while Clef and Laya are inert.
    pub teacher: TeacherRef,
}

/// What was known about the task before the worker ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetState {
    pub task_digest: String,
    pub plan_digest: Option<String>,
    pub task_features: FleetTaskFeatures,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetTaskFeatures {
    pub phase: Option<String>,
    /// The ledger `routing.mode`.
    pub routing_mode: Option<String>,
    pub resumed: bool,
}

/// The worker configuration of the record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetConfig {
    /// `<teammate>|<harness>|<model>|<effort or ->`, as a round spells it.
    pub config_id: String,
    pub teammate: String,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// The latest segment's run row, with the totals over every segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FleetFacts {
    /// The latest segment row. Its `tokens` and `cost_microusd` are the
    /// sums over the distinct sessions of the record.
    #[serde(flatten)]
    pub run: RunRow,
    /// The record's segment count.
    pub segments: u32,
    /// The sum of the segment durations.
    pub active_ms: i64,
}

/// The export rows of the fleet store: 1 per worker record, oldest first.
pub fn fleet_rows(paths: &DatasetPaths) -> Result<Vec<FleetExportRow>> {
    let runs = read_rows::<RunRow>(paths, FleetFile::Runs)?.rows;
    let verdicts = read_rows::<VerdictRow>(paths, FleetFile::Verdicts)?.rows;
    Ok(build_fleet_rows(&runs, &verdicts))
}

/// [`fleet_rows`] of rows in file order. The last verdict row of a record
/// counts; a segment with 2 rows keeps its last.
pub fn build_fleet_rows(runs: &[RunRow], verdicts: &[VerdictRow]) -> Vec<FleetExportRow> {
    let mut records: BTreeMap<&str, BTreeMap<u32, &RunRow>> = BTreeMap::new();
    for run in runs.iter().filter(|r| r.kind == KIND_WORKER) {
        records
            .entry(run.record_id.as_str())
            .or_default()
            .insert(run.segment, run);
    }
    let latest_verdict: BTreeMap<&str, &str> = verdicts
        .iter()
        .map(|v| (v.record_id.as_str(), v.verdict.as_str()))
        .collect();
    let mut rows: Vec<(String, FleetExportRow)> = records
        .into_values()
        .filter_map(|segments| {
            let first = segments.values().next()?.started_at.clone();
            let verdict = latest_verdict
                .get(segments.values().next()?.record_id.as_str())
                .copied();
            Some((first, export_row(&segments, verdict)?))
        })
        .collect();
    rows.sort_by(|a, b| (&a.0, &a.1.facts.run.record_id).cmp(&(&b.0, &b.1.facts.run.record_id)));
    rows.into_iter().map(|(_, row)| row).collect()
}

/// The row of one record from its segment rows, by segment.
fn export_row(segments: &BTreeMap<u32, &RunRow>, verdict: Option<&str>) -> Option<FleetExportRow> {
    let latest = *segments.values().next_back()?;
    // The latest row of each session: a resumed session's transcript holds
    // the earlier segments' tokens too.
    let mut sessions: BTreeMap<Option<&str>, &RunRow> = BTreeMap::new();
    for row in segments.values() {
        sessions.insert(row.session_id.as_deref(), row);
    }
    let mut run = latest.clone();
    run.tokens = sum_tokens(sessions.values().filter_map(|r| r.tokens));
    run.cost_microusd = sessions
        .values()
        .filter_map(|r| r.cost_microusd)
        .reduce(|a, b| a + b);
    let facts = FleetFacts {
        segments: u32::try_from(segments.len()).unwrap_or(u32::MAX),
        active_ms: segments.values().map(|r| r.duration_ms).sum(),
        run,
    };
    let routing_mode = facts
        .run
        .routing
        .as_ref()
        .and_then(|r| r.get("mode"))
        .and_then(|m| m.as_str())
        .map(str::to_owned);
    let question = Question {
        kind: QuestionKind::Choice,
        instructions: "Did the orchestrator accept this worker's result?".to_string(),
        options: VERDICTS.iter().map(|v| v.to_string()).collect(),
        criteria: None,
    };
    let answer = verdict.map(|v| Answer {
        choice: Some(v.to_string()),
        probabilities: BTreeMap::from([(v.to_string(), 1.0)]),
        confidence: None,
    });
    let r = &facts.run;
    Some(FleetExportRow {
        api: API.to_string(),
        schema: FLEET_EXPORT_SCHEMA.to_string(),
        label_policy_version: FLEET_LABEL_POLICY.to_string(),
        task_id: r.task_digest.clone(),
        state: FleetState {
            task_digest: r.task_digest.clone(),
            plan_digest: r.plan_digest.clone(),
            task_features: FleetTaskFeatures {
                phase: r.phase.clone(),
                routing_mode,
                resumed: r.resumed,
            },
        },
        questions: BTreeMap::from([(VERDICT_QUESTION.to_string(), question)]),
        answers: BTreeMap::from([(VERDICT_QUESTION.to_string(), answer)]),
        config: FleetConfig {
            config_id: fleet_config_id(r),
            teammate: r.teammate.clone(),
            harness: r.harness.clone(),
            model: r.model.clone(),
            effort: r.effort.clone(),
        },
        teacher: TeacherRef::none(),
        facts,
    })
}

/// The sum of `tokens`; `None` when there are none.
fn sum_tokens(tokens: impl Iterator<Item = RunTokens>) -> Option<RunTokens> {
    tokens.reduce(|a, b| RunTokens {
        input: a.input + b.input,
        output: a.output + b.output,
        cache_read: a.cache_read + b.cache_read,
        cache_write_5m: a.cache_write_5m + b.cache_write_5m,
        cache_write_1h: a.cache_write_1h + b.cache_write_1h,
    })
}

/// The round export's config id of a run row. A value the planner's types
/// refuse (an old record, harness `none` without a model) is spelled as is,
/// with `-` for a missing one.
fn fleet_config_id(r: &RunRow) -> String {
    let harness = r.harness.as_deref().unwrap_or("-");
    let model = r.model.as_deref().unwrap_or("-");
    match (r.teammate.parse(), harness.parse(), model.parse()) {
        (Ok(teammate), Ok(harness), Ok(model)) => {
            config_id(&teammate, harness, &model, r.effort.as_deref())
        }
        _ => format!(
            "{}|{harness}|{model}|{}",
            r.teammate,
            r.effort.as_deref().unwrap_or("-")
        ),
    }
}

/// The rows as JSONL: one canonical JSON object per line, as the round
/// export writes them.
pub fn render_fleet_jsonl(rows: &[FleetExportRow]) -> Result<String> {
    let mut out = String::new();
    for row in rows {
        out.push_str(&canonical_json(&serde_json::to_value(row)?));
        out.push('\n');
    }
    Ok(out)
}

/// Write `exports/fleet-observed-1/<ts>.jsonl` once (0600), with the round
/// export's name format. Returns the file's path.
pub fn write_fleet_export(
    paths: &DatasetPaths,
    rows: &[FleetExportRow],
    ts: DateTime<Utc>,
) -> Result<PathBuf> {
    let dir = paths.exports_dir(FLEET_LABEL_POLICY)?;
    fsx::ensure_private_dir(&paths.exports_root())?;
    fsx::ensure_private_dir(&dir)?;
    let file = dir.join(format!("{}.jsonl", ts.format("%Y%m%dT%H%M%S%.3fZ")));
    fsx::create_immutable(
        &file,
        render_fleet_jsonl(rows)?.as_bytes(),
        fsx::PRIVATE_FILE,
    )?;
    Ok(file)
}

/// The runs and the median duration of one config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetConfigCoverage {
    pub runs: u32,
    /// The median of the records' `active_ms`.
    pub median_duration_ms: i64,
}

/// The counts of the `fleet observed:` section (§6).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetReadiness {
    /// Fleet export rows: worker records with a run row.
    pub rows: u32,
    pub with_verdict: u32,
    /// Distinct `task_digest`s.
    pub distinct_tasks: u32,
    /// Verdict word → rows whose latest verdict it is.
    pub verdicts: BTreeMap<String, u32>,
    /// `config_id` → coverage.
    pub configs: BTreeMap<String, FleetConfigCoverage>,
}

/// Count the fleet export rows.
pub fn fleet_readiness(rows: &[FleetExportRow]) -> FleetReadiness {
    let mut report = FleetReadiness {
        rows: count(rows.len()),
        ..FleetReadiness::default()
    };
    let mut tasks = BTreeSet::new();
    let mut durations: BTreeMap<&str, Vec<i64>> = BTreeMap::new();
    for row in rows {
        tasks.insert(row.state.task_digest.as_str());
        let choice = row
            .answers
            .get(VERDICT_QUESTION)
            .and_then(Option::as_ref)
            .and_then(|a| a.choice.as_deref());
        if let Some(choice) = choice {
            report.with_verdict += 1;
            *report.verdicts.entry(choice.to_string()).or_default() += 1;
        }
        durations
            .entry(row.config.config_id.as_str())
            .or_default()
            .push(row.facts.active_ms);
    }
    report.distinct_tasks = count(tasks.len());
    report.configs = durations
        .into_iter()
        .map(|(id, mut ms)| {
            ms.sort_unstable();
            let mid = ms.len() / 2;
            let median = if ms.len() % 2 == 1 {
                ms[mid]
            } else {
                (ms[mid - 1] + ms[mid]) / 2
            };
            let coverage = FleetConfigCoverage {
                runs: count(ms.len()),
                median_duration_ms: median,
            };
            (id.to_string(), coverage)
        })
        .collect();
    report
}

/// The `fleet observed:` section that `readiness` prints after its round
/// report. Every verdict word is listed, also at 0.
pub fn render_fleet_readiness(report: &FleetReadiness) -> String {
    let mix: Vec<String> = VERDICTS
        .iter()
        .map(|v| format!("{v} {}", report.verdicts.get(*v).copied().unwrap_or(0)))
        .collect();
    let mut out = format!(
        "fleet observed:\n  rows: {}\n  rows with a verdict: {}\n  distinct tasks: {}\n  verdicts: {}\n  configs: {}\n",
        report.rows,
        report.with_verdict,
        report.distinct_tasks,
        mix.join(", "),
        report.configs.len()
    );
    for (id, c) in &report.configs {
        out.push_str(&format!(
            "    {id} runs {}, median duration_ms {}\n",
            c.runs, c.median_duration_ms
        ));
    }
    out
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
