//! `multi-herdr-dataset fleet sync [--all-projects]`: a run row for every
//! finished record that has none (fleet-dataset §4.2, FDS-15), and the sync
//! that `export` and `readiness` run first (FDS-18).

use std::path::{Path, PathBuf};

use anyhow::Result;
use horch_core::competition::budget::UsageMeter;
use horch_core::competition::observe::TelemetryUsage;
use horch_core::execution::legacy::LedgerRecordV1;
use horch_core::execution::store::{read_all_ledgers, slug, ExecutionStore};
use horch_core::fleet_runs::facts::RunSources;
use horch_core::fleet_runs::sync::{sync_project, SyncReport};
use horch_core::measure::paths::DatasetPaths;
use horch_core::runtime::RuntimeContext;
use horch_core::usage::Locations;
use horch_core::vcs::git::GitCli;

use super::exit;

/// `fleet sync`: sync the current project, or every ledger of the state
/// root with `all_projects`. Prints 1 line per project and 1 total line.
pub(crate) fn sync(ctx: &RuntimeContext, all_projects: bool) -> Result<u8> {
    let projects = if all_projects {
        every_project(&ctx.paths.state_root)
    } else {
        let project = ctx.paths.project()?;
        let store = ExecutionStore::for_project(&ctx.paths.state_root, &project.to_string_lossy());
        vec![(project, store.read()?)]
    };
    let mut total = SyncReport::default();
    for (project, records) in &projects {
        let report = sync_records(ctx, project, records)?;
        println!("{}", report_line(&project.display().to_string(), &report));
        add(&mut total, &report);
    }
    println!("{}", report_line("total", &total));
    Ok(exit::SUCCESS)
}

/// Sync the context's project before `export` or `readiness` read the fleet
/// store. A failure is 1 NOTE line on stderr; the command goes on.
pub(crate) fn sync_first(ctx: &RuntimeContext) {
    let synced = ctx.paths.project().and_then(|project| {
        let store = ExecutionStore::for_project(&ctx.paths.state_root, &project.to_string_lossy());
        sync_records(ctx, &project, &store.read()?)
    });
    if let Err(e) = synced {
        eprintln!("multi-herdr-dataset: NOTE: fleet run rows not synced: {e:#}");
    }
}

/// Write every missing run row of `records`, the ledger of `project`.
fn sync_records(
    ctx: &RuntimeContext,
    project: &Path,
    records: &[LedgerRecordV1],
) -> Result<SyncReport> {
    let paths = DatasetPaths::new(&ctx.paths.state_root, project);
    let git = GitCli::new(ctx.bins.harness.git.clone());
    let usage = TelemetryUsage {
        locations: Locations::from_context(ctx),
    };
    let sources = RunSources {
        usage: &usage,
        meter: &UsageMeter::default(),
        git: &git,
    };
    sync_project(
        &paths,
        project,
        records,
        &sources,
        horch_core::clock::now(),
        usize::MAX,
    )
}

/// Every ledger `<state root>/*.json` with its project: the `project` of a
/// record whose slug names the file, else of the first record that has one.
/// A ledger without a project has no dataset directory and is left out.
fn every_project(state_root: &Path) -> Vec<(PathBuf, Vec<LedgerRecordV1>)> {
    read_all_ledgers(state_root)
        .into_iter()
        .filter_map(|(file, records)| {
            let stem = file.file_stem()?.to_string_lossy().into_owned();
            let projects = || records.iter().filter_map(|r| r.project.as_deref());
            let project = projects()
                .find(|p| slug(p) == stem)
                .or_else(|| projects().next())?
                .to_string();
            Some((PathBuf::from(project), records))
        })
        .collect()
}

/// `<name>: written <n> (tokens <n>, unpriced <n>), already <n>, deferred
/// <n>, skipped <n>`.
fn report_line(name: &str, r: &SyncReport) -> String {
    format!(
        "{name}: written {} (tokens {}, unpriced {}), already {}, deferred {}, skipped {}",
        r.written,
        r.with_tokens,
        r.unpriced,
        r.already,
        r.deferred,
        r.skipped_total()
    )
}

fn add(total: &mut SyncReport, r: &SyncReport) {
    total.written += r.written;
    total.with_tokens += r.with_tokens;
    total.unpriced += r.unpriced;
    total.already += r.already;
    total.deferred += r.deferred;
    for (reason, n) in &r.skipped {
        *total.skipped.entry(reason).or_default() += n;
    }
}
