use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::fleetd::worker_profiles::application::conformance::conformance_json;
use crate::fleetd::worker_profiles::domain::conformance::ConformanceReport;

pub fn docs_root(fleet_root: &Path) -> PathBuf {
    fleet_root.join("docs")
}

pub fn write_conformance(docs_root: &Path, report: &ConformanceReport) -> Result<PathBuf> {
    let dir = docs_root.join(&report.cli).join(&report.version);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("conformance.json");
    let tmp = dir.join(format!(".conformance.json.{}", std::process::id()));
    std::fs::write(&tmp, conformance_json(report))
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}
