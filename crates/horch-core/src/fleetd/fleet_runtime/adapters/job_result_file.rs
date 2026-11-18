//! The job result file `<fleet root>/run/jobs/<job id>-<run>.json`: a job
//! process writes it atomically, and fleetd reads it with the checks and
//! ignores a result of another run.
//!
//! Requirements: RR-5, RR-8, SR-6. Ticket 33 of fleetd-p5 fills it (Q14 of
//! `docs/tickets/fleetd-p5/MAP.md`).
//!
//! - The job process writes `name.tmp`, fsyncs, then renames it to the
//!   name, with mode 0600, in the folder `run/jobs/` (mode 0700).
//! - fleetd reads each file with the protected file reader (the root
//!   `run/jobs/`, the limit `job_result_max_bytes`). The job id and the run
//!   in the payload must equal those of the name. A reader ignores a temp
//!   file (protocol rule 1) and the folder `archive/`.
//! - After the commit of its pass, fleetd moves each file to
//!   `run/jobs/archive/`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_platform::channel::User;
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fsx;

/// The folder of the result files, under the fleet root.
const FOLDER: &str = "run/jobs";

/// The folder of the result files that fleetd ingested.
const ARCHIVE: &str = "archive";

/// The part of a name that marks a temp file (protocol rule 1).
const TMP: &str = ".tmp";

/// The mode of a result file.
const FILE_MODE: u32 = 0o600;

/// What a job process writes: the run that it ran, and its outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobResult {
    pub job_id: i64,
    pub run: i64,
    pub passed: bool,
    pub verdict: bool,
    pub text: String,
    /// The facts of the result for the reader of the run, when its
    /// executor gives facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// What a read of 1 file gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Read {
    /// The file passed each check.
    Valid { sha256: String, result: JobResult },
    /// The file failed a check. `sha256` is known when the bytes were read.
    Rejected {
        sha256: Option<String>,
        reason: String,
    },
    /// The path has an `ingested` row: it is not read again, only archived.
    Seen,
}

/// 1 file of the folder, and what its read gave.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRead {
    pub path: PathBuf,
    pub read: Read,
}

/// The folder of the result files under `fleet_root`.
pub fn folder(fleet_root: &Path) -> PathBuf {
    fleet_root.join(FOLDER)
}

/// The result file of the run `run` of the job `job_id`.
pub fn path(fleet_root: &Path, job_id: i64, run: i64) -> PathBuf {
    folder(fleet_root).join(format!("{job_id}-{run}.json"))
}

/// Write `result` atomically to its file under `fleet_root`, with mode
/// 0600, in the folder `run/jobs/` with mode 0700. Gives the path.
pub fn write(fleet_root: &Path, result: &JobResult) -> Result<PathBuf> {
    fsx::ensure_private_dir(&folder(fleet_root))?;
    let path = path(fleet_root, result.job_id, result.run);
    let bytes = serde_json::to_vec(result).context("serializing a job result")?;
    fsx::write_atomic(&path, &bytes, FILE_MODE)?;
    Ok(path)
}

/// Read each result file under `fleet_root`, in the order of the names,
/// with the owner `owner` and the size limit `limit`. A path for which
/// `seen` is true gives [`Read::Seen`] with no read. A missing folder gives
/// no file.
pub fn read_folder(
    fleet_root: &Path,
    owner: &User,
    limit: u64,
    seen: &mut dyn FnMut(&Path) -> Result<bool>,
) -> Result<Vec<FileRead>> {
    let root = folder(fleet_root);
    let entries = match std::fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", root.display())),
    };
    let mut names = Vec::new();
    for entry in entries {
        let name = entry
            .with_context(|| format!("reading {}", root.display()))?
            .file_name();
        let name = name.to_string_lossy().into_owned();
        if name != ARCHIVE && !name.contains(TMP) {
            names.push(name);
        }
    }
    names.sort();
    let rule = Rule {
        root: &root,
        owner,
        limit,
    };
    let mut out = Vec::new();
    for name in names {
        let path = root.join(&name);
        let read = if seen(&path)? {
            Read::Seen
        } else {
            read_one(&path, &name, &rule)
        };
        out.push(FileRead { path, read });
    }
    Ok(out)
}

/// Read the file `path` named `name` under `rule`.
fn read_one(path: &Path, name: &str, rule: &Rule<'_>) -> Read {
    let rejected = |sha256: Option<String>, reason: String| Read::Rejected { sha256, reason };
    let Some(ids) = ids_of(name) else {
        return rejected(None, format!("the name {name} is not <job id>-<run>.json"));
    };
    let bytes = match read_protected(path, rule) {
        Ok(bytes) => bytes,
        Err(e) => return rejected(None, e.reason),
    };
    let sha256 = crate::digest::sha256_bytes(&bytes).hex();
    let result: JobResult = match serde_json::from_slice(&bytes) {
        Ok(r) => r,
        Err(e) => {
            return rejected(
                Some(sha256),
                format!("the payload is not a job result: {e}"),
            )
        }
    };
    if (result.job_id, result.run) != ids {
        return rejected(
            Some(sha256),
            format!(
                "the payload names job {} run {}, and the name names job {} run {}",
                result.job_id, result.run, ids.0, ids.1
            ),
        );
    }
    Read::Valid { sha256, result }
}

/// The job id and the run of the name `<job id>-<run>.json`.
fn ids_of(name: &str) -> Option<(i64, i64)> {
    let (job_id, run) = name.strip_suffix(".json")?.split_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(job_id) || !digits(run) {
        return None;
    }
    Some((job_id.parse().ok()?, run.parse().ok()?))
}

/// Move the file `path` to `run/jobs/archive/` under `fleet_root`. A file
/// that is gone already is not an error, so a second archive is safe.
pub fn archive(fleet_root: &Path, path: &Path) -> Result<()> {
    let to = folder(fleet_root).join(ARCHIVE);
    fsx::ensure_private_dir(&to)?;
    let name = path
        .file_name()
        .with_context(|| format!("{} has no file name", path.display()))?;
    match std::fs::rename(path, to.join(name)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        r => r.with_context(|| format!("archiving {}", path.display())),
    }
}
