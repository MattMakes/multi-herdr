//! Where the dataset lives on disk (OD3).
//!
//! The root is `<state_root>/multi-herdr/<project-slug>/`. It is a
//! subdirectory on purpose: `telemetry::collect::read_ledgers` parses every
//! `state_root/*.json`, and must never see a dataset file (MEA-08).
//!
//! Every accessor that joins an id or a label checks that it is one plain
//! path component (see [`component`]). The id types accept legacy ids with
//! `/` or `..`; such an id never becomes a path.

use std::fmt;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;

use crate::fsx;
use crate::ids::{ExperimentId, RoundId};

/// The directory under the state root that holds every project's dataset.
pub(crate) const DATASET_DIR: &str = "multi-herdr";

/// An id or label that is not one plain path component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadPathComponent {
    /// What the value names, such as `round id`.
    pub what: &'static str,
    pub value: String,
}

impl fmt::Display for BadPathComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {:?} is not a plain path component",
            self.what, self.value
        )
    }
}

impl std::error::Error for BadPathComponent {}

pub(crate) type PathResult = Result<PathBuf, BadPathComponent>;

/// `value`, when it is one plain path component: not empty, not `.` or
/// `..`, and without `/`, `\`, `..` or NUL.
pub fn component<'a>(what: &'static str, value: &'a str) -> Result<&'a str, BadPathComponent> {
    let bad = value.is_empty()
        || value == "."
        || value.contains("..")
        || value.contains(['/', '\\', '\0']);
    if bad {
        return Err(BadPathComponent {
            what,
            value: value.to_string(),
        });
    }
    Ok(value)
}

/// Every path of one project's dataset. Pure: nothing touches the disk
/// except [`DatasetPaths::ensure`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatasetPaths {
    root: PathBuf,
}

impl DatasetPaths {
    /// The dataset of `project`, slugged as the ledger slugs it.
    pub fn new(state_root: &Path, project: &Path) -> DatasetPaths {
        Self::from_slug(
            state_root,
            &crate::execution::store::slug(&project.to_string_lossy()),
        )
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

    pub fn experiment_dir(&self, exp: &ExperimentId) -> PathResult {
        Ok(self
            .experiments_dir()
            .join(component("experiment id", exp.as_str())?))
    }

    pub fn manifest(&self, exp: &ExperimentId) -> PathResult {
        Ok(self.experiment_dir(exp)?.join("manifest.json"))
    }

    pub fn artifacts_dir(&self, exp: &ExperimentId, round: &RoundId) -> PathResult {
        Ok(self
            .experiment_dir(exp)?
            .join("artifacts")
            .join(component("round id", round.as_str())?))
    }

    pub fn judge_input_dir(&self, exp: &ExperimentId, round: &RoundId) -> PathResult {
        Ok(self.artifacts_dir(exp, round)?.join("judge-input"))
    }

    pub fn validation_dir(&self, exp: &ExperimentId, round: &RoundId, label: &str) -> PathResult {
        Ok(self
            .artifacts_dir(exp, round)?
            .join("validation")
            .join(component("label", label)?))
    }

    pub fn judgements_dir(&self) -> PathBuf {
        self.root.join("judgements")
    }

    pub fn judgement(&self, round: &RoundId) -> PathResult {
        Ok(self.judgements_dir().join(json_name(round)?))
    }

    pub fn promotions_dir(&self) -> PathBuf {
        self.root.join("promotions")
    }

    pub fn promotion(&self, round: &RoundId) -> PathResult {
        Ok(self.promotions_dir().join(json_name(round)?))
    }

    pub fn exports_root(&self) -> PathBuf {
        self.root.join("exports")
    }

    pub fn exports_dir(&self, label_policy_version: &str) -> PathResult {
        Ok(self
            .exports_root()
            .join(component("label policy version", label_policy_version)?))
    }

    pub fn jobs_root(&self) -> PathBuf {
        self.root.join("jobs")
    }

    pub fn jobs_dir(&self, round: &RoundId) -> PathResult {
        Ok(self
            .jobs_root()
            .join(component("round id", round.as_str())?))
    }

    /// `jobs/<round>/judge-<attempt>/`.
    pub fn job_dir(&self, round: &RoundId, attempt: u32) -> PathResult {
        Ok(self.jobs_dir(round)?.join(format!("judge-{attempt}")))
    }

    pub fn worktrees_root(&self) -> PathBuf {
        self.root.join("worktrees")
    }

    /// The default `--worktree-root` of an experiment.
    pub fn default_worktree_root(&self, exp: &ExperimentId) -> PathResult {
        Ok(self
            .worktrees_root()
            .join(component("experiment id", exp.as_str())?))
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

/// `<round>.json`, for a round id that is a plain path component.
fn json_name(round: &RoundId) -> Result<String, BadPathComponent> {
    Ok(format!("{}.json", component("round id", round.as_str())?))
}
