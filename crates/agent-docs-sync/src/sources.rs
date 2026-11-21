use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const SOURCES_TOML: &str = include_str!("../docs-sources.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Sitemap,
    LlmsTxt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub tool: String,
    pub kind: Kind,
    pub url: String,
    pub md_suffix: Option<String>,
    pub include: Vec<String>,
    pub also_git: Option<String>,
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

pub fn sources() -> Result<Vec<Source>> {
    parse_sources(SOURCES_TOML)
}

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
