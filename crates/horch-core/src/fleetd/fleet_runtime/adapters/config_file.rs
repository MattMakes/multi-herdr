//! The read of `<fleet root>/fleet.toml`.
//!
//! A missing file, or a missing key, gives the value of
//! [`FleetConfig::default`]. An unknown key, a value of the wrong type and a
//! value out of range are refused, with the name of the key.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::fleetd::fleet_runtime::domain::config::{FleetConfig, JobLimits};

/// The name of the config file in the fleet root.
pub const FILE_NAME: &str = "fleet.toml";

/// Read `<fleet_root>/fleet.toml`. No file gives the defaults.
pub fn load(fleet_root: &Path) -> Result<FleetConfig> {
    let path = fleet_root.join(FILE_NAME);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(FleetConfig::default()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    parse(&text).with_context(|| format!("reading {}", path.display()))
}

/// Parse the text of `fleet.toml`, then validate it.
pub fn parse(text: &str) -> Result<FleetConfig> {
    // serde names an unknown key and a value of the wrong type.
    let file: FileKeys = toml::from_str(text).map_err(|e| anyhow::anyhow!("fleet.toml: {e}"))?;
    let config = file.into_config();
    config
        .validate()
        .map_err(|e| anyhow::anyhow!("fleet.toml: {e}"))?;
    Ok(config)
}

/// The keys as the file has them: each is optional.
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
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileJobLimits {
    worktree_add: Option<u64>,
    worktree_remove: Option<u64>,
    worktree_list: Option<u64>,
    docs_sync: Option<u64>,
    conformance: Option<u64>,
}

impl FileKeys {
    fn into_config(self) -> FleetConfig {
        let d = FleetConfig::default();
        let j = self.job_limit_s;
        let dj = d.job_limit_s;
        FleetConfig {
            pass_period_s: self.pass_period_s.unwrap_or(d.pass_period_s),
            pass_time_budget_s: self.pass_time_budget_s.unwrap_or(d.pass_time_budget_s),
            scan_budget_ms: self.scan_budget_ms.unwrap_or(d.scan_budget_ms),
            pass_budget: self.pass_budget.unwrap_or(d.pass_budget),
            stop_grace_s: self.stop_grace_s.unwrap_or(d.stop_grace_s),
            retention_days: self.retention_days.unwrap_or(d.retention_days),
            abandoned_run_limit: self.abandoned_run_limit.unwrap_or(d.abandoned_run_limit),
            job_backoff_s: self.job_backoff_s.unwrap_or(d.job_backoff_s),
            job_limit_s: JobLimits {
                worktree_add: j.worktree_add.unwrap_or(dj.worktree_add),
                worktree_remove: j.worktree_remove.unwrap_or(dj.worktree_remove),
                worktree_list: j.worktree_list.unwrap_or(dj.worktree_list),
                docs_sync: j.docs_sync.unwrap_or(dj.docs_sync),
                conformance: j.conformance.unwrap_or(dj.conformance),
            },
            health_min_free_disk_gb: self
                .health_min_free_disk_gb
                .unwrap_or(d.health_min_free_disk_gb),
            health_max_load_per_cpu: self
                .health_max_load_per_cpu
                .unwrap_or(d.health_max_load_per_cpu),
            health_min_free_memory_mb: self
                .health_min_free_memory_mb
                .unwrap_or(d.health_min_free_memory_mb),
            health_max_process_share: self
                .health_max_process_share
                .unwrap_or(d.health_max_process_share),
            health_max_open_file_share: self
                .health_max_open_file_share
                .unwrap_or(d.health_max_open_file_share),
        }
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
    fn load_reads_fleet_toml_and_names_the_file_on_error() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("fleet.toml");
        std::fs::write(&file, "stop_grace_s = 7\n").unwrap();
        assert_eq!(load(root.path()).unwrap().stop_grace_s, 7);
        std::fs::write(&file, "stop_grace_s = \"x\"\n").unwrap();
        let err = format!("{:#}", load(root.path()).unwrap_err());
        assert!(err.contains("fleet.toml"), "{err}");
    }
}
