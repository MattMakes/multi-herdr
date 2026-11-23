use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_platform::channel::User;
use serde::Deserialize;

use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::task_execution::adapters::task_files::attempt_dir;
use crate::fleetd::task_execution::domain::result::{validate, Refusal, ResultFile};

pub const RESULT_JSON: &str = "result.json";

pub const QUESTIONS: &str = "questions";

const TMP_SUFFIX: &str = ".tmp";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptRef {
    pub task_id: String,
    pub attempt_id: String,
}

impl AttemptRef {
    pub fn attempt(&self) -> &str {
        self.attempt_id
            .rsplit_once('/')
            .map_or(self.attempt_id.as_str(), |(_, a)| a)
    }

    pub fn outbox(&self, fleet_root: &Path) -> PathBuf {
        attempt_dir(fleet_root, &self.task_id, self.attempt()).join("outbox")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Result,
    Question,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct QuestionFile {
    pub task: String,
    pub attempt: String,
    pub text: String,
    #[serde(default)]
    pub mail_id: Option<String>,
}

pub const MAIL_ID_MAX_BYTES: usize = 128;

fn is_mail_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAIL_ID_MAX_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Payload {
    Result(ResultFile),
    Question(QuestionFile),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    Valid {
        sha256: String,
        payload: Payload,
    },
    Rejected {
        sha256: Option<String>,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRead {
    pub path: PathBuf,
    pub kind: FileKind,
    pub read: Read,
}

mod check {
    pub(super) const REGULAR_FILE: &str = "regular_file";
    pub(super) const NO_LINK: &str = "no_link";
    pub(super) const REAL_PATH: &str = "real_path";
    pub(super) const SIZE: &str = "size";
    pub(super) const READ: &str = "read";
    pub(super) const PROTECTED: &str = "protected";
    pub(super) const SCHEMA: &str = "schema";
    pub(super) const TASK_ID: &str = "task_id";
    pub(super) const ATTEMPT_ID: &str = "attempt_id";
}

fn rejected(check: &str, detail: impl std::fmt::Display) -> Read {
    Read::Rejected {
        sha256: None,
        reason: format!("{check}: {detail}"),
    }
}

pub fn read_outbox(
    fleet_root: &Path,
    attempt: &AttemptRef,
    owner: &User,
    max_bytes: u64,
    done: &mut dyn FnMut(&Path) -> Result<bool>,
) -> Result<Vec<FileRead>> {
    let outbox = attempt.outbox(fleet_root);
    let attempt_folder = attempt_dir(fleet_root, &attempt.task_id, attempt.attempt());
    let real_outbox = match std::fs::canonicalize(&attempt_folder) {
        Ok(real) => real.join("outbox"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("resolve {}", attempt_folder.display())),
    };
    let mut files = vec![(outbox.join(RESULT_JSON), FileKind::Result)];
    for name in names(&outbox.join(QUESTIONS))? {
        files.push((outbox.join(QUESTIONS).join(name), FileKind::Question));
    }
    let mut out = Vec::new();
    for (path, kind) in files {
        if done(&path)? {
            continue;
        }
        let home = match kind {
            FileKind::Result => real_outbox.clone(),
            FileKind::Question => real_outbox.join(QUESTIONS),
        };
        let rule = Rule {
            root: &real_outbox,
            owner,
            limit: max_bytes,
        };
        if let Some(read) = read_file(&path, &home, kind, attempt, &rule) {
            out.push(FileRead { path, kind, read });
        }
    }
    Ok(out)
}

fn names(dir: &Path) -> Result<Vec<String>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("list {}", dir.display())),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("list {}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(TMP_SUFFIX) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

fn read_file(
    path: &Path,
    home: &Path,
    kind: FileKind,
    attempt: &AttemptRef,
    rule: &Rule<'_>,
) -> Option<Read> {
    let max_bytes = rule.limit;
    let checked = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return None,
        Err(e) => return Some(rejected(check::READ, e)),
    };
    if checked.file_type().is_symlink() {
        return Some(rejected(check::NO_LINK, "the file is a link"));
    }
    if !checked.is_file() {
        return Some(rejected(
            check::REGULAR_FILE,
            "the file is not a regular file",
        ));
    }
    match std::fs::canonicalize(path) {
        Ok(real) if real.parent() == Some(home) => {}
        Ok(real) => {
            return Some(rejected(
                check::REAL_PATH,
                format!("{} is not in {}", real.display(), home.display()),
            ))
        }
        Err(e) => return Some(rejected(check::REAL_PATH, e)),
    }
    if checked.len() > max_bytes {
        return Some(rejected(check::SIZE, too_large(checked.len(), max_bytes)));
    }
    let bytes = match read_protected(path, rule) {
        Ok(bytes) => bytes,
        Err(refusal) => return Some(rejected(check::PROTECTED, refusal.reason)),
    };
    let sha256 = crate::digest::sha256_bytes(&bytes).hex();
    Some(match payload(&bytes, kind, attempt, max_bytes) {
        Ok(payload) => Read::Valid { sha256, payload },
        Err(reason) => Read::Rejected {
            sha256: Some(sha256),
            reason,
        },
    })
}

fn too_large(bytes: u64, max: u64) -> String {
    format!("the file has {bytes} bytes; the limit is {max} bytes")
}

fn payload(
    bytes: &[u8],
    kind: FileKind,
    attempt: &AttemptRef,
    max_bytes: u64,
) -> Result<Payload, String> {
    let (task, attempt) = (attempt.task_id.as_str(), attempt.attempt());
    match kind {
        FileKind::Result => validate(bytes, task, attempt, max_bytes)
            .map(Payload::Result)
            .map_err(|refusal| {
                let check = match refusal {
                    Refusal::TooLarge { .. } => check::SIZE,
                    Refusal::NotJson(_) | Refusal::Schema(_) => check::SCHEMA,
                    Refusal::OtherTask { .. } => check::TASK_ID,
                    Refusal::OtherAttempt { .. } => check::ATTEMPT_ID,
                };
                format!("{check}: {refusal}")
            }),
        FileKind::Question => {
            let file: QuestionFile = serde_json::from_slice(bytes)
                .map_err(|e| format!("{}: the question file: {e}", check::SCHEMA))?;
            if file.task != task {
                return Err(format!(
                    "{}: the question file names task {:?}, but its path is of task {task:?}",
                    check::TASK_ID,
                    file.task
                ));
            }
            if file.attempt != attempt {
                return Err(format!(
                    "{}: the question file names attempt {:?}, but its path is of attempt {attempt:?}",
                    check::ATTEMPT_ID,
                    file.attempt
                ));
            }
            if file.mail_id.as_deref().is_some_and(|id| !is_mail_id(id)) {
                return Err(format!(
                    "{}: the question file has a mail_id that is not a mail id",
                    check::SCHEMA
                ));
            }
            Ok(Payload::Question(file))
        }
    }
}
