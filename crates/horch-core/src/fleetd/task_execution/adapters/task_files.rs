use std::fmt;
use std::path::{Path, PathBuf};

use crate::fleetd::ideas::domain::answer::{DECISIONS_MD, FEATURES};
use crate::fleetd::ideas::domain::idea_folder::REQUIREMENTS_YAML;
use crate::fleetd::task_execution::domain::task_files::{
    lint, render_start_md, render_task_yaml, Secret, Sr2Refusal, TaskFacts,
};
use crate::fsx::{self, FsxError};

const IDEAS: &str = "ideas";

const MODE: u32 = 0o600;
const DIR_MODE: u32 = 0o700;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptFiles {
    pub task_yaml: PathBuf,
    pub start_md: PathBuf,
}

#[derive(Debug)]
pub enum TaskFilesError {
    Secret(Sr2Refusal),
    Exists(PathBuf),
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

pub fn attempt_dir(fleet_root: &Path, task_id: &str, attempt: &str) -> PathBuf {
    fleet_root
        .join("tasks")
        .join(task_id)
        .join("attempts")
        .join(attempt)
}

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

pub fn write_attempt_files(
    fleet_root: &Path,
    facts: &TaskFacts,
    secrets: &[Secret],
) -> Result<AttemptFiles, TaskFilesError> {
    write_attempt_files_with(fleet_root, facts, None, secrets)
}

pub fn write_attempt_files_with(
    fleet_root: &Path,
    facts: &TaskFacts,
    project_facts: Option<&[String]>,
    secrets: &[Secret],
) -> Result<AttemptFiles, TaskFilesError> {
    let dir = attempt_dir(fleet_root, &facts.task_id, &facts.attempt);
    let report_to = format!("{}/", dir.join("outbox").display());
    let read_first = read_first(fleet_root, facts);
    let yaml = render_task_yaml(facts, &report_to, &read_first);
    let mut start = render_start_md(facts, &report_to, &read_first);
    lint("task.yaml", &yaml, secrets).map_err(TaskFilesError::Secret)?;
    lint("inbox/START.md", &start, secrets).map_err(TaskFilesError::Secret)?;

    let files = AttemptFiles {
        task_yaml: dir.join("task.yaml"),
        start_md: dir.join("inbox").join("START.md"),
    };
    let inbox = dir.join("inbox");
    make_dirs(&dir, &inbox)?;
    if let Some(list) = project_facts {
        start.push('\n');
        start.push_str(&facts_section(&dir.join(FACTS_MD), list, secrets)?);
        lint("inbox/START.md", &start, secrets).map_err(TaskFilesError::Secret)?;
    }
    fsx::create_immutable(&files.start_md, start.as_bytes(), MODE)?;
    fsx::create_immutable(&files.task_yaml, yaml.as_bytes(), MODE)?;
    Ok(files)
}

pub const FACTS_MD: &str = "facts.md";

fn facts_section(
    path: &Path,
    list: &[String],
    secrets: &[Secret],
) -> Result<String, TaskFilesError> {
    let kept: Vec<String> = list
        .iter()
        .filter(|f| lint("project fact", f, secrets).is_ok())
        .cloned()
        .collect();
    let text = crate::project_notes::data_section(&kept);
    match fsx::create_immutable(path, text.as_bytes(), MODE) {
        Ok(_) => Ok(text),
        Err(FsxError::Conflict { .. }) => std::fs::read_to_string(path).map_err(|source| {
            TaskFilesError::Io(FsxError::Io {
                path: path.to_path_buf(),
                source,
            })
        }),
        Err(e) => Err(e.into()),
    }
}

fn make_dirs(dir: &Path, inbox: &Path) -> Result<(), FsxError> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| FsxError::Io { path, source }
    };
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    let _ = horch_platform::fs::create_dir_mode(&mut builder, DIR_MODE);
    builder.create(inbox).map_err(io(inbox))?;
    for folder in [dir, inbox] {
        match horch_platform::fs::set_mode(folder, DIR_MODE) {
            Err(e)
                if e.get_ref()
                    .is_some_and(|inner| inner.is::<horch_platform::NotSupported>()) => {}
            r => r.map_err(io(folder))?,
        }
    }
    Ok(())
}
