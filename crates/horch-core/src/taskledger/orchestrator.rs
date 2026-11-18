//! The orchestrator of a run before Phase 6, and its retirement (FM-16).
//!
//! There is no long-lived orchestrator conversation: 1 sub-orchestrator
//! for each feature takes its place (`docs/specs/fleetd/design.md`, FM-16;
//! `docs/tickets/fleetd-p6/45-retire-orch.md`). `horch fleet` starts no
//! orchestrator, and the pass of fleetd respawns none. [`launch_run`] and
//! [`claim`] refuse each orchestrator launch with [`RETIRED`], which names
//! `horch idea add`.
//!
//! A live orchestrator of a run in flight at the update is not stopped: it
//! ends by itself. What stays here serves the rows and the locks of such an
//! orchestrator: the guard columns of its run ([`guard`], [`record_stop`],
//! [`status_line`]), its harness ([`record_harness`]) and the lock of its
//! workspace ([`lock`], [`holder`]).

use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{params, Connection, Transaction};
use serde::Serialize;

use super::util;

/// The line of each refused orchestrator launch and claim (FM-16).
pub const RETIRED: &str = "horch starts no orchestrator: there is no long-lived orchestrator \
                           conversation, and 1 sub-orchestrator for each feature takes its place. \
                           Add an idea: horch idea add \"...\"";

/// The respawn guard of 1 run, from its `runs` row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Guard {
    /// When `horch fleet stop` or `run close` stopped the orchestrator.
    pub stopped_at: Option<String>,
    pub respawns: i64,
    pub last_respawn_at: Option<String>,
}

/// The respawn guard of `run_id`.
pub fn guard(conn: &Connection, run_id: &str) -> Result<Guard> {
    Ok(conn.query_row(
        "SELECT orchestrator_stopped_at, orchestrator_respawns, last_respawn_at
         FROM runs WHERE run_id = ?1",
        [run_id],
        |r| {
            Ok(Guard {
                stopped_at: r.get(0)?,
                respawns: r.get(1)?,
                last_respawn_at: r.get(2)?,
            })
        },
    )?)
}

/// Write `harness`, and `model` when the row has none, on each live
/// orchestrator row of `workspace` that has no harness (TRF-44). Returns
/// the rows changed.
pub fn record_harness(
    conn: &Connection,
    workspace: &str,
    harness: &str,
    model: Option<&str>,
) -> Result<usize> {
    let live = super::sql_words::list(super::sql_words::WORKERS_LIVE);
    Ok(conn.execute(
        &format!(
            "UPDATE workers SET harness = ?2, model = coalesce(model, ?3)
             WHERE kind = 'orchestrator' AND workspace_id = ?1 AND harness IS NULL
             AND status IN ({live})"
        ),
        params![workspace, harness, model],
    )?)
}

/// Record a deliberate stop of the orchestrator of `run_id`: no respawn
/// follows. The first stop time stays. True when this call recorded it.
pub fn record_stop(conn: &Connection, run_id: &str) -> Result<bool> {
    let n = conn.execute(
        "UPDATE runs SET orchestrator_stopped_at = ?2
         WHERE run_id = ?1 AND orchestrator_stopped_at IS NULL",
        params![run_id, util::now()],
    )?;
    Ok(n == 1)
}

/// The run that an orchestrator launch continues: none. Each launch is
/// refused with [`RETIRED`] (FM-16).
pub fn launch_run() -> Result<()> {
    anyhow::bail!(RETIRED)
}

/// What 1 orchestrator launch did to its run ([`claim`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Claimed {
    /// The run is not active, or it already had this workspace and no stop.
    Unchanged,
    /// The workspace or the stop changed: the text of the 1 `note` event.
    Changed(String),
    /// The run's orchestrator still runs in the run's workspace: the run
    /// stays there. `text` says why. `workspace` and `pid` name the run's
    /// workspace and the lock holder's PID that the claim read.
    Skipped {
        text: String,
        workspace: String,
        pid: u32,
    },
}

/// The claim of `run_id` by an orchestrator pane in `workspace`: refused
/// with [`RETIRED`] (FM-16). The run, its workspace and its rows do not
/// change. The parameters stay for the `orchestrator_claim` request until
/// its kind is retired (ticket 48 of fleetd-p6).
pub fn claim(
    _tx: &Transaction,
    _run_id: &str,
    _workspace: &str,
    _pane: Option<&str>,
    _probe: &dyn Probe,
) -> Result<Claimed> {
    anyhow::bail!(RETIRED)
}

