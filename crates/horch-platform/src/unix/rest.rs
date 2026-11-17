//! The concern rest for the code of macOS and Linux.

use std::io;
use std::path::Path;
use std::process::Command;

use crate::rest::{context, flat_name, not_a_file};
use crate::NotSupported;

pub(crate) const RELEASE_ARCHIVE_SUFFIX: &str = "tar.gz";
pub(crate) const USER_PATH_IN_REGISTRY: bool = false;
pub(crate) const RUNNING_EXE_IS_LOCKED: bool = false;
pub(crate) const SHELL_LAUNCHERS: bool = true;
pub(crate) const STATUS_HAS_START_TIMES: bool = true;

pub(crate) fn extract_release(archive: &[u8], dir: &Path) -> io::Result<()> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    tar.entries()
        .map_err(|e| context("the archive is not a .tar.gz", e))?
        .try_for_each(|entry| extract_entry(entry, dir))
}

fn extract_entry(entry: io::Result<tar::Entry<'_, impl io::Read>>, dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut entry = entry.map_err(|e| context("reading the archive", e))?;
    let raw = entry
        .path()
        .map_err(|e| context("reading the archive", e))?;
    let name = flat_name(&raw.to_string_lossy())?.to_string();
    if !entry.header().entry_type().is_file() {
        return Err(not_a_file(&name));
    }
    let dest = dir.join(&name);
    let mut file = std::fs::File::create(&dest)
        .map_err(|e| context(format!("cannot write {}", dest.display()), e))?;
    io::copy(&mut entry, &mut file).map_err(|e| context("reading the archive", e))?;
    std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

pub(crate) fn replace_process(mut cmd: Command) -> io::Error {
    use std::os::unix::process::CommandExt;
    cmd.exec()
}

/// macOS and Linux keep the user PATH in the shell config, not in a
/// registry (`docs/specs/fleetd/preservation-06-wiring-and-platform.md`,
/// section 6, row "Other").
pub(crate) fn ensure_user_path(
    _dir: &Path,
    _prepend: &dyn Fn(&str, &str) -> Option<String>,
) -> Result<io::Result<bool>, NotSupported> {
    Err(NotSupported::new("rest", std::env::consts::OS))
}

pub(crate) fn path_help(dir: &Path) -> String {
    format!(
        "  Add to ~/.zshrc (or ~/.bashrc):\n    export PATH=\"{}:$PATH\"",
        dir.display()
    )
}

pub(crate) fn path_extensions(_pathext: Option<&str>) -> Vec<String> {
    Vec::new()
}
