//! The concern sandbox for Windows: not supported. Windows has no Seatbelt
//! and no bubblewrap, so a check never runs there in a sandbox
//! (`docs/specs/fleetd/design.md`, "What each OS provides", row Sandbox).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::NotSupported;

pub(crate) fn program(_path: Option<&OsStr>) -> Result<PathBuf, NotSupported> {
    Err(NotSupported::new("sandbox", "windows")
        .because("Windows has no Seatbelt and no bubblewrap"))
}

/// Windows has no sandbox host, so it has no arguments. [`program`] gives
/// `NotSupported` first, so no caller gets here.
pub(crate) fn args(
    _writable: &[PathBuf],
    _temp: Option<&Path>,
    _hidden: &[PathBuf],
) -> Vec<OsString> {
    Vec::new()
}
