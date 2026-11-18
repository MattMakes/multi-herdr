//! `docs-sources.toml`: 1 entry for each tool whose docs are mirrored.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

/// The sources file of the design, built into the crate.
const SOURCES_TOML: &str = include_str!("../docs-sources.toml");

/// How the pages of a source are found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// An XML sitemap that lists HTML pages under `/docs/`.
    Sitemap,
    /// An `llms.txt` index that links to markdown pages.
    LlmsTxt,
}

/// 1 tool of `docs-sources.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The table name, also the folder name under the output folder.
    pub tool: String,
    pub kind: Kind,
    pub url: String,
    /// Added to a page URL that does not end with it, to get markdown.
    pub md_suffix: Option<String>,
    /// The pages of an `llms_txt` source, as paths relative to the docs.
    pub include: Vec<String>,
    /// A git repository whose `docs/` folder is copied too.
    pub also_git: Option<String>,
    /// The command that prints the installed version.
    pub version_cmd: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    kind: Kind,
    url: String,
    md_suffix: Option<String>,
    #[serde(default)]
    include: Vec<String>,
    also_git: Option<String>,
    version_cmd: String,
}

/// The built-in sources, sorted by tool name.
pub fn sources() -> Result<Vec<Source>> {
    parse_sources(SOURCES_TOML)
}

/// Parse a sources file. An unknown key is an error that names it.
pub fn parse_sources(text: &str) -> Result<Vec<Source>> {
    let tables: BTreeMap<String, toml::Value> =
        toml::from_str(text).context("reading docs-sources.toml")?;
    tables
        .into_iter()
        .map(|(tool, value)| source(tool, value))
        .collect()
}

fn source(tool: String, value: toml::Value) -> Result<Source> {
    let entry: Entry = value
        .try_into()
        .with_context(|| format!("docs-sources.toml: source [{tool}]"))?;
    if entry.kind == Kind::LlmsTxt && entry.include.is_empty() {
        bail!("docs-sources.toml: source [{tool}] of kind llms_txt has no include");
    }
    Ok(Source {
        tool,
        kind: entry.kind,
        url: entry.url,
        md_suffix: entry.md_suffix,
        include: entry.include,
        also_git: entry.also_git,
        version_cmd: entry.version_cmd,
    })
}
