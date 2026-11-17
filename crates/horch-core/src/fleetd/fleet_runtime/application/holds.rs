//! The holds on new dispatch: the 5 health guards, pacing of a pool, and the
//! events of a global hold (OR-4, OR-5).
//!
//! In Phase 1 the dispatch points are `horch spawn` and `horch ledger spawn`
//! (`docs/tickets/fleetd-p1/MAP.md`, Q16). Each calls [`holds`] and refuses
//! with each hold and its cause. The reconcile pass calls
//! [`record_hold_changes`] in each pass (ticket 21).

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::adapters::config_file;
use crate::fleetd::fleet_runtime::domain::health::{
    self, Fact, Finding, Floors, Hold, PaceRule, PoolWindow, Reading, Readings,
};
use crate::routing::quota::{model_family, pool_for, QuotaView, State};
use crate::runtime::machine::{self, Known, MachineSnapshot, ProbeBins};
use crate::runtime::RuntimeContext;
use crate::taskledger::model::{Actor, EventKind};

/// The readings of the platform beside [`MachineSnapshot`]: the 2 counts of
/// the user and the open file limit of the system. `None` when the read
/// fails or the platform does not support it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub user_processes: Option<u64>,
    pub user_open_files: Option<u64>,
    pub open_file_limit: Option<u64>,
}

impl Counts {
    /// The counts of this machine now.
    pub fn read() -> Self {
        use horch_platform::machine as m;
        Counts {
            user_processes: m::user_process_count().ok().flatten(),
            user_open_files: m::user_open_file_count().ok().flatten(),
            open_file_limit: m::system_open_file_limit().ok().flatten(),
        }
    }
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
        user_processes: counts.user_processes.into(),
        process_limit: known(snapshot.max_processes),
        user_open_files: counts.user_open_files.into(),
        open_file_limit: counts.open_file_limit.into(),
    }
}

/// The holds and unknowns of the health guards now. The machine probe reads
/// the disk of the project, or the fixture `HORCH_MACHINE_FILE`; the floors
/// come from `<fleet root>/fleet.toml`.
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
    Ok(health::health(&r, &Floors::from(&config)))
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

/// The global holds (the health floors) of `holds`, with their causes.
fn floors_of(holds: &[Hold]) -> Vec<(Fact, &str)> {
    holds
        .iter()
        .filter_map(|h| match h {
            Hold::Floor { fact, cause } => Some((*fact, cause.as_str())),
            Hold::Pacing { .. } => None,
        })
        .collect()
}

/// Record 1 `hold_started` event for each global hold of `now` that is not
/// in `before`, and 1 `hold_ended` event for each global hold of `before`
/// that is not in `now`, each with its cause. A pacing hold is not global
/// and records nothing.
pub fn record_hold_changes(tx: &Connection, before: &[Hold], now: &[Hold]) -> Result<()> {
    let (before, now) = (floors_of(before), floors_of(now));
    let has = |list: &[(Fact, &str)], fact: Fact| list.iter().any(|(f, _)| *f == fact);
    for (fact, cause) in &now {
        if !has(&before, *fact) {
            append(tx, EventKind::HoldStarted, *fact, cause)?;
        }
    }
    for (fact, cause) in &before {
        if !has(&now, *fact) {
            append(tx, EventKind::HoldEnded, *fact, cause)?;
        }
    }
    Ok(())
}

fn append(tx: &Connection, kind: EventKind, fact: Fact, cause: &str) -> Result<()> {
    let data = serde_json::json!({ "fact": fact.as_str() }).to_string();
    tx.execute(
        "INSERT INTO events (at, actor, kind, text, cause, data_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            crate::taskledger::util::now(),
            Actor::Horch.as_str(),
            kind.as_str(),
            format!("{}: {cause}", kind.as_str()),
            cause,
            data
        ],
    )
    .with_context(|| format!("recording the {} event of {}", kind.as_str(), fact.as_str()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
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
        let cause = disk.cause().to_string();
        let data = r#"{"fact":"free_disk"}"#.to_string();
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

    /// The guards with injected readings: a reading holds past its floor,
    /// and no reading (a denied sysctl) is unknown with its reason.
    #[test]
    fn injected_readings_hold_past_the_floor_and_none_is_unknown_with_a_reason() {
        let floors = Floors::from(&FleetConfig::default());
        assert_eq!(macos_free_memory(Some(8 << 30)), Reading::Known(8 << 30));
        assert_eq!(macos_free_memory(None), Reading::Unknown(MACOS_MEMORY));
        let mac = healthy();
        let counts = |limit: Option<u64>| Counts {
            user_processes: Some(500),
            user_open_files: Some(901),
            open_file_limit: limit,
        };
        // 901 open files of the user against a system limit of 1000.
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
