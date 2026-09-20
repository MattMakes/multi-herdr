//! `horch install` - replaces `scripts/install-herdr-fleet.mjs`.
//!
//! The Node script had to inject a shell function into `~/.zshrc` or the
//! PowerShell profile, because the real entry point was
//! `just --justfile <repo>/justfile herdr-fleet` and so depended on the repo
//! staying put. `horch` embeds its prompts and needs no repo, so installing is
//! just putting one binary on PATH - no profile editing, nothing to keep in sync.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_core::agent;

/// Where to put the binary when the caller does not say.
pub fn default_dir() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .map(|base| Path::new(&base).join("Programs").join("horch"))
            .unwrap_or_else(|| agent::home_dir().join("horch"))
    } else {
        agent::home_dir().join(".local").join("bin")
    }
}

pub fn install(dir: Option<&str>) -> Result<()> {
    let dir = dir.map(PathBuf::from).unwrap_or_else(default_dir);
    let source = std::env::current_exe().context("locating the running horch binary")?;
    let file_name = source
        .file_name()
        .context("the running binary has no file name")?;
    let target = dir.join(file_name);

    // `same_file` follows symlinks, so a link that resolves to this binary
    // would count as installed. The install must be a real file: not a link.
    if same_file(&source, &target) && !is_symlink(&target) {
        println!("horch is already installed at {}", target.display());
    } else {
        place_binary(&source, &target)?;
        println!("installed horch to {}", target.display());
    }

    if agent::on_path(&dir) {
        println!("\nReady. cd into a project and run: horch fleet");
        println!("(needs a running herdr server: launch the herdr app, or `herdr server`)");
    } else {
        println!("\n{} is not on your PATH. Add it:\n", dir.display());
        if cfg!(windows) {
            println!("  PowerShell (current session):");
            println!("    $env:Path = \"{};$env:Path\"", dir.display());
            println!("\n  Permanently, for future sessions:");
            println!(
                "    setx PATH \"{};$($env:Path)\"",
                dir.display()
            );
        } else {
            println!("  Add to ~/.zshrc (or ~/.bashrc):");
            println!("    export PATH=\"{}:$PATH\"", dir.display());
        }
        println!("\nThen open a new terminal, cd into a project, and run: horch fleet");
    }

    println!("\nInstalled version: {}", installed_version(&target)?);
    Ok(())
}

/// Copies `source` to `target`, replacing whatever is there.
fn place_binary(source: &Path, target: &Path) -> Result<()> {
    let dir = target
        .parent()
        .context("the install path has no parent directory")?;
    let file_name = target
        .file_name()
        .context("the install path has no file name")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // The running binary can be reached through the very symlink removed below,
    // so resolve it to the real file first.
    let source = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    remove_symlink(target)?;
    // Replace rather than write in place: overwriting a running binary fails
    // on Windows and can crash a live process on Unix.
    let staged = dir.join(format!(".{}.new", file_name.to_string_lossy()));
    std::fs::copy(&source, &staged)
        .with_context(|| format!("copying {} to {}", source.display(), staged.display()))?;
    agent::make_executable(&staged)?;
    std::fs::rename(&staged, target).with_context(|| {
        format!("installing to {} (is a horch process running from there?)", target.display())
    })?;
    Ok(())
}

/// Removes `path` when it is a symlink, and says so. A link at the destination
/// is usually a stale one to a script in another checkout, and the install must
/// replace the link and never the file it points to.
fn remove_symlink(path: &Path) -> Result<()> {
    if !is_symlink(path) {
        return Ok(());
    }
    let points_to = std::fs::read_link(path)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "an unreadable path".to_string());
    std::fs::remove_file(path)
        .with_context(|| format!("removing the symlink {}", path.display()))?;
    println!(
        "removed the symlink {} (it pointed to {}) and replaced it with a real file",
        path.display(),
        points_to
    );
    Ok(())
}

/// True when `path` is itself a symlink, whether or not its target exists.
fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Runs `<target> --version`. That reports the installed version and proves
/// the installed file runs.
fn installed_version(target: &Path) -> Result<String> {
    let out = std::process::Command::new(target)
        .arg("--version")
        .output()
        .with_context(|| format!("running {} --version", target.display()))?;
    anyhow::ensure!(
        out.status.success(),
        "{} --version exited with {}",
        target.display(),
        out.status
    );
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// True when both paths name the same existing file.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dir_is_platform_appropriate() {
        let dir = default_dir();
        if cfg!(windows) {
            assert!(dir.ends_with("horch"), "{}", dir.display());
        } else {
            assert!(dir.ends_with(".local/bin"), "{}", dir.display());
        }
    }

    #[test]
    fn same_file_needs_both_paths_to_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        std::fs::write(&a, "x").unwrap();
        assert!(same_file(&a, &a));
        assert!(!same_file(&a, &tmp.path().join("missing")));
    }

    #[cfg(unix)]
    #[test]
    fn place_binary_replaces_a_symlink_and_leaves_its_target_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let old_script = tmp.path().join("old-script");
        std::fs::write(&old_script, "#!/bin/sh\necho old\n").unwrap();
        let source = tmp.path().join("new-binary");
        std::fs::write(&source, "new").unwrap();
        let target = tmp.path().join("bin").join("horch");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&old_script, &target).unwrap();

        place_binary(&source, &target).unwrap();

        assert!(!is_symlink(&target));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(
            std::fs::read_to_string(&old_script).unwrap(),
            "#!/bin/sh\necho old\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn place_binary_works_when_the_source_is_reached_through_the_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real-binary");
        std::fs::write(&real, "binary").unwrap();
        let target = tmp.path().join("bin").join("horch");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real, &target).unwrap();

        // Running `<target> install` reports the link itself as the source.
        place_binary(&target, &target).unwrap();

        assert!(!is_symlink(&target));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "binary");
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "binary");
    }

    #[test]
    fn place_binary_leaves_no_staged_file_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("new-binary");
        std::fs::write(&source, "new").unwrap();
        let target = tmp.path().join("bin").join("horch");

        place_binary(&source, &target).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        let names: Vec<_> = std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("horch")]);
    }

    #[test]
    fn is_symlink_is_false_for_a_missing_path() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!is_symlink(&tmp.path().join("missing")));
    }
}
