use std::path::{Path, PathBuf};
use std::process::ExitCode;

use agent_docs_sync::{sources, Fetch, HttpFetch, Outcome, Source};
use anyhow::{anyhow, Context, Result};
use clap::Subcommand;
use horch_core::fleetd::worker_profiles::adapters::docs_dir;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::docs_sync::{
    sync_cli, DocsSyncRunner, FailedPage, Synced,
};
use horch_core::runtime::RuntimeContext;

use crate::output;

const FIXTURE_ENV: &str = "HORCH_TEST_DOCS_FIXTURE";

#[derive(Subcommand)]
pub enum DocsCommand {
    /// Fetch the docs of each CLI into <fleet root>/docs/<cli>/<version>/,
    /// with a manifest of page hashes and <cli>/CHANGES.md.
    Sync {
        /// Sync only this source of docs-sources.toml (herdr, claude,
        /// codex, agy).
        #[arg(long)]
        cli: Option<String>,
        /// Write into this folder in place of <fleet root>/docs.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

pub fn docs(ctx: &RuntimeContext, command: DocsCommand) -> Result<ExitCode> {
    match command {
        DocsCommand::Sync { cli, out } => sync(ctx, cli.as_deref(), out),
    }
}

fn sync(ctx: &RuntimeContext, cli: Option<&str>, out: Option<PathBuf>) -> Result<ExitCode> {
    let runner = AgentDocs::new()?;
    let clis: Vec<String> = match cli {
        Some(cli) => vec![runner.source(cli)?.tool.clone()],
        None => runner.sources.iter().map(|s| s.tool.clone()).collect(),
    };
    let out = out.unwrap_or_else(|| docs_dir::docs_root(&ctx.paths.fleet_root));
    let mut failed = false;
    for cli in &clis {
        match sync_cli(&Launcher, &runner, cli, &out) {
            Ok(run) => {
                failed |= !run.passed();
                output::println(&run.summary());
            }
            Err(e) => {
                failed = true;
                output::println(&format!("docs of {cli}: {e:#}"));
            }
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

pub struct AgentDocs {
    sources: Vec<Source>,
    fetch: Box<dyn Fetch>,
}

impl AgentDocs {
    pub fn new() -> Result<Self> {
        let mut sources = sources()?;
        let fixture = std::env::var_os(FIXTURE_ENV)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let fetch: Box<dyn Fetch> = match fixture {
            Some(dir) => {
                for source in &mut sources {
                    if let Some(url) = &source.also_git {
                        source.also_git =
                            Some(fixture_path(&dir, url).to_string_lossy().into_owned());
                    }
                }
                Box::new(FixtureFetch(dir))
            }
            None => Box::new(HttpFetch),
        };
        Ok(AgentDocs { sources, fetch })
    }

    fn source(&self, cli: &str) -> Result<&Source> {
        self.sources.iter().find(|s| s.tool == cli).ok_or_else(|| {
            let known: Vec<&str> = self.sources.iter().map(|s| s.tool.as_str()).collect();
            anyhow!(
                "no docs source {cli} in docs-sources.toml; the sources are {}",
                known.join(", ")
            )
        })
    }
}

impl DocsSyncRunner for AgentDocs {
    fn version_cmd(&self, cli: &str) -> Result<String> {
        Ok(self.source(cli)?.version_cmd.clone())
    }

    fn sync(&self, cli: &str, version: &str, out: &Path) -> Result<Synced> {
        let report = agent_docs_sync::sync(self.source(cli)?, version, self.fetch.as_ref(), out)
            .with_context(|| format!("docs sync of {cli}"))?;
        let pages = report
            .pages
            .iter()
            .filter(|p| {
                matches!(
                    p.outcome,
                    Outcome::Added | Outcome::Updated | Outcome::Unchanged
                )
            })
            .count();
        let failed = report
            .pages
            .iter()
            .filter_map(|p| match &p.outcome {
                Outcome::Failed(why) => Some(FailedPage {
                    file: p.file.clone(),
                    why: why.clone(),
                }),
                _ => None,
            })
            .collect();
        Ok(Synced {
            dir: report.dir,
            pages,
            failed,
        })
    }
}

struct FixtureFetch(PathBuf);

impl Fetch for FixtureFetch {
    fn get(&self, url: &str) -> Result<String> {
        let path = fixture_path(&self.0, url);
        std::fs::read_to_string(&path).with_context(|| format!("{url}: {}", path.display()))
    }
}

fn fixture_path(dir: &Path, url: &str) -> PathBuf {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = dir.join(rest);
    if rest.ends_with('/') {
        path.join("index.html")
    } else {
        path
    }
}
