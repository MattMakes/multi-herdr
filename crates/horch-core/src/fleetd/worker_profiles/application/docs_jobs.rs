//! Queue the `docs_sync` and `conformance` jobs of each CLI through
//! [`jobs::queue`], with no repository (fleetd-p7 ticket 28). The rules are
//! in [`crate::fleetd::worker_profiles::domain::docs_jobs`].
//!
//! The pass gives the facts: the fleet clock, the binary id of the horch
//! binary that runs fleetd, and 1 [`CliBinary`] for each CLI that the scan
//! of the CLI binaries read ([`scan_cli`]). Docs sync and conformance read
//! the installed binary and never update it (preservation row 06 S7).

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::exeid::exe_identity;
use crate::fleetd::fleet_runtime::adapters::jobs_store::{self as store, JobRow};
use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::worker_profiles::domain::docs_jobs::{
    due, input_key, subject, CliFacts, DocsJob, LastRun, Why,
};
use crate::harness::HarnessKind;

/// The count of hex digits of a binary id.
const BINARY_ID_LEN: usize = 16;

/// The docs source of each CLI with an exec adapter, with its harness: the
/// 3 CLIs of the adapter table (MAP.md Q5). Each other docs source (herdr)
/// has no conformance suite.
const EXEC_SOURCES: [(&str, HarnessKind); 3] = [
    ("claude", HarnessKind::Claude),
    ("codex", HarnessKind::Codex),
    ("agy", HarnessKind::Antigravity),
];

/// The harness of the docs source `cli` when it has an exec adapter, and
/// so a conformance suite: the docs source `agy` is the CLI of the harness
/// `antigravity`.
pub fn exec_harness(cli: &str) -> Option<HarnessKind> {
    EXEC_SOURCES
        .iter()
        .find(|(name, _)| *name == cli)
        .map(|(_, kind)| *kind)
}

/// A CLI and the binary id of its binary now: 1 fact of the scan of the
/// CLI binaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliBinary {
    pub cli: String,
    pub binary_id: String,
}

/// What 1 pass queued for 1 run of a CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedDocsJob {
    pub kind: Kind,
    pub subject: String,
    pub why: Why,
    pub queued: Queued,
}

/// The binary id of the file at `path`: the hash of its identity (path,
/// size and modification time), or `None` when it cannot be read. The
/// horch binary and each CLI binary have one.
pub fn binary_id(path: &Path) -> Option<String> {
    let id = exe_identity(path)?;
    let hash = Sha256::digest(format!("{}\0{}\0{}", id.path, id.len, id.mtime_ns));
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    Some(hex[..BINARY_ID_LEN].to_string())
}

/// The fact of the CLI `cli` whose binary is at `path`, or `None` when the
/// binary cannot be read. A CLI with no fact gets no run: a fact that is
/// unknown does not hold.
pub fn scan_cli(cli: &str, path: &Path) -> Option<CliBinary> {
    Some(CliBinary {
        cli: cli.to_string(),
        binary_id: binary_id(path)?,
    })
}

/// Queue the runs that the rules ask for, for each CLI of `clis`, in `tx`
/// at `clock` (part 2 of a pass). `horch_id` is the binary id of the horch
/// binary that runs this fleetd.
pub fn queue_docs_jobs(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    clock: FleetSeconds,
    horch_id: &str,
    clis: &[CliBinary],
) -> Result<Vec<QueuedDocsJob>> {
    let mut out = Vec::new();
    for cli in clis {
        let docs = last_run(tx, Kind::DocsSync, &cli.cli)?;
        let conf = last_run(tx, Kind::Conformance, &cli.cli)?;
        let facts = CliFacts {
            binary_id: &cli.binary_id,
            conformance: exec_harness(&cli.cli).is_some(),
            last_docs_sync: docs.as_ref(),
            last_conformance: conf.as_ref(),
        };
        let subject = subject(&cli.cli, &cli.binary_id);
        let input_key = input_key(&subject, horch_id);
        for d in due(clock.0, horch_id, &facts) {
            let kind = match d.job {
                DocsJob::DocsSync => Kind::DocsSync,
                DocsJob::Conformance => Kind::Conformance,
            };
            let key = JobKey {
                kind,
                subject: &subject,
                input_key: &input_key,
            };
            let queued = jobs::queue(ctx, tx, &key, None, clock)?;
            out.push(QueuedDocsJob {
                kind,
                subject: subject.clone(),
                why: d.why,
                queued,
            });
        }
    }
    Ok(out)
}

/// The last run of `kind` of the CLI `cli`, of any binary id.
fn last_run(tx: &Connection, kind: Kind, cli: &str) -> Result<Option<LastRun>> {
    Ok(store::last_of_word(tx, kind, cli)?.map(|r| as_last_run(&r)))
}

/// The row `r` as the rules read it. Its start on the fleet clock is its
/// deadline less its time limit.
fn as_last_run(r: &JobRow) -> LastRun {
    let limit = i64::try_from(r.time_limit_s).unwrap_or(i64::MAX);
    LastRun {
        id: r.id,
        subject: r.subject.clone(),
        input_key: r.input_key.clone(),
        started: r.deadline.map(|d| d.0.saturating_sub(limit)),
        active: matches!(r.state, State::Queued | State::Running),
        has_result: matches!(r.state, State::Passed | State::Failed),
    }
}
