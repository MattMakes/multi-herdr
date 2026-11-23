use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExeIdentity {
    pub path: String,
    pub len: u64,
    pub mtime_ns: u64,
}

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
