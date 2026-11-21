use std::path::Path;
use std::sync::Once;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::fleetd::fleet_runtime::domain::config::{
    ConfigError, FleetConfig, JobLimits, WorkingHours,
};

pub const FILE_NAME: &str = "fleet.toml";

pub fn load(fleet_root: &Path) -> Result<FleetConfig> {
    let path = fleet_root.join(FILE_NAME);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FleetConfig::default()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let (config, warning) =
        parse_with_warning(&text).with_context(|| format!("reading {}", path.display()))?;
    if let Some(warning) = warning {
        static WARNED: Once = Once::new();
        WARNED.call_once(|| eprintln!("warning: {warning}"));
    }
    Ok(config)
}

pub fn parse(text: &str) -> Result<FleetConfig> {
    parse_with_warning(text).map(|(config, _)| config)
}

pub fn parse_with_warning(text: &str) -> Result<(FleetConfig, Option<String>)> {
    let mut file: FileKeys =
        toml::from_str(text).map_err(|e| anyhow::anyhow!("fleet.toml: {e}"))?;
    let warning = file.orchestrator_seats.take().map(|_| {
        "fleet.toml: the key orchestrator_seats is retired and gives no operator right. \
         Remove it."
            .to_string()
    });
    let config = file
        .into_config()
        .map_err(|e| anyhow::anyhow!("fleet.toml: {e}"))?;
    config
        .validate()
        .map_err(|e| anyhow::anyhow!("fleet.toml: {e}"))?;
    Ok((config, warning))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileKeys {
    pass_period_s: Option<u64>,
    pass_time_budget_s: Option<u64>,
    scan_budget_ms: Option<u64>,
    pass_budget: Option<u64>,
    stop_grace_s: Option<u64>,
    retention_days: Option<u64>,
    abandoned_run_limit: Option<u64>,
    job_backoff_s: Option<Vec<u64>>,
    #[serde(default)]
    job_limit_s: FileJobLimits,
    health_min_free_disk_gb: Option<u64>,
    health_max_load_per_cpu: Option<f64>,
    health_min_free_memory_mb: Option<u64>,
    health_max_process_share: Option<f64>,
    health_max_open_file_share: Option<f64>,
    heartbeat_every_s: Option<u64>,
    heartbeat_max_age_s: Option<u64>,
    question_limit_s: Option<u64>,
    replan_limit_s: Option<u64>,
    pool_full_use: Option<f64>,
    worktree_list_period_s: Option<u64>,
    worktree_disk_budget_gb: Option<u64>,
    question_max_bytes: Option<u64>,
    request_max_bytes: Option<u64>,
    heartbeat_max_bytes: Option<u64>,
    progress_max_bytes: Option<u64>,
    progress_max_lines: Option<u64>,
    progress_line_max_bytes: Option<u64>,
    transcript_tail_bytes: Option<u64>,
    transcript_line_max_bytes: Option<u64>,
    job_result_max_bytes: Option<u64>,
    orchestrator_seats: Option<toml::Value>,
    fetch_period_s: Option<u64>,
    push_failure_limit: Option<u64>,
    rescue_unique_budget_mb: Option<u64>,
    landing_ready_limit_s: Option<u64>,
    landing_repair_limit_s: Option<u64>,
    landing_job_margin_s: Option<u64>,
    bundle_passed_limit_s: Option<u64>,
    result_max_bytes: Option<u64>,
    resolver_max_lines: Option<u64>,
    resolver_max_files: Option<u64>,
    idle_stop_s: Option<u64>,
    park_after_days: Option<u64>,
    reminder_every_s: Option<u64>,
    working_hours: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileJobLimits {
    worktree_add: Option<u64>,
    worktree_remove: Option<u64>,
    worktree_list: Option<u64>,
    docs_sync: Option<u64>,
    conformance: Option<u64>,
    rescue: Option<u64>,
    fast_gate: Option<u64>,
    full_gate: Option<u64>,
    trial_merge: Option<u64>,
    bundle_build: Option<u64>,
    rebase: Option<u64>,
}

impl FileKeys {
    fn into_config(self) -> Result<FleetConfig, ConfigError> {
        let defaults = FleetConfig::default();
        let working_hours = match &self.working_hours {
            Some(text) => WorkingHours::parse(text)?,
            None => defaults.working_hours,
        };
        let file_jobs = self.job_limit_s;
        let default_jobs = defaults.job_limit_s;
        Ok(FleetConfig {
            pass_period_s: self.pass_period_s.unwrap_or(defaults.pass_period_s),
            pass_time_budget_s: self
                .pass_time_budget_s
                .unwrap_or(defaults.pass_time_budget_s),
            scan_budget_ms: self.scan_budget_ms.unwrap_or(defaults.scan_budget_ms),
            pass_budget: self.pass_budget.unwrap_or(defaults.pass_budget),
            stop_grace_s: self.stop_grace_s.unwrap_or(defaults.stop_grace_s),
            retention_days: self.retention_days.unwrap_or(defaults.retention_days),
            abandoned_run_limit: self
                .abandoned_run_limit
                .unwrap_or(defaults.abandoned_run_limit),
            job_backoff_s: self.job_backoff_s.unwrap_or(defaults.job_backoff_s),
            job_limit_s: JobLimits {
                worktree_add: file_jobs.worktree_add.unwrap_or(default_jobs.worktree_add),
                worktree_remove: file_jobs
                    .worktree_remove
                    .unwrap_or(default_jobs.worktree_remove),
                worktree_list: file_jobs
                    .worktree_list
                    .unwrap_or(default_jobs.worktree_list),
                docs_sync: file_jobs.docs_sync.unwrap_or(default_jobs.docs_sync),
                conformance: file_jobs.conformance.unwrap_or(default_jobs.conformance),
                rescue: file_jobs.rescue.unwrap_or(default_jobs.rescue),
                fast_gate: file_jobs.fast_gate.unwrap_or(default_jobs.fast_gate),
                full_gate: file_jobs.full_gate.unwrap_or(default_jobs.full_gate),
                trial_merge: file_jobs.trial_merge.unwrap_or(default_jobs.trial_merge),
                bundle_build: file_jobs.bundle_build.unwrap_or(default_jobs.bundle_build),
                rebase: file_jobs.rebase.unwrap_or(default_jobs.rebase),
            },
            health_min_free_disk_gb: self
                .health_min_free_disk_gb
                .unwrap_or(defaults.health_min_free_disk_gb),
            health_max_load_per_cpu: self
                .health_max_load_per_cpu
                .unwrap_or(defaults.health_max_load_per_cpu),
            health_min_free_memory_mb: self
                .health_min_free_memory_mb
                .unwrap_or(defaults.health_min_free_memory_mb),
            health_max_process_share: self
                .health_max_process_share
                .unwrap_or(defaults.health_max_process_share),
            health_max_open_file_share: self
                .health_max_open_file_share
                .unwrap_or(defaults.health_max_open_file_share),
            heartbeat_every_s: self.heartbeat_every_s.unwrap_or(defaults.heartbeat_every_s),
            heartbeat_max_age_s: self
                .heartbeat_max_age_s
                .unwrap_or(defaults.heartbeat_max_age_s),
            question_limit_s: self.question_limit_s.unwrap_or(defaults.question_limit_s),
            replan_limit_s: self.replan_limit_s.unwrap_or(defaults.replan_limit_s),
            pool_full_use: self.pool_full_use.unwrap_or(defaults.pool_full_use),
            worktree_list_period_s: self
                .worktree_list_period_s
                .unwrap_or(defaults.worktree_list_period_s),
            worktree_disk_budget_gb: self
                .worktree_disk_budget_gb
                .unwrap_or(defaults.worktree_disk_budget_gb),
            question_max_bytes: self
                .question_max_bytes
                .unwrap_or(defaults.question_max_bytes),
            request_max_bytes: self.request_max_bytes.unwrap_or(defaults.request_max_bytes),
            heartbeat_max_bytes: self
                .heartbeat_max_bytes
                .unwrap_or(defaults.heartbeat_max_bytes),
            progress_max_bytes: self
                .progress_max_bytes
                .unwrap_or(defaults.progress_max_bytes),
            progress_max_lines: self
                .progress_max_lines
                .unwrap_or(defaults.progress_max_lines),
            progress_line_max_bytes: self
                .progress_line_max_bytes
                .unwrap_or(defaults.progress_line_max_bytes),
            transcript_tail_bytes: self
                .transcript_tail_bytes
                .unwrap_or(defaults.transcript_tail_bytes),
            transcript_line_max_bytes: self
                .transcript_line_max_bytes
                .unwrap_or(defaults.transcript_line_max_bytes),
            job_result_max_bytes: self
                .job_result_max_bytes
                .unwrap_or(defaults.job_result_max_bytes),
            fetch_period_s: self.fetch_period_s.unwrap_or(defaults.fetch_period_s),
            push_failure_limit: self
                .push_failure_limit
                .unwrap_or(defaults.push_failure_limit),
            rescue_unique_budget_mb: self
                .rescue_unique_budget_mb
                .unwrap_or(defaults.rescue_unique_budget_mb),
            landing_ready_limit_s: self
                .landing_ready_limit_s
                .unwrap_or(defaults.landing_ready_limit_s),
            landing_repair_limit_s: self
                .landing_repair_limit_s
                .unwrap_or(defaults.landing_repair_limit_s),
            landing_job_margin_s: self
                .landing_job_margin_s
                .unwrap_or(defaults.landing_job_margin_s),
            bundle_passed_limit_s: self
                .bundle_passed_limit_s
                .unwrap_or(defaults.bundle_passed_limit_s),
            result_max_bytes: self.result_max_bytes.unwrap_or(defaults.result_max_bytes),
            resolver_max_lines: self
                .resolver_max_lines
                .unwrap_or(defaults.resolver_max_lines),
            resolver_max_files: self
                .resolver_max_files
                .unwrap_or(defaults.resolver_max_files),
            idle_stop_s: self.idle_stop_s.unwrap_or(defaults.idle_stop_s),
            park_after_days: self.park_after_days.unwrap_or(defaults.park_after_days),
            reminder_every_s: self.reminder_every_s.unwrap_or(defaults.reminder_every_s),
            working_hours,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_the_defaults() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(load(root.path()).unwrap(), FleetConfig::default());
        let gone = root.path().join("no-such-folder");
        assert_eq!(load(&gone).unwrap(), FleetConfig::default());
    }

    #[test]
    fn unknown_key_is_refused_with_its_name() {
        let err = parse("pass_period_s = 5\npass_perod_s = 6\n").unwrap_err();
        assert!(format!("{err:#}").contains("pass_perod_s"), "{err:#}");
        let err = parse("[job_limit_s]\nworktree_ad = 1\n").unwrap_err();
        assert!(format!("{err:#}").contains("worktree_ad"), "{err:#}");
    }

    #[test]
    fn a_partial_file_keeps_the_default_of_each_absent_key() {
        let c = parse(
            "pass_budget = 50\njob_backoff_s = [10, 20]\nhealth_max_load_per_cpu = 2.5\n\
             [job_limit_s]\ndocs_sync = 60\n",
        )
        .unwrap();
        let want = FleetConfig {
            pass_budget: 50,
            job_backoff_s: vec![10, 20],
            health_max_load_per_cpu: 2.5,
            job_limit_s: JobLimits {
                docs_sync: 60,
                ..JobLimits::default()
            },
            ..FleetConfig::default()
        };
        assert_eq!(c, want);
        assert_eq!(parse("").unwrap(), FleetConfig::default());
    }

    #[test]
    fn a_file_value_out_of_range_is_refused() {
        let err = parse("pass_time_budget_s = 30\n").unwrap_err();
        assert!(format!("{err:#}").contains("pass_time_budget_s"), "{err:#}");
    }

    #[test]
    fn phase2_keys_have_their_defaults() {
        let root = tempfile::tempdir().unwrap();
        let c = load(root.path()).unwrap();
        assert_eq!(c.heartbeat_every_s, 10);
        assert_eq!(c.heartbeat_max_age_s, 30);
        assert_eq!(c.question_limit_s, 900);
        assert_eq!(c.replan_limit_s, 1800);
        assert_eq!(c.pool_full_use, 0.98);
    }

    #[test]
    fn a_file_sets_each_phase2_key() {
        let c = parse(
            "heartbeat_every_s = 5\nheartbeat_max_age_s = 20\nquestion_limit_s = 600\n\
             replan_limit_s = 3600\npool_full_use = 0.95\n",
        )
        .unwrap();
        let want = FleetConfig {
            heartbeat_every_s: 5,
            heartbeat_max_age_s: 20,
            question_limit_s: 600,
            replan_limit_s: 3600,
            pool_full_use: 0.95,
            ..FleetConfig::default()
        };
        assert_eq!(c, want);
    }

    #[test]
    fn fleet_toml_sets_the_phase_3_keys() {
        let c = parse(
            "worktree_list_period_s = 30\nworktree_disk_budget_gb = 50\n\
             [job_limit_s]\nrescue = 900\n",
        )
        .unwrap();
        let want = FleetConfig {
            worktree_list_period_s: 30,
            worktree_disk_budget_gb: 50,
            job_limit_s: JobLimits {
                rescue: 900,
                ..JobLimits::default()
            },
            ..FleetConfig::default()
        };
        assert_eq!(c, want);
        let err = format!("{:#}", parse("worktree_disk_budget_gb = 0\n").unwrap_err());
        assert!(err.contains("worktree_disk_budget_gb"), "{err}");
    }

    #[test]
    fn heartbeat_max_age_must_be_above_heartbeat_every() {
        let err = parse("heartbeat_every_s = 30\nheartbeat_max_age_s = 30\n").unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("heartbeat_max_age_s"), "{err}");
        assert!(err.contains("heartbeat_every_s"), "{err}");
    }

    #[test]
    fn pool_full_use_out_of_range_is_refused() {
        for text in ["pool_full_use = 0.5\n", "pool_full_use = 1.5\n"] {
            let err = format!("{:#}", parse(text).unwrap_err());
            assert!(err.contains("pool_full_use"), "{text}: {err}");
        }
    }

    #[test]
    fn load_reads_fleet_toml_and_names_the_file_on_error() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("fleet.toml");
        std::fs::write(&file, "stop_grace_s = 7\n").unwrap();
        assert_eq!(load(root.path()).unwrap().stop_grace_s, 7);
        std::fs::write(&file, "stop_grace_s = \"x\"\n").unwrap();
        let err = format!("{:#}", load(root.path()).unwrap_err());
        assert!(err.contains("fleet.toml"), "{err}");
    }

    #[test]
    fn fleet_toml_sets_the_phase_4_keys() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(FILE_NAME),
            "fetch_period_s = 61\npush_failure_limit = 4\nrescue_unique_budget_mb = 100\n\
             landing_ready_limit_s = 62\nlanding_repair_limit_s = 63\n\
             landing_job_margin_s = 64\nbundle_passed_limit_s = 65\n\
             result_max_bytes = 66\nresolver_max_lines = 67\nresolver_max_files = 3\n\
             [job_limit_s]\nfast_gate = 1\nfull_gate = 2\ntrial_merge = 3\n\
             bundle_build = 4\nrebase = 5\n",
        )
        .unwrap();
        let c = load(root.path()).unwrap();
        let want = FleetConfig {
            fetch_period_s: 61,
            push_failure_limit: 4,
            rescue_unique_budget_mb: 100,
            landing_ready_limit_s: 62,
            landing_repair_limit_s: 63,
            landing_job_margin_s: 64,
            bundle_passed_limit_s: 65,
            result_max_bytes: 66,
            resolver_max_lines: 67,
            resolver_max_files: 3,
            job_limit_s: JobLimits {
                fast_gate: 1,
                full_gate: 2,
                trial_merge: 3,
                bundle_build: 4,
                rebase: 5,
                ..JobLimits::default()
            },
            ..FleetConfig::default()
        };
        assert_eq!(c, want);
        let err = format!("{:#}", parse("[job_limit_s]\nfull_gate = 0\n").unwrap_err());
        assert!(err.contains("job_limit_s.full_gate"), "{err}");
        let err = format!("{:#}", parse("resolver_max_file = 2\n").unwrap_err());
        assert!(err.contains("resolver_max_file"), "{err}");
    }
}
