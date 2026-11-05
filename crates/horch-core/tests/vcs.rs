//! The git client (`vcs::git`), on temp repos with real git.
//!
//! Real git touches only temp dirs (NFR-07). Every git process gets an empty
//! `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_NOSYSTEM=1` and pinned identities and
//! dates, so hashes are the same on every machine. Without git on `PATH` the
//! git tests are skipped, unless `HORCH_REQUIRE_GIT=1`.

mod common;

use std::path::{Path, PathBuf};

use horch_core::vcs::git::{CherryPick, GitCli, GitClient, GitIdentity};
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
fn git_cherry_pick_conflict_reports_paths() {
    let Some(f) = Fixture::new() else { return };
    let base = f.git.head(&f.repo).unwrap();
    f.git_in(&f.repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(f.repo.join("a.txt"), "one\ntwo side\nthree\n").unwrap();
    f.commit_in(&f.repo, "side edits a");
    std::fs::write(f.repo.join("c.txt"), "sea\n").unwrap();
    let side_c = f.commit_in(&f.repo, "side adds c");
    f.git_in(&f.repo, &["checkout", "-q", "main"]);
    std::fs::write(f.repo.join("a.txt"), "one\ntwo main\nthree\n").unwrap();
    let main = f.commit_in(&f.repo, "main edits a");

    let id = GitIdentity {
        name: "picker".into(),
        email: "picker@horch.invalid".into(),
        date: "2026-10-02T11:00:00Z".into(),
    };
    let r = f
        .git
        .cherry_pick(&f.repo, &format!("{base}..side"), &id)
        .unwrap();
    assert_eq!(
        r,
        CherryPick::Conflict {
            paths: vec!["a.txt".into()]
        }
    );
    // Aborted: the checkout is as it was.
    assert_eq!(f.git.head(&f.repo).unwrap(), main);
    assert_eq!(f.git.status_porcelain(&f.repo).unwrap(), "");
    assert_eq!(f.git.rev_parse(&f.repo, "CHERRY_PICK_HEAD").unwrap(), None);

    // A clean pick reports the new head, committed as `id`.
    let CherryPick::Clean { head } = f.git.cherry_pick(&f.repo, &side_c, &id).unwrap() else {
        panic!("expected a clean pick");
    };
    assert_eq!(head, f.git.head(&f.repo).unwrap());
    assert_eq!(
        common::utc_offset_date(&f.git_in(&f.repo, &["log", "-1", "--format=%cn %cI %an", &head])),
        "picker 2026-10-02T11:00:00+00:00 Horch Fixture"
    );
    assert_eq!(
        f.git.rev_list(&f.repo, &format!("{main}..{head}")).unwrap(),
        std::slice::from_ref(&head)
    );
    assert!(f.git.is_ancestor(&f.repo, &main, &head).unwrap());
    assert!(!f.git.is_ancestor(&f.repo, &head, &main).unwrap());
}

#[test]
fn git_update_ref_cas_mismatch_is_false() {
    let Some(f) = Fixture::new() else { return };
    let base = f.git.head(&f.repo).unwrap();
    f.git_in(&f.repo, &["branch", "target"]);
    std::fs::write(f.repo.join("c.txt"), "sea\n").unwrap();
    let next = f.commit_in(&f.repo, "next");
    let target = "refs/heads/target";

    // The ref holds `base`, not `next`: no swap, no error.
    assert!(!f.git.update_ref_cas(&f.repo, target, &next, &next).unwrap());
    assert_eq!(
        f.git.rev_parse(&f.repo, target).unwrap().as_deref(),
        Some(base.as_str())
    );
    assert!(f.git.update_ref_cas(&f.repo, target, &next, &base).unwrap());
    assert_eq!(
        f.git.rev_parse(&f.repo, target).unwrap().as_deref(),
        Some(next.as_str())
    );

    // Option-shaped arguments never reach git. Each of these, run by git,
    // would succeed: `--no-deref` drops the compare, `-d` deletes the ref.
    for (refname, new, old) in [
        ("--no-deref", target, base.as_str()),
        (target, "-d", next.as_str()),
        (target, base.as_str(), "--no-deref"),
    ] {
        let tried = f.git.update_ref_cas(&f.repo, refname, new, old);
        assert!(tried.is_err(), "{refname} {new} {old}: {tried:?}");
    }
    assert_eq!(
        f.git.rev_parse(&f.repo, target).unwrap().as_deref(),
        Some(next.as_str())
    );
    assert!(f.git.rev_parse(&f.repo, "--all").is_err());

    assert!(f.git.version().unwrap().starts_with("git version "));
    assert_eq!(f.git.toplevel(&f.repo).unwrap(), f.repo);
}

/// A refused update is a lost race when the ref no longer holds the value the
/// caller expected, and an error when it still does. An absent ref holds the
/// all-zero value.
#[test]
fn git_update_ref_cas_tells_a_failure_from_a_lost_race() {
    let Some(f) = Fixture::new() else { return };
    let base = f.git.head(&f.repo).unwrap();
    f.git_in(&f.repo, &["branch", "target"]);
    std::fs::write(f.repo.join("c.txt"), "sea\n").unwrap();
    let next = f.commit_in(&f.repo, "next");
    let target = "refs/heads/target";
    let ghost = "refs/heads/ghost";
    let zero = "0".repeat(40);
    // Well formed, but no such object: git refuses to write it into a ref.
    let missing = "1".repeat(40);

    // The ref holds `base`, as expected, and git still refuses: an error.
    let err = f
        .git
        .update_ref_cas(&f.repo, target, &missing, &base)
        .unwrap_err()
        .to_string();
    let (status, detail) = err
        .strip_prefix("git update-ref refs/heads/target failed (")
        .and_then(|rest| rest.split_once("): "))
        .unwrap_or_else(|| panic!("unexpected message: {err}"));
    assert_ne!(status.parse::<i32>().unwrap(), 0, "{err}");
    assert!(!detail.is_empty(), "git's own message is kept: {err}");
    assert_eq!(
        f.git.rev_parse(&f.repo, target).unwrap().as_deref(),
        Some(base.as_str())
    );

    // No such ref, and the caller expects it absent (all zeros): an error.
    let err = f
        .git
        .update_ref_cas(&f.repo, ghost, &missing, &zero)
        .unwrap_err()
        .to_string();
    assert!(
        err.starts_with("git update-ref refs/heads/ghost failed ("),
        "{err}"
    );
    assert_eq!(f.git.rev_parse(&f.repo, ghost).unwrap(), None);

    // No such ref, but the caller expected `base`: a lost race, no error.
    assert!(!f.git.update_ref_cas(&f.repo, ghost, &next, &base).unwrap());
    assert_eq!(f.git.rev_parse(&f.repo, ghost).unwrap(), None);

    // All zeros mean "absent": create the ref, then delete it.
    assert!(f.git.update_ref_cas(&f.repo, ghost, &next, &zero).unwrap());
    assert_eq!(
        f.git.rev_parse(&f.repo, ghost).unwrap().as_deref(),
        Some(next.as_str())
    );
    assert!(f.git.update_ref_cas(&f.repo, ghost, &zero, &next).unwrap());
    assert_eq!(f.git.rev_parse(&f.repo, ghost).unwrap(), None);
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
