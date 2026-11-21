mod changes;
mod clean;
mod git;
mod llms;
mod page;
mod sitemap;
mod sources;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use changes::{CHANGES_FILE, VERSIONS_FILE};
pub use git::GIT_DIR;
use sitemap::Sitemap;
pub use sources::{parse_sources, sources, Kind, Source};

pub const MANIFEST_FILE: &str = "manifest.json";

pub trait Fetch {
    fn get(&self, url: &str) -> Result<String>;
}

pub struct HttpFetch;

impl Fetch for HttpFetch {
    fn get(&self, url: &str) -> Result<String> {
        ureq::get(url)
            .call()
            .with_context(|| format!("fetching {url}"))?
            .body_mut()
            .read_to_string()
            .with_context(|| format!("reading {url}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Added,
    Updated,
    Unchanged,
    Removed,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageReport {
    pub file: String,
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub dir: PathBuf,
    pub pages: Vec<PageReport>,
}

impl Report {
    pub fn failed(&self) -> Vec<&PageReport> {
        self.pages
            .iter()
            .filter(|p| matches!(p.outcome, Outcome::Failed(_)))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub tool: String,
    pub version: String,
    pub pages: BTreeMap<String, String>,
}

struct Wanted {
    file: String,
    url: Result<String>,
}

pub fn sync(source: &Source, version: &str, fetch: &dyn Fetch, out: &Path) -> Result<Report> {
    check_version(version)?;
    let dir = out.join(&source.tool).join(version);
    let wanted = match source.kind {
        Kind::Sitemap => sitemap_pages(source, fetch)?,
        Kind::LlmsTxt => llms_pages(source, fetch)?,
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let mut pages: Vec<PageReport> = wanted
        .iter()
        .map(|w| PageReport {
            file: w.file.clone(),
            outcome: write_wanted(w, source.kind, fetch, &dir),
        })
        .collect();
    let any_failed = pages
        .iter()
        .any(|p| matches!(p.outcome, Outcome::Failed(_)));
    if !any_failed {
        pages.extend(remove_stale(&dir, &wanted)?);
    }
    write_manifest(&source.tool, version, &dir)?;
    changes::write(&out.join(&source.tool), &source.tool, version)?;
    if let Some(url) = &source.also_git {
        pages.push(PageReport {
            file: GIT_DIR.to_string(),
            outcome: git::copy_docs(url, &dir),
        });
    }
    Ok(Report { dir, pages })
}

fn check_version(version: &str) -> Result<()> {
    if version.is_empty() || version == "." || version == ".." || version.contains(['/', '\\']) {
        bail!("the version text {version:?} cannot name a folder");
    }
    Ok(())
}

fn sitemap_pages(source: &Source, fetch: &dyn Fetch) -> Result<Vec<Wanted>> {
    let mut urls = Vec::new();
    match sitemap::parse(&fetch.get(&source.url)?)? {
        Sitemap::Urls(found) => urls.extend(found),
        Sitemap::Index(children) => {
            for child in children {
                if let Sitemap::Urls(found) = sitemap::parse(&fetch.get(&child)?)? {
                    urls.extend(found);
                }
            }
        }
    }
    let pages = sitemap::doc_pages(&urls);
    if pages.is_empty() {
        bail!(
            "the sitemap {} listed no pages under {}. Has the docs site moved?",
            source.url,
            sitemap::DOCS_PREFIX
        );
    }
    Ok(pages
        .into_iter()
        .map(|p| Wanted {
            file: p.file_name,
            url: Ok(p.url),
        })
        .collect())
}

fn llms_pages(source: &Source, fetch: &dyn Fetch) -> Result<Vec<Wanted>> {
    let links = llms::links(&fetch.get(&source.url)?, &source.url);
    let suffix = source.md_suffix.as_deref();
    Ok(source
        .include
        .iter()
        .map(|entry| Wanted {
            file: llms::file_name(entry),
            url: llms::pick(&links, entry, suffix)
                .map(|link| llms::page_url(link, suffix))
                .ok_or_else(|| anyhow!("{entry} is not listed in llms.txt at {}", source.url)),
        })
        .collect())
}

fn write_wanted(wanted: &Wanted, kind: Kind, fetch: &dyn Fetch, dir: &Path) -> Outcome {
    let written = wanted
        .url
        .as_ref()
        .map_err(|e| anyhow!("{e:#}"))
        .and_then(|url| {
            let text = fetch.get(url)?;
            let markdown = match kind {
                Kind::Sitemap => page::to_markdown(&text)?,
                Kind::LlmsTxt => text,
            };
            write_page(&dir.join(&wanted.file), &markdown)
        });
    written.unwrap_or_else(|e| Outcome::Failed(format!("{e:#}")))
}

fn write_page(target: &Path, markdown: &str) -> Result<Outcome> {
    let outcome = match std::fs::read_to_string(target).ok() {
        Some(current) if current == markdown => Outcome::Unchanged,
        Some(_) => Outcome::Updated,
        None => Outcome::Added,
    };
    if outcome != Outcome::Unchanged {
        std::fs::write(target, markdown).context("writing failed")?;
    }
    Ok(outcome)
}

fn remove_stale(dir: &Path, wanted: &[Wanted]) -> Result<Vec<PageReport>> {
    let expected: BTreeSet<&str> = wanted.iter().map(|w| w.file.as_str()).collect();
    let mut removed = Vec::new();
    for file in markdown_files(dir)? {
        if !expected.contains(file.as_str()) {
            let path = dir.join(&file);
            std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            removed.push(PageReport {
                file,
                outcome: Outcome::Removed,
            });
        }
    }
    Ok(removed)
}

fn markdown_files(dir: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "md") {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                files.push(name.to_string());
            }
        }
    }
    files.sort();
    Ok(files)
}

fn write_manifest(tool: &str, version: &str, dir: &Path) -> Result<()> {
    let mut pages = BTreeMap::new();
    for file in markdown_files(dir)? {
        let bytes = std::fs::read(dir.join(&file))?;
        pages.insert(file, format!("{:x}", Sha256::digest(&bytes)));
    }
    let manifest = Manifest {
        tool: tool.to_string(),
        version: version.to_string(),
        pages,
    };
    let path = dir.join(MANIFEST_FILE);
    std::fs::write(&path, serde_json::to_string_pretty(&manifest)? + "\n")
        .with_context(|| format!("writing {}", path.display()))
}
