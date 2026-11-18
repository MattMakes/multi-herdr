//! Steps 1 and 2 of 1 docs sync (`docs/specs/fleetd/design.md`, "Docs sync
//! and adapter conformance"): run the version command of a CLI through the
//! launcher (SR-7), then write its pages, the manifest and CHANGES.md into
//! `<out>/<cli>/<version>/`.
//!
//! horch-core has no HTTP crate (NFR-06), so the fetch and the pages live
//! behind the port [`DocsSyncRunner`]; `horch` implements it with
//! `agent-docs-sync`. `horch docs sync` and the executor of the
//! `docs_sync` job both call [`sync_cli`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, ProcessHost};

/// The port of the fetch and the pages of 1 docs source.
pub trait DocsSyncRunner {
    /// The `version_cmd` of the source `cli` of `docs-sources.toml`. An
    /// unknown `cli` is an error that names it.
    fn version_cmd(&self, cli: &str) -> Result<String>;

    /// Write the pages, the manifest and CHANGES.md of `cli` at `version`
    /// into `<out>/<cli>/<version>/`. An error means that no page list was
    /// read or that CHANGES.md was not written.
    fn sync(&self, cli: &str, version: &str, out: &Path) -> Result<Synced>;
}

/// What 1 sync wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Synced {
    /// `<out>/<cli>/<version>`.
    pub dir: PathBuf,
    /// The count of pages that the sync wrote or found unchanged.
    pub pages: usize,
    pub failed: Vec<FailedPage>,
}

/// A page that was not written, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailedPage {
    pub file: String,
    pub why: String,
}

/// The result of steps 1 and 2 for 1 CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DocsRun {
    pub cli: String,
    /// The version text, also the name of the version folder.
    pub version: String,
    pub dir: PathBuf,
    pub pages: usize,
    pub failed: Vec<FailedPage>,
}

impl DocsRun {
    /// Each page and CHANGES.md are written.
    pub fn passed(&self) -> bool {
        self.failed.is_empty()
    }

    /// 1 line for a person.
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

/// Steps 1 and 2 of 1 sync of `cli` into `out`: the version text from its
/// version command, started through `host`, then the pages.
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

/// The first line of the output of `cmd`, trimmed. The command starts
/// through `host`, so it has no forbidden name (SR-7). Its output goes to
/// a file in `scratch`, which is removed after the read.
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
