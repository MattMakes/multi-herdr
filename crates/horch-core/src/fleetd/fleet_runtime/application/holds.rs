//! The holds on new dispatch: the 5 health guards, pacing of a pool, the
//! worktree disk budget, and the events of a global hold (OR-1, OR-4,
//! OR-5, FM-28).
//!
//! In Phase 1 the dispatch points are `horch spawn` and `horch ledger spawn`
//! (`docs/tickets/fleetd-p1/MAP.md`, Q16). Each calls [`holds`] and refuses
//! with each hold and its cause. The reconcile pass calls
//! [`record_hold_changes`] in each pass (ticket 21).

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::sqlite::generation;
use crate::fleetd::fleet_runtime::adapters::{clock_store, config_file, jobs_store};
use crate::fleetd::fleet_runtime::application::jobs::{self, NeedsYouItems, NewItem};
use crate::fleetd::fleet_runtime::domain::health::{
    self, Finding, Floors, Hold, PaceRule, PoolWindow, Reading, Readings,
};
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State as JobState};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::integration::application::queries as integration;
use crate::fleetd::integration::domain::project::{self, ProjectSettings};
use crate::fleetd::task_execution::application::dispatch::{choices, CheckSandboxes};
use crate::fleetd::task_execution::domain::dispatch::{Choice, Hold as DispatchHold, Holds};
use crate::fleetd::worktree_custody::adapters::sqlite::{self as trees, WorktreeState};
use crate::fleetd::worktree_custody::application::disk;
use crate::fleetd::worktree_custody::domain::disk::hold_cause;
use crate::routing::quota::{model_family, pool_for, QuotaView, State};
use crate::runtime::machine::{self, Known, MachineSnapshot, ProbeBins};
use crate::runtime::RuntimeContext;
use crate::taskledger::model::{Actor, EventKind};
use crate::taskledger::tasks;

/// The readings of the platform beside [`MachineSnapshot`]: the count of
/// processes of the user, and the count and the limit of open files of the
/// system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Counts {
    pub user_processes: Reading<u64>,
    pub system_open_files: Reading<u64>,
    pub system_open_file_limit: Reading<u64>,
}

impl Counts {
    /// The counts of this machine now.
    pub fn read() -> Self {
        use horch_platform::machine as m;
        Counts {
            user_processes: platform_reading(m::user_process_count()),
            system_open_files: platform_reading(m::system_open_file_count()),
            system_open_file_limit: platform_reading(m::system_open_file_limit()),
        }
    }
}

/// 1 reading of the platform. A fact that the platform does not support is
/// unknown with [`health::NOT_SUPPORTED`], and a read that fails is unknown
/// with [`health::NO_READING`].
pub fn platform_reading(
    reading: Result<Option<u64>, horch_platform::NotSupported>,
) -> Reading<u64> {
    reading.map_or(Reading::Unknown(health::NOT_SUPPORTED), Reading::from)
}

/// Why the memory guard does not hold on macOS when the kernel gives no free
/// memory: the available memory of the probe is the free, inactive and
/// speculative pages, so the active file cache counts as used memory.
pub const MACOS_MEMORY: &str = "on macOS the kernel gives no free memory, \
     and the probe counts active file cache as used memory";

/// The readings of the health guards from the machine probe and the counts.
/// The process limit is the limit of the user (`RLIMIT_NPROC`), the same
/// scope as the count of processes of the user.
pub fn readings(snapshot: &MachineSnapshot, counts: &Counts) -> Readings {
    let known = |k: Known<u64>| Reading::from(Option::from(k));
    Readings {
        free_disk_bytes: known(snapshot.disk_free_bytes),
        load_1m: snapshot.load_average_1m.into(),
        cpus: Option::<u32>::from(snapshot.cpus).into(),
        free_memory_bytes: known(snapshot.mem_available_bytes),
        user_processes: counts.user_processes,
        user_process_limit: known(snapshot.max_processes),
        system_open_files: counts.system_open_files,
        system_open_file_limit: counts.system_open_file_limit,
    }
}

