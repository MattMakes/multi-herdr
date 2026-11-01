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

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::execution::legacy::LedgerRecordV1;
use crate::fleet_runs::facts::{
    build_run_row, latest_starts, run_ids, run_key, skip_reason, RunKey, RunSources,
};
use crate::fleet_runs::rows::{RunRow, WrittenBy};
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

/// Write a `sync` run row for every finished record of `records` whose
/// current segment has none, at most `max_new` of them, oldest first. Candidate, judge and
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
    let mut pending = pending_records(records, &have, &mut report);
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
    append_new_rows(paths, rows, &mut report).map(|()| report)
}

/// The records that need a row. A record that gets none, or has one, is
/// counted in `report`.
fn pending_records<'a>(
    records: &'a [LedgerRecordV1],
    have: &BTreeSet<RunKey>,
    report: &mut SyncReport,
) -> Vec<&'a LedgerRecordV1> {
    let mut pending = Vec::new();
    for record in records {
        if let Some(reason) = skip_reason(record) {
            *report.skipped.entry(reason).or_default() += 1;
        } else if have.contains(&run_key(record)) {
            report.already += 1;
        } else {
            pending.push(record);
        }
    }
    pending
}

/// Append `rows` under the fleet lock. The keys are read again under the
/// lock: another writer can add a row after the first read.
fn append_new_rows(paths: &DatasetPaths, rows: Vec<RunRow>, report: &mut SyncReport) -> Result<()> {
    let _guard = store::lock(paths)?;
    let mut have = run_ids(paths)?;
    for row in rows {
        if have.insert((row.record_id.clone(), row.segment)) {
            store::append_locked(paths, FleetFile::Runs, &row)?;
            report.written += 1;
            report.with_tokens += usize::from(row.tokens.is_some());
            report.unpriced += usize::from(row.cost_source == "unpriced");
        } else {
            report.already += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleet_runs::facts::tests::run_row;

    fn paths() -> (tempfile::TempDir, DatasetPaths) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DatasetPaths::new(&tmp.path().join("state"), &tmp.path().join("proj"));
        (tmp, paths)
    }

    /// A row added by another writer after the first read counts as
    /// already recorded, and a repeated key in `rows` is written once.
    #[test]
    fn append_new_rows_counts_rows_found_under_the_lock() {
        let (_tmp, paths) = paths();
        store::append(&paths, FleetFile::Runs, &run_row("r-old", 1)).unwrap();
        let mut report = SyncReport::default();
        let rows = vec![
            run_row("r-old", 1),
            run_row("r-new", 1),
            run_row("r-new", 1),
        ];
        append_new_rows(&paths, rows, &mut report).unwrap();
        assert_eq!((report.written, report.already), (1, 2));
        assert_eq!((report.with_tokens, report.unpriced), (0, 1));
        let ids: Vec<_> = run_ids(&paths).unwrap().into_iter().collect();
        assert_eq!(ids, [("r-new".to_string(), 1), ("r-old".to_string(), 1)]);
    }

    #[test]
    fn pending_records_triages_each_record_once() {
        let rec = |id: &str, state: &str| -> LedgerRecordV1 {
            serde_json::from_value(serde_json::json!({
                "record_id": id, "agent": "claude", "tier": "t", "model": "m",
                "effort": "high", "phase": "implementation", "role": "w", "status": "done",
                "task": "t", "history": [], "created_at": "2026-10-07T10:00:00Z",
                "updated_at": "2026-10-07T10:00:00Z", "state": {"state": state}
            }))
            .unwrap()
        };
        let records = [
            rec("r-new", "done"),
            rec("r-have", "done"),
            rec("r-run", "running"),
        ];
        let have = BTreeSet::from([("r-have".to_string(), 1)]);
        let mut report = SyncReport::default();
        let pending = pending_records(&records, &have, &mut report);
        let ids: Vec<_> = pending.iter().map(|r| r.record_id.as_str()).collect();
        assert_eq!(ids, ["r-new"]);
        assert_eq!(report.already, 1);
        assert_eq!(report.skipped.get("not finished"), Some(&1));
    }
}
