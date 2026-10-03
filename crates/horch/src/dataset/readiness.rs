//! `multi-herdr-dataset readiness`: coverage per arm, the verdict and the
//! gaps (EXP-05).

use std::path::Path;

use anyhow::Result;
use horch_core::competition::planner::LABEL_POLICY_VERSION;
use horch_core::dataset::readiness::{readiness as judge, ReadinessReport, ReadinessThresholds};
use horch_core::runtime::RuntimeContext;

use super::{dataset_paths, exit};
use crate::output;

pub fn readiness(
    ctx: &RuntimeContext,
    policy: Option<&Path>,
    label_policy: Option<&str>,
) -> Result<u8> {
    let paths = dataset_paths(ctx)?;
    let thresholds = ReadinessThresholds::load(policy)?;
    let rows = super::export::rows(ctx, &paths, label_policy.unwrap_or(LABEL_POLICY_VERSION))?;
    output::print(&render(&judge(&rows, &thresholds)));
    Ok(exit::SUCCESS)
}

/// The text `readiness` prints.
pub fn render(report: &ReadinessReport) -> String {
    let verdict = serde_json::to_value(report.verdict)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let mut out = format!(
        "verdict: {verdict}\njudged rounds: {}\ndistinct tasks: {}\narms: {}\n",
        report.judged_rounds,
        report.distinct_tasks,
        report.arms.len()
    );
    for (arm, cov) in &report.arms {
        out.push_str(&format!(
            "  {arm} {}\n",
            serde_json::to_string(cov).unwrap_or_default()
        ));
    }
    if report.gaps.is_empty() {
        out.push_str("gaps: none\n");
    } else {
        out.push_str("gaps:\n");
        for gap in &report.gaps {
            out.push_str(&format!("  {gap}\n"));
        }
    }
    out
}