/// The holds and unknowns of the health guards now, and the hold of the
/// worktree disk budget. The machine probe reads the disk of the project,
/// or the fixture `HORCH_MACHINE_FILE`; the floors and the budget come from
/// `<fleet root>/fleet.toml`. A ledger that does not exist has no trees.
pub fn holds(ctx: &RuntimeContext) -> Result<Vec<Finding>> {
    let config = config_file::load(&ctx.paths.fleet_root)?;
    let project = ctx
        .paths
        .project()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let snapshot = machine::probe(
        &project,
        &ProbeBins::default(),
        ctx.settings.machine_file.as_deref(),
    );
    let mut r = readings(&snapshot, &Counts::read());
    if ctx.settings.machine_file.is_none() && snapshot.os == "macos" {
        r.free_memory_bytes =
            macos_free_memory(horch_platform::machine::kernel_free_memory().ok().flatten());
    }
    let mut found = health::health(&r, &Floors::from(&config));
    // The open that migrates, as the dispatch points open the ledger next:
    // a ledger of an older schema has no `worktrees.state` yet.
    if let Some(store) = crate::taskledger::open_existing(ctx)? {
        let disk = worktree_disk(store.conn(), config.worktree_disk_budget_gb)
            .context("reading the worktree disk budget")?;
        found.extend(disk.map(Finding::Hold));
    }
    Ok(found)
}

/// The global hold of the worktree disk budget of `budget_gb` GB: the total
/// size of all trees stays above it after each tree that the budget may
/// remove (FM-28, OR-1).
pub fn worktree_disk(conn: &Connection, budget_gb: u64) -> Result<Option<Hold>> {
    let d = disk::decision(conn, budget_gb)?;
    Ok(d.hold.then(|| Hold::WorktreeDisk {
        cause: hold_cause(&d),
    }))
}

/// The free memory of macOS for the guard: the free memory of the kernel,
/// which counts the file cache as free. With no reading (a sandbox can deny
/// the sysctl) the guard does not hold, and says why ([`MACOS_MEMORY`]).
pub fn macos_free_memory(kernel_free: Option<u64>) -> Reading<u64> {
    kernel_free.map_or(Reading::Unknown(MACOS_MEMORY), Reading::Known)
}

/// The pacing hold of the pool of `agent` and `model` in `view`. Only a
/// fresh reading that today's rules call `tight` can pace: a stale or
/// missing reading is unknown, not a hold.
pub fn pacing(view: &QuotaView, agent: &str, model: &str) -> Option<Hold> {
    if view.assess(agent, model).state != State::Tight {
        return None;
    }
    let pool = pool_for(agent, model);
    let scope = model_family(model);
    let windows: Vec<PoolWindow> = view
        .file
        .pools
        .get(pool)?
        .windows
        .iter()
        .filter(|w| w.scope_model.is_none() || w.scope_model.as_deref() == scope)
        .map(|w| PoolWindow {
            label: w.label(),
            pace: w.pace(view.now),
            used: w.used_at(view.now),
        })
        .collect();
    let rule = PaceRule {
        pace_factor: view.policy.pace_factor,
        pace_min_used: view.policy.pace_min_used,
    };
    health::pacing(pool, &windows, &rule)
}

/// The hold of 1 project: it holds only the new dispatch of the tasks of
/// that project. A task in flight goes on, and no task is lost or
/// cancelled (OR-1, "Two declared gates").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectHold {
    pub project: String,
    pub cause: String,
}

/// The hold of the project `project` when `settings` has no fast gate.
pub fn missing_fast_gate(project: &str, settings: &ProjectSettings) -> Option<ProjectHold> {
    settings.fast_gate().is_none().then(|| ProjectHold {
        project: project.to_string(),
        cause: project::missing_fast_gate_cause(project),
    })
}

