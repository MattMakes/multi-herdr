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
use crate::measure::worker_run::{RunConfig, WorkerRun};

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

/// One arm's counts while the rows are walked.
#[derive(Default)]
struct Tally {
    cov: ArmCoverage,
    gates: u32,
    passed: u32,
    tasks: BTreeSet<TaskId>,
}

impl Tally {
    fn add_run(&mut self, run: &WorkerRun, winner: Option<&str>, task_id: &TaskId) {
        self.cov.runs += 1;
        if run.facts.status.is_terminal() {
            self.cov.terminal_runs += 1;
        }
        if winner == Some(run.config.config_id.as_str()) {
            self.cov.wins += 1;
        }
        self.gates += count(run.scores.gates.len());
        self.passed += count(
            run.scores
                .gates
                .iter()
                .filter(|g| g.status == GateStatus::Passed)
                .count(),
        );
        self.tasks.insert(task_id.clone());
    }

    fn finish(mut self) -> ArmCoverage {
        if self.gates > 0 {
            self.cov.gate_pass_rate = f64::from(self.passed) / f64::from(self.gates);
        }
        self.cov.distinct_tasks = count(self.tasks.len());
        self.cov
    }
}

/// What the rows hold, before any threshold is read.
struct Tallies {
    arms: BTreeMap<String, Tally>,
    judged_rounds: u32,
    tasks: BTreeSet<TaskId>,
}

fn tally(rows: &[ExportRow]) -> Tallies {
    let mut out = Tallies {
        arms: BTreeMap::new(),
        judged_rounds: 0,
        tasks: BTreeSet::new(),
    };
    for row in rows {
        // A judgment scores every judged candidate's components.
        let judged = !row.component_quality.is_empty();
        out.judged_rounds += u32::from(judged);
        out.tasks.insert(row.task_id.clone());
        let mut arms_in_row = BTreeSet::new();
        for run in &row.worker_runs {
            let key = ArmKey::of(&run.config).key();
            let tally = out.arms.entry(key.clone()).or_default();
            tally.add_run(run, row.winner.as_deref(), &row.task_id);
            if judged && arms_in_row.insert(key) {
                tally.cov.judged_rounds += 1;
            }
        }
    }
    out
}

/// The unmet thresholds of one tier. Clef also checks distinct tasks
/// (`tasks` is `(have, need)`); both tiers check judged rounds and the arms
/// with `runs_per_arm` runs.
fn tier_gaps(
    tier: &str,
    judged_rounds: (u32, u32),
    arms_with: impl Fn(u32) -> u32,
    runs_per_arm: u32,
    min_arms: u32,
    tasks: Option<(u32, u32)>,
) -> Vec<String> {
    let mut gaps = Vec::new();
    if judged_rounds.0 < judged_rounds.1 {
        gaps.push(format!(
            "{tier}: judged rounds {}/{}",
            judged_rounds.0, judged_rounds.1
        ));
    }
    let arms = arms_with(runs_per_arm);
    if arms < min_arms {
        gaps.push(format!(
            "{tier}: arms with >= {runs_per_arm} runs {arms}/{min_arms}"
        ));
    }
    if let Some((have, need)) = tasks.filter(|(have, need)| have < need) {
        gaps.push(format!("{tier}: distinct tasks {have}/{need}"));
    }
    gaps
}

/// Count `rows` and judge them against `t`.
///
/// "≥ 50 runs per arm" for Laya is "at least `clef_min_arms` arms with
/// ≥ `laya_runs_per_arm` runs each", the same arm count as Clef, so a rarely
/// explored arm does not block Laya (dataset design 4.10.1).
pub fn readiness(rows: &[ExportRow], t: &ReadinessThresholds) -> ReadinessReport {
    let Tallies {
        arms,
        judged_rounds,
        tasks,
    } = tally(rows);
    let arms: BTreeMap<String, ArmCoverage> = arms
        .into_iter()
        .map(|(k, tally)| (k, tally.finish()))
        .collect();
    let distinct_tasks = count(tasks.len());
    let arms_with = |runs: u32| count(arms.values().filter(|a| a.runs >= runs).count());
    let clef_gaps = tier_gaps(
        "clef",
        (judged_rounds, t.clef_judged_rounds),
        arms_with,
        t.clef_runs_per_arm,
        t.clef_min_arms,
        Some((distinct_tasks, t.clef_distinct_tasks)),
    );
    let laya_gaps = tier_gaps(
        "laya",
        (judged_rounds, t.laya_judged_rounds),
        arms_with,
        t.laya_runs_per_arm,
        t.clef_min_arms,
        None,
    );
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
