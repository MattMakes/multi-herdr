use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, ProcessHost};

pub trait DocsSyncRunner {
    fn version_cmd(&self, cli: &str) -> Result<String>;

    fn sync(&self, cli: &str, version: &str, out: &Path) -> Result<Synced>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Synced {
    pub dir: PathBuf,
    pub pages: usize,
    pub failed: Vec<FailedPage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedPage {
    pub file: String,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocsRun {
    pub cli: String,
    pub version: String,
    pub dir: PathBuf,
    pub pages: usize,
    pub failed: Vec<FailedPage>,
}

impl DocsRun {
    pub fn passed(&self) -> bool {
        self.failed.is_empty()
    }

    pub fn summary(&self) -> String {
        let head = format!(
            "docs of {} {} in {}: {} pages",
            self.cli,
            self.version,
            self.dir.display(),
            self.pages
        );
        if self.passed() {
            return head;
        }
        let failed: Vec<String> = self
            .failed
            .iter()
            .map(|f| format!("{} ({})", f.file, f.why))
            .collect();
        format!(
            "{head}, {} failed: {}",
            self.failed.len(),
            failed.join(", ")
        )
    }
}

pub fn sync_cli(
    host: &dyn ProcessHost,
    runner: &dyn DocsSyncRunner,
    cli: &str,
    out: &Path,
) -> Result<DocsRun> {
    let cmd = runner.version_cmd(cli)?;
    let version = version_text(host, &cmd, out).with_context(|| format!("`{cmd}`"))?;
    let synced = runner.sync(cli, &version, out)?;
    Ok(DocsRun {
        cli: cli.to_string(),
        version,
        dir: synced.dir,
        pages: synced.pages,
        failed: synced.failed,
    })
}

pub fn version_text(host: &dyn ProcessHost, cmd: &str, scratch: &Path) -> Result<String> {
    let mut words = cmd.split_whitespace();
    let Some(program) = words.next() else {
        bail!("the version command is empty");
    };
    std::fs::create_dir_all(scratch).with_context(|| format!("creating {}", scratch.display()))?;
    let output = scratch.join(format!(".version-{}.out", std::process::id()));
    let _ = std::fs::remove_file(&output);
    let spec = LaunchSpec {
        program: program.into(),
        args: words.map(OsString::from).collect(),
        cwd: None,
        env_rule: Default::default(),
        detach: false,
        output: Some(output.clone()),
    };
    let status = host
        .launch(&spec)
        .map_err(anyhow::Error::from)?
        .child
        .wait();
    let text = std::fs::read_to_string(&output);
    let _ = std::fs::remove_file(&output);
    let status = status?;
    if !status.success() {
        bail!("the version command ended with {status}");
    }
    let text = text.context("reading the output of the version command")?;
    match text.lines().map(str::trim).find(|l| !l.is_empty()) {
        Some(line) => Ok(line.to_string()),
        None => bail!("the version command printed no version"),
    }
}
