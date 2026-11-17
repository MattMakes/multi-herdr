//! The health guards and pacing (OR-4, OR-5).
//!
//! `docs/specs/fleetd/design.md`, "Worker profiles and adapters" (No budget,
//! no cap, and the model guide) and "Supervision, liveness and
//! reconciliation" (Holds on dispatch are visible). The 5 health guards
//! pause new dispatch while 1 reading of the machine probe is past its
//! floor, and dispatch resumes by itself when the reading comes back. They
//! are not a cap on parallel work (D8). A reading that the probe does not
//! know is neither a pass nor a hold: [`health`] reports it as
//! [`Finding::Unknown`] (`preservation-06`, row S12). Pacing holds a pool
//! whose usage runs ahead of its window.
//!
//! This module is pure: the inputs are readings and floors, the output is a
//! list of holds and unknowns.

use super::config::FleetConfig;

/// 1 GB of the disk floor, in bytes (binary).
const GB: u64 = 1 << 30;
/// 1 MB of the memory floor, in bytes (binary).
const MB: u64 = 1 << 20;

/// Why a probe with no value is unknown.
pub const NO_READING: &str = "the probe has no reading";

/// 1 reading of the machine probe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reading<T> {
    Known(T),
    /// The probe has no safe value, and why: the read failed, the platform
    /// does not support it, or the value does not mean what the guard needs.
    Unknown(&'static str),
}

impl<T> From<Option<T>> for Reading<T> {
    fn from(value: Option<T>) -> Self {
        value.map_or(Reading::Unknown(NO_READING), Reading::Known)
    }
}

/// The readings of the 5 health guards. Each count has a limit of the same
/// scope: the processes of the user with the process limit of the user,
/// and the open files of the user with the open file limit of the system.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Readings {
    pub free_disk_bytes: Reading<u64>,
    pub load_1m: Reading<f64>,
    pub cpus: Reading<u32>,
    pub free_memory_bytes: Reading<u64>,
    pub user_processes: Reading<u64>,
    pub process_limit: Reading<u64>,
    pub user_open_files: Reading<u64>,
    pub open_file_limit: Reading<u64>,
}

/// The floors of the health guards, from `fleet.toml`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Floors {
    pub min_free_disk_gb: u64,
    pub max_load_per_cpu: f64,
    pub min_free_memory_mb: u64,
    pub max_process_share: f64,
    pub max_open_file_share: f64,
}

impl From<&FleetConfig> for Floors {
    fn from(c: &FleetConfig) -> Self {
        Floors {
            min_free_disk_gb: c.health_min_free_disk_gb,
            max_load_per_cpu: c.health_max_load_per_cpu,
            min_free_memory_mb: c.health_min_free_memory_mb,
            max_process_share: c.health_max_process_share,
            max_open_file_share: c.health_max_open_file_share,
        }
    }
}

/// The fact that 1 health guard reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fact {
    FreeDisk,
    Load,
    FreeMemory,
    Processes,
    OpenFiles,
}

impl Fact {
    /// The name of the fact in an event and a note.
    pub fn as_str(self) -> &'static str {
        match self {
            Fact::FreeDisk => "free_disk",
            Fact::Load => "load",
            Fact::FreeMemory => "free_memory",
            Fact::Processes => "processes",
            Fact::OpenFiles => "open_files",
        }
    }

    /// The `fleet.toml` key of the floor of the fact.
    pub fn key(self) -> &'static str {
        match self {
            Fact::FreeDisk => "health_min_free_disk_gb",
            Fact::Load => "health_max_load_per_cpu",
            Fact::FreeMemory => "health_min_free_memory_mb",
            Fact::Processes => "health_max_process_share",
            Fact::OpenFiles => "health_max_open_file_share",
        }
    }
}

/// A hold on new dispatch, with its cause: the floor and the reading.
#[derive(Debug, Clone, PartialEq)]
pub enum Hold {
    /// A health floor: a global hold.
    Floor { fact: Fact, cause: String },
    /// Pacing of 1 pool: it holds only the dispatch to that pool.
    Pacing { pool: String, cause: String },
}

impl Hold {
    pub fn cause(&self) -> &str {
        match self {
            Hold::Floor { cause, .. } | Hold::Pacing { cause, .. } => cause,
        }
    }
}

/// 1 output of [`health`].
#[derive(Debug, Clone, PartialEq)]
pub enum Finding {
    Hold(Hold),
    /// The probe has no safe reading for the fact: no hold, and no pass.
    Unknown {
        fact: Fact,
        why: &'static str,
    },
}

