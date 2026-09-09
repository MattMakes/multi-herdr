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

    if same_file(&source, &target) {
        println!("horch is already installed at {}", target.display());
    } else {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating {}", dir.display()))?;
        // Replace rather than write in place: overwriting a running binary fails
        // on Windows and can crash a live process on Unix.
        let staged = dir.join(format!(".{}.new", file_name.to_string_lossy()));
        std::fs::copy(&source, &staged)
            .with_context(|| format!("copying {} to {}", source.display(), staged.display()))?;
        agent::make_executable(&staged)?;
        std::fs::rename(&staged, &target).with_context(|| {
            format!("installing to {} (is a horch process running from there?)", target.display())
        })?;
        println!("installed horch to {}", target.display());
    }

    if agent::on_path(&dir) {
        println!("\nReady. cd into a project and run: horch fleet");
        println!("(needs a running herdr server: launch the herdr app, or `herdr server`)");
        return Ok(());
    }

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
    Ok(())
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
}
