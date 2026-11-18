//! The docs files of a CLI under `<fleet root>/docs/<cli>/<version>/`.
//! fleetd-p7 ticket 25 adds `conformance.json`.

use std::path::{Path, PathBuf};

/// The folder of the docs of each CLI: `<fleet root>/docs`. A sync writes
/// `<cli>/<version>/` and `<cli>/CHANGES.md` in it.
pub fn docs_root(fleet_root: &Path) -> PathBuf {
    fleet_root.join("docs")
}
