//! Putting the downloaded release where Herdr expects it.
//!
//! Unix is simple: one binary at `~/.local/bin/herdr`, matching `install.sh`.
//!
//! Windows is not, and the complexity is load-bearing. The official installer
//! unpacks each version into its own directory and points a `current` junction
//! plus a visible bin directory at it, precisely so an update never has to
//! overwrite a `herdr.exe` that is currently running. The Windows asset is also a
//! zip carrying an app-local ConPTY runtime next to the binary, so the whole
//! archive has to be extracted, not just the executable.
//!
//! Path computation lives in pure functions so it can be verified off Windows.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// The executable's name on this platform.
pub fn exe_name() -> &'static str {
    if cfg!(windows) {
        "herdr.exe"
    } else {
        "herdr"
    }
}

/// Where a Unix install puts the binary: `$HERDR_INSTALL_DIR`, else
/// `~/.local/bin`.
pub fn unix_install_dir(home: &Path, override_dir: Option<&Path>) -> PathBuf {
    match override_dir {
        Some(dir) => dir.to_path_buf(),
        None => home.join(".local").join("bin"),
    }
}

/// The directories a Windows install touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsLayout {
    /// `~/.herdr/packages/standalone/releases`
    pub releases_dir: PathBuf,
    /// The versioned directory this install unpacks into.
    pub release_dir: PathBuf,
    /// The `current` junction, retargeted after a successful unpack.
    pub current_dir: PathBuf,
    /// The junction that goes on PATH.
    pub visible_bin: PathBuf,
}

/// Compute the Windows install layout.
///
/// Mirrors the official installer so `herdr update` keeps working against the same
/// directories.
pub fn windows_layout(
    herdr_home: &Path,
    local_app_data: Option<&Path>,
    install_dir_override: Option<&Path>,
    version: &str,
    triple: &str,
) -> WindowsLayout {
    let standalone_root = herdr_home.join("packages").join("standalone");
    let releases_dir = standalone_root.join("releases");
    let release_name = format!("{}-{triple}", crate::manifest::safe_version(version));
    let default_visible = local_app_data
        .map(|base| base.join("Programs").join("Herdr").join("bin"))
        .unwrap_or_else(|| standalone_root.join("bin"));
    WindowsLayout {
        release_dir: releases_dir.join(release_name),
        releases_dir,
        current_dir: standalone_root.join("current"),
        visible_bin: install_dir_override
            .map(Path::to_path_buf)
            .unwrap_or(default_visible),
    }
}

/// Write a bare binary to `target`, replacing whatever is there.
///
/// Staged then renamed: overwriting a running binary fails on Windows and can
/// crash a live process on Unix.
pub fn place_binary(bytes: &[u8], target: &Path) -> Result<()> {
    let dir = target.parent().context("install target has no parent")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let staged = dir.join(format!(
        ".{}.download",
        target.file_name().unwrap_or_default().to_string_lossy()
    ));
    std::fs::write(&staged, bytes).with_context(|| format!("writing {}", staged.display()))?;
    make_executable(&staged)?;
    std::fs::rename(&staged, target).with_context(|| {
        format!(
            "installing to {} (is herdr currently running from there?)",
            target.display()
        )
    })
}

pub fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)
            .with_context(|| format!("marking {} executable", path.display()))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Extract a zip into `dest`, refusing entries that escape it.
///
/// `enclosed_name` is what rejects `../` and absolute paths, so a hostile archive
/// cannot write outside the release directory.
pub fn extract_zip(bytes: &[u8], dest: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("the download is not a valid zip archive")?;
    std::fs::create_dir_all(dest).with_context(|| format!("creating {}", dest.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name() else {
            bail!(
                "archive entry '{}' escapes the extraction directory; refusing to install",
                entry.name()
            );
        };
        let out_path = dest.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf)?;
        std::fs::write(&out_path, &buf)
            .with_context(|| format!("writing {}", out_path.display()))?;
        if out_path.extension().is_some_and(|e| e == "exe") {
            make_executable(&out_path)?;
        }
    }
    Ok(())
}

