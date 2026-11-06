//! A typed git client (phase B2). [`GitClient`] is the seam: the messaging
//! commands call it, and a test can put a fake behind it. [`GitCli`] is the
//! real one; every process it starts goes through
//! [`horch_marketplace::git::GitRunner`], which removes the API key and the
//! repo-locating `GIT_*` variables and turns hooks off.
//!
//! Every argument that names a ref or a revision is refused when it starts
//! with `-`, so a label or a branch name can never become an option.

use std::path::{Path, PathBuf};

use anyhow::bail;
use horch_marketplace::git::{GitOutput, GitRunner};

pub trait GitClient {
    fn toplevel(&self, dir: &Path) -> anyhow::Result<PathBuf>;
    /// `None` on a detached HEAD.
    fn current_branch(&self, dir: &Path) -> anyhow::Result<Option<String>>;
    /// The full hash `rev` names, or `None` when it names nothing.
    fn rev_parse(&self, dir: &Path, rev: &str) -> anyhow::Result<Option<String>>;
}

/// Wraps [`GitRunner`], which sets `GIT_TERMINAL_PROMPT=0` and `LC_ALL=C`
/// and strips `FORBIDDEN_ENV` and the repository variables. The caller passes
/// the git binary (later from `RuntimeContext`, `HORCH_GIT_BIN`); nothing here
/// reads the environment.
#[derive(Debug, Clone)]
pub struct GitCli {
    runner: GitRunner,
}

impl GitCli {
    pub fn new(git_bin: PathBuf) -> Self {
        Self {
            runner: GitRunner::new(git_bin),
        }
    }

    /// Add one variable to every git child, for example `GIT_CONFIG_GLOBAL`
    /// in a hermetic test.
    pub fn with_env(self, key: &str, value: &str) -> Self {
        Self {
            runner: self.runner.with_env(key, value),
        }
    }

    fn run(&self, dir: &Path, args: &[&str]) -> anyhow::Result<GitOutput> {
        Ok(self.runner.run(dir, args)?)
    }

    fn text(&self, dir: &Path, args: &[&str]) -> anyhow::Result<String> {
        Ok(self.run(dir, args)?.stdout_text())
    }
}

fn refuse_option(arg: &str) -> anyhow::Result<()> {
    if arg.is_empty() || arg.starts_with('-') {
        bail!("'{arg}' is not a git revision or ref name");
    }
    Ok(())
}

impl GitClient for GitCli {
    fn toplevel(&self, dir: &Path) -> anyhow::Result<PathBuf> {
        Ok(PathBuf::from(
            self.text(dir, &["rev-parse", "--show-toplevel"])?,
        ))
    }

    fn current_branch(&self, dir: &Path) -> anyhow::Result<Option<String>> {
        let out = self
            .runner
            .output(dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        match out.status {
            0 => Ok(Some(out.stdout_text())),
            1 => Ok(None),
            s => bail!(
                "git symbolic-ref HEAD failed ({s}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }

    fn rev_parse(&self, dir: &Path, rev: &str) -> anyhow::Result<Option<String>> {
        refuse_option(rev)?;
        let out = self
            .runner
            .output(dir, &["rev-parse", "--verify", "--quiet", rev])?;
        match out.status {
            0 => Ok(Some(out.stdout_text())),
            1 => Ok(None),
            s => bail!(
                "git rev-parse {rev} failed ({s}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_shaped_revisions_are_refused() {
        assert!(refuse_option("--upload-pack=x").is_err());
        assert!(refuse_option("").is_err());
        assert!(refuse_option("main").is_ok());
        assert!(refuse_option("a..b").is_ok());
    }
}
