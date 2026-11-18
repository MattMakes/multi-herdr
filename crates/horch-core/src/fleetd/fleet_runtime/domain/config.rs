//! The keys of Phases 1 to 6 of `fleet.toml`, their default or
//! starting values (`docs/specs/fleetd/design.md`, "The numbers of the
//! design", and `docs/tickets/fleetd-p1/MAP.md`, Q1 and Q2), and the range
//! of each.
//!
//! This module does no I/O and parses no TOML: `adapters/config_file.rs`
//! reads and parses `<fleet root>/fleet.toml` into a [`FleetConfig`], then
//! calls [`FleetConfig::validate`]. It parses only the value of the key
//! `working_hours` ([`WorkingHours::parse`]).

/// The hang limit of fleetd, in seconds of awake time. A fixed contract
/// (PL-5), not a key: the time budget of a pass stays below it.
pub const HANG_LIMIT_S: u64 = 30;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

/// The Phase 1 to Phase 6 keys of `fleet.toml`.
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
    /// The size limit of a question file. Starting value 64 KiB (design,
    /// Protocol rules, rule 7; SR-6).
    pub question_max_bytes: u64,
    /// The size limit of a request file and of a request line. Starting
    /// value 64 KiB (rule 7; `docs/tickets/fleetd-p5/MAP.md`, Q15).
    pub request_max_bytes: u64,
    /// The size limit of `heartbeat.json`. Starting value 4 KiB (rule 7).
    pub heartbeat_max_bytes: u64,
    /// The size limit of `outbox/progress.jsonl`. Starting value 8 MiB
    /// (rule 7).
    pub progress_max_bytes: u64,
    /// The line limit of `outbox/progress.jsonl`. Starting value 10000
    /// (rule 7).
    pub progress_max_lines: u64,
    /// The size limit of 1 line of `outbox/progress.jsonl`. Starting value
    /// 4 KiB (rule 7).
    pub progress_line_max_bytes: u64,
    /// The tail of `transcript.jsonl` that fleetd reads. Starting value
    /// 256 KiB (rule 7).
    pub transcript_tail_bytes: u64,
    /// The size limit of 1 line of `transcript.jsonl`. Starting value 1 MiB
    /// (rule 7).
    pub transcript_line_max_bytes: u64,
    /// The size limit of a job result file. Starting value 64 KiB
    /// (`docs/tickets/fleetd-p5/MAP.md`, Q14).
    pub job_result_max_bytes: u64,
    /// The personas whose attempt with a null task is an orchestrator seat.
    /// fleetd knows a seat only from the ledger and this key, never from an
    /// environment variable (`docs/tickets/fleetd-p5/MAP.md`, Q2). Default: the
    /// list `FLEET_ORCHESTRATORS` of `roster`. A domain module imports no
    /// other module, so a test of `adapters/config_file.rs` pins the copy.
    pub orchestrator_seats: Vec<String>,
    /// The period of the `fetch` job of each repository. Default 60 s
    /// (design, "Between features"; `docs/tickets/fleetd-p4/MAP.md`, Q6).
    pub fetch_period_s: u64,
    /// The failed `push` runs in a row before a needs-you item. Default 3
    /// (design, "The numbers of the design").
    pub push_failure_limit: u64,
    /// The budget of the `unique` rescue refs. Over it, 1 item goes on the
    /// needs-you list and nothing is deleted (design, "No work is lost").
    /// Starting value 5120 MB: a starting value, to tune from real runs.
    pub rescue_unique_budget_mb: u64,
    /// The time limit of the landing state `ready`. Starting value 1800 s
    /// (`docs/tickets/fleetd-p4/MAP.md`, Q6): a starting value, to tune from
    /// real runs.
    pub landing_ready_limit_s: u64,
    /// The time limit of the landing state `repair`. Starting value 3600 s
    /// (Q6): a starting value, to tune from real runs.
    pub landing_repair_limit_s: u64,
    /// The margin above the job limits for a landing state that waits on a
    /// job. Starting value 600 s (Q6): a starting value, to tune from real
    /// runs.
    pub landing_job_margin_s: u64,
    /// The time limit of a bundle in the state `passed`. Starting value
    /// 600 s (Q6): a starting value, to tune from real runs.
    pub bundle_passed_limit_s: u64,
    /// The size limit of `result.json` of a worker. Starting value 64 KiB
    /// (Q7; SR-6): a starting value, to tune from real runs.
    pub result_max_bytes: u64,
    /// The changed lines of a resolver task at most. Starting value 100
    /// (Q7): a starting value, to tune from real runs.
    pub resolver_max_lines: u64,
    /// The files of a resolver task at most. Starting value 2 (Q7): a
    /// starting value, to tune from real runs.
    pub resolver_max_files: u64,
    /// The time after which fleetd stops an idle sub-orchestrator process
    /// of an idea that waits on you. Default 1800 s, 60 to 86400, on the
    /// fleet clock (design, "Stop of an idle sub-orchestrator"; FM-29, PL-4).
    pub idle_stop_s: u64,
    /// The days of calendar time with no answer after which an idea that
    /// waits on you moves to `parked`. Default 7, 1 to 365 (design, "Park of
    /// an idea that waits on you"; FM-29, PL-4).
    pub park_after_days: u64,
    /// The period of the reminder of an idea that waits on you, in seconds
    /// of calendar time. Default 3600, 600 to 86400 (FM-29, PL-4).
    pub reminder_every_s: u64,
    /// The hours in which fleetd sends a reminder, in the local time zone
    /// of the machine. Starting value `Mon-Fri 09:00-18:00`
    /// (`docs/tickets/fleetd-p6/MAP.md`, Q3).
    pub working_hours: WorkingHours,
}