/// The hold of the project `project` with no fast gate in `settings`, and
/// its needs-you item `missing_fast_gate:<project>`: open while the hold
/// holds, resolved when the project declares the gate. It writes only that
/// item, in `tx`.
pub fn sync_missing_fast_gate(
    tx: &Connection,
    needs_you: &dyn NeedsYouItems,
    project: &str,
    settings: &ProjectSettings,
) -> Result<Option<ProjectHold>> {
    let key = project::missing_fast_gate_key(project);
    let hold = missing_fast_gate(project, settings);
    match &hold {
        None => {
            needs_you.resolve_item(tx, &key)?;
        }
        Some(h) => {
            let text = format!(
                "fleetd holds new dispatch of the tasks of {project}: {}. Tasks in flight go on.",
                h.cause
            );
            let item = NewItem {
                key: &key,
                kind: project::MISSING_FAST_GATE_KIND,
                entity: Entity::Task,
                entity_id: project,
                text: &text,
            };
            needs_you.open_item(tx, &item)?;
        }
    }
    Ok(hold)
}

/// The key of 1 hold that its events record: the name of a global hold,
/// or `pool:<pool>` for the pacing of 1 pool.
fn key(h: &Hold) -> Option<String> {
    match h {
        Hold::Pacing { pool, .. } => Some(format!("pool:{pool}")),
        _ => h.global().map(str::to_string),
    }
}

/// The data of the events of the hold `h`: a global hold names its `fact`,
/// as the events of the health floors do, and a pacing hold names its
/// `pool`.
fn data(h: &Hold) -> serde_json::Value {
    match h {
        Hold::Pacing { pool, .. } => serde_json::json!({ "pool": pool }),
        _ => serde_json::json!({ "fact": h.global() }),
    }
}

/// The SQL key of a hold event: the same text as [`key`].
const EVENT_KEY: &str = "COALESCE(json_extract(data_json, '$.fact'),
    'pool:' || json_extract(data_json, '$.pool'))";

/// 1 hold that its last event says is on: a `hold_started` event with no
/// `hold_ended` event after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenHold {
    /// The name of a global hold, or `pool:<pool>`.
    pub key: String,
    /// The pool of a pacing hold; `None` for a global hold.
    pub pool: Option<String>,
    pub cause: String,
}

