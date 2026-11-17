//! The keys of Phases 1 to 3 of `fleet.toml`, their default or
//! starting values (`docs/specs/fleetd/design.md`, "The numbers of the
//! design", and `docs/tickets/fleetd-p1/MAP.md`, Q1 and Q2), and the range
//! of each.
//!
//! This module does no I/O and parses no text: `adapters/config_file.rs`
//! reads and parses `<fleet root>/fleet.toml` into a [`FleetConfig`], then
//! calls [`FleetConfig::validate`].

/// The hang limit of fleetd, in seconds of awake time. A fixed contract
/// (PL-5), not a key: the time budget of a pass stays below it.
pub const HANG_LIMIT_S: u64 = 30;

/// The Phase 1 to Phase 3 keys of `fleet.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct FleetConfig {
    /// The period of the reconcile pass. Default 5 s.
    pub pass_period_s: u64,
    /// The time budget of 1 pass, below [`HANG_LIMIT_S`]. Starting value 20 s.
    pub pass_time_budget_s: u64,
    /// The time budget of each fact scan in 1 pass. Starting value 2000 ms.
    pub scan_budget_ms: u64,
    /// The rows of each kind that 1 pass handles. Starting value 200.
    pub pass_budget: u64,
    /// The grace before the kill of a process group. Default 20 s.
    pub stop_grace_s: u64,
    /// The retention of a needs-you item that is not open, of a trash ref
    /// and of a `superseded` rescue ref. Default 30 days.
    pub retention_days: u64,
    /// The runs in a row of 1 job key that end `abandoned` before a
    /// needs-you item. Default 3.
    pub abandoned_run_limit: u64,
    /// The backoff before each run after an `abandoned` run. Default 30 s,
    /// 120 s, 480 s.
    pub job_backoff_s: Vec<u64>,
    /// The time limit of a job of each kind. `task_check` has no key: it
    /// uses `CHECK_TIMEOUT` of `taskledger/finish/land.rs`.
    pub job_limit_s: JobLimits,
    /// The free disk floor. Starting value 10 GB.
    pub health_min_free_disk_gb: u64,
    /// The load for each CPU at which new dispatch holds. Starting value
    /// 4.0, a value to tune: a busy fleet keeps a load of 10 to 60 on a Mac
    /// with 14 CPUs. The `load >= cpus` note of `horch spawn`
    /// (`crates/horch/src/cmd/spawn.rs`) stays at 1.0 for each CPU and only
    /// prints.
    pub health_max_load_per_cpu: f64,
    /// The free memory floor. Starting value 1024 MB.
    pub health_min_free_memory_mb: u64,
    /// The share of the process limit of the user. Starting value 0.9.
    pub health_max_process_share: f64,
    /// The share of the open file limit of the system that the open files
    /// of the user may use. Starting value 0.9.
    pub health_max_open_file_share: f64,
    /// The period of the wrapper heartbeat. Starting value 10 s, 1 to 60
    /// (design, "The numbers of the design", "Wrapper heartbeat").
    pub heartbeat_every_s: u64,
    /// The age under which a heartbeat renews a lease. Starting value 30 s,
    /// above `heartbeat_every_s` and at most 300 (design, "Heartbeat age for
    /// a lease renewal").
    pub heartbeat_max_age_s: u64,
    /// The time limit of a question. Starting value 900 s (15 min), 60 to
    /// 86400 (design, "Time limit of a question").
    pub question_limit_s: u64,
    /// The replan limit of a `failed` task. Starting value 1800 s (30 min),
    /// 60 to 86400. The design names it `replan_limit`; the key has the unit
    /// in its name (`docs/tickets/fleetd-p2/MAP.md`, Q1).
    pub replan_limit_s: u64,
    /// The used share of the binding window of a pool at which the pool
    /// counts as limited (RR-11), and below which the pool probe reports ok.
    /// Starting value 0.98, above 0.5 and at most 1.0. Source: today's
    /// `exhausted_used` in `crates/horch-core/src/routing/policy.rs`
    /// (`docs/tickets/fleetd-p2/MAP.md`, Q1).
    pub pool_full_use: f64,
    /// The period of the `worktree_list` job. Default 60 s (design, "Period
    /// of the `worktree_list` job"; RR-10).
    pub worktree_list_period_s: u64,
    /// The worktree disk budget. Default 20 GB (design, "Worktree disk
    /// budget"; FM-28). The design names it `worktree_disk_budget`; the key
    /// has the unit in its name (`docs/tickets/fleetd-p3/MAP.md`, Q20).
    pub worktree_disk_budget_gb: u64,
}

