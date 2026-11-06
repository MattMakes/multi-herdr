//! The git client (`vcs::git`), on temp repos with real git.
//!
//! Real git touches only temp dirs (NFR-07). Every git process gets an empty
//! `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_NOSYSTEM=1` and pinned identities and
//! dates, so hashes are the same on every machine. Without git on `PATH` the
//! git tests are skipped, unless `HORCH_REQUIRE_GIT=1`.

mod common;

use std::path::{Path, PathBuf};

use horch_core::vcs::git::{GitCli, GitClient};
use horch_marketplace::git::GitRunner;
use tempfile::TempDir;

/// The fixture's commit date, as in the e2e harness.
const GIT_DATE: &str = "2026-09-28T12:00:00+00:00";

fn find_git() -> Option<PathBuf> {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join("git"))
            .find(|p| p.is_absolute() && p.is_file())
    });
    if found.is_none() {
        assert!(
            std::env::var_os("HORCH_REQUIRE_GIT").is_none_or(|v| v != "1"),
            "HORCH_REQUIRE_GIT=1 is set but git is not on PATH"
        );
    }
    found
}

/// A temp repo on `main` with one commit: `a.txt` (3 lines) and `b.txt`.
struct Fixture {
    _tmp: TempDir,
    repo: PathBuf,
    /// For fixture setup; pinned like `git`.
    runner: GitRunner,
    git: GitCli,
}

impl Fixture {
    fn new() -> Option<Self> {
        let bin = find_git()?;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let temp_root = std::env::temp_dir().canonicalize().unwrap();
        assert!(root.starts_with(&temp_root), "real git only on temp repos");
        let config = root.join("gitconfig");
        std::fs::write(&config, "").unwrap();
        let config = config.to_str().unwrap().to_owned();
        let pins = [
            ("GIT_CONFIG_GLOBAL", config.as_str()),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Horch Fixture"),
            ("GIT_AUTHOR_EMAIL", "fixture@horch.invalid"),
            ("GIT_COMMITTER_NAME", "Horch Fixture"),
            ("GIT_COMMITTER_EMAIL", "fixture@horch.invalid"),
            ("GIT_AUTHOR_DATE", GIT_DATE),
            ("GIT_COMMITTER_DATE", GIT_DATE),
        ];
        let mut runner = GitRunner::new(&bin);
        let mut git = GitCli::new(bin);
        for (k, v) in pins {
            runner = runner.with_env(k, v);
            git = git.with_env(k, v);
        }
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        let f = Fixture {
            _tmp: tmp,
            repo,
            runner,
            git,
        };
        f.git_in(&f.repo, &["init", "-q", "-b", "main"]);
        std::fs::write(f.repo.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(f.repo.join("b.txt"), "bee\n").unwrap();
        f.commit_in(&f.repo, "base");
        Some(f)
    }

    fn git_in(&self, dir: &Path, args: &[&str]) -> String {
        self.runner.run(dir, args).unwrap().stdout_text()
    }

    fn commit_in(&self, dir: &Path, message: &str) -> String {
        self.git_in(dir, &["add", "-A"]);
        self.git_in(dir, &["commit", "-q", "-m", message]);
        self.git_in(dir, &["rev-parse", "HEAD"])
    }
}

#[test]
fn git_rev_parse_and_toplevel() {
    let Some(f) = Fixture::new() else { return };
    let head = f.git_in(&f.repo, &["rev-parse", "HEAD"]);
    assert_eq!(
        f.git.rev_parse(&f.repo, "main").unwrap().as_deref(),
        Some(head.as_str())
    );
    assert_eq!(f.git.rev_parse(&f.repo, "refs/heads/ghost").unwrap(), None);
    // An option-shaped revision never reaches git.
    assert!(f.git.rev_parse(&f.repo, "--all").is_err());
    assert_eq!(f.git.toplevel(&f.repo).unwrap(), f.repo);
}

/// D20 item 5: the fixture with `GIT_DIR` and `GIT_WORK_TREE` aimed at a
/// decoy repository leaves the decoy unchanged.
#[test]
fn git_env_cannot_reach_another_repository() {
    let Some(bin) = find_git() else { return };
    common::assert_decoy_untouched(&bin, "git_env_decoy_child");
}

#[test]
#[ignore = "run by git_env_cannot_reach_another_repository, with GIT_DIR set"]
fn git_env_decoy_child() {
    assert!(Fixture::new().is_some());
}
