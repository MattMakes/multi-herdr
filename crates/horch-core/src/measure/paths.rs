//! Where the dataset lives on disk (OD3).
//!
//! The root is `<state_root>/multi-herdr/<project-slug>/`. It is a
//! subdirectory on purpose: `telemetry::collect::read_ledgers` parses every
//! `state_root/*.json`, and must never see a dataset file (MEA-08).

use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use crate::fsx;
use crate::ids::{ExperimentId, RoundId};
use crate::ledger;

/// The directory under the state root that holds every project's dataset.
pub const DATASET_DIR: &str = "multi-herdr";

/// Every path of one project's dataset. Pure: nothing touches the disk
/// except [`DatasetPaths::ensure`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetPaths {
    root: PathBuf,
}

impl DatasetPaths {
    /// The dataset of `project`, slugged as the ledger slugs it.
    pub fn new(state_root: &Path, project: &Path) -> DatasetPaths {
        Self::from_slug(state_root, &ledger::slug(&project.to_string_lossy()))
    }

    /// The dataset for an already slugged project name.
    pub fn from_slug(state_root: &Path, project_slug: &str) -> DatasetPaths {
        DatasetPaths {
            root: state_root.join(DATASET_DIR).join(project_slug),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn events_dir(&self) -> PathBuf {
        self.root.join("events")
    }

    /// `events/YYYY-MM-DD.jsonl`, one file per UTC day.
    pub fn events_file(&self, day: NaiveDate) -> PathBuf {
        self.events_dir()
            .join(format!("{}.jsonl", day.format("%Y-%m-%d")))
    }

    /// `events.lock/`: the `DirLock` taken as `DirLock::acquire(root, "events", ..)`.
    pub fn events_lock_dir(&self) -> PathBuf {
        self.root.join("events.lock")
    }

    pub fn experiments_dir(&self) -> PathBuf {
        self.root.join("experiments")
    }

    pub fn experiment_dir(&self, exp: &ExperimentId) -> PathBuf {
        self.experiments_dir().join(exp.as_str())
    }

    pub fn manifest(&self, exp: &ExperimentId) -> PathBuf {
        self.experiment_dir(exp).join("manifest.json")
    }

    pub fn rounds_dir(&self, exp: &ExperimentId) -> PathBuf {
        self.experiment_dir(exp).join("rounds")
    }

    pub fn round_file(&self, exp: &ExperimentId, round: &RoundId) -> PathBuf {
        self.rounds_dir(exp).join(format!("{round}.json"))
    }

    pub fn artifacts_dir(&self, exp: &ExperimentId, round: &RoundId) -> PathBuf {
        self.experiment_dir(exp)
            .join("artifacts")
            .join(round.as_str())
    }

    pub fn judge_input_dir(&self, exp: &ExperimentId, round: &RoundId) -> PathBuf {
        self.artifacts_dir(exp, round).join("judge-input")
    }

    pub fn validation_dir(&self, exp: &ExperimentId, round: &RoundId, label: &str) -> PathBuf {
        self.artifacts_dir(exp, round)
            .join("validation")
            .join(label)
    }

    pub fn judgements_dir(&self) -> PathBuf {
        self.root.join("judgements")
    }

    pub fn judgement(&self, round: &RoundId) -> PathBuf {
        self.judgements_dir().join(format!("{round}.json"))
    }

    pub fn promotions_dir(&self) -> PathBuf {
        self.root.join("promotions")
    }

    pub fn promotion(&self, round: &RoundId) -> PathBuf {
        self.promotions_dir().join(format!("{round}.json"))
    }

    pub fn exports_root(&self) -> PathBuf {
        self.root.join("exports")
    }

    pub fn exports_dir(&self, label_policy_version: &str) -> PathBuf {
        self.exports_root().join(label_policy_version)
    }

    pub fn jobs_root(&self) -> PathBuf {
        self.root.join("jobs")
    }

    pub fn jobs_dir(&self, round: &RoundId) -> PathBuf {
        self.jobs_root().join(round.as_str())
    }

    /// `jobs/<round>/judge-<attempt>/`.
    pub fn job_dir(&self, round: &RoundId, attempt: u32) -> PathBuf {
        self.jobs_dir(round).join(format!("judge-{attempt}"))
    }

    pub fn worktrees_root(&self) -> PathBuf {
        self.root.join("worktrees")
    }

    /// The default `--worktree-root` of an experiment.
    pub fn default_worktree_root(&self, exp: &ExperimentId) -> PathBuf {
        self.worktrees_root().join(exp.as_str())
    }

    /// Create the root and its fixed top-level directories, each 0700
    /// (SEC-05). Per-experiment directories are created by their writers.
    pub fn ensure(&self) -> fsx::Result<()> {
        if let Some(parent) = self.root.parent() {
            fsx::ensure_private_dir(parent)?;
        }
        fsx::ensure_private_dir(&self.root)?;
        for dir in [
            self.events_dir(),
            self.experiments_dir(),
            self.judgements_dir(),
            self.promotions_dir(),
            self.exports_root(),
            self.jobs_root(),
            self.worktrees_root(),
        ] {
            fsx::ensure_private_dir(&dir)?;
        }
        Ok(())
    }
}