/// The `job_limit_s` table: the time limit of each job kind, in seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobLimits {
    pub worktree_add: u64,
    pub worktree_remove: u64,
    pub worktree_list: u64,
    pub docs_sync: u64,
    pub conformance: u64,
    /// Starting value 600 s (`docs/tickets/fleetd-p3/MAP.md`, Q25).
    pub rescue: u64,
}

impl Default for JobLimits {
    fn default() -> Self {
        JobLimits {
            worktree_add: 600,
            worktree_remove: 600,
            worktree_list: 600,
            docs_sync: 3600,
            conformance: 3600,
            rescue: 600,
        }
    }
}

impl Default for FleetConfig {
    fn default() -> Self {
        FleetConfig {
            pass_period_s: 5,
            pass_time_budget_s: 20,
            scan_budget_ms: 2000,
            pass_budget: 200,
            stop_grace_s: 20,
            retention_days: 30,
            abandoned_run_limit: 3,
            job_backoff_s: vec![30, 120, 480],
            job_limit_s: JobLimits::default(),
            health_min_free_disk_gb: 10,
            health_max_load_per_cpu: 4.0,
            health_min_free_memory_mb: 1024,
            health_max_process_share: 0.9,
            health_max_open_file_share: 0.9,
            heartbeat_every_s: 10,
            heartbeat_max_age_s: 30,
            question_limit_s: 900,
            replan_limit_s: 1800,
            pool_full_use: 0.98,
            worktree_list_period_s: 60,
            worktree_disk_budget_gb: 20,
        }
    }
}

/// A value of `fleet.toml` out of its range. The text names the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

impl FleetConfig {
    /// Refuse a value out of its range, with the name of its key.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let jobs = &self.job_limit_s;
        let positive = [
            ("pass_period_s", self.pass_period_s),
            ("pass_time_budget_s", self.pass_time_budget_s),
            ("scan_budget_ms", self.scan_budget_ms),
            ("pass_budget", self.pass_budget),
            ("stop_grace_s", self.stop_grace_s),
            ("retention_days", self.retention_days),
            ("abandoned_run_limit", self.abandoned_run_limit),
            ("health_min_free_disk_gb", self.health_min_free_disk_gb),
            ("health_min_free_memory_mb", self.health_min_free_memory_mb),
            ("job_limit_s.worktree_add", jobs.worktree_add),
            ("job_limit_s.worktree_remove", jobs.worktree_remove),
            ("job_limit_s.worktree_list", jobs.worktree_list),
            ("job_limit_s.docs_sync", jobs.docs_sync),
            ("job_limit_s.conformance", jobs.conformance),
            ("job_limit_s.rescue", jobs.rescue),
            ("worktree_list_period_s", self.worktree_list_period_s),
            ("worktree_disk_budget_gb", self.worktree_disk_budget_gb),
        ];
        for (key, v) in positive {
            if v == 0 {
                return refuse(format!("{key} must be at least 1, got 0"));
            }
        }
        if self.pass_time_budget_s >= HANG_LIMIT_S {
            return refuse(format!(
                "pass_time_budget_s must be below the hang limit of {HANG_LIMIT_S} s, got {}",
                self.pass_time_budget_s
            ));
        }
        if self.scan_budget_ms >= self.pass_time_budget_s * 1000 {
            return refuse(format!(
                "scan_budget_ms must be below pass_time_budget_s ({} s), got {} ms",
                self.pass_time_budget_s, self.scan_budget_ms
            ));
        }
        if self.job_backoff_s.is_empty() || self.job_backoff_s.contains(&0) {
            return refuse(format!(
                "job_backoff_s must be a list of values of at least 1, got {:?}",
                self.job_backoff_s
            ));
        }
        if !(self.health_max_load_per_cpu.is_finite() && self.health_max_load_per_cpu > 0.0) {
            return refuse(format!(
                "health_max_load_per_cpu must be above 0, got {}",
                self.health_max_load_per_cpu
            ));
        }
        for (key, v) in [
            ("health_max_process_share", self.health_max_process_share),
            (
                "health_max_open_file_share",
                self.health_max_open_file_share,
            ),
        ] {
            if !(v > 0.0 && v <= 1.0) {
                return refuse(format!("{key} must be above 0 and at most 1, got {v}"));
            }
        }
        for (key, v, min, max) in [
            ("heartbeat_every_s", self.heartbeat_every_s, 1, 60),
            ("question_limit_s", self.question_limit_s, 60, 86_400),
            ("replan_limit_s", self.replan_limit_s, 60, 86_400),
        ] {
            if !(min..=max).contains(&v) {
                return refuse(format!("{key} must be {min} to {max}, got {v}"));
            }
        }
        if self.heartbeat_max_age_s <= self.heartbeat_every_s || self.heartbeat_max_age_s > 300 {
            return refuse(format!(
                "heartbeat_max_age_s must be above heartbeat_every_s ({} s) and at most 300, got {}",
                self.heartbeat_every_s, self.heartbeat_max_age_s
            ));
        }
        if !(self.pool_full_use > 0.5 && self.pool_full_use <= 1.0) {
            return refuse(format!(
                "pool_full_use must be above 0.5 and at most 1, got {}",
                self.pool_full_use
            ));
        }
        Ok(())
    }
}

