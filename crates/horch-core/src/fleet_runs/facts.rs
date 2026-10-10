//! The start row and the run row (fleet-dataset §3.1, §3.2, §4).
//!
//! The plan rule and the row builders are pure apart from what they read:
//! the plan file, `git rev-parse HEAD` and the session transcript. The run
//! row prices tokens with the code that prices a dataset candidate
//! ([`TelemetryUsage`], [`UsageMeter`]) and keeps the transcript's path and
//! digest, never a copy (SEC-03).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::competition::budget::{UsageMeter, UsageSource};
use crate::competition::observe::TelemetryUsage;
use crate::execution::legacy::{LedgerRecordV1, KIND_ORCHESTRATOR, KIND_WORKER};
use crate::execution::store::slug;
use crate::fleet_runs::rows::{
    RunEnd, RunRow, RunTokens, SkillExpected, StartRow, WrittenBy, RUN_SCHEMA, START_SCHEMA,
};
use crate::fleet_runs::store::{self, FleetFile};
use crate::measure::digest::sha256_bytes;
use crate::measure::paths::DatasetPaths;
use crate::vcs::git::GitClient;

/// The most plan text a start row keeps: 64 KiB.
pub const PLAN_TEXT_CAP: usize = 65_536;

/// What a token loses at its end before the `.md` check.
const TRAILING: &[char] = &['.', ',', ';', ':', ')', '"', '\''];

/// What the run row reads besides the ledger record: the transcripts, the
/// price table and git.
pub struct RunSources<'a> {
    pub usage: &'a TelemetryUsage,
    pub meter: &'a UsageMeter,
    pub git: &'a dyn GitClient,
}

/// The plan file a task names (§3.1): the first whitespace-separated token
/// that, without trailing `.,;:)"'`, ends in `.md` and names an existing
/// regular file, absolute or relative to `project`. The path is relative to
/// `project` when it is inside it.
pub fn plan_file(task: &str, project: &Path) -> Option<PathBuf> {
    task.split_whitespace()
        .map(|token| token.trim_end_matches(TRAILING))
        .filter(|token| token.ends_with(".md"))
        .find(|token| project.join(token).is_file())
        .map(|token| {
            let path = Path::new(token);
            path.strip_prefix(project).unwrap_or(path).to_path_buf()
        })
}

/// `text` cut to at most `cap` bytes on a UTF-8 boundary, and whether it
/// was cut.
fn cap_utf8(text: &str, cap: usize) -> (&str, bool) {
    if text.len() <= cap {
        return (text, false);
    }
    let mut end = cap;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

/// `git rev-parse HEAD` in `project`; `None` outside a repository or
/// before the first commit.
fn head_sha(git: &dyn GitClient, project: &Path) -> Option<String> {
    git.head(project).ok()
}

/// The start row of one spawn: the base commit and the plan file of `task`.
pub fn start_row(
    project: &Path,
    git: &dyn GitClient,
    record_id: &str,
    task: &str,
    now: DateTime<Utc>,
) -> StartRow {
    let plan = plan_file(task, project)
        .and_then(|rel| std::fs::read(project.join(&rel)).ok().map(|b| (rel, b)));
    let (plan_path, plan_digest, plan_text, plan_truncated) = match plan {
        Some((rel, bytes)) => {
            let text = String::from_utf8_lossy(&bytes);
            let (kept, cut) = cap_utf8(&text, PLAN_TEXT_CAP);
            (
                Some(rel.to_string_lossy().into_owned()),
                Some(sha256_bytes(&bytes).to_string()),
                Some(kept.to_string()),
                cut,
            )
        }
        None => (None, None, None, false),
    };
    StartRow {
        schema: START_SCHEMA.to_string(),
        record_id: record_id.to_string(),
        at: crate::clock::stamp(now),
        base_sha: head_sha(git, project),
        plan_path,
        plan_digest,
        plan_text,
        plan_truncated,
    }
}

/// Build and append the start row of one spawn.
pub fn record_start(
    paths: &DatasetPaths,
    project: &Path,
    git: &dyn GitClient,
    record_id: &str,
    task: &str,
    now: DateTime<Utc>,
) -> Result<StartRow> {
    let row = start_row(project, git, record_id, task, now);
    store::append(paths, FleetFile::Starts, &row)?;
    Ok(row)
}

/// Whether a record belongs to a dataset round: a candidate or a judge. The
/// ledger `kind` of both is `worker`; `experiment_id`, `round_id` and
/// `label` tell them apart (`execution::store::kind_of`). Such records get
/// no run row (§4.3).
pub fn is_round_record(record: &LedgerRecordV1) -> bool {
    record.experiment_id.is_some() || record.round_id.is_some() || record.label.is_some()
}

/// Whether a record has ended: its `status` is not `working` and its state
/// is terminal (§4.2).
pub fn is_finished(record: &LedgerRecordV1) -> bool {
    record.status != crate::execution::legacy::STATUS_WORKING
        && record.execution_status().is_terminal()
}

/// Why a record gets no run row, or `None` when it gets one.
pub fn skip_reason(record: &LedgerRecordV1) -> Option<&'static str> {
    if is_round_record(record) {
        Some("candidate or judge record")
    } else if !is_finished(record) {
        Some("not finished")
    } else if record.kind != KIND_WORKER && record.kind != KIND_ORCHESTRATOR {
        Some("unknown kind")
    } else {
        None
    }
}