/// A day of the week.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

const WEEKDAYS: [(&str, Weekday); 7] = [
    ("Mon", Weekday::Mon),
    ("Tue", Weekday::Tue),
    ("Wed", Weekday::Wed),
    ("Thu", Weekday::Thu),
    ("Fri", Weekday::Fri),
    ("Sat", Weekday::Sat),
    ("Sun", Weekday::Sun),
];

/// The value of `working_hours`: a day range and an hour range, as
/// `Mon-Fri 09:00-18:00`. The day range includes both days and can pass the
/// end of the week (`Fri-Mon`). The hour range includes its start and not
/// its end, and its start is before its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkingHours {
    first_day: Weekday,
    last_day: Weekday,
    start_minute: u16,
    end_minute: u16,
}

impl WorkingHours {
    /// Parse the form `Ddd-Ddd HH:MM-HH:MM`. The text of an error names the
    /// key and the form.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let bad = || {
            ConfigError(format!(
                "working_hours must have the form \"Mon-Fri 09:00-18:00\", got {text:?}"
            ))
        };
        let (days, hours) = text.split_once(' ').ok_or_else(bad)?;
        let (first, last) = days.split_once('-').ok_or_else(bad)?;
        let (start, end) = hours.split_once('-').ok_or_else(bad)?;
        let hours = WorkingHours {
            first_day: weekday(first).ok_or_else(bad)?,
            last_day: weekday(last).ok_or_else(bad)?,
            start_minute: minute_of_day(start).ok_or_else(bad)?,
            end_minute: minute_of_day(end).ok_or_else(bad)?,
        };
        if hours.start_minute >= hours.end_minute {
            return Err(bad());
        }
        Ok(hours)
    }

    /// Whether the local `day` at the local `minute` of the day (0 to 1439)
    /// is inside the working hours.
    pub fn contains(&self, day: Weekday, minute: u16) -> bool {
        let (first, last, d) = (self.first_day as u8, self.last_day as u8, day as u8);
        let in_days = if first <= last {
            (first..=last).contains(&d)
        } else {
            d >= first || d <= last
        };
        in_days && (self.start_minute..self.end_minute).contains(&minute)
    }
}

fn weekday(text: &str) -> Option<Weekday> {
    WEEKDAYS
        .iter()
        .find(|(name, _)| *name == text)
        .map(|(_, d)| *d)
}

