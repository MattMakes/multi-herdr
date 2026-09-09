//! `herdr-docs-sync` - mirror the Herdr documentation into a local directory.
//!
//! Discovers every English page under https://herdr.dev/docs/ from the sitemap,
//! converts each to markdown, and writes it to `herdr-docs/`. Safe to run as often
//! as you like: it reports what changed, and removes local files for pages that no
//! longer exist so the directory stays a faithful mirror rather than an accreting
//! pile.

mod clean;
mod page;
mod sitemap;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Parser;

use sitemap::{DocPage, Sitemap};

const SITEMAP_URL: &str = "https://herdr.dev/sitemap.xml";

#[derive(Parser)]
#[command(
    name = "herdr-docs-sync",
    version,
    about = "Mirror the Herdr documentation as local markdown",
    long_about = "Mirror the Herdr documentation (https://herdr.dev/docs/) as local markdown.\n\n\
                  Discovers pages from the sitemap, converts each to markdown, and writes\n\
                  them to the output directory. Re-run any time to refresh."
)]
struct Cli {
    /// Where to write the markdown files.
    #[arg(long, short = 'o', default_value = "herdr-docs", value_name = "DIR")]
    out: PathBuf,

    /// Report what would change without writing anything.
    #[arg(long)]
    dry_run: bool,

    /// Leave local files whose pages are gone from the site.
    #[arg(long)]
    keep_stale: bool,

    /// Where to discover pages from.
    #[arg(long, default_value = SITEMAP_URL, value_name = "URL")]
    sitemap: String,
}

/// What happened to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Added,
    Updated,
    Unchanged,
    Removed,
    Failed,
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(true) => std::process::ExitCode::SUCCESS,
        // Some pages failed; the rest were still written.
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(e) => {
            eprintln!("herdr-docs-sync: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<bool> {
    let cli = Cli::parse();

    println!("Discovering pages from {}...", cli.sitemap);
    let pages = discover(&cli.sitemap)?;
    if pages.is_empty() {
        bail!(
            "the sitemap listed no pages under {}. Has the docs site moved?",
            sitemap::DOCS_PREFIX
        );
    }
    println!("Found {} documentation pages.\n", pages.len());

    if !cli.dry_run {
        std::fs::create_dir_all(&cli.out)
            .with_context(|| format!("creating {}", cli.out.display()))?;
    }

    let mut counts = [0usize; 5];
    for doc in &pages {
        counts[sync_page(doc, &cli.out, cli.dry_run) as usize] += 1;
    }

    // Pruning deletes local documentation, so only do it from a complete picture.
    // If a page could not be fetched or converted, the run does not know what the
    // site really contains, and deleting on that basis could destroy good docs.
    let prune = !cli.keep_stale && counts[Outcome::Failed as usize] == 0;
    if !cli.keep_stale && !prune {
        eprintln!("\n  skipping cleanup of stale files: some pages failed this run");
    }
    if prune {
        for removed in stale_files(&cli.out, &pages)? {
            let verb = if cli.dry_run { "would remove" } else { "removed" };
            println!("  {verb:<14} {}", removed.file_name().unwrap_or_default().to_string_lossy());
            if !cli.dry_run {
                std::fs::remove_file(&removed)
                    .with_context(|| format!("removing {}", removed.display()))?;
            }
            counts[Outcome::Removed as usize] += 1;
        }
    }

    let failed = counts[Outcome::Failed as usize];
    println!(
        "\n{} added, {} updated, {} unchanged, {} removed, {} failed{}",
        counts[Outcome::Added as usize],
        counts[Outcome::Updated as usize],
        counts[Outcome::Unchanged as usize],
        counts[Outcome::Removed as usize],
        failed,
        if cli.dry_run { "  (dry run: nothing written)" } else { "" }
    );
    if failed > 0 {
        eprintln!("\n{failed} page(s) failed; everything else was written.");
    }
    Ok(failed == 0)
}

/// Resolve the sitemap, following a `<sitemapindex>` to the `<urlset>`s it names.
fn discover(sitemap_url: &str) -> Result<Vec<DocPage>> {
    let mut urls = Vec::new();
    match sitemap::parse(&fetch(sitemap_url)?)? {
        Sitemap::Urls(found) => urls.extend(found),
        Sitemap::Index(children) => {
            for child in children {
                match sitemap::parse(&fetch(&child)?)? {
                    Sitemap::Urls(found) => urls.extend(found),
                    // One level of indirection is all the spec allows.
                    Sitemap::Index(_) => {
                        eprintln!("  note: skipping nested sitemap index at {child}")
                    }
                }
            }
        }
    }
    Ok(sitemap::doc_pages(&urls))
}

fn sync_page(doc: &DocPage, out_dir: &Path, dry_run: bool) -> Outcome {
    let target = out_dir.join(&doc.file_name);
    let markdown = match fetch(&doc.url).and_then(|html| page::to_markdown(&html)) {
        Ok(markdown) => markdown,
        Err(e) => {
            eprintln!("  {:<14} {} - {e:#}", "FAILED", doc.file_name);
            return Outcome::Failed;
        }
    };

    let existing = std::fs::read_to_string(&target).ok();
    let outcome = match &existing {
        Some(current) if *current == markdown => Outcome::Unchanged,
        Some(_) => Outcome::Updated,
        None => Outcome::Added,
    };

    if outcome != Outcome::Unchanged && !dry_run {
        if let Err(e) = std::fs::write(&target, &markdown) {
            eprintln!("  {:<14} {} - writing failed: {e}", "FAILED", doc.file_name);
            return Outcome::Failed;
        }
    }

    let label = match (outcome, dry_run) {
        (Outcome::Added, false) => "added",
        (Outcome::Added, true) => "would add",
        (Outcome::Updated, false) => "updated",
        (Outcome::Updated, true) => "would update",
        _ => "unchanged",
    };
    println!("  {label:<14} {}", doc.file_name);
    outcome
}

/// Markdown files in `out_dir` that no page maps to any more.
fn stale_files(out_dir: &Path, pages: &[DocPage]) -> Result<Vec<PathBuf>> {
    let expected: BTreeSet<&str> = pages.iter().map(|p| p.file_name.as_str()).collect();
    let Ok(entries) = std::fs::read_dir(out_dir) else {
        return Ok(Vec::new());
    };
    Ok(entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|e| e == "md"))
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| !expected.contains(name))
        })
        .collect())
}

