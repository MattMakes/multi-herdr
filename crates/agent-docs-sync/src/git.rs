use std::path::Path;
use std::process::Stdio;

use anyhow::{bail, Context, Result};
use horch_platform::start;

use crate::Outcome;

pub const GIT_DIR: &str = "git";

pub(crate) fn copy_docs(url: &str, version_dir: &Path) -> Outcome {
    let target = version_dir.join(GIT_DIR);
    let existed = target.exists();
    match clone(url, &target) {
        Ok(()) if existed => Outcome::Updated,
        Ok(()) => Outcome::Added,
        Err(e) => Outcome::Failed(format!("{e:#}")),
    }
}

fn clone(url: &str, target: &Path) -> Result<()> {
    if target.exists() {
        std::fs::remove_dir_all(target)
            .with_context(|| format!("removing {}", target.display()))?;
    }
    let mut clone = start::command("git");
    clone
        .args([
            "clone",
            "--depth",
            "1",
            "--filter=blob:none",
            "--sparse",
            "--",
        ])
        .arg(url)
        .arg(target);
    run(clone)?;
    let mut sparse = start::command("git");
    sparse
        .arg("-C")
        .arg(target)
        .args(["sparse-checkout", "set", "docs"]);
    run(sparse)
}

fn run(mut cmd: start::Command) -> Result<()> {
    let out = cmd
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .context("starting git")?;
    if !out.status.success() {
        bail!(
            "git failed ({}): {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}
