//! `agent-docs-sync` - mirror the docs of each agent CLI as local markdown.
//!
//! [`sync`] fetches the pages of 1 source of `docs-sources.toml` into
//! `<out>/<tool>/<version>/`, reports what changed, removes local pages that
//! are gone, and writes a manifest of page hashes. It then writes
//! `<out>/<tool>/CHANGES.md` against the previous version and copies the
//! `docs/` folder of an `also_git` repository. The caller gives the version
//! text and the fetch port; the only process this crate starts is git.

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

/// The manifest of page hashes in each version folder.
pub const MANIFEST_FILE: &str = "manifest.json";

/// The fetch port: the text at a URL.
pub trait Fetch {
    fn get(&self, url: &str) -> Result<String>;
}

/// The production fetch port, over HTTPS.
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

/// What happened to 1 file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Added,
    Updated,
    Unchanged,
    Removed,
    /// The page was not written; the text says why.
    Failed(String),
}

/// 1 file of a sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageReport {
    pub file: String,
    pub outcome: Outcome,
}

/// The result of 1 sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// `<out>/<tool>/<version>`.
    pub dir: PathBuf,
    pub pages: Vec<PageReport>,
}

impl Report {
    /// The pages that were not written.
    pub fn failed(&self) -> Vec<&PageReport> {
        self.pages
            .iter()
            .filter(|p| matches!(p.outcome, Outcome::Failed(_)))
            .collect()
    }
}

/// `manifest.json`: the sha256 of each page in a version folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub tool: String,
    pub version: String,
    /// File name to lowercase hex sha256.
    pub pages: BTreeMap<String, String>,
}

/// 1 page to write: its file name and its URL, or why it has none.
struct Wanted {
    file: String,
    url: Result<String>,
}

/// Mirror the pages of `source` into `<out>/<tool>/<version>/`.
///
/// A page that fails is reported, and the others are still written. Stale
/// pages are removed only when no page failed. The `also_git` clone is the
/// report entry [`GIT_DIR`]. An error means that the page list itself could
/// not be read, or that CHANGES.md could not be written.
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
    // Pruning deletes local docs, so only do it from a complete picture.
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

/// The version text names a folder, so it must be 1 plain path component.
fn check_version(version: &str) -> Result<()> {
    if version.is_empty() || version == "." || version == ".." || version.contains(['/', '\\']) {
        bail!("the version text {version:?} cannot name a folder");
    }
    Ok(())
}

/// The English pages under `/docs/` of the sitemap, following an index 1 level.
fn sitemap_pages(source: &Source, fetch: &dyn Fetch) -> Result<Vec<Wanted>> {
    let mut urls = Vec::new();
    match sitemap::parse(&fetch.get(&source.url)?)? {
        Sitemap::Urls(found) => urls.extend(found),
        Sitemap::Index(children) => {
            for child in children {
                // 1 level of indirection is all the sitemap spec allows.
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

/// The included pages of the `llms.txt` index, in the order of `include`.
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

/// Compare `markdown` with what is on disk and write it when it differs.
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

/// Remove the markdown files of `dir` that no wanted page maps to.
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

/// The names of the `.md` files of `dir`, sorted.
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

/// Write `manifest.json` with the hash of each page now in `dir`.
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
