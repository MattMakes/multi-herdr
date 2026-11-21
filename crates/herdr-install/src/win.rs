#![cfg_attr(not(windows), allow(dead_code))]

use std::path::Path;

use anyhow::Result;

pub fn prepend_path_entry(current: &str, entry: &str) -> Option<String> {
    let normalize = |s: &str| s.trim().trim_end_matches('\\').to_ascii_lowercase();
    let target = normalize(entry);

    let parts: Vec<&str> = current
        .split(';')
        .filter(|p| !p.trim().is_empty())
        .collect();
    let kept: Vec<&str> = parts
        .iter()
        .copied()
        .filter(|p| normalize(p) != target)
        .collect();
    let already_first = current
        .split(';')
        .next()
        .is_some_and(|p| !p.trim().is_empty() && normalize(p) == target);
    if already_first && kept.len() + 1 == parts.len() {
        return None;
    }

    Some(
        std::iter::once(entry)
            .chain(kept)
            .collect::<Vec<_>>()
            .join(";"),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JunctionAction {
    Create,
    Replace,
    Refuse,
}

pub fn plan_junction(exists: bool, is_link: bool, is_empty_dir: bool) -> JunctionAction {
    match (exists, is_link, is_empty_dir) {
        (false, _, _) => JunctionAction::Create,
        (true, true, _) => JunctionAction::Replace,
        (true, false, true) => JunctionAction::Replace,
        (true, false, false) => JunctionAction::Refuse,
    }
}

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

#[cfg(windows)]
pub fn set_junction(link: &Path, target: &Path) -> Result<()> {
    clear_link(link)?;
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    mklink_junction(link, target)
}

#[cfg(windows)]
fn clear_link(link: &Path) -> Result<()> {
    use anyhow::{bail, Context};

    match inspect_junction_target(link) {
        JunctionAction::Refuse => bail!(
            "{} already exists and holds files this installer did not create. \
             Move or delete it, then re-run.",
            link.display()
        ),
        JunctionAction::Replace => {
            if std::fs::remove_dir(link).is_err() {
                std::fs::remove_dir_all(link)
                    .with_context(|| format!("clearing {}", link.display()))?;
            }
            Ok(())
        }
        JunctionAction::Create => Ok(()),
    }
}

#[cfg(windows)]
fn mklink_junction(link: &Path, target: &Path) -> Result<()> {
    use anyhow::{bail, Context};

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

    #[test]
    fn prepend_matches_case_insensitively_and_ignores_trailing_slashes() {
        assert_eq!(
            prepend_path_entry(r"c:\herdr\bin\;C:\Windows", r"C:\Herdr\Bin"),
            None
        );
    }

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
    fn prepend_rewrites_when_a_leading_empty_part_precedes_the_entry() {
        assert_eq!(
            prepend_path_entry(r";C:\herdr\bin;C:\Windows", r"C:\herdr\bin").unwrap(),
            r"C:\herdr\bin;C:\Windows"
        );
    }

    #[test]
    fn prepend_removes_a_later_duplicate_of_the_first_entry() {
        assert_eq!(
            prepend_path_entry(r"C:\herdr\bin;C:\Windows;c:\herdr\bin\", r"C:\herdr\bin").unwrap(),
            r"C:\herdr\bin;C:\Windows"
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
