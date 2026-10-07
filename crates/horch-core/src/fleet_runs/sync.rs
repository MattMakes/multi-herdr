//! Sync: a run row for every finished record that has none (fleet-dataset
//! §4.2).
//!
//! It covers the records that ended without `horch done` (pane closed,
//! launch failed, abandoned) and those whose `horch done` could not write.
//! A transcript is read only for a record that gets a new row, so a sync
//! with nothing to do reads the ledger and `runs.jsonl` only. `max_new`
//! bounds the rows of one sync: a backfill of a long ledger reads hundreds
//! of transcripts (240 took 31 s in a debug build), and `horch spawn` must
//! stay quick. The oldest records go first: their transcripts expire first.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::execution::legacy::LedgerRecordV1;
use crate::fleet_runs::facts::{build_run_row, latest_starts, run_ids, skip_reason, RunSources};
use crate::fleet_runs::rows::WrittenBy;
use crate::fleet_runs::store::{self, FleetFile};
use crate::measure::paths::DatasetPaths;

/// What one sync did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Run rows written now.
    pub written: usize,
    /// Rows written now that carry tokens.
    pub with_tokens: usize,
    /// Rows written now whose `cost_source` is `unpriced`.
    pub unpriced: usize,
    /// Records that already had a row.
    pub already: usize,
    /// Records that need a row and wait for a later sync (`max_new`).
    pub deferred: usize,
    /// Records that get no row, by reason.
    pub skipped: BTreeMap<&'static str, usize>,
}

impl SyncReport {
    /// Every record that got no new row and had none.
    pub fn skipped_total(&self) -> usize {
        self.skipped.values().sum()
    }
}

/// Write a `sync` run row for every finished record of `records` that has
/// none, at most `max_new` of them, oldest first. Candidate, judge and
/// unfinished records are skipped and counted.
pub fn sync_project(
    paths: &DatasetPaths,
    project: &Path,
    records: &[LedgerRecordV1],
    sources: &RunSources,
    now: DateTime<Utc>,
    max_new: usize,
) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    let have = run_ids(paths)?;
    let mut pending = Vec::new();
    for record in records {
        if let Some(reason) = skip_reason(record) {
            *report.skipped.entry(reason).or_default() += 1;
        } else if have.contains(&record.record_id) {
            report.already += 1;
        } else {
            pending.push(record);
        }
    }
    pending.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    report.deferred = pending.len().saturating_sub(max_new);
    pending.truncate(max_new);
    if pending.is_empty() {
        return Ok(report);
    }
    let starts = latest_starts(paths)?;
    // The transcripts are read before the lock is taken.
    let rows: Vec<_> = pending
        .into_iter()
        .map(|r| {
            let start = starts.get(&r.record_id);
            build_run_row(r, start, sources, project, now, WrittenBy::Sync)
        })
        .collect();
    let _guard = store::lock(paths)?;
    let mut have = run_ids(paths)?;
    for row in rows {
        if !have.insert(row.record_id.clone()) {
            report.already += 1;
            continue;
        }
        store::append_locked(paths, FleetFile::Runs, &row)?;
        report.written += 1;
        report.with_tokens += usize::from(row.tokens.is_some());
        report.unpriced += usize::from(row.cost_source == "unpriced");
    }
    Ok(report)
}
