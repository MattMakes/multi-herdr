//! Writes `task.yaml` and `START.md` under the folder of an attempt (ticket 07).
//!
//! The folder is `<fleet root>/tasks/<task id>/attempts/<attempt>/`
//! (`docs/tickets/fleetd-p4/MAP.md`, Q3), so each attempt has its own 2
//! immutable files. Both texts pass the SR-2 lint before either file is
//! written. Each file is written 1 time through [`fsx::create_immutable`]:
//! a temp file, an fsync, then an atomic link, so a reader sees the whole
//! file or no file (protocol rule 1). A file that exists is never
//! rewritten (rule 2): the same bytes again are accepted, other bytes are
//! refused. `START.md` is written before `task.yaml`, so an attempt with a
//! `task.yaml` also has its `START.md`.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::fleetd::ideas::domain::answer::{DECISIONS_MD, FEATURES};
use crate::fleetd::ideas::domain::idea_folder::REQUIREMENTS_YAML;
use crate::fleetd::task_execution::domain::task_files::{
    lint, render_start_md, render_task_yaml, Secret, Sr2Refusal, TaskFacts,
};
use crate::fsx::{self, FsxError};

/// The folder of the ideas in the fleet root.
const IDEAS: &str = "ideas";

/// The file mode of both files: only the user of fleetd and its workers.
const MODE: u32 = 0o600;

/// The 2 files of an attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptFiles {
    pub task_yaml: PathBuf,
    pub start_md: PathBuf,
}

/// Why the 2 files were not written.
#[derive(Debug)]
pub enum TaskFilesError {
    /// The SR-2 lint refused a text. No file is written.
    Secret(Sr2Refusal),
    /// The file exists with other content. It stays as it is.
    Exists(PathBuf),
    /// A file system call failed.
    Io(FsxError),
}

impl fmt::Display for TaskFilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskFilesError::Secret(r) => write!(f, "refused: {r}"),
            TaskFilesError::Exists(path) => write!(
                f,
                "{} exists with other content; a task file is never rewritten",
                path.display()
            ),
            TaskFilesError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TaskFilesError {}

impl From<FsxError> for TaskFilesError {
    fn from(e: FsxError) -> Self {
        match e {
            FsxError::Conflict { path } => TaskFilesError::Exists(path),
            e => TaskFilesError::Io(e),
        }
    }
}

/// The folder of attempt `attempt` of task `task_id`.
pub fn attempt_dir(fleet_root: &Path, task_id: &str, attempt: &str) -> PathBuf {
    fleet_root
        .join("tasks")
        .join(task_id)
        .join("attempts")
        .join(attempt)
}

/// The files the worker of `facts` reads before it starts, in order: the
/// ticket, the `requirements.yaml` of its idea and the decisions file of
/// its feature when they exist (RR-12; MAP.md, Q15), and the verdict file
/// of a fix attempt. A task of a run in flight with no idea folder lists
/// only its ticket.
fn read_first(fleet_root: &Path, facts: &TaskFacts) -> Vec<String> {
    let idea = facts
        .idea
        .as_ref()
        .map(|i| fleet_root.join(IDEAS).join(i).join(REQUIREMENTS_YAML));
    let feature = facts
        .feature
        .as_ref()
        .map(|f| fleet_root.join(FEATURES).join(f).join(DECISIONS_MD));
    let ticket = Some(facts.read_first.clone()).filter(|t| !t.is_empty());
    let found = [idea, feature]
        .into_iter()
        .flatten()
        .filter(|p| p.is_file())
        .map(|p| p.display().to_string());
    ticket
        .into_iter()
        .chain(found)
        .chain(facts.verdict.clone())
        .collect()
}

/// Render, lint and write the 2 files of the attempt of `facts`. `secrets`
/// are the env values of the profile and of `FORBIDDEN_ENV`.
pub fn write_attempt_files(
    fleet_root: &Path,
    facts: &TaskFacts,
    secrets: &[Secret],
) -> Result<AttemptFiles, TaskFilesError> {
    let dir = attempt_dir(fleet_root, &facts.task_id, &facts.attempt);
    let report_to = format!("{}/", dir.join("outbox").display());
    let read_first = read_first(fleet_root, facts);
    let yaml = render_task_yaml(facts, &report_to, &read_first);
    let start = render_start_md(facts, &report_to, &read_first);
    lint("task.yaml", &yaml, secrets).map_err(TaskFilesError::Secret)?;
    lint("inbox/START.md", &start, secrets).map_err(TaskFilesError::Secret)?;

    let files = AttemptFiles {
        task_yaml: dir.join("task.yaml"),
        start_md: dir.join("inbox").join("START.md"),
    };
    let inbox = dir.join("inbox");
    std::fs::create_dir_all(&inbox).map_err(|source| FsxError::Io {
        path: inbox.clone(),
        source,
    })?;
    fsx::create_immutable(&files.start_md, start.as_bytes(), MODE)?;
    fsx::create_immutable(&files.task_yaml, yaml.as_bytes(), MODE)?;
    Ok(files)
}
