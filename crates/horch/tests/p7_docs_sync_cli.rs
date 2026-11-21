//! `horch docs sync` (fleetd-p7 ticket 22): steps 1 and 2 of 1 sync as an
//! operator command that needs no fleetd.
//!
//! No test uses the network: `HORCH_TEST_DOCS_FIXTURE` makes the fetch port read
//! each URL from a local folder, and the `also_git` repository of Codex is a
//! local repository in that folder. The version commands are scripts in a
//! temp folder, not real agent CLIs. Each run uses temp folders for the
//! ledger, the data and the state of horch.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use agent_docs_sync::{sources, Kind, Source, CHANGES_FILE, MANIFEST_FILE};

/// The folder of a URL in the fixture: its host and path, and
/// `index.html` for a path that ends with `/`.
fn fixture_path(fixture: &Path, url: &str) -> PathBuf {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let mut path = fixture.join(rest);
    if rest.ends_with('/') {
        path = path.join("index.html");
    }
    path
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// The herdr pages: the sitemap fixture of agent-docs-sync.
fn herdr_fixture(fixture: &Path, source: &Source) {
    let pages =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent-docs-sync/tests/fixtures/herdr");
    let read = |f: &str| std::fs::read_to_string(pages.join(f)).unwrap();
    put(&fixture_path(fixture, &source.url), &read("sitemap.xml"));
    put(
        &fixture_path(fixture, "https://herdr.dev/docs/"),
        &read("index.html"),
    );
    put(
        &fixture_path(fixture, "https://herdr.dev/docs/install/"),
        &read("install.html"),
    );
}

/// An `llms.txt` that links each included page, and each page.
fn llms_fixture(fixture: &Path, source: &Source) {
    let base = source.url.trim_end_matches("llms.txt");
    let suffix = source.md_suffix.as_deref().unwrap_or("");
    let mut index = format!("# {}\n\n", source.tool);
    for entry in &source.include {
        let url = format!("{base}{entry}{suffix}");
        index.push_str(&format!("- [{entry}]({url}): {entry}.\n"));
        put(
            &fixture_path(fixture, &url),
            &format!("# {entry}\n\nUse `--flag-{}`.\n", entry.replace('/', "-")),
        );
    }
    put(&fixture_path(fixture, &source.url), &index);
}

/// A local repository with a `docs/` folder, at the fixture path of `url`.
fn git_fixture(fixture: &Path, url: &str) {
    let repo = fixture_path(fixture, url);
    put(&repo.join("docs/guide.md"), "# Guide\n");
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "docs"]);
}

/// The pages of each source of `docs-sources.toml` under `fixture`.
fn fixture(fixture: &Path) {
    for source in sources().unwrap() {
        match source.kind {
            Kind::Sitemap => herdr_fixture(fixture, &source),
            Kind::LlmsTxt => llms_fixture(fixture, &source),
        }
        if let Some(url) = &source.also_git {
            git_fixture(fixture, url);
        }
    }
}

/// The version text of the fake `cli`.
fn version_of(cli: &str) -> String {
    format!("{cli}-9.8.7")
}

/// A fake binary of each source that prints its version text.
fn fake_bins(dir: &Path, clis: &[&str]) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for cli in clis {
        let path = bin.join(cli);
        std::fs::write(
            &path,
            format!("#!/bin/sh\nprintf '%s\\n' '{}'\n", version_of(cli)),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

fn horch(root: &Path, bin: &Path, args: &[&str]) -> Output {
    std::fs::create_dir_all(root.join("project")).unwrap();
    Command::new(env!("CARGO_BIN_EXE_horch"))
        .args(args)
        .current_dir(root.join("project"))
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("HORCH_DATA_DIR", root.join("data"))
        .env("HORCH_STATE_DIR", root.join("state"))
        .env("HORCH_LEDGER_DB", root.join("ledger.sqlite3"))
        .env("HORCH_PROJECT_DIR", root.join("project"))
        .env("FLEET_ROOT", root.join("fleet"))
        .env("HORCH_TEST_DOCS_FIXTURE", root.join("fixture"))
        .output()
        .unwrap()
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "exit {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn docs_sync_writes_each_cli_into_the_fleet_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
    fixture(&root.join("fixture"));
    let all = sources().unwrap();
    let clis: Vec<&str> = all
        .iter()
        .map(|s| s.version_cmd.split_whitespace().next().unwrap())
        .collect();
    let bin = fake_bins(&root, &clis);

    let out = horch(&root, &bin, &["docs", "sync"]);

    assert_ok(&out);
    let docs = root.join("fleet/docs");
    for (source, cli) in all.iter().zip(&clis) {
        let dir = docs.join(&source.tool).join(version_of(cli));
        assert!(
            dir.join(MANIFEST_FILE).is_file(),
            "no manifest in {}",
            dir.display()
        );
        assert!(
            docs.join(&source.tool).join(CHANGES_FILE).is_file(),
            "no CHANGES.md for {}",
            source.tool
        );
        for entry in &source.include {
            let file = format!("{}.md", entry.replace('/', "-"));
            let page = std::fs::read_to_string(dir.join(&file))
                .unwrap_or_else(|e| panic!("{}/{file}: {e}", dir.display()));
            assert!(page.starts_with(&format!("# {entry}\n")), "{page}");
        }
        if source.also_git.is_some() {
            assert!(dir.join("git/docs/guide.md").is_file());
        }
    }
    assert!(docs
        .join("herdr")
        .join(version_of("herdr"))
        .join("install.md")
        .is_file());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for source in &all {
        assert!(stdout.contains(&source.tool), "{stdout}");
    }
}

#[test]
fn docs_sync_cli_herdr_out_refreshes_a_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
    fixture(&root.join("fixture"));
    // Only herdr has a binary: a sync of another CLI would fail.
    let bin = fake_bins(&root, &["herdr"]);
    let dir = root
        .join("project/herdr-docs/herdr")
        .join(version_of("herdr"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("retired.md"), "# Gone\n").unwrap();

    let out = horch(
        &root,
        &bin,
        &["docs", "sync", "--cli", "herdr", "--out", "herdr-docs"],
    );

    assert_ok(&out);
    assert!(dir.join("README.md").is_file());
    assert!(dir.join("install.md").is_file());
    assert!(dir.join(MANIFEST_FILE).is_file());
    assert!(!dir.join("retired.md").exists(), "a stale page is pruned");
    assert!(root
        .join("project/herdr-docs/herdr")
        .join(CHANGES_FILE)
        .is_file());
    let tools: Vec<_> = std::fs::read_dir(root.join("project/herdr-docs"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(tools, ["herdr"]);
    assert!(
        !root.join("fleet/docs").exists(),
        "--out replaces the fleet root"
    );
}

#[test]
fn docs_sync_of_an_unknown_cli_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
    let bin = fake_bins(&root, &[]);

    let out = horch(&root, &bin, &["docs", "sync", "--cli", "nope"]);

    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("nope"), "{stderr}");
}
