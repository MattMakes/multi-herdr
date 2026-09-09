//! Windows-only install steps: directory junctions and the user PATH entry.
//!
//! The pure string and decision logic is compiled everywhere so it can be tested
//! on any platform; only the two pieces that actually touch Windows APIs are
//! `cfg`-gated.
//!
//! Off Windows the only callers are the tests, so the compiler sees these as dead.
//! They are the real implementation on Windows.
#![cfg_attr(not(windows), allow(dead_code))]

use std::path::Path;

use anyhow::Result;

/// Prepend `entry` to a PATH-style string, returning `None` when it is already
/// first.
///
/// PATH order decides which `herdr` wins, so the managed directory goes first. An
/// existing occurrence elsewhere in the list is removed rather than duplicated.
pub fn prepend_path_entry(current: &str, entry: &str) -> Option<String> {
    let normalize = |s: &str| s.trim().trim_end_matches('\\').to_ascii_lowercase();
    let target = normalize(entry);

    let mut kept: Vec<&str> = Vec::new();
    let mut already_first = false;
    for (index, part) in current.split(';').enumerate() {
        if part.trim().is_empty() {
            continue;
        }
        if normalize(part) == target {
            if index == 0 {
                already_first = true;
            }
            continue;
        }
        kept.push(part);
    }
    if already_first && kept.len() + 1 == current.split(';').filter(|p| !p.trim().is_empty()).count()
    {
        return None;
    }

    let mut next = String::from(entry);
    for part in kept {
        next.push(';');
        next.push_str(part);
    }
    Some(next)
}

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

/// Inspect `path` and decide what `set_junction` should do with it.
pub fn inspect_junction_target(path: &Path) -> JunctionAction {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return JunctionAction::Create;
    };
    let is_link = meta.file_type().is_symlink() || is_reparse_point(&meta);
    let is_empty_dir = meta.is_dir()
        && std::fs::read_dir(path)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
    plan_junction(true, is_link, is_empty_dir)
}

#[cfg(windows)]
fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_meta: &std::fs::Metadata) -> bool {
    false
}

/// Point `link` at `target` as a directory junction.
///
/// Junctions rather than symlinks: creating a directory symlink on Windows needs
/// Developer Mode or elevation, while `mklink /J` does not. `mklink` is a cmd
/// built-in, so no extra tooling is required.
#[cfg(windows)]
pub fn set_junction(link: &Path, target: &Path) -> Result<()> {
    use anyhow::{bail, Context};

    match inspect_junction_target(link) {
        JunctionAction::Refuse => bail!(
            "{} already exists and holds files this installer did not create. \
             Move or delete it, then re-run.",
            link.display()
        ),
        JunctionAction::Replace => {
            // A junction is removed with remove_dir, not remove_dir_all, or the
            // delete would follow the link and take the release with it.
            if std::fs::remove_dir(link).is_err() {
                std::fs::remove_dir_all(link)
                    .with_context(|| format!("clearing {}", link.display()))?;
            }
        }
        JunctionAction::Create => {}
    }
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .context("running mklink to create a directory junction")?;
    if !output.status.success() {
        bail!(
            "could not link {} -> {}: {}",
            link.display(),
            target.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn set_junction(_link: &Path, _target: &Path) -> Result<()> {
    anyhow::bail!("directory junctions are a Windows-only concept")
}

/// Add `dir` to the current user's persistent PATH. Returns true when changed.
///
/// Writes `HKCU\Environment` directly rather than using `setx`, which truncates
/// PATH at 1024 characters.
#[cfg(windows)]
pub fn ensure_user_path(dir: &Path) -> Result<bool> {
    use anyhow::Context;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let env = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .context("opening HKCU\\Environment")?;
    let current: String = env.get_value("Path").unwrap_or_default();
    let Some(next) = prepend_path_entry(&current, &dir.to_string_lossy()) else {
        return Ok(false);
    };
    env.set_value("Path", &next)
        .context("writing the user PATH")?;
    Ok(true)
}

#[cfg(not(windows))]
pub fn ensure_user_path(_dir: &Path) -> Result<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepend_adds_a_missing_entry_at_the_front() {
        let next = prepend_path_entry(r"C:\Windows;C:\Windows\System32", r"C:\herdr\bin").unwrap();
        assert_eq!(next, r"C:\herdr\bin;C:\Windows;C:\Windows\System32");
    }

    #[test]
    fn prepend_is_a_noop_when_already_first() {
        assert_eq!(
            prepend_path_entry(r"C:\herdr\bin;C:\Windows", r"C:\herdr\bin"),
            None
        );
    }

    /// Case and trailing backslashes are insignificant on Windows.
    #[test]
    fn prepend_matches_case_insensitively_and_ignores_trailing_slashes() {
        assert_eq!(
            prepend_path_entry(r"c:\herdr\bin\;C:\Windows", r"C:\Herdr\Bin"),
            None
        );
    }

    /// An entry further down the list is moved to the front, not duplicated.
    #[test]
    fn prepend_moves_an_existing_entry_rather_than_duplicating_it() {
        let next = prepend_path_entry(r"C:\Windows;C:\herdr\bin", r"C:\herdr\bin").unwrap();
        assert_eq!(next, r"C:\herdr\bin;C:\Windows");
        assert_eq!(next.matches(r"herdr\bin").count(), 1);
    }

    #[test]
    fn prepend_handles_an_empty_and_ragged_path() {
        assert_eq!(prepend_path_entry("", r"C:\h\bin").unwrap(), r"C:\h\bin");
        assert_eq!(
            prepend_path_entry(r";;C:\Windows;", r"C:\h\bin").unwrap(),
            r"C:\h\bin;C:\Windows"
        );
    }

    #[test]
    fn junction_plan_covers_every_state() {
        assert_eq!(plan_junction(false, false, false), JunctionAction::Create);
        assert_eq!(plan_junction(true, true, false), JunctionAction::Replace);
        assert_eq!(plan_junction(true, false, true), JunctionAction::Replace);
        assert_eq!(plan_junction(true, false, false), JunctionAction::Refuse);
    }

    #[test]
    fn inspecting_a_missing_path_says_create() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            inspect_junction_target(&tmp.path().join("absent")),
            JunctionAction::Create
        );
    }

    #[test]
    fn inspecting_an_empty_directory_says_replace() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("empty");
        std::fs::create_dir(&dir).unwrap();
        assert_eq!(inspect_junction_target(&dir), JunctionAction::Replace);
    }

    /// The user's own files must never be deleted to make room for a junction.
    #[test]
    fn inspecting_a_directory_with_files_says_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("mine");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("important.txt"), "keep me").unwrap();
        assert_eq!(inspect_junction_target(&dir), JunctionAction::Refuse);
    }

    #[cfg(unix)]
    #[test]
    fn inspecting_a_symlink_says_replace() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("f"), "x").unwrap();
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(inspect_junction_target(&link), JunctionAction::Replace);
    }
}
