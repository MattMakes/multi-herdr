//! The concern path for Windows.

use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};

use crate::path::{inspect_junction_target, JunctionAction};
use crate::NotSupported;

const OS: &str = "windows";

pub const HOME_VAR: &str = "USERPROFILE";
pub const CONFIG_BASE_VAR: Option<&str> = Some("APPDATA");
pub const INSTALL_BASE_VAR: Option<&str> = Some("LOCALAPPDATA");
pub const CLAUDE_MANAGED_SETTINGS: &str = r"C:\Program Files\ClaudeCode\managed-settings.json";
pub const BLENDER_APP: Option<&str> = None;
pub const GODOT_APP: Option<&str> = None;
pub const GODOT_MONO_APP: Option<&str> = None;

pub fn passwd_home() -> Result<Option<PathBuf>, NotSupported> {
    Err(NotSupported::new("path", OS))
}

pub fn symlink(target: &Path, at: &Path) -> io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, at)
    } else {
        std::os::windows::fs::symlink_file(target, at)
    }
}

pub fn copy_symlink(from: &Path, _to: &Path) -> io::Result<()> {
    std::fs::read_link(from)?;
    Err(NotSupported::new("path", OS).into())
}

pub fn private_link_home_supported() -> bool {
    false
}

pub fn is_reparse_point(meta: &Metadata) -> Result<bool, NotSupported> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    Ok(meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

// Junctions rather than symlinks: creating a directory symlink on Windows
// needs Developer Mode or elevation, while `mklink /J` does not. `mklink` is
// a cmd built-in, so no extra tooling is required.
pub fn set_junction(link: &Path, target: &Path) -> io::Result<()> {
    clear_link(link)?;
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    mklink_junction(link, target)
}

pub fn clear_link(link: &Path) -> io::Result<()> {
    match inspect_junction_target(link) {
        JunctionAction::Refuse => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "{} already exists and holds files this installer did not create. \
                 Move or delete it, then re-run.",
                link.display()
            ),
        )),
        JunctionAction::Replace => {
            // A junction is removed with remove_dir, not remove_dir_all, or the
            // delete would follow the link and take the release with it.
            if std::fs::remove_dir(link).is_err() {
                std::fs::remove_dir_all(link).map_err(|e| {
                    io::Error::new(e.kind(), format!("clearing {}: {e}", link.display()))
                })?;
            }
            Ok(())
        }
        JunctionAction::Create => Ok(()),
    }
}

fn mklink_junction(link: &Path, target: &Path) -> io::Result<()> {
    let output = crate::start::command("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("running mklink to create a directory junction: {e}"),
            )
        })?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "could not link {} -> {}: {}",
            link.display(),
            target.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}