/// Each hold whose last event is `hold_started`, in key order (OR-1). The
/// ledger keeps them, so they survive a restart of fleetd.
pub fn open_holds(conn: &Connection) -> Result<Vec<OpenHold>> {
    let sql = format!(
        "SELECT {EVENT_KEY}, json_extract(data_json, '$.pool'), cause FROM events
         WHERE event_id IN (
            SELECT max(event_id) FROM events
            WHERE kind IN ('hold_started', 'hold_ended') GROUP BY {EVENT_KEY})
         AND kind = 'hold_started'
         ORDER BY 1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(OpenHold {
            key: r.get(0)?,
            pool: r.get(1)?,
            cause: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Record 1 `hold_started` event for each global hold and each pacing hold
/// of `now` that is not on, and 1 `hold_ended` event for each hold that is
/// on and not in `now`, each with its cause. A hold is on when it is in
/// `before` or its last event in the ledger is `hold_started`: a fleetd
/// that starts again with no holds in memory starts no hold twice, and
/// ends a hold that ended while no fleetd ran.
pub fn record_hold_changes(tx: &Connection, before: &[Hold], now: &[Hold]) -> Result<()> {
    let mut on: Vec<(String, String, serde_json::Value)> = open_holds(tx)?
        .into_iter()
        .map(|o| {
            let data = match &o.pool {
                Some(pool) => serde_json::json!({ "pool": pool }),
                None => serde_json::json!({ "fact": o.key }),
            };
            (o.key, o.cause, data)
        })
        .collect();
    for h in before {
        if let Some(k) = key(h) {
            if !on.iter().any(|(n, ..)| *n == k) {
                on.push((k, h.cause().to_string(), data(h)));
            }
        }
    }
    let now: Vec<(String, &Hold)> = now.iter().filter_map(|h| key(h).map(|k| (k, h))).collect();
    for (k, h) in &now {
        if !on.iter().any(|(n, ..)| n == k) {
            append(tx, EventKind::HoldStarted, k, h.cause(), &data(h))?;
        }
    }
    for (k, cause, data) in &on {
        if !now.iter().any(|(n, _)| n == k) {
            append(tx, EventKind::HoldEnded, k, cause, data)?;
        }
    }
    Ok(())
}

/// The event of the hold `name`, with `data`.
fn append(
    tx: &Connection,
    kind: EventKind,
    name: &str,
    cause: &str,
    data: &serde_json::Value,
) -> Result<()> {
    tx.execute(
        "INSERT INTO events (at, actor, kind, text, cause, data_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            crate::taskledger::util::now(),
            Actor::Horch.as_str(),
            kind.as_str(),
            format!("{}: {cause}", kind.as_str()),
            cause,
            data.to_string()
        ],
    )
    .with_context(|| format!("recording the {} event of {name}", kind.as_str()))?;
    Ok(())
}

/// Each persona is supported: the board shows a persona with no check
/// sandbox in its needs-you list.
struct EverySandbox;

impl CheckSandboxes for EverySandbox {
    fn supported(&self, _persona: &str) -> bool {
        true
    }
}

/// The hold on dispatch of each task, by task id, from the ledger (OR-1,
/// FM-45): a ready task held by an open global hold, an unhealthy profile,
/// the pacing of the pool that `pool_of` gives for its persona, a missing
/// fast gate or `not_before`; and a task whose `worktree_add` waits for a
/// git job that holds its repository.
pub fn task_holds(
    conn: &Connection,
    pool_of: &dyn Fn(&str) -> Option<String>,
) -> Result<BTreeMap<String, String>> {
    let open = open_holds(conn)?;
    let holds = Holds {
        // With no fleetd that ran, no `not_before` is judged.
        clock: match generation(conn)? {
            Some(_) => clock_store::read(conn)?.0,
            None => i64::MAX,
        },
        global: open
            .iter()
            .filter(|o| o.pool.is_none())
            .map(|o| o.cause.clone())
            .collect(),
        projects: integration::integration(conn)?
            .holds
            .into_iter()
            .map(|h| (h.project, h.cause))
            .collect(),
        ..Holds::default()
    };
    let mut out = git_job_holds(conn)?;
    for choice in choices(conn, &holds, &EverySandbox, None)? {
        let (task_id, hold) = match choice {
            Choice::Held { task_id, hold } => (task_id, Some(hold)),
            Choice::Bind { task_id, .. } => (task_id, None),
        };
        // Pacing holds 1 persona: after a global hold and an unhealthy
        // profile, before the rest, as dispatch orders its holds.
        let first = matches!(
            hold,
            Some(DispatchHold::Global(_) | DispatchHold::UnhealthyProfile { .. })
        );
        let paced = if first {
            None
        } else {
            let t = tasks::get(conn, &task_id)?;
            let persona = t.claimable_by.or(t.primary_persona).unwrap_or_default();
            let pool = pool_of(&persona);
            open.iter()
                .find(|o| o.pool.is_some() && o.pool == pool)
                .map(|o| DispatchHold::Persona {
                    persona,
                    cause: o.cause.clone(),
                })
        };
        let shown = paced.or(hold).filter(|h| {
            matches!(
                h,
                DispatchHold::Global(_)
                    | DispatchHold::UnhealthyProfile { .. }
                    | DispatchHold::Persona { .. }
                    | DispatchHold::Project { .. }
                    | DispatchHold::NotBefore { .. }
            )
        });
        if let Some(h) = shown {
            out.insert(task_id, h.to_string());
        }
    }
    Ok(out)
}

/// The hold of each task whose `reserved` tree waits for its `queued`
/// `worktree_add` run while another git writer of its repository runs:
/// "git job <kind> <id> holds <repo>" (1 git writer for each repository,
/// MAP.md of fleetd-p3, Q12).
fn git_job_holds(conn: &Connection) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for tree in trees::in_states(conn, &[WorktreeState::Reserved])? {
        let Some(task) = tree.task_id else {
            continue;
        };
        let add = jobs_store::active(conn, Kind::WorktreeAdd, &tree.worktree_id)?;
        if add.is_none_or(|j| j.state != JobState::Queued) {
            continue;
        }
        let repo = tree.repo_root;
        if let Some(j) = jobs::git_writers_running(conn, &repo)?.first() {
            let cause = format!("git job {} {} holds {repo}", j.kind.as_str(), j.id);
            out.insert(task, cause);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
    use crate::fleetd::fleet_runtime::domain::health::Fact;
    use crate::taskledger::Store;

    fn floor(fact: Fact, cause: &str) -> Hold {
        Hold::Floor {
            fact,
            cause: cause.into(),
        }
    }

    fn events(store: &Store) -> Vec<(String, String, String, String)> {
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT kind, text, cause, data_json FROM events
                 WHERE kind IN ('hold_started', 'hold_ended') ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// The rows of the needs-you list: key, kind, entity id, state, text.
    fn needs_you_rows(store: &Store) -> Vec<(String, String, String, String, String)> {
        let mut stmt = store
            .conn()
            .prepare("SELECT key, kind, entity_id, state, text FROM needs_you ORDER BY key")
            .unwrap();
        stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    fn gates(fast: Option<&str>) -> ProjectSettings {
        let mut s = ProjectSettings::default();
        s.gates.fast = fast.map(str::to_string);
        s.gates.full = Some("cargo test".into());
        s
    }

    #[test]
    fn project_with_no_fast_gate_holds_its_dispatch_with_its_cause() {
        use crate::fleetd::ideas::application::needs_you::NeedsYouList;
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let hold = store
            .immediate(|tx| sync_missing_fast_gate(tx, &NeedsYouList, "P1", &gates(None)))
            .unwrap();
        let cause = "missing fast gate: add [gates] fast to fleet.toml in P1";
        assert_eq!(
            hold,
            Some(ProjectHold {
                project: "P1".into(),
                cause: cause.into(),
            })
        );
        // A second pass keeps 1 item.
        store
            .immediate(|tx| sync_missing_fast_gate(tx, &NeedsYouList, "P1", &gates(None)))
            .unwrap();
        let rows = needs_you_rows(&store);
        assert_eq!(rows.len(), 1, "{rows:?}");
        let (key, kind, id, state, text) = &rows[0];
        assert_eq!(
            (key.as_str(), kind.as_str(), id.as_str(), state.as_str()),
            ("missing_fast_gate:P1", "missing_fast_gate", "P1", "open")
        );
        assert!(text.contains(cause), "{text}");
        // Another project with its gate has no hold.
        assert_eq!(
            missing_fast_gate("P2", &gates(Some("cargo test --lib"))),
            None
        );
    }

    #[test]
    fn the_hold_ends_when_the_gate_is_declared() {
        use crate::fleetd::ideas::application::needs_you::NeedsYouList;
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        store
            .immediate(|tx| sync_missing_fast_gate(tx, &NeedsYouList, "P1", &gates(None)))
            .unwrap();
        let hold = store
            .immediate(|tx| {
                sync_missing_fast_gate(tx, &NeedsYouList, "P1", &gates(Some("cargo test --lib")))
            })
            .unwrap();
        assert_eq!(hold, None);
        let rows = needs_you_rows(&store);
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].3, "resolved");
    }

    #[test]
    fn a_hold_records_1_event_when_it_starts_and_1_when_it_ends() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let disk = floor(
            Fact::FreeDisk,
            "free disk 2.0 GB is below the floor of 10 GB",
        );
        let pace = Hold::Pacing {
            pool: "claude".into(),
            cause: "pool claude is ahead".into(),
        };
        let passes: [&[Hold]; 4] = [&[], &[disk.clone(), pace], std::slice::from_ref(&disk), &[]];
        for pair in passes.windows(2) {
            store
                .immediate(|tx| record_hold_changes(tx, pair[0], pair[1]))
                .unwrap();
        }
        // A pacing hold records its events too, with its pool (T-599), so
        // the board shows it.
        let row = |kind: &str, cause: &str, data: &str| {
            (
                kind.to_string(),
                format!("{kind}: {cause}"),
                cause.to_string(),
                data.to_string(),
            )
        };
        let (fact, pool) = (r#"{"fact":"free_disk"}"#, r#"{"pool":"claude"}"#);
        assert_eq!(
            events(&store),
            vec![
                row("hold_started", disk.cause(), fact),
                row("hold_started", "pool claude is ahead", pool),
                row("hold_ended", "pool claude is ahead", pool),
                row("hold_ended", disk.cause(), fact),
            ]
        );
    }

    #[test]
    fn the_worktree_disk_hold_records_1_event_when_it_starts_and_1_when_it_ends() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let disk = Hold::WorktreeDisk {
            cause: "the worktrees use 25.0 GB".into(),
        };
        let passes: [&[Hold]; 4] = [
            &[],
            std::slice::from_ref(&disk),
            std::slice::from_ref(&disk),
            &[],
        ];
        for pair in passes.windows(2) {
            store
                .immediate(|tx| record_hold_changes(tx, pair[0], pair[1]))
                .unwrap();
        }
        let cause = disk.cause().to_string();
        let data = r#"{"fact":"worktree_disk"}"#.to_string();
        assert_eq!(
            events(&store),
            vec![
                (
                    "hold_started".into(),
                    format!("hold_started: {cause}"),
                    cause.clone(),
                    data.clone()
                ),
                (
                    "hold_ended".into(),
                    format!("hold_ended: {cause}"),
                    cause,
                    data
                ),
            ]
        );
    }

    /// The dispatch points refuse on each hold of [`holds`]: over the
    /// budget with no spare tree, it gives the worktree disk hold.
    #[test]
    fn holds_give_the_worktree_disk_hold_over_the_budget_and_none_under_it() {
        use crate::fleetd::fleet_runtime::application::ledger::Cause;
        use crate::fleetd::worktree_custody::adapters::sqlite::{self as trees, NewWorktree};
        use crate::fleetd::worktree_custody::domain::disk::GB;
        use crate::taskledger::model::WorktreeStatus;

        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("ledger.sqlite3");
        let machine = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/machine/mac-m5-128g.json"
        );
        let ctx = RuntimeContext::from_env(
            &crate::runtime::MapEnv::new("/")
                .with(
                    "HORCH_STATE_DIR",
                    dir.path().join("state").to_str().unwrap(),
                )
                .with("FLEET_ROOT", dir.path().join("fleet").to_str().unwrap())
                .with("HORCH_LEDGER_DB", db.to_str().unwrap())
                .with("HORCH_MACHINE_FILE", machine)
                .with("HORCH_PROJECT_DIR", dir.path().to_str().unwrap()),
        )
        .unwrap();
        let disk_holds = |ctx: &RuntimeContext| -> Vec<String> {
            holds(ctx)
                .unwrap()
                .into_iter()
                .filter_map(|f| match f {
                    Finding::Hold(h @ Hold::WorktreeDisk { .. }) => Some(h.cause().to_string()),
                    _ => None,
                })
                .collect()
        };
        assert!(disk_holds(&ctx).is_empty(), "no ledger has no trees");
        let mut store = Store::open(&db).unwrap();
        let new = NewWorktree {
            worktree_id: "live".into(),
            task_id: None,
            worker_id: None,
            repo_root: dir.path().display().to_string(),
            path: dir.path().join(".worktrees/live").display().to_string(),
            branch: "fleet/live".into(),
            base: "main".into(),
            base_commit: None,
            status: WorktreeStatus::Open,
            mode: None,
        };
        let cause = Cause {
            actor: Actor::Horch,
            text: "a test",
        };
        let set_bytes = |store: &mut Store, gb: u64| {
            store
                .immediate(|tx| {
                    tx.execute(
                        "UPDATE worktrees SET state = 'active', bytes_on_disk = ?1",
                        [i64::try_from(gb * GB).unwrap()],
                    )?;
                    Ok(())
                })
                .unwrap();
        };
        store
            .immediate(|tx| trees::insert_orphan(tx, None, &new, cause))
            .unwrap();
        set_bytes(&mut store, 25);
        assert_eq!(
            disk_holds(&ctx),
            [
                "the worktrees use 25.0 GB, above the budget of 20 GB, and no landed or rescued \
              tree of a merged or cancelled idea is left to remove (worktree_disk_budget_gb)"
            ]
        );
        set_bytes(&mut store, 15);
        assert!(disk_holds(&ctx).is_empty());
    }

    /// On a live Mac the memory guard reads the free memory of the kernel,
    /// which counts the file cache as free. When a sandbox denies the
    /// sysctl, the guard says why and does not hold.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_live_mac_reads_the_kernel_free_memory_or_says_why() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = RuntimeContext::from_env(
            &crate::runtime::MapEnv::new("/")
                .with(
                    "HORCH_STATE_DIR",
                    dir.path().join("state").to_str().unwrap(),
                )
                .with("FLEET_ROOT", dir.path().join("fleet").to_str().unwrap())
                .with(
                    "HORCH_LEDGER_DB",
                    dir.path().join("ledger.sqlite3").to_str().unwrap(),
                )
                .with("HORCH_PROJECT_DIR", dir.path().to_str().unwrap()),
        )
        .unwrap();
        let found = holds(&ctx).unwrap();
        let memory: Vec<&Finding> = found
            .iter()
            .filter(|f| {
                matches!(
                    f,
                    Finding::Hold(Hold::Floor {
                        fact: Fact::FreeMemory,
                        ..
                    }) | Finding::Unknown {
                        fact: Fact::FreeMemory,
                        ..
                    }
                )
            })
            .collect();
        // A sandbox can deny the sysctl: then the guard says why and does
        // not hold.
        let expected: Vec<Finding> = match horch_platform::machine::kernel_free_memory() {
            Ok(Some(_)) => vec![],
            _ => vec![Finding::Unknown {
                fact: Fact::FreeMemory,
                why: MACOS_MEMORY,
            }],
        };
        assert_eq!(memory, expected.iter().collect::<Vec<_>>());
    }

    /// A count that the platform does not support is unknown with its own
    /// reason, apart from a count that the probe failed to read.
    #[test]
    fn not_supported_count_keeps_its_reason() {
        let gap = horch_platform::NotSupported::new("machine", "windows");
        assert_eq!(
            platform_reading(Err(gap)),
            Reading::Unknown(health::NOT_SUPPORTED)
        );
        assert_eq!(
            platform_reading(Ok(None)),
            Reading::Unknown(health::NO_READING)
        );
        assert_eq!(platform_reading(Ok(Some(3))), Reading::Known(3));
        let counts = Counts {
            user_processes: Reading::Known(500),
            system_open_files: platform_reading(Err(gap)),
            system_open_file_limit: Reading::Known(1),
        };
        let found = health::health(
            &readings(&healthy(), &counts),
            &Floors::from(&FleetConfig::default()),
        );
        assert_eq!(
            found,
            vec![Finding::Unknown {
                fact: Fact::OpenFiles,
                why: health::NOT_SUPPORTED
            }]
        );
    }

    /// The guards with injected readings: a reading holds past its floor,
    /// and no reading (a denied sysctl) is unknown with its reason.
    #[test]
    fn injected_readings_hold_past_the_floor_and_none_is_unknown_with_a_reason() {
        let floors = Floors::from(&FleetConfig::default());
        assert_eq!(macos_free_memory(Some(8 << 30)), Reading::Known(8 << 30));
        assert_eq!(macos_free_memory(None), Reading::Unknown(MACOS_MEMORY));
        let mac = healthy();
        let counts = |limit: Option<u64>| Counts {
            user_processes: Reading::Known(500),
            system_open_files: Reading::Known(901),
            system_open_file_limit: limit.into(),
        };
        // 901 open files of the system against a system limit of 1000.
        let found = health::health(&readings(&mac, &counts(Some(1000))), &floors);
        assert!(
            matches!(
                found.as_slice(),
                [Finding::Hold(Hold::Floor {
                    fact: Fact::OpenFiles,
                    ..
                })]
            ),
            "{found:?}"
        );
        let mut r = readings(&mac, &counts(None));
        r.free_memory_bytes = macos_free_memory(None);
        assert_eq!(
            health::health(&r, &floors),
            vec![
                Finding::Unknown {
                    fact: Fact::FreeMemory,
                    why: MACOS_MEMORY
                },
                Finding::Unknown {
                    fact: Fact::OpenFiles,
                    why: health::NO_READING
                },
            ]
        );
    }

    /// A view of the claude pool with 1 window of 7 days that is half
    /// elapsed and `used`.
    fn half_elapsed(used: f64) -> QuotaView {
        use crate::routing::quota::{PoolReading, QuotaFile, Window};
        let now = crate::clock::parse("2026-09-28T18:00:00Z").unwrap();
        let mut file = QuotaFile::default();
        file.pools.insert(
            "claude".into(),
            PoolReading {
                observed_at: Some("2026-09-28T17:55:00Z".into()),
                windows: vec![Window::new(
                    10_080,
                    used,
                    Some("2026-10-02T06:00:00Z".into()),
                    None,
                )],
                ..Default::default()
            },
        );
        QuotaView::new(file, now, Default::default(), false)
    }

    #[test]
    fn pacing_reads_the_pool_window_of_the_spawn() {
        // Half elapsed: 0.63 / 0.5 is a pace of 1.26, above 1.25.
        let hold = pacing(&half_elapsed(0.63), "claude", "claude-opus-5-5").expect("paced");
        assert_eq!(
            hold.cause(),
            "pool claude window 7d is 63% used at pace 1.3x, above 1.25x"
        );
        // 0.6 / 0.5 is a pace of 1.2: on pace.
        assert_eq!(
            pacing(&half_elapsed(0.6), "claude", "claude-opus-5-5"),
            None
        );
        // Another pool has no reading: unknown, not a hold.
        assert_eq!(pacing(&half_elapsed(0.63), "codex", "gpt-5.5"), None);
    }

    /// The child of [`a_soft_open_file_limit_of_256_holds_nothing`]: it runs
    /// with a soft `RLIMIT_NOFILE` of 256.
    #[cfg(unix)]
    #[test]
    #[ignore = "runs only as the child of a_soft_open_file_limit_of_256_holds_nothing"]
    fn child_with_a_soft_open_file_limit_of_256() {
        assert_eq!(
            horch_platform::machine::max_open_files().unwrap(),
            Some(256),
            "the parent sets the soft limit"
        );
        let counts = Counts::read();
        let r = readings(&healthy(), &counts);
        let found = health::health(&r, &Floors::from(&FleetConfig::default()));
        assert!(
            !found.iter().any(|f| matches!(
                f,
                Finding::Hold(Hold::Floor {
                    fact: Fact::OpenFiles,
                    ..
                })
            )),
            "{found:?} with {counts:?}"
        );
    }

    fn healthy() -> MachineSnapshot {
        MachineSnapshot {
            os: "macos".into(),
            arch: "arm64".into(),
            cpus: Known::Known(8),
            load_average_1m: Some(1.0),
            mem_total_bytes: Known::Known(16 << 30),
            mem_available_bytes: Known::Known(8 << 30),
            disk_free_bytes: Known::Known(200 << 30),
            disk_total_bytes: Known::Known(500 << 30),
            gpu: machine::GpuClass::None,
            max_open_files: Known::Known(256),
            max_processes: Known::Known(8000),
        }
    }

    /// A default macOS Terminal has a soft `RLIMIT_NOFILE` of 256, below the
    /// open files of the user. The open file guard reads the limit of the
    /// system, so it does not hold there.
    #[cfg(unix)]
    #[test]
    fn a_soft_open_file_limit_of_256_holds_nothing() {
        let exe = std::env::current_exe().unwrap();
        let out = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                r#"ulimit -Sn 256 && exec "$0" --exact --ignored --nocapture "$1""#,
            ])
            .arg(&exe)
            .arg(
                "fleetd::fleet_runtime::application::holds::tests::child_with_a_soft_open_file_limit_of_256",
            )
            .output()
            .unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{text}");
        assert!(text.contains("1 passed"), "{text}");
    }
}
