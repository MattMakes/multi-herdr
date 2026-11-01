//! Which binary a long-lived horch process runs (#216).
//!
//! A detached horch process (the telemetry collector, a task-ledger
//! watchdog) keeps running the binary it started with. After `horch
//! install` replaces that file, the process runs old code. Each such
//! process records its [`ExeIdentity`] when it starts, and its starter
//! compares that with the binary that should run now.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// A binary file as it is now: a rebuilt or reinstalled binary at the same
/// path has another size or modification time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExeIdentity {
    /// The path, symlinks resolved when they resolve.
    pub path: String,
    pub len: u64,
    /// Nanoseconds since the Unix epoch.
    pub mtime_ns: u64,
}

/// The identity of the file at `path` now, or `None` when it cannot be read.
pub fn exe_identity(path: &Path) -> Option<ExeIdentity> {
    let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let meta = std::fs::metadata(&real).ok()?;
    let mtime = meta.modified().ok()?;
    let mtime_ns = mtime.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    Some(ExeIdentity {
        path: real.to_string_lossy().into_owned(),
        len: meta.len(),
        mtime_ns: u64::try_from(mtime_ns).ok()?,
    })
}
