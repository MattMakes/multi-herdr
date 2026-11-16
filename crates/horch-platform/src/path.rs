//! The concern path: paths and home directories, symlinks and links (PL-2).
//!
//! The gaps of an OS are in the rows "Paths and home dirs" and "Symlinks" of
//! `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6.

use std::path::Path;

/// The variable that names the home folder: `HOME`, or `USERPROFILE` on
/// Windows. The caller reads it and falls back to `.` when it is unset or
/// empty.
pub use crate::os::path::HOME_VAR;

/// Where Claude Code reads managed (organisation) settings.
pub use crate::os::path::CLAUDE_MANAGED_SETTINGS;

/// The executables of the Blender, Godot and Godot .NET app bundles: the last
/// place each lookup tries. Only macOS has them, so they are `None` elsewhere.
pub use crate::os::path::{BLENDER_APP, GODOT_APP, GODOT_MONO_APP};

/// The variable of the base folder of the herdr config: `Some("APPDATA")` on
/// Windows, `None` elsewhere. The lookup order of today is
/// `HERDR_CONFIG_PATH`, then `<this variable>\herdr\config.toml` when it is
/// set and not empty, then `<home>/.config/herdr/config.toml`.
pub use crate::os::path::CONFIG_BASE_VAR;

/// The variable of the base folder of the default install folder:
/// `Some("LOCALAPPDATA")` on Windows, `None` elsewhere. The order of today is
/// `<this variable>\Programs\horch` when it is set and not empty, else
/// `<home>\horch`. With `None`, the folder is `<home>/.local/bin`.
pub use crate::os::path::INSTALL_BASE_VAR;

/// The home folder of this user in the passwd database, not `$HOME`.
/// `Ok(None)` when the lookup fails.
///
/// Windows has no passwd database and gives `NotSupported`, so no ledger
/// counts as shared there (row "Paths and home dirs").
pub use crate::os::path::passwd_home;

/// Make a symlink at `at` that points at `target`. On Windows it is a
/// directory symlink when `target` is a directory, else a file symlink.
pub use crate::os::path::symlink;

/// Copy the symlink `from` to `to` with the same target text.
///
/// Windows reads the link and then gives an `Unsupported` error, so a plugin
/// symlink is not copied there (row "Symlinks"). The caller keeps its own
/// message.
pub use crate::os::path::copy_symlink;

/// Whether a private `CODEX_HOME` of symlinks is possible. It is false on
/// Windows, where symlinks need Developer Mode or an elevated process, and
/// the codex rules go into the shared rules file (row "Symlinks").
pub use crate::os::path::private_link_home_supported;

/// Whether `meta` is of a reparse point (a junction or a symlink).
///
/// Only Windows has reparse points. macOS and Linux give `NotSupported`
/// (row "Symlinks"), and today's caller counts that as `false`.
pub use crate::os::path::is_reparse_point;

/// Point `link` at `target` as a directory junction: clear `link` with
/// [`clear_link`], make its parent folder, then run `cmd /C mklink /J`.
///
/// Only Windows has junctions. macOS and Linux give an `Unsupported` error
/// (row "Symlinks").
pub use crate::os::path::set_junction;

/// Remove what is at `link` so a junction can take its place. It refuses real
/// content with an `AlreadyExists` error.
///
/// Only Windows has junctions. macOS and Linux give an `Unsupported` error
/// (row "Symlinks").
pub use crate::os::path::clear_link;

/// What to do about a path that should become a junction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunctionAction {
    /// Nothing is there; create it.
    Create,
    /// A link or empty directory is there; replace it.
    Replace,
    /// Real content is there; refuse rather than delete the user's files.
    Refuse,
}

/// Decide how to treat an existing link path.
pub fn plan_junction(exists: bool, is_link: bool, is_empty_dir: bool) -> JunctionAction {
    match (exists, is_link, is_empty_dir) {
        (false, _, _) => JunctionAction::Create,
        (true, true, _) => JunctionAction::Replace,
        (true, false, true) => JunctionAction::Replace,
        (true, false, false) => JunctionAction::Refuse,
    }
}

/// Inspect `path` and decide what [`set_junction`] does with it. A reparse
/// point counts as a link. Off Windows, [`is_reparse_point`] gives
/// `NotSupported`, which counts as `false`, as today.
pub fn inspect_junction_target(path: &Path) -> JunctionAction {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return JunctionAction::Create;
    };
    let is_link = meta.file_type().is_symlink() || is_reparse_point(&meta).unwrap_or(false);
    let is_empty_dir = meta.is_dir()
        && std::fs::read_dir(path)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
    plan_junction(true, is_link, is_empty_dir)
}
