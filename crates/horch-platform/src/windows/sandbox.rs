//! The concern sandbox for Windows: not supported. Windows has no Seatbelt
//! and no bubblewrap, so a check never runs there in a sandbox
//! (`docs/specs/fleetd/design.md`, "What each OS provides", row Sandbox).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::sandbox::Host;
use crate::NotSupported;

pub(crate) fn host(
    _writable: &[PathBuf],
    _temp: &Path,
    _path: Option<&OsStr>,
) -> Result<Host, NotSupported> {
    Err(NotSupported::new("sandbox", "windows")
        .because("Windows has no Seatbelt and no bubblewrap"))
}