/// The folder of the workspace locks: beside the ledger, as the watchdog's.
pub fn lock_dir(ledger_db: &Path) -> PathBuf {
    ledger_db
        .parent()
        .map(|p| p.join("locks"))
        .unwrap_or_else(|| PathBuf::from("locks"))
}

/// The lock name of `workspace`, safe as a file name.
fn lock_name(workspace: &str) -> String {
    let safe: String = workspace
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("orchestrator-{safe}")
}

/// Take the orchestrator lock of `workspace`. `None` while a live process
/// holds it. A dead holder's lock is broken. The guard releases on drop.
pub fn lock(dir: &Path, workspace: &str) -> Result<Option<crate::fsx::DirLockGuard>> {
    match crate::fsx::DirLock::acquire(
        dir,
        &lock_name(workspace),
        std::time::Duration::from_secs(365 * 24 * 3600),
        std::time::Duration::ZERO,
    ) {
        Ok(g) => Ok(Some(g)),
        Err(crate::fsx::FsxError::LockTimeout { .. }) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// The live process that holds the orchestrator lock of a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Holder {
    pub pid: u32,
    pub started: Option<u64>,
}

/// The live holder of the orchestrator lock of `workspace`, if any.
pub fn holder(dir: &Path, workspace: &str) -> Option<Holder> {
    let owner = dir
        .join(format!("{}.lock", lock_name(workspace)))
        .join("owner");
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(owner).ok()?).ok()?;
    let pid = u32::try_from(v["pid"].as_u64()?).ok()?;
    let started = v["started"].as_u64();
    crate::procid::alive(pid, started).then_some(Holder { pid, started })
}

/// What an orchestrator claim reads outside the ledger.
pub trait Probe {
    /// The live holder of the orchestrator lock of `workspace`.
    fn holder(&self, workspace: &str) -> Option<Holder>;
    /// Whether `pid` runs with start time `started`.
    fn alive(&self, pid: u32, started: Option<u64>) -> bool;
    /// Whether herdr still has `workspace`. `None` when herdr cannot say.
    fn workspace_open(&self, workspace: &str) -> Option<bool>;
}

/// The status line of the respawns of `run_id`; `None` before the first
/// respawn and before a stop.
pub fn status_line(conn: &Connection, run_id: &str) -> Result<Option<String>> {
    let g = guard(conn, run_id)?;
    let mut parts = Vec::new();
    if g.respawns > 0 {
        parts.push(format!(
            "respawned {} times, last at {}",
            g.respawns,
            g.last_respawn_at.as_deref().unwrap_or("-")
        ));
    }
    if let Some(at) = &g.stopped_at {
        parts.push(format!("stopped at {at}, no respawn"));
    }
    Ok((!parts.is_empty()).then(|| format!("Orchestrator: {}.", parts.join("; "))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::Store;

    /// No lock holder, no live process, an open workspace.
    struct Dead;

    impl Probe for Dead {
        fn holder(&self, _: &str) -> Option<Holder> {
            None
        }
        fn alive(&self, _: u32, _: Option<u64>) -> bool {
            false
        }
        fn workspace_open(&self, _: &str) -> Option<bool> {
            Some(true)
        }
    }

    #[test]
    fn fm_16_each_orchestrator_launch_and_claim_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let err = store
            .immediate(|tx| {
                let r = runs::create(
                    tx,
                    dir.path(),
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )?;
                claim(tx, &r.run_id, "w1", Some("p1"), &Dead)
            })
            .unwrap_err()
            .to_string();
        assert!(err.contains("horch idea add"), "{err}");
        let err = launch_run().unwrap_err().to_string();
        assert!(err.contains("horch idea add"), "{err}");
    }

    #[test]
    fn trf_40_the_workspace_lock_admits_1_orchestrator() {
        let dir = tempfile::tempdir().unwrap();
        let held = lock(dir.path(), "w1")
            .unwrap()
            .expect("the first launcher locks");
        let me = holder(dir.path(), "w1").expect("this process holds it");
        assert_eq!(me.pid, std::process::id());
        assert!(
            lock(dir.path(), "w1").unwrap().is_none(),
            "a 2nd orchestrator refuses"
        );
        assert!(
            lock(dir.path(), "w2").unwrap().is_some(),
            "another workspace has its own"
        );
        drop(held);
        assert!(holder(dir.path(), "w1").is_none());
        assert!(
            lock(dir.path(), "w1").unwrap().is_some(),
            "free after the holder ends"
        );
        assert_eq!(lock_name("ws/1:a"), "orchestrator-ws_1_a");
    }
}