/// `HH:MM` with 2 digits each, from `00:00` to `23:59`.
fn minute_of_day(text: &str) -> Option<u16> {
    let (h, m) = text.split_once(':')?;
    let two_digits = |s: &str| s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit());
    if !(two_digits(h) && two_digits(m)) {
        return None;
    }
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
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
    /// Starting value 1200 s (`docs/tickets/fleetd-p4/MAP.md`, Q-B): a
    /// starting value, to tune from real runs.
    pub fast_gate: u64,
    /// Starting value 3600 s (Q-B): a starting value, to tune from real runs.
    pub full_gate: u64,
    /// Starting value 180 s (Q-B): a starting value, to tune from real runs.
    pub trial_merge: u64,
    /// Starting value 600 s (Q-B): a starting value, to tune from real runs.
    pub bundle_build: u64,
    /// Starting value 600 s (Q-B): a starting value, to tune from real runs.
    pub rebase: u64,
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
            fast_gate: 1200,
            full_gate: 3600,
            trial_merge: 180,
            bundle_build: 600,
            rebase: 600,
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
            question_max_bytes: 64 * KIB,
            request_max_bytes: 64 * KIB,
            heartbeat_max_bytes: 4 * KIB,
            progress_max_bytes: 8 * MIB,
            progress_max_lines: 10_000,
            progress_line_max_bytes: 4 * KIB,
            transcript_tail_bytes: 256 * KIB,
            transcript_line_max_bytes: MIB,
            job_result_max_bytes: 64 * KIB,
            orchestrator_seats: vec!["orchestrator".to_string(), "orchestrator-codex".to_string()],
            fetch_period_s: 60,
            push_failure_limit: 3,
            rescue_unique_budget_mb: 5120,
            landing_ready_limit_s: 1800,
            landing_repair_limit_s: 3600,
            landing_job_margin_s: 600,
            bundle_passed_limit_s: 600,
            result_max_bytes: 64 * KIB,
            resolver_max_lines: 100,
            resolver_max_files: 2,
            idle_stop_s: 1800,
            park_after_days: 7,
            reminder_every_s: 3600,
            working_hours: WorkingHours {
                first_day: Weekday::Mon,
                last_day: Weekday::Fri,
                start_minute: 9 * 60,
                end_minute: 18 * 60,
            },
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
            ("question_max_bytes", self.question_max_bytes),
            ("request_max_bytes", self.request_max_bytes),
            ("heartbeat_max_bytes", self.heartbeat_max_bytes),
            ("progress_max_bytes", self.progress_max_bytes),
            ("progress_max_lines", self.progress_max_lines),
            ("progress_line_max_bytes", self.progress_line_max_bytes),
            ("transcript_tail_bytes", self.transcript_tail_bytes),
            ("transcript_line_max_bytes", self.transcript_line_max_bytes),
            ("job_result_max_bytes", self.job_result_max_bytes),
            ("job_limit_s.fast_gate", jobs.fast_gate),
            ("job_limit_s.full_gate", jobs.full_gate),
            ("job_limit_s.trial_merge", jobs.trial_merge),
            ("job_limit_s.bundle_build", jobs.bundle_build),
            ("job_limit_s.rebase", jobs.rebase),
            ("fetch_period_s", self.fetch_period_s),
            ("push_failure_limit", self.push_failure_limit),
            ("rescue_unique_budget_mb", self.rescue_unique_budget_mb),
            ("landing_ready_limit_s", self.landing_ready_limit_s),
            ("landing_repair_limit_s", self.landing_repair_limit_s),
            ("landing_job_margin_s", self.landing_job_margin_s),
            ("bundle_passed_limit_s", self.bundle_passed_limit_s),
            ("result_max_bytes", self.result_max_bytes),
            ("resolver_max_lines", self.resolver_max_lines),
            ("resolver_max_files", self.resolver_max_files),
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
            ("idle_stop_s", self.idle_stop_s, 60, 86_400),
            ("park_after_days", self.park_after_days, 1, 365),
            ("reminder_every_s", self.reminder_every_s, 600, 86_400),
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
                fast_gate: 1200,
                full_gate: 3600,
                trial_merge: 180,
                bundle_build: 600,
                rebase: 600,
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

    #[test]
    fn phase4_keys_have_the_design_defaults() {
        let c = FleetConfig::default();
        let j = &c.job_limit_s;
        assert_eq!(
            (
                j.fast_gate,
                j.full_gate,
                j.trial_merge,
                j.bundle_build,
                j.rebase
            ),
            (1200, 3600, 180, 600, 600)
        );
        assert_eq!(c.fetch_period_s, 60);
        assert_eq!(c.push_failure_limit, 3);
        assert_eq!(c.rescue_unique_budget_mb, 5120);
        assert_eq!(c.landing_ready_limit_s, 1800);
        assert_eq!(c.landing_repair_limit_s, 3600);
        assert_eq!(c.landing_job_margin_s, 600);
        assert_eq!(c.bundle_passed_limit_s, 600);
        assert_eq!(c.result_max_bytes, 65_536);
        assert_eq!(c.resolver_max_lines, 100);
        assert_eq!(c.resolver_max_files, 2);
        assert_eq!(c.validate(), Ok(()));
    }

    #[test]
    fn a_bad_phase4_key_names_the_key() {
        type SetBad = fn(&mut FleetConfig);
        let cases: [(&str, SetBad); 15] = [
            ("job_limit_s.fast_gate", |c| c.job_limit_s.fast_gate = 0),
            ("job_limit_s.full_gate", |c| c.job_limit_s.full_gate = 0),
            ("job_limit_s.trial_merge", |c| c.job_limit_s.trial_merge = 0),
            ("job_limit_s.bundle_build", |c| {
                c.job_limit_s.bundle_build = 0
            }),
            ("job_limit_s.rebase", |c| c.job_limit_s.rebase = 0),
            ("fetch_period_s", |c| c.fetch_period_s = 0),
            ("push_failure_limit", |c| c.push_failure_limit = 0),
            ("rescue_unique_budget_mb", |c| c.rescue_unique_budget_mb = 0),
            ("landing_ready_limit_s", |c| c.landing_ready_limit_s = 0),
            ("landing_repair_limit_s", |c| c.landing_repair_limit_s = 0),
            ("landing_job_margin_s", |c| c.landing_job_margin_s = 0),
            ("bundle_passed_limit_s", |c| c.bundle_passed_limit_s = 0),
            ("result_max_bytes", |c| c.result_max_bytes = 0),
            ("resolver_max_lines", |c| c.resolver_max_lines = 0),
            ("resolver_max_files", |c| c.resolver_max_files = 0),
        ];
        for (key, set) in cases {
            let mut c = FleetConfig::default();
            set(&mut c);
            let err = c.validate().unwrap_err().to_string();
            assert_eq!(err, format!("{key} must be at least 1, got 0"));
        }
    }
}
