//! The concern rest for Windows.

use std::io;
use std::path::Path;
use std::process::Command;

use crate::rest::{context, flat_name, not_a_file};
use crate::NotSupported;

pub(crate) const RELEASE_ARCHIVE_SUFFIX: &str = "zip";
pub(crate) const USER_PATH_IN_REGISTRY: bool = true;
pub(crate) const RUNNING_EXE_IS_LOCKED: bool = true;
pub(crate) const SHELL_LAUNCHERS: bool = false;
pub(crate) const STATUS_HAS_START_TIMES: bool = false;
pub(crate) const HOST_TOOLS: &str = "host tools vary; check command support before use";

pub(crate) fn extract_release(archive: &[u8], dir: &Path) -> io::Result<()> {
    let mut zip = zip::ZipArchive::new(io::Cursor::new(archive))
        .map_err(|e| context("the archive is not a .zip", e))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| context("reading the archive", e))?;
        let name = flat_name(entry.name())?.to_string();
        if !entry.is_file() {
            return Err(not_a_file(&name));
        }
        let dest = dir.join(&name);
        let mut file = std::fs::File::create(&dest)
            .map_err(|e| context(format!("cannot write {}", dest.display()), e))?;
        io::copy(&mut entry, &mut file).map_err(|e| context("reading the archive", e))?;
    }
    Ok(())
}

pub(crate) fn replace_process(mut cmd: Command) -> io::Error {
    match cmd.status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => error,
    }
}

/// Writes `HKCU\Environment` directly rather than using `setx`, which
/// truncates PATH at 1024 characters.
pub(crate) fn ensure_user_path(
    dir: &Path,
    prepend: &dyn Fn(&str, &str) -> Option<String>,
) -> Result<io::Result<bool>, NotSupported> {
    Ok(write_user_path(dir, prepend))
}

fn write_user_path(dir: &Path, prepend: &dyn Fn(&str, &str) -> Option<String>) -> io::Result<bool> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let env = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .map_err(|e| context("opening HKCU\\Environment", e))?;
    let current: String = env.get_value("Path").unwrap_or_default();
    let Some(next) = prepend(&current, &dir.to_string_lossy()) else {
        return Ok(false);
    };
    env.set_value("Path", &next)
        .map_err(|e| context("writing the user PATH", e))?;
    Ok(true)
}

pub(crate) fn path_help(dir: &Path) -> String {
    format!(
        "  PowerShell (current session):\n    $env:Path = \"{d};$env:Path\"\n\n  Permanently, for future sessions:\n    setx PATH \"{d};$($env:Path)\"",
        d = dir.display()
    )
}

pub(crate) fn path_extensions(pathext: Option<&str>) -> Vec<String> {
    pathext
        .unwrap_or(".EXE;.CMD;.BAT;.COM")
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
        .collect()
}
