//! `herdr-docs-sync` - mirror the Herdr documentation into a local directory.
//!
//! Discovers every English page under <https://herdr.dev/docs/> from the sitemap,
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

impl Outcome {
    /// The word printed beside a file name; a dry run says what would happen.
    fn label(self, dry_run: bool) -> &'static str {
        // Indexed by `Outcome as usize`: (real run, dry run).
        const LABELS: [(&str, &str); 5] = [
            ("added", "would add"),
            ("updated", "would update"),
            ("unchanged", "unchanged"),
            ("removed", "would remove"),
            ("FAILED", "FAILED"),
        ];
        let (done, would) = LABELS[self as usize];
        if dry_run {
            would
        } else {
            done
        }
    }
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

    prune_stale(&cli, &pages, &mut counts)?;
    Ok(print_summary(&counts, cli.dry_run) == 0)
}

/// Remove local files whose pages are gone, and count them.
fn prune_stale(cli: &Cli, pages: &[DocPage], counts: &mut [usize; 5]) -> Result<()> {
    // Pruning deletes local documentation, so only do it from a complete picture.
    // If a page could not be fetched or converted, the run does not know what the
    // site really contains, and deleting on that basis could destroy good docs.
    if cli.keep_stale {
        Ok(())
    } else if counts[Outcome::Failed as usize] > 0 {
        eprintln!("\n  skipping cleanup of stale files: some pages failed this run");
        Ok(())
    } else {
        remove_stale(cli, pages, counts)
    }
}

fn remove_stale(cli: &Cli, pages: &[DocPage], counts: &mut [usize; 5]) -> Result<()> {
    for removed in stale_files(&cli.out, pages)? {
        println!(
            "  {:<14} {}",
            Outcome::Removed.label(cli.dry_run),
            removed.file_name().unwrap_or_default().to_string_lossy()
        );
        if !cli.dry_run {
            std::fs::remove_file(&removed)
                .with_context(|| format!("removing {}", removed.display()))?;
        }
        counts[Outcome::Removed as usize] += 1;
    }
    Ok(())
}

/// Print the totals and return how many pages failed.
fn print_summary(counts: &[usize; 5], dry_run: bool) -> usize {
    let failed = counts[Outcome::Failed as usize];
    let suffix = if dry_run {
        "  (dry run: nothing written)"
    } else {
        ""
    };
    println!(
        "\n{} added, {} updated, {} unchanged, {} removed, {} failed{suffix}",
        counts[Outcome::Added as usize],
        counts[Outcome::Updated as usize],
        counts[Outcome::Unchanged as usize],
        counts[Outcome::Removed as usize],
        failed,
    );
    if failed > 0 {
        eprintln!("\n{failed} page(s) failed; everything else was written.");
    }
    failed
}

/// Resolve the sitemap, following a `<sitemapindex>` to the `<urlset>`s it names.
fn discover(sitemap_url: &str) -> Result<Vec<DocPage>> {
    let mut urls = Vec::new();
    match sitemap::parse(&fetch(sitemap_url)?)? {
        Sitemap::Urls(found) => urls.extend(found),
        Sitemap::Index(children) => child_urls(children, &mut urls)?,
    }
    Ok(sitemap::doc_pages(&urls))
}

/// Collect the page URLs of each child sitemap.
fn child_urls(children: Vec<String>, urls: &mut Vec<String>) -> Result<()> {
    for child in children {
        match sitemap::parse(&fetch(&child)?)? {
            Sitemap::Urls(found) => urls.extend(found),
            // One level of indirection is all the spec allows.
            Sitemap::Index(_) => {
                eprintln!("  note: skipping nested sitemap index at {child}")
            }
        }
    }
    Ok(())
}

fn sync_page(doc: &DocPage, out_dir: &Path, dry_run: bool) -> Outcome {
    match fetch_and_write(doc, &out_dir.join(&doc.file_name), dry_run) {
        Ok(outcome) => {
            println!("  {:<14} {}", outcome.label(dry_run), doc.file_name);
            outcome
        }
        Err(e) => {
            eprintln!(
                "  {:<14} {} - {e:#}",
                Outcome::Failed.label(dry_run),
                doc.file_name
            );
            Outcome::Failed
        }
    }
}

fn fetch_and_write(doc: &DocPage, target: &Path, dry_run: bool) -> Result<Outcome> {
    let markdown = page::to_markdown(&fetch(&doc.url)?)?;
    write_page(target, &markdown, dry_run)
}

/// Compare `markdown` with what is on disk and write it when it differs.
fn write_page(target: &Path, markdown: &str, dry_run: bool) -> Result<Outcome> {
    let outcome = match std::fs::read_to_string(target).ok() {
        Some(current) if current == markdown => Outcome::Unchanged,
        Some(_) => Outcome::Updated,
        None => Outcome::Added,
    };
    if outcome != Outcome::Unchanged && !dry_run {
        std::fs::write(target, markdown).context("writing failed")?;
    }
    Ok(outcome)
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
        .filter(|path| is_markdown_file(path))
        .filter(|path| is_unexpected(path, &expected))
        .collect())
}

/// A file with the extension `md`.
fn is_markdown_file(path: &Path) -> bool {
    path.is_file() && path.extension().is_some_and(|e| e == "md")
}