fn fetch(url: &str) -> Result<String> {
    ureq::get(url)
        .call()
        .with_context(|| format!("fetching {url}"))?
        .body_mut()
        .read_to_string()
        .with_context(|| format!("reading {url}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn defaults_write_to_herdr_docs_and_prune() {
        let cli = Cli::try_parse_from(["herdr-docs-sync"]).unwrap();
        assert_eq!(cli.out, PathBuf::from("herdr-docs"));
        assert!(!cli.dry_run);
        assert!(!cli.keep_stale, "pruning is the default so the mirror stays faithful");
        assert_eq!(cli.sitemap, SITEMAP_URL);
    }

    fn pages(names: &[&str]) -> Vec<DocPage> {
        names
            .iter()
            .map(|n| DocPage {
                url: format!("https://herdr.dev/docs/{n}/"),
                file_name: format!("{n}.md"),
            })
            .collect()
    }

    /// A renamed or retired page leaves a stale local file, which must be found.
    #[test]
    fn stale_files_finds_only_unexpected_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["install.md", "agent-guide.md", "notes.txt", "README.md"] {
            std::fs::write(tmp.path().join(name), "x").unwrap();
        }
        std::fs::create_dir(tmp.path().join("subdir.md")).unwrap();

        let mut stale: Vec<String> = stale_files(tmp.path(), &pages(&["install"]))
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        stale.sort();

        // README.md is not in `pages`, so it is stale here; notse.txt is not
        // markdown and the directory is not a file.
        assert_eq!(stale, vec!["README.md", "agent-guide.md"]);
    }

    #[test]
    fn stale_files_is_empty_for_a_fresh_directory() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(stale_files(&tmp.path().join("absent"), &pages(&["install"]))
            .unwrap()
            .is_empty());
        assert!(stale_files(tmp.path(), &pages(&["install"])).unwrap().is_empty());
    }
}
