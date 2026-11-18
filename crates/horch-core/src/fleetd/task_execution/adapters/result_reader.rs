//! Reads `result.json` and the question files of an attempt (ticket 13).
//!
//! `docs/specs/fleetd/design.md`, "Workspace layout and file protocol",
//! protocol rule 7 and SR-6: a file that a worker writes is not trusted.
//! Before this reader reads a file, it checks that the file is a regular
//! file and not a link, that its real path is in the outbox of its own
//! attempt, and that it is under its size limit. Then it reads the file
//! through the protected file reader (ticket 07 of fleetd-p5), which opens
//! it without a link, sees a swap between the check and the open, and
//! checks the owner and the mode of the file and of each folder up to the
//! outbox (SR-6; `docs/tickets/fleetd-p5/MAP.md`, Q3). Then the payload
//! must name the task and the attempt of its path. A file that fails 1 check is a [`Read::Rejected`]
//! with the check in its reason. A question file uses the same reader
//! (`docs/tickets/fleetd-p4/MAP.md`, Q10) and the same size limit.
//!
//! Readers ignore `*.tmp` (protocol rule 1): a writer renames its temp file
//! when the file is whole.

use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_platform::channel::User;
use serde::Deserialize;

use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::task_execution::adapters::task_files::attempt_dir;
use crate::fleetd::task_execution::domain::result::{validate, Refusal, ResultFile};

/// The name of the result file in the outbox.
pub const RESULT_JSON: &str = "result.json";

/// The folder of the question files in the outbox.
pub const QUESTIONS: &str = "questions";

/// The suffix of a temp file that a writer has not renamed yet.
const TMP_SUFFIX: &str = ".tmp";

/// 1 attempt of a task, by its ledger ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptRef {
    pub task_id: String,
    /// The id of the `attempts` row: `<task id>/<attempt>`.
    pub attempt_id: String,
}

impl AttemptRef {
    /// The attempt part of the id, `A<n>`: the name of the attempt folder
    /// and the `attempt` of a payload.
    pub fn attempt(&self) -> &str {
        self.attempt_id
            .rsplit_once('/')
            .map_or(self.attempt_id.as_str(), |(_, a)| a)
    }

    /// The outbox of the attempt under `fleet_root`.
    pub fn outbox(&self, fleet_root: &Path) -> PathBuf {
        attempt_dir(fleet_root, &self.task_id, self.attempt()).join("outbox")
    }
}

/// The kind of an outbox file that this reader reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Result,
    Question,
}

/// A valid question file. Ticket 16 writes it with also `question_id` and
/// `at`; the ingest needs only these keys.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct QuestionFile {
    pub task: String,
    pub attempt: String,
    pub text: String,
}

/// The payload of a valid file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Payload {
    Result(ResultFile),
    Question(QuestionFile),
}

/// What the reader found in 1 file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Read {
    /// The file passed each check.
    Valid { sha256: String, payload: Payload },
    /// The file failed a check. `reason` starts with the name of the check
    /// and a colon. `sha256` is known when the bytes were read.
    Rejected {
        sha256: Option<String>,
        reason: String,
    },
}

/// 1 file of an outbox and what the reader found in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRead {
    /// The path under the fleet root, as the ledger records it.
    pub path: PathBuf,
    pub kind: FileKind,
    pub read: Read,
}

/// The names of the checks, as each reason starts.
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

/// Read the files of the outbox of `attempt` under `fleet_root`:
/// `result.json` first, then each question file in name order. A file
/// whose path `done` names is not read: the ledger has it. `max_bytes` is
/// the size limit of each file (`result_max_bytes` of `fleet.toml`).
/// `owner` is the user that owns the ledger: each file and each folder up to
/// the outbox must be its own. An outbox that does not exist gives no file.
pub fn read_outbox(
    fleet_root: &Path,
    attempt: &AttemptRef,
    owner: &User,
    max_bytes: u64,
    done: &mut dyn FnMut(&Path) -> Result<bool>,
) -> Result<Vec<FileRead>> {
    let outbox = attempt.outbox(fleet_root);
    let attempt_folder = attempt_dir(fleet_root, &attempt.task_id, attempt.attempt());
    // fleetd makes the attempt folder, so its real path is the base of the
    // real path check. A missing folder has no outbox.
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

/// The names in folder `dir` that are not temp files, in order. A missing
/// folder has none.
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

/// The checks of SR-6 and the payload of the file `path`, whose real
/// folder must be `home`. The stat checks come first, so each reason names
/// its check; then `rule` reads the file. `None`: no file is at `path`.
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

/// The payload of `bytes` of a file of `kind` of `attempt`, or the reason
/// of its refusal.
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
            Ok(Payload::Question(file))
        }
    }
}
