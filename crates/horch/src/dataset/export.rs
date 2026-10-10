//! `multi-herdr-dataset export`: the decided rounds as JSONL (EXP-03,
//! EXP-04), with the execution store adapted as the facts source, and the
//! fleet rows (fleet-dataset §6).

use std::collections::BTreeMap;

use anyhow::Result;
use horch_core::clock;
use horch_core::competition::observe::{load_usage_records, UsageRecord};
use horch_core::competition::planner::LABEL_POLICY_VERSION;
use horch_core::dataset::export::{self as core_export, ExecutionFactsSource, ExportRow};
use horch_core::dataset::fleet;
use horch_core::execution::legacy::LedgerRecordV1;
use horch_core::execution::store::ExecutionStore;
use horch_core::execution::ExecutionStatus;
use horch_core::ids::{ExecutionId, SessionId};
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::worker_run::ExecutionFacts;
use horch_core::runtime::RuntimeContext;
use horch_core::usage::money::{CostSource, MicroUsd};

use super::{dataset_paths, exit};

/// Sync the fleet run rows first (FDS-18), then write the decided rounds and
/// the fleet rows (`exports/fleet-observed-1/`) with one timestamp.
pub(crate) fn export(ctx: &RuntimeContext, label_policy: Option<&str>) -> Result<u8> {
    super::fleet::sync_first(ctx);
    let paths = dataset_paths(ctx)?;
    let lpv = label_policy.unwrap_or(LABEL_POLICY_VERSION);
    let rows = rows(ctx, &paths, lpv)?;
    let now = clock::now();
    let file = core_export::write_export(&paths, lpv, &rows, now)?;
    println!("exported {} rows to {}", rows.len(), file.display());
    let fleet_rows = fleet::fleet_rows(&paths)?;
    let file = fleet::write_fleet_export(&paths, &fleet_rows, now)?;
    println!(
        "exported {} fleet rows to {}",
        fleet_rows.len(),
        file.display()
    );
    Ok(exit::SUCCESS)
}

/// Every exportable row of `label_policy_version`.
pub(crate) fn rows(
    ctx: &RuntimeContext,
    paths: &DatasetPaths,
    label_policy_version: &str,
) -> Result<Vec<ExportRow>> {
    let project = ctx.paths.project()?;
    let store = ExecutionStore::for_project(&ctx.paths.state_root, &project.to_string_lossy());
    let facts = StoreFacts {
        records: store.read()?,
        usage: load_usage_records(paths),
    };
    core_export::export(paths, &facts, label_policy_version)
}

/// The execution store as an [`ExecutionFactsSource`].
pub(crate) struct StoreFacts {
    pub records: Vec<LedgerRecordV1>,
    /// The coordinator's usage records, by execution id.
    pub usage: BTreeMap<String, UsageRecord>,
}

impl ExecutionFactsSource for StoreFacts {
    fn facts(&self, execution_id: &ExecutionId) -> Option<ExecutionFacts> {
        let r = self
            .records
            .iter()
            .find(|r| r.record_id == execution_id.as_str())?;
        Some(facts_of(
            execution_id,
            r,
            self.usage.get(execution_id.as_str()),
        ))
    }
}

/// The execution part of a WorkerRun, from one record and the usage record
/// the coordinator's meter wrote when the candidate was frozen. A run
/// without a usage record is `Unpriced` with zero tokens.
pub(crate) fn facts_of(
    execution_id: &ExecutionId,
    r: &LedgerRecordV1,
    usage: Option<&UsageRecord>,
) -> ExecutionFacts {
    let status = r.state.clone().unwrap_or(if r.status == "done" {
        ExecutionStatus::Done
    } else {
        ExecutionStatus::Running
    });
    ExecutionFacts {
        execution_id: execution_id.clone(),
        status,
        session_id: r.session_id.as_deref().and_then(|s| SessionId::new(s).ok()),
        transcript_ref: usage.and_then(|u| u.transcript_ref.clone()),
        transcript_digest: usage.and_then(|u| u.transcript_digest),
        started_at: r.created_at.clone(),
        finished_at: r.finished_at.clone(),
        tokens: usage.map(|u| u.tokens).unwrap_or_default(),
        cost_microusd: usage.map_or(MicroUsd(0), |u| u.cost_microusd),
        cost_source: usage
            .and_then(|u| u.cost_source.parse().ok())
            .unwrap_or(CostSource::Unpriced),
        skills: r.skills.clone(),
        routing: r.routing.clone(),
    }
}