/// Swap `staging` into place as `release_dir`, keeping a backup until it succeeds.
pub fn promote_release(staging: &Path, release_dir: &Path) -> Result<()> {
    if let Some(parent) = release_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let backup = release_dir.with_file_name(format!(
        ".backup.{}",
        release_dir.file_name().unwrap_or_default().to_string_lossy()
    ));
    let had_previous = release_dir.exists();
    if had_previous {
        let _ = std::fs::remove_dir_all(&backup);
        std::fs::rename(release_dir, &backup)
            .with_context(|| format!("moving aside {}", release_dir.display()))?;
    }
    match std::fs::rename(staging, release_dir) {
        Ok(()) => {
            if had_previous {
                let _ = std::fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(e) => {
            // Put the previous release back rather than leaving nothing installed.
            if had_previous && !release_dir.exists() {
                let _ = std::fs::rename(&backup, release_dir);
            }
            Err(e).with_context(|| format!("installing {}", release_dir.display()))
        }
    }
}

/// Delete release directories other than the one just installed, keeping `keep`
/// of the most recently modified.
pub fn prune_old_releases(releases_dir: &Path, current: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(releases_dir) else {
        return;
    };
    let mut others: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir() && e.path() != current)
        .filter_map(|e| {
            let modified = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((modified, e.path()))
        })
        .collect();
    others.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in others.into_iter().skip(keep) {
        let _ = std::fs::remove_dir_all(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let options: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for (name, contents) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(contents).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn unix_install_dir_defaults_to_local_bin() {
        let home = Path::new("/Users/a");
        assert_eq!(
            unix_install_dir(home, None),
            PathBuf::from("/Users/a/.local/bin")
        );
        assert_eq!(
            unix_install_dir(home, Some(Path::new("/opt/bin"))),
            PathBuf::from("/opt/bin")
        );
    }

    /// The versioned-directory plus junction scheme is what lets an update land
    /// while herdr.exe is running, so the paths must match the official installer.
    #[test]
    fn windows_layout_matches_the_official_installer() {
        let layout = windows_layout(
            Path::new(r"C:\Users\a\.herdr"),
            Some(Path::new(r"C:\Users\a\AppData\Local")),
            None,
            "0.8.0-preview.2026-08-04-abc",
            "x86_64-pc-windows-msvc",
        );
        assert!(layout
            .release_dir
            .ends_with("0.8.0-preview.2026-08-04-abc-x86_64-pc-windows-msvc"));
        assert!(layout.release_dir.starts_with(&layout.releases_dir));
        assert!(layout.current_dir.ends_with("current"));
        assert_eq!(
            layout.visible_bin,
            PathBuf::from(r"C:\Users\a\AppData\Local").join("Programs").join("Herdr").join("bin")
        );
    }

    #[test]
    fn windows_layout_honours_an_install_dir_override() {
        let layout = windows_layout(
            Path::new(r"C:\h"),
            Some(Path::new(r"C:\lad")),
            Some(Path::new(r"D:\tools\herdr")),
            "1.0",
            "x86_64-pc-windows-msvc",
        );
        assert_eq!(layout.visible_bin, PathBuf::from(r"D:\tools\herdr"));
    }

    /// A hostile version string must not be able to escape the releases dir: the
    /// release directory is always exactly one component below it.
    #[test]
    fn windows_release_dir_stays_directly_inside_releases() {
        for version in ["../../etc/evil", r"..\..\evil", "0.8.0"] {
            let layout = windows_layout(
                Path::new("/h"),
                None,
                None,
                version,
                "x86_64-pc-windows-msvc",
            );
            assert!(layout.release_dir.starts_with(&layout.releases_dir), "{version}");
            assert_eq!(
                layout.release_dir.parent(),
                Some(layout.releases_dir.as_path()),
                "{version} must not add path components"
            );
        }
    }

    #[test]
    fn place_binary_creates_and_replaces() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nested").join("herdr");

        place_binary(b"v1", &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"v1");

        place_binary(b"v2", &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"v2");

        // No staging leftovers.
        let leftovers: Vec<_> = std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[cfg(unix)]
    #[test]
    fn place_binary_marks_the_result_executable() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("herdr");
        place_binary(b"#!/bin/sh\n", &target).unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_ne!(mode & 0o111, 0, "mode was {mode:o}");
    }

    /// The whole archive must land, not just herdr.exe: the ConPTY runtime beside
    /// it is a runtime dependency.
    #[test]
    fn extract_zip_writes_every_entry_including_nested_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = zip_with(&[
            ("herdr.exe", b"binary"),
            ("conpty/conpty.dll", b"dll"),
            ("conpty/x64/OpenConsole.exe", b"console"),
            ("THIRD-PARTY-NOTICES/LICENSE.txt", b"legal"),
        ]);

        extract_zip(&bytes, tmp.path()).unwrap();
        assert_eq!(std::fs::read(tmp.path().join("herdr.exe")).unwrap(), b"binary");
        assert_eq!(
            std::fs::read(tmp.path().join("conpty").join("conpty.dll")).unwrap(),
            b"dll"
        );
        assert!(tmp.path().join("conpty").join("x64").join("OpenConsole.exe").exists());
        assert!(tmp.path().join("THIRD-PARTY-NOTICES").join("LICENSE.txt").exists());
    }

    /// Zip-slip: an entry pointing outside the destination must abort the install.
    #[test]
    fn extract_zip_refuses_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("dest");
        let bytes = zip_with(&[("../escaped.txt", b"pwned")]);

        let err = extract_zip(&bytes, &dest).unwrap_err().to_string();
        assert!(err.contains("escapes the extraction directory"), "{err}");
        assert!(!tmp.path().join("escaped.txt").exists());
    }

    #[test]
    fn extract_zip_rejects_a_non_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let err = extract_zip(b"not a zip at all", tmp.path()).unwrap_err().to_string();
        assert!(err.contains("not a valid zip archive"), "{err}");
    }

    #[test]
    fn promote_release_replaces_an_existing_version() {
        let tmp = tempfile::tempdir().unwrap();
        let release = tmp.path().join("releases").join("v1");
        std::fs::create_dir_all(&release).unwrap();
        std::fs::write(release.join("herdr.exe"), b"old").unwrap();

        let staging = tmp.path().join("releases").join(".staging");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("herdr.exe"), b"new").unwrap();

        promote_release(&staging, &release).unwrap();
        assert_eq!(std::fs::read(release.join("herdr.exe")).unwrap(), b"new");
        assert!(!staging.exists());
        // The backup is cleaned up on success.
        let leftovers: Vec<_> = std::fs::read_dir(release.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".backup"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn prune_keeps_the_current_release_and_the_newest_others() {
        let tmp = tempfile::tempdir().unwrap();
        let releases = tmp.path().to_path_buf();
        let current = releases.join("v3");
        for name in ["v1", "v2", "v3"] {
            let dir = releases.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("herdr.exe"), name).unwrap();
            // Distinct mtimes so ordering is deterministic.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        prune_old_releases(&releases, &current, 1);
        assert!(current.exists(), "current release must survive");
        assert!(releases.join("v2").exists(), "newest other must survive");
        assert!(!releases.join("v1").exists(), "oldest must be pruned");
    }
}
