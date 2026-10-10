//! The data-readiness report (dataset design §4.10, EXP-05, OD4).
//!
//! Clef and Laya stay inert until the local data justifies them. [`readiness`]
//! counts the export rows per arm and compares the counts with the
//! thresholds of `assets/dataset-policy.json`. The report names every unmet
//! threshold of the next level with its numbers, such as
//! `clef: judged rounds 12/50`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::dataset::export::ExportRow;
use crate::evaluation::validator::GateStatus;
use crate::harness::HarnessKind;
use crate::ids::{ModelId, TaskId, TeammateName};
use crate::measure::worker_run::RunConfig;

/// The built-in `dataset-policy.json`.
pub(crate) const DEFAULT_POLICY: &str = include_str!("../../assets/dataset-policy.json");

/// One arm: a worker configuration the router can choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ArmKey {
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: ModelId,
    pub effort: Option<String>,
}

impl ArmKey {
    pub(crate) fn of(config: &RunConfig) -> ArmKey {
        ArmKey {
            teammate: config.teammate.clone(),
            harness: config.harness,
            model: config.model.clone(),
            effort: config.effort.clone(),
        }
    }

    /// `<teammate>|<harness>|<model>|<effort>`, with `-` for no effort, as
    /// the planner spells a config id.
    pub(crate) fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.teammate,
            self.harness,
            self.model,
            self.effort.as_deref().unwrap_or("-")
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArmCoverage {
    pub runs: u32,
    pub terminal_runs: u32,
    /// Rounds with a judgment in which this arm ran.
    pub judged_rounds: u32,
    pub wins: u32,
    /// Passed gates / all gates over the arm's runs; 0.0 without gates.
    pub gate_pass_rate: f64,
    pub distinct_tasks: u32,
}

/// The `readiness` part of `dataset-policy.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadinessThresholds {
    pub clef_judged_rounds: u32,
    pub clef_min_arms: u32,
    pub clef_runs_per_arm: u32,
    pub clef_distinct_tasks: u32,
    pub laya_judged_rounds: u32,
    pub laya_runs_per_arm: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DatasetPolicy {
    schema_version: String,
    readiness: ReadinessThresholds,
}

impl ReadinessThresholds {
    /// The thresholds of a `dataset-policy.json` text.
    pub(crate) fn from_policy_json(text: &str) -> Result<ReadinessThresholds> {
        let policy: DatasetPolicy =
            serde_json::from_str(text).context("parsing the dataset policy")?;
        Ok(policy.readiness)
    }

    /// The built-in thresholds (`DEFAULT_POLICY`).
    pub fn builtin() -> ReadinessThresholds {
        Self::from_policy_json(DEFAULT_POLICY).expect("the built-in dataset policy parses")
    }

    /// The thresholds of `file`, or the built-in ones without a file.
    pub fn load(file: Option<&Path>) -> Result<ReadinessThresholds> {
        match file {
            None => Ok(Self::builtin()),
            Some(file) => {
                let text = std::fs::read_to_string(file)
                    .with_context(|| format!("reading {}", file.display()))?;
                Self::from_policy_json(&text).with_context(|| format!("in {}", file.display()))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessVerdict {
    NotReady,
    ClefReady,
    LayaReady,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadinessReport {
    /// `ArmKey::key` → coverage.
    pub arms: BTreeMap<String, ArmCoverage>,
    /// Rows with a judgment.
    pub judged_rounds: u32,
    /// Distinct tasks over all rows.
    pub distinct_tasks: u32,
    pub verdict: ReadinessVerdict,
    /// The unmet thresholds of the next level, with their numbers.
    pub gaps: Vec<String>,
}

/// Count `rows` and judge them against `t`.
///
/// "≥ 50 runs per arm" for Laya is "at least `clef_min_arms` arms with
/// ≥ `laya_runs_per_arm` runs each", the same arm count as Clef, so a rarely
/// explored arm does not block Laya (dataset design 4.10.1).
pub fn readiness(rows: &[ExportRow], t: &ReadinessThresholds) -> ReadinessReport {
    #[derive(Default)]
    struct Tally {
        cov: ArmCoverage,
        gates: u32,
        passed: u32,
        tasks: BTreeSet<TaskId>,
    }
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut judged_rounds = 0;
    let mut tasks = BTreeSet::new();
    for row in rows {
        // A judgment scores every judged candidate's components.
        let judged = !row.component_quality.is_empty();
        if judged {
            judged_rounds += 1;
        }
        tasks.insert(row.task_id.clone());
        let mut arms_in_row = BTreeSet::new();
        for run in &row.worker_runs {
            let key = ArmKey::of(&run.config).key();
            let tally = tallies.entry(key.clone()).or_default();
            tally.cov.runs += 1;
            if run.facts.status.is_terminal() {
                tally.cov.terminal_runs += 1;
            }
            if row.winner.as_deref() == Some(run.config.config_id.as_str()) {
                tally.cov.wins += 1;
            }
            for gate in &run.scores.gates {
                tally.gates += 1;
                if gate.status == GateStatus::Passed {
                    tally.passed += 1;
                }
            }
            tally.tasks.insert(row.task_id.clone());
            if judged && arms_in_row.insert(key) {
                tally.cov.judged_rounds += 1;
            }
        }
    }
    let arms: BTreeMap<String, ArmCoverage> = tallies
        .into_iter()
        .map(|(key, mut tally)| {
            if tally.gates > 0 {
                tally.cov.gate_pass_rate = f64::from(tally.passed) / f64::from(tally.gates);
            }
            tally.cov.distinct_tasks = count(tally.tasks.len());
            (key, tally.cov)
        })
        .collect();
    let distinct_tasks = count(tasks.len());
    let arms_with = |runs: u32| count(arms.values().filter(|a| a.runs >= runs).count());

    let mut clef_gaps = Vec::new();
    if judged_rounds < t.clef_judged_rounds {
        clef_gaps.push(format!(
            "clef: judged rounds {judged_rounds}/{}",
            t.clef_judged_rounds
        ));
    }
    let clef_arms = arms_with(t.clef_runs_per_arm);
    if clef_arms < t.clef_min_arms {
        clef_gaps.push(format!(
            "clef: arms with >= {} runs {clef_arms}/{}",
            t.clef_runs_per_arm, t.clef_min_arms
        ));
    }
    if distinct_tasks < t.clef_distinct_tasks {
        clef_gaps.push(format!(
            "clef: distinct tasks {distinct_tasks}/{}",
            t.clef_distinct_tasks
        ));
    }
    let mut laya_gaps = Vec::new();
    if judged_rounds < t.laya_judged_rounds {
        laya_gaps.push(format!(
            "laya: judged rounds {judged_rounds}/{}",
            t.laya_judged_rounds
        ));
    }
    let laya_arms = arms_with(t.laya_runs_per_arm);
    if laya_arms < t.clef_min_arms {
        laya_gaps.push(format!(
            "laya: arms with >= {} runs {laya_arms}/{}",
            t.laya_runs_per_arm, t.clef_min_arms
        ));
    }

    let (verdict, gaps) = if !clef_gaps.is_empty() {
        (ReadinessVerdict::NotReady, clef_gaps)
    } else if !laya_gaps.is_empty() {
        (ReadinessVerdict::ClefReady, laya_gaps)
    } else {
        (ReadinessVerdict::LayaReady, Vec::new())
    };
    ReadinessReport {
        arms,
        judged_rounds,
        distinct_tasks,
        verdict,
        gaps,
    }
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