fn refuse(text: String) -> Result<(), ConfigError> {
    Err(ConfigError(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_numbers_of_the_design() {
        let c = FleetConfig::default();
        assert_eq!(c.pass_period_s, 5);
        assert_eq!(c.pass_time_budget_s, 20);
        assert_eq!(c.scan_budget_ms, 2000);
        assert_eq!(c.pass_budget, 200);
        assert_eq!(c.stop_grace_s, 20);
        assert_eq!(c.retention_days, 30);
        assert_eq!(c.abandoned_run_limit, 3);
        assert_eq!(c.job_backoff_s, vec![30, 120, 480]);
        assert_eq!(
            c.job_limit_s,
            JobLimits {
                worktree_add: 600,
                worktree_remove: 600,
                worktree_list: 600,
                docs_sync: 3600,
                conformance: 3600,
                rescue: 600,
            }
        );
        assert_eq!(c.health_min_free_disk_gb, 10);
        assert_eq!(c.health_max_load_per_cpu, 4.0);
        assert_eq!(c.health_min_free_memory_mb, 1024);
        assert_eq!(c.health_max_process_share, 0.9);
        assert_eq!(c.health_max_open_file_share, 0.9);
        assert_eq!(HANG_LIMIT_S, 30);
        assert_eq!(c.validate(), Ok(()));
    }

    #[test]
    fn pass_time_budget_at_or_over_the_hang_limit_is_refused() {
        for s in [HANG_LIMIT_S, HANG_LIMIT_S + 1] {
            let c = FleetConfig {
                pass_time_budget_s: s,
                ..FleetConfig::default()
            };
            let err = c.validate().unwrap_err();
            assert!(err.to_string().contains("pass_time_budget_s"), "{err}");
        }
        let c = FleetConfig {
            pass_time_budget_s: HANG_LIMIT_S - 1,
            ..FleetConfig::default()
        };
        assert_eq!(c.validate(), Ok(()));
    }

    #[test]
    fn a_value_out_of_range_is_refused_with_its_key() {
        let cases: [(&str, FleetConfig); 8] = [
            (
                "pass_period_s",
                FleetConfig {
                    pass_period_s: 0,
                    ..Default::default()
                },
            ),
            (
                "scan_budget_ms",
                FleetConfig {
                    scan_budget_ms: 20_000,
                    ..Default::default()
                },
            ),
            (
                "job_backoff_s",
                FleetConfig {
                    job_backoff_s: vec![],
                    ..Default::default()
                },
            ),
            (
                "health_min_free_disk_gb",
                FleetConfig {
                    health_min_free_disk_gb: 0,
                    ..Default::default()
                },
            ),
            (
                "health_min_free_memory_mb",
                FleetConfig {
                    health_min_free_memory_mb: 0,
                    ..Default::default()
                },
            ),
            (
                "health_max_process_share",
                FleetConfig {
                    health_max_process_share: 1.5,
                    ..Default::default()
                },
            ),
            (
                "health_max_load_per_cpu",
                FleetConfig {
                    health_max_load_per_cpu: 0.0,
                    ..Default::default()
                },
            ),
            (
                "job_limit_s.docs_sync",
                FleetConfig {
                    job_limit_s: JobLimits {
                        docs_sync: 0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
        ];
        for (key, c) in cases {
            let err = c.validate().unwrap_err();
            assert!(err.to_string().contains(key), "{key}: {err}");
        }
    }

    #[test]
    fn phase2_keys_have_their_defaults() {
        let c = FleetConfig::default();
        assert_eq!(c.heartbeat_every_s, 10);
        assert_eq!(c.heartbeat_max_age_s, 30);
        assert_eq!(c.question_limit_s, 900);
        assert_eq!(c.replan_limit_s, 1800);
        assert_eq!(c.pool_full_use, 0.98);
    }

    #[test]
    fn heartbeat_max_age_must_be_above_heartbeat_every() {
        for age in [10, 9] {
            let c = FleetConfig {
                heartbeat_every_s: 10,
                heartbeat_max_age_s: age,
                ..FleetConfig::default()
            };
            let err = c.validate().unwrap_err().to_string();
            assert!(err.contains("heartbeat_max_age_s"), "{err}");
            assert!(err.contains("heartbeat_every_s"), "{err}");
        }
        let c = FleetConfig {
            heartbeat_every_s: 10,
            heartbeat_max_age_s: 11,
            ..FleetConfig::default()
        };
        assert_eq!(c.validate(), Ok(()));
    }

    #[test]
    fn pool_full_use_out_of_range_is_refused() {
        for v in [0.5, 0.0, 1.01, f64::NAN] {
            let c = FleetConfig {
                pool_full_use: v,
                ..FleetConfig::default()
            };
            let err = c.validate().unwrap_err().to_string();
            assert!(err.contains("pool_full_use"), "{v}: {err}");
        }
        for v in [0.51, 1.0] {
            let c = FleetConfig {
                pool_full_use: v,
                ..FleetConfig::default()
            };
            assert_eq!(c.validate(), Ok(()), "{v}");
        }
    }

    #[test]
    fn a_phase2_value_out_of_range_is_refused_with_its_key() {
        let d = FleetConfig::default;
        let cases = [
            (
                "heartbeat_every_s",
                FleetConfig {
                    heartbeat_every_s: 0,
                    ..d()
                },
            ),
            (
                "heartbeat_every_s",
                FleetConfig {
                    heartbeat_every_s: 61,
                    heartbeat_max_age_s: 300,
                    ..d()
                },
            ),
            (
                "heartbeat_max_age_s",
                FleetConfig {
                    heartbeat_max_age_s: 301,
                    ..d()
                },
            ),
            (
                "question_limit_s",
                FleetConfig {
                    question_limit_s: 59,
                    ..d()
                },
            ),
            (
                "question_limit_s",
                FleetConfig {
                    question_limit_s: 86_401,
                    ..d()
                },
            ),
            (
                "replan_limit_s",
                FleetConfig {
                    replan_limit_s: 59,
                    ..d()
                },
            ),
            (
                "replan_limit_s",
                FleetConfig {
                    replan_limit_s: 86_401,
                    ..d()
                },
            ),
        ];
        for (key, c) in cases {
            let err = c.validate().unwrap_err().to_string();
            assert!(err.contains(key), "{key}: {err}");
        }
        let edges = [
            FleetConfig {
                heartbeat_every_s: 1,
                ..d()
            },
            FleetConfig {
                heartbeat_every_s: 60,
                heartbeat_max_age_s: 300,
                ..d()
            },
            FleetConfig {
                question_limit_s: 60,
                replan_limit_s: 86_400,
                ..d()
            },
            FleetConfig {
                question_limit_s: 86_400,
                replan_limit_s: 60,
                ..d()
            },
        ];
        for c in edges {
            assert_eq!(c.validate(), Ok(()), "{c:?}");
        }
    }

    #[test]
    fn worktree_list_period_s_defaults_to_60() {
        assert_eq!(FleetConfig::default().worktree_list_period_s, 60);
    }

    #[test]
    fn worktree_disk_budget_gb_defaults_to_20() {
        assert_eq!(FleetConfig::default().worktree_disk_budget_gb, 20);
    }

    #[test]
    fn worktree_disk_budget_gb_of_0_is_refused() {
        let c = FleetConfig {
            worktree_disk_budget_gb: 0,
            ..FleetConfig::default()
        };
        let err = c.validate().unwrap_err().to_string();
        assert!(err.contains("worktree_disk_budget_gb"), "{err}");
        let c = FleetConfig {
            worktree_disk_budget_gb: 1,
            ..FleetConfig::default()
        };
        assert_eq!(c.validate(), Ok(()));
    }

    #[test]
    fn job_limit_s_rescue_defaults_to_600() {
        assert_eq!(FleetConfig::default().job_limit_s.rescue, 600);
    }
}
