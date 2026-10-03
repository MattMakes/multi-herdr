//! `multi-herdr-dataset export`: the decided rounds as JSONL (EXP-03,
//! EXP-04), with the execution store adapted as the facts source.

use anyhow::Result;
use horch_core::clock;
use horch_core::competition::planner::LABEL_POLICY_VERSION;
use horch_core::dataset::export::{self as core_export, ExecutionFactsSource, ExportRow};
use horch_core::execution::legacy::LedgerRecordV1;
use horch_core::execution::store::ExecutionStore;
use horch_core::execution::ExecutionStatus;
use horch_core::ids::{ExecutionId, SessionId};
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::worker_run::ExecutionFacts;
use horch_core::runtime::RuntimeContext;
use horch_core::usage::money::{CostSource, MicroUsd};
use horch_core::usage::Tokens;

use super::{dataset_paths, exit};

pub fn export(ctx: &RuntimeContext, label_policy: Option<&str>) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let lpv = label_policy.unwrap_or(LABEL_POLICY_VERSION);
    let rows = rows(ctx, &paths, lpv)?;
    let file = core_export::write_export(&paths, lpv, &rows, clock::now())?;
    println!("exported {} rows to {}", rows.len(), file.display());
    Ok(exit::SUCCESS)
}

/// Every exportable row of `label_policy_version`.
pub fn rows(
    ctx: &RuntimeContext,
    paths: &DatasetPaths,
    label_policy_version: &str,
) -> Result<Vec<ExportRow>> {
    let project = ctx.paths.project()?;
    let store = ExecutionStore::for_project(&ctx.paths.state_root, &project.to_string_lossy());
    let facts = StoreFacts {
        records: store.read()?,
    };
    core_export::export(paths, &facts, label_policy_version)
}

/// The execution store as an [`ExecutionFactsSource`].
pub struct StoreFacts {
    pub records: Vec<LedgerRecordV1>,
}

impl ExecutionFactsSource for StoreFacts {
    fn facts(&self, execution_id: &ExecutionId) -> Option<ExecutionFacts> {
        let r = self
            .records
            .iter()
            .find(|r| r.record_id == execution_id.as_str())?;
        Some(facts_of(execution_id, r))
    }
}

/// The execution part of a WorkerRun, from one record.
///
/// B3: the record has no tokens and no cost yet. The coordinator's usage
/// meter fills them; until then a run is `Unpriced` with zero tokens.
pub fn facts_of(execution_id: &ExecutionId, r: &LedgerRecordV1) -> ExecutionFacts {
    let status = r.state.clone().unwrap_or(if r.status == "done" {
        ExecutionStatus::Done
    } else {
        ExecutionStatus::Running
    });
    ExecutionFacts {
        execution_id: execution_id.clone(),
        status,
        session_id: r.session_id.as_deref().and_then(|s| SessionId::new(s).ok()),
        transcript_ref: None,
        transcript_digest: None,
        started_at: r.created_at.clone(),
        finished_at: r.finished_at.clone(),
        tokens: Tokens::default(),
        cost_microusd: MicroUsd(0),
        cost_source: CostSource::Unpriced,
        skills: r.skills.clone(),
        routing: r.routing.clone(),
    }
}