/// A path whose file name is text that no page maps to.
fn is_unexpected(path: &Path, expected: &BTreeSet<&str>) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| !expected.contains(name))
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
        assert!(
            !cli.keep_stale,
            "pruning is the default so the mirror stays faithful"
        );
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

    /// Only a file whose extension is `md` can be stale, and it is given by its
    /// path: the case, a longer extension and a name with no extension all
    /// leave a file alone.
    #[test]
    fn stale_files_wants_the_extension_md_and_gives_the_path() {
        let tmp = tempfile::tempdir().unwrap();
        for name in [
            "old.md",
            "UPPER.MD",
            "old.md.bak",
            "old.markdown",
            "old",
            "md",
            ".md",
        ] {
            std::fs::write(tmp.path().join(name), "x").unwrap();
        }
        let stale = stale_files(tmp.path(), &pages(&["install"])).unwrap();
        assert_eq!(stale, vec![tmp.path().join("old.md")]);
        // A page that maps to the file keeps it.
        assert!(stale_files(tmp.path(), &pages(&["old"]))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn stale_files_is_empty_for_a_fresh_directory() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(
            stale_files(&tmp.path().join("absent"), &pages(&["install"]))
                .unwrap()
                .is_empty()
        );
        assert!(stale_files(tmp.path(), &pages(&["install"]))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn labels_say_what_would_happen_in_a_dry_run() {
        assert_eq!(Outcome::Added.label(false), "added");
        assert_eq!(Outcome::Added.label(true), "would add");
        assert_eq!(Outcome::Updated.label(false), "updated");
        assert_eq!(Outcome::Updated.label(true), "would update");
        assert_eq!(Outcome::Removed.label(false), "removed");
        assert_eq!(Outcome::Removed.label(true), "would remove");
        assert_eq!(Outcome::Unchanged.label(true), "unchanged");
        assert_eq!(Outcome::Unchanged.label(false), "unchanged");
        assert_eq!(Outcome::Failed.label(true), "FAILED");
        assert_eq!(Outcome::Failed.label(false), "FAILED");
    }

    #[test]
    fn write_page_adds_updates_and_leaves_unchanged_files() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("install.md");

        assert_eq!(write_page(&target, "v1", false).unwrap(), Outcome::Added);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v1");
        assert_eq!(
            write_page(&target, "v1", false).unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(write_page(&target, "v2", false).unwrap(), Outcome::Updated);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "v2");
    }

    #[test]
    fn write_page_writes_nothing_in_a_dry_run() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("install.md");
        assert_eq!(write_page(&target, "v1", true).unwrap(), Outcome::Added);
        assert!(!target.exists());

        std::fs::write(&target, "old").unwrap();
        assert_eq!(write_page(&target, "new", true).unwrap(), Outcome::Updated);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
    }

    #[test]
    fn write_page_reports_a_write_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("missing-dir").join("install.md");
        let err = format!("{:#}", write_page(&target, "x", false).unwrap_err());
        assert!(err.starts_with("writing failed: "), "{err}");
    }

    fn stale_fixture(args: &[&str]) -> (tempfile::TempDir, Cli) {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("install.md"), "keep").unwrap();
        std::fs::write(tmp.path().join("old.md"), "stale").unwrap();
        let out = tmp.path().to_str().unwrap().to_string();
        let mut argv = vec!["herdr-docs-sync", "--out", out.as_str()];
        argv.extend_from_slice(args);
        let cli = Cli::try_parse_from(argv).unwrap();
        (tmp, cli)
    }

    #[test]
    fn prune_stale_removes_files_whose_pages_are_gone() {
        let (tmp, cli) = stale_fixture(&[]);
        let mut counts = [0usize; 5];
        prune_stale(&cli, &pages(&["install"]), &mut counts).unwrap();
        assert!(!tmp.path().join("old.md").exists());
        assert!(tmp.path().join("install.md").exists());
        assert_eq!(counts[Outcome::Removed as usize], 1);
    }

    #[test]
    fn prune_stale_deletes_nothing_when_a_page_failed() {
        let (tmp, cli) = stale_fixture(&[]);
        let mut counts = [0usize; 5];
        counts[Outcome::Failed as usize] = 1;
        prune_stale(&cli, &pages(&["install"]), &mut counts).unwrap();
        assert!(tmp.path().join("old.md").exists());
        assert_eq!(counts[Outcome::Removed as usize], 0);
    }

    #[test]
    fn prune_stale_honours_keep_stale_and_dry_run() {
        let (tmp, keep) = stale_fixture(&["--keep-stale"]);
        let mut counts = [0usize; 5];
        prune_stale(&keep, &pages(&["install"]), &mut counts).unwrap();
        assert!(tmp.path().join("old.md").exists());
        assert_eq!(counts[Outcome::Removed as usize], 0);

        let (tmp, dry) = stale_fixture(&["--dry-run"]);
        let mut counts = [0usize; 5];
        prune_stale(&dry, &pages(&["install"]), &mut counts).unwrap();
        assert!(
            tmp.path().join("old.md").exists(),
            "dry run must not delete"
        );
        assert_eq!(counts[Outcome::Removed as usize], 1, "but it still counts");
    }

    #[test]
    fn print_summary_returns_the_failure_count() {
        assert_eq!(print_summary(&[1, 2, 3, 4, 0], false), 0);
        assert_eq!(print_summary(&[0, 0, 0, 0, 3], true), 3);
    }
}