/// The history event of a resume (`execution::plan`, `Ledger::resume`).
const RESUMED: &str = "resumed";

/// The segment of a record (§3.2): 1 for its first run, 1 more for each
/// resume history event.
pub fn segment(record: &LedgerRecordV1) -> u32 {
    1 + record.history.iter().filter(|h| h.event == RESUMED).count() as u32
}

/// When the record's current segment started: `created_at` for segment 1,
/// else the time of the last resume history event.
fn segment_start(record: &LedgerRecordV1) -> &str {
    record
        .history
        .iter()
        .rfind(|h| h.event == RESUMED)
        .map_or(record.created_at.as_str(), |h| h.at.as_str())
}

/// Milliseconds from `start` to `end`; 0 when either does not parse.
fn duration_ms(start: &str, end: &str) -> i64 {
    match (crate::clock::parse(start), crate::clock::parse(end)) {
        (Some(s), Some(e)) => (e - s).num_milliseconds(),
        _ => 0,
    }
}

/// A value as the plain string serde gives it (`Phase` is `"plan"`).
fn serde_text<T: serde::Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
}

/// The run row of a finished record (§3.2). `start` is the record's start
/// row, when it has one.
pub fn build_run_row(
    record: &LedgerRecordV1,
    start: Option<&StartRow>,
    sources: &RunSources,
    project: &Path,
    now: DateTime<Utc>,
    written_by: WrittenBy,
) -> RunRow {
    let project_text = record
        .project
        .clone()
        .unwrap_or_else(|| project.to_string_lossy().into_owned());
    let session = record.session_id.as_deref().filter(|s| !s.is_empty());
    let harness = record.agent.as_str();
    let usage = sources.usage.usage(harness, session);
    let (tokens, cost_microusd, cost_source) = match &usage {
        Some(usage) => {
            let t = usage.tokens();
            let (cost, source) = sources.meter.price(usage);
            let tokens = RunTokens {
                input: t.input,
                output: t.output,
                cache_read: t.cache_read,
                cache_write_5m: t.cache_write_5m,
                cache_write_1h: t.cache_write_1h,
            };
            (Some(tokens), Some(cost.0), source.to_string())
        }
        None => (None, None, "unpriced".to_string()),
    };
    let transcript = usage
        .as_ref()
        .and_then(|_| sources.usage.transcript(harness, session));
    let transcript_digest = transcript
        .as_ref()
        .and_then(|p| std::fs::read(p).ok())
        .map(|b| sha256_bytes(&b).to_string());
    let finished_at = record
        .finished_at
        .clone()
        .unwrap_or_else(|| record.updated_at.clone());
    let started_at = segment_start(record).to_string();
    let text_or_none = |s: &str| (!s.is_empty()).then(|| s.to_string());
    RunRow {
        schema: RUN_SCHEMA.to_string(),
        record_id: record.record_id.clone(),
        session_id: record.session_id.clone(),
        role: record.role.clone(),
        segment: segment(record),
        kind: record.kind.clone(),
        project: project_text.clone(),
        project_slug: slug(&project_text),
        teammate: record.tier.clone(),
        harness: text_or_none(&record.agent),
        model: text_or_none(&record.model),
        effort: record.effort.clone(),
        phase: record.phase.as_ref().and_then(serde_text),
        routing: record
            .routing
            .as_ref()
            .and_then(|r| serde_json::to_value(r).ok()),
        substitution_reason: record.substitution_reason.clone(),
        skills_expected: record
            .skills
            .iter()
            .map(|s| SkillExpected {
                id: s.id.to_string(),
                version: s.version.0.clone(),
                digest: s.digest.to_string(),
            })
            .collect(),
        task: record.task.clone(),
        task_digest: sha256_bytes(record.task.as_bytes()).to_string(),
        plan_path: start.and_then(|s| s.plan_path.clone()),
        plan_digest: start.and_then(|s| s.plan_digest.clone()),
        base_sha: start.and_then(|s| s.base_sha.clone()),
        duration_ms: duration_ms(&started_at, &finished_at),
        started_at,
        finished_at,
        end: RunEnd {
            status: record.status.clone(),
            state: record
                .state
                .as_ref()
                .and_then(|s| serde_json::to_value(s).ok()),
            exit_code: record.exit_code,
        },
        done_summary: record
            .history
            .iter()
            .rfind(|h| h.event == "done")
            .map(|h| h.text.clone()),
        notes: record.history.iter().filter(|h| h.event == "note").count() as u32,
        resumed: record.history.iter().any(|h| h.event == RESUMED),
        tokens,
        cost_microusd,
        cost_source,
        transcript_ref: transcript.map(|p| p.to_string_lossy().into_owned()),
        transcript_digest,
        usage_at: crate::clock::stamp(now),
        head_sha: head_sha(sources.git, project),
        written_by,
        horch_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// The newest start row of each record.
pub fn latest_starts(paths: &DatasetPaths) -> Result<std::collections::BTreeMap<String, StartRow>> {
    let starts = store::read_rows::<StartRow>(paths, FleetFile::Starts)?;
    Ok(starts
        .rows
        .into_iter()
        .map(|s| (s.record_id.clone(), s))
        .collect())
}

/// The key of a run row: its record and segment.
pub type RunKey = (String, u32);

/// The `(record_id, segment)` pairs that have a run row. A row without a
/// `segment` is segment 1.
pub fn run_ids(paths: &DatasetPaths) -> Result<BTreeSet<RunKey>> {
    Ok(
        store::read_rows::<serde_json::Value>(paths, FleetFile::Runs)?
            .rows
            .into_iter()
            .filter_map(|v| {
                let id = v.get("record_id")?.as_str()?.to_owned();
                let segment = v.get("segment").and_then(|s| s.as_u64()).unwrap_or(1);
                Some((id, u32::try_from(segment).ok()?))
            })
            .collect(),
    )
}

/// The run key of a record's current segment.
pub fn run_key(record: &LedgerRecordV1) -> RunKey {
    (record.record_id.clone(), segment(record))
}

/// What [`record_run`] did.
#[derive(Debug, Clone, PartialEq)]
pub enum RunWrite {
    Written(Box<RunRow>),
    /// `runs.jsonl` already has a row for the record's segment. Nothing
    /// was written.
    AlreadyRecorded,
    /// The record gets no row; the reason says why.
    Skipped(&'static str),
}

/// Write the run row of `record`, once per `(record_id, segment)`: the
/// check runs under the fleet lock.
pub fn record_run(
    paths: &DatasetPaths,
    project: &Path,
    record: &LedgerRecordV1,
    sources: &RunSources,
    now: DateTime<Utc>,
    written_by: WrittenBy,
) -> Result<RunWrite> {
    if let Some(reason) = skip_reason(record) {
        return Ok(RunWrite::Skipped(reason));
    }
    let key = run_key(record);
    // Cheap check first: the transcript is read only for a new row.
    if run_ids(paths)?.contains(&key) {
        return Ok(RunWrite::AlreadyRecorded);
    }
    let start = latest_starts(paths)?.remove(&record.record_id);
    let row = build_run_row(record, start.as_ref(), sources, project, now, written_by);
    let _guard = store::lock(paths)?;
    if run_ids(paths)?.contains(&key) {
        return Ok(RunWrite::AlreadyRecorded);
    }
    store::append_locked(paths, FleetFile::Runs, &row)?;
    Ok(RunWrite::Written(Box::new(row)))
}