/// The holds and the unknowns of the 5 health guards. A guard that passes
/// gives nothing.
///
/// Free disk and free memory hold below their floor. Load holds at or above
/// `max_load_per_cpu` for each CPU, with the `>=` of the load note of
/// `horch spawn`. The 2 counts hold above their share of the limit.
pub fn health(r: &Readings, f: &Floors) -> Vec<Finding> {
    [
        free_disk(r, f),
        load(r, f),
        free_memory(r, f),
        share(
            Fact::Processes,
            r.user_processes,
            r.process_limit,
            f.max_process_share,
        ),
        share(
            Fact::OpenFiles,
            r.user_open_files,
            r.open_file_limit,
            f.max_open_file_share,
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn unknown(fact: Fact, why: &'static str) -> Option<Finding> {
    Some(Finding::Unknown { fact, why })
}

fn hold(fact: Fact, cause: String) -> Option<Finding> {
    Some(Finding::Hold(Hold::Floor {
        fact,
        cause: format!("{cause} ({})", fact.key()),
    }))
}

fn free_disk(r: &Readings, f: &Floors) -> Option<Finding> {
    let free = match r.free_disk_bytes {
        Reading::Known(free) => free,
        Reading::Unknown(why) => return unknown(Fact::FreeDisk, why),
    };
    let floor = f.min_free_disk_gb.saturating_mul(GB);
    (free < floor).then_some(())?;
    hold(
        Fact::FreeDisk,
        format!(
            "free disk {:.1} GB is below the floor of {} GB",
            free as f64 / GB as f64,
            f.min_free_disk_gb
        ),
    )
}

fn load(r: &Readings, f: &Floors) -> Option<Finding> {
    let (load, cpus) = match (r.load_1m, r.cpus) {
        (Reading::Known(load), Reading::Known(cpus)) if load.is_finite() && cpus > 0 => {
            (load, cpus)
        }
        (Reading::Unknown(why), _) | (_, Reading::Unknown(why)) => return unknown(Fact::Load, why),
        _ => return unknown(Fact::Load, NO_READING),
    };
    let limit = f.max_load_per_cpu * f64::from(cpus);
    (load >= limit).then_some(())?;
    hold(
        Fact::Load,
        format!(
            "1-minute load {load:.2} is at or above {limit:.2} ({} for each of {cpus} CPUs)",
            f.max_load_per_cpu
        ),
    )
}

fn free_memory(r: &Readings, f: &Floors) -> Option<Finding> {
    let free = match r.free_memory_bytes {
        Reading::Known(free) => free,
        Reading::Unknown(why) => return unknown(Fact::FreeMemory, why),
    };
    let floor = f.min_free_memory_mb.saturating_mul(MB);
    (free < floor).then_some(())?;
    hold(
        Fact::FreeMemory,
        format!(
            "free memory {} MB is below the floor of {} MB",
            free / MB,
            f.min_free_memory_mb
        ),
    )
}

fn share(fact: Fact, count: Reading<u64>, limit: Reading<u64>, max: f64) -> Option<Finding> {
    let (count, limit) = match (count, limit) {
        (Reading::Known(count), Reading::Known(limit)) if limit > 0 => (count, limit),
        (Reading::Unknown(why), _) | (_, Reading::Unknown(why)) => return unknown(fact, why),
        _ => return unknown(fact, NO_READING),
    };
    let ceiling = max * limit as f64;
    (count as f64 > ceiling).then_some(())?;
    let what = match fact {
        Fact::Processes => "processes of the user",
        _ => "open files of the user",
    };
    hold(
        fact,
        format!("{count} {what} is above {max} of the limit {limit}"),
    )
}

/// 1 usage window of a pool, as pacing reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolWindow {
    /// The label of the window: `5h`, `7d`, `7d:fable`.
    pub label: String,
    /// `used / elapsed` of the window. `None` when the reset time is not
    /// known.
    pub pace: Option<f64>,
    /// The fraction of the window that is used, 0 to 1.
    pub used: f64,
}

/// The pacing rule of today (`routing/policy.rs`): a window paces its pool
/// when its pace is above `pace_factor` and its use is at least
/// `pace_min_used`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaceRule {
    pub pace_factor: f64,
    pub pace_min_used: f64,
}

/// The pacing hold of `pool`: its first window that runs ahead of its
/// window (OR-4). `None` when each window is on pace, so the usage of the
/// pool is consumed by the reset.
pub fn pacing(pool: &str, windows: &[PoolWindow], rule: &PaceRule) -> Option<Hold> {
    windows.iter().find_map(|w| {
        let pace = w.pace.filter(|p| *p > rule.pace_factor)?;
        (w.used >= rule.pace_min_used).then(|| Hold::Pacing {
            pool: pool.to_string(),
            cause: format!(
                "pool {pool} window {} is {:.0}% used at pace {pace:.1}x, above {}x",
                w.label,
                w.used * 100.0,
                rule.pace_factor
            ),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floors() -> Floors {
        Floors::from(&FleetConfig::default())
    }

    /// A healthy machine for the default floors.
    fn normal() -> Readings {
        Readings {
            free_disk_bytes: Reading::Known(200 * GB),
            load_1m: Reading::Known(2.0),
            cpus: Reading::Known(8),
            free_memory_bytes: Reading::Known(8 * 1024 * MB),
            user_processes: Reading::Known(500),
            process_limit: Reading::Known(8000),
            user_open_files: Reading::Known(24_000),
            open_file_limit: Reading::Known(368_640),
        }
    }

    fn holds(r: &Readings) -> Vec<Hold> {
        health(r, &floors())
            .into_iter()
            .filter_map(|f| match f {
                Finding::Hold(h) => Some(h),
                Finding::Unknown { .. } => None,
            })
            .collect()
    }

    fn facts(r: &Readings) -> Vec<Fact> {
        holds(r)
            .into_iter()
            .map(|h| match h {
                Hold::Floor { fact, .. } => fact,
                Hold::Pacing { .. } => panic!("health gives no pacing hold"),
            })
            .collect()
    }

    #[test]
    fn low_disk_probe_pauses_dispatch_and_normal_probe_resumes() {
        let low = Readings {
            free_disk_bytes: Reading::Known(2 * GB),
            ..normal()
        };
        let held = holds(&low);
        assert_eq!(held.len(), 1, "{held:?}");
        assert_eq!(
            held[0].cause(),
            "free disk 2.0 GB is below the floor of 10 GB (health_min_free_disk_gb)"
        );
        assert!(health(&normal(), &floors()).is_empty());
    }

    #[test]
    fn each_floor_pauses_only_past_its_value() {
        let f = floors();
        let at = Readings {
            free_disk_bytes: Reading::Known(f.min_free_disk_gb * GB),
            // Load holds at its value (`>=`), as the load note of today.
            load_1m: Reading::Known(31.99),
            cpus: Reading::Known(8),
            free_memory_bytes: Reading::Known(f.min_free_memory_mb * MB),
            user_processes: Reading::Known(900),
            process_limit: Reading::Known(1000),
            user_open_files: Reading::Known(900),
            open_file_limit: Reading::Known(1000),
        };
        assert!(health(&at, &f).is_empty(), "{:?}", health(&at, &f));
        let cases: [(Fact, Readings); 5] = [
            (
                Fact::FreeDisk,
                Readings {
                    free_disk_bytes: Reading::Known(f.min_free_disk_gb * GB - 1),
                    ..at
                },
            ),
            (
                Fact::Load,
                Readings {
                    load_1m: Reading::Known(32.0),
                    ..at
                },
            ),
            (
                Fact::FreeMemory,
                Readings {
                    free_memory_bytes: Reading::Known(f.min_free_memory_mb * MB - 1),
                    ..at
                },
            ),
            (
                Fact::Processes,
                Readings {
                    user_processes: Reading::Known(901),
                    ..at
                },
            ),
            (
                Fact::OpenFiles,
                Readings {
                    user_open_files: Reading::Known(901),
                    ..at
                },
            ),
        ];
        for (fact, past) in cases {
            assert_eq!(facts(&past), vec![fact], "{fact:?}");
        }
    }

    #[test]
    fn pres_06_s12_unknown_probe_neither_pauses_nor_passes() {
        // Windows today: the probe knows nothing.
        let none = Reading::Unknown(NO_READING);
        let unknown = Readings {
            free_disk_bytes: none,
            load_1m: Reading::Unknown(NO_READING),
            cpus: Reading::Unknown(NO_READING),
            free_memory_bytes: none,
            user_processes: none,
            process_limit: none,
            user_open_files: none,
            open_file_limit: none,
        };
        let found = health(&unknown, &floors());
        let expected: Vec<Finding> = [
            Fact::FreeDisk,
            Fact::Load,
            Fact::FreeMemory,
            Fact::Processes,
            Fact::OpenFiles,
        ]
        .into_iter()
        .map(|fact| Finding::Unknown {
            fact,
            why: NO_READING,
        })
        .collect();
        assert_eq!(found, expected);
        // 1 known reading past its floor still holds beside the unknowns.
        let low = Readings {
            free_disk_bytes: Reading::Known(GB),
            ..unknown
        };
        assert_eq!(facts(&low), vec![Fact::FreeDisk]);
    }

    #[test]
    fn pacing_holds_a_pool_ahead_of_its_window_and_frees_one_on_pace() {
        // The numbers of today: `routing/policy.rs`.
        let rule = PaceRule {
            pace_factor: 1.25,
            pace_min_used: 0.50,
        };
        let window = |label: &str, pace: Option<f64>, used: f64| PoolWindow {
            label: label.into(),
            pace,
            used,
        };
        let ahead = [window("5h", Some(1.0), 0.2), window("7d", Some(1.6), 0.8)];
        assert_eq!(
            pacing("claude", &ahead, &rule),
            Some(Hold::Pacing {
                pool: "claude".into(),
                cause: "pool claude window 7d is 80% used at pace 1.6x, above 1.25x".into(),
            })
        );
        for on_pace in [
            [window("7d", Some(1.25), 0.8)],
            [window("7d", Some(1.6), 0.49)],
            [window("7d", None, 0.9)],
        ] {
            assert_eq!(pacing("claude", &on_pace, &rule), None, "{on_pace:?}");
        }
    }
}
