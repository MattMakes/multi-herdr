//! `WorktreeManager` refusals that the main vcs tests do not reach: a reused
//! worktree or kept branch that no longer contains the base, and a freeze on
//! the wrong branch or below the base. Real git, temp dirs only.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{DateTime, Utc};
use horch_core::ids::ExecutionId;
use horch_core::vcs::git::{GitCli, GitClient};
use horch_core::vcs::worktree::{WorktreeManager, WorktreeSpec};

fn run(dir: &Path, cfg: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", cfg)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@t.invalid")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@t.invalid")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

struct Repo {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    cfg: PathBuf,
    git: GitCli,
    base1: String,
    base2: String,
}

impl Repo {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let cfg = root.join("gitconfig");
        std::fs::write(&cfg, "").unwrap();
        let repo = root.join("repo");
        std::fs::create_dir(&repo).unwrap();
        run(&repo, &cfg, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        run(&repo, &cfg, &["add", "-A"]);
        run(&repo, &cfg, &["commit", "-q", "-m", "one"]);
        let base1 = run(&repo, &cfg, &["rev-parse", "HEAD"]);
        std::fs::write(repo.join("a.txt"), "two\n").unwrap();
        run(&repo, &cfg, &["commit", "-q", "-am", "two"]);
        let base2 = run(&repo, &cfg, &["rev-parse", "HEAD"]);
        let git = GitCli::new("git".into())
            .with_env("GIT_CONFIG_GLOBAL", cfg.to_str().unwrap())
            .with_env("GIT_CONFIG_NOSYSTEM", "1")
            .with_env("GIT_AUTHOR_NAME", "T")
            .with_env("GIT_AUTHOR_EMAIL", "t@t.invalid")
            .with_env("GIT_COMMITTER_NAME", "T")
            .with_env("GIT_COMMITTER_EMAIL", "t@t.invalid");
        Repo {
            _tmp: tmp,
            root,
            repo,
            cfg,
            git,
            base1,
            base2,
        }
    }

    fn spec(&self, base: &str) -> WorktreeSpec {
        WorktreeSpec {
            repo: self.repo.clone(),
            root: self.root.join("wt"),
            exp8: "0a1b2c3d".into(),
            round_index: 1,
            label: "c1".into(),
            base_sha: base.into(),
        }
    }

    fn mgr(&self) -> WorktreeManager<'_, GitCli> {
        WorktreeManager { git: &self.git }
    }
}

fn at() -> DateTime<Utc> {
    "2026-10-02T10:00:00Z".parse().unwrap()
}

#[test]
fn create_refuses_a_reused_worktree_below_the_base() {
    let r = Repo::new();
    r.mgr().create(&r.spec(&r.base1)).unwrap();
    let err = r.mgr().create(&r.spec(&r.base2)).unwrap_err().to_string();
    assert!(
        err.contains(&format!(
            "is at {}, which does not contain base {}",
            r.base1, r.base2
        )),
        "{err}"
    );
}

#[test]
fn create_refuses_a_kept_branch_below_the_base() {
    let r = Repo::new();
    let first = r.spec(&r.base1);
    r.mgr().create(&first).unwrap();
    r.mgr().remove(&first).unwrap();
    let err = r.mgr().create(&r.spec(&r.base2)).unwrap_err().to_string();
    assert!(
        err.contains(&format!(
            "branch {} is at {}, which does not contain base {}",
            first.branch(),
            r.base1,
            r.base2
        )),
        "{err}"
    );
}

#[test]
fn freeze_refuses_a_detached_head() {
    let r = Repo::new();
    let spec = r.spec(&r.base1);
    let path = r.mgr().create(&spec).unwrap();
    run(&path, &r.cfg, &["checkout", "-q", "--detach"]);
    let err = r
        .mgr()
        .freeze(&spec, &ExecutionId::mint(at()), at())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(&format!("is on a detached HEAD, not on {}", spec.branch())),
        "{err}"
    );
}

#[test]
fn freeze_refuses_a_head_below_the_base() {
    let r = Repo::new();
    let path = r.mgr().create(&r.spec(&r.base1)).unwrap();
    assert_eq!(r.git.head(&path).unwrap(), r.base1);
    let err = r
        .mgr()
        .freeze(&r.spec(&r.base2), &ExecutionId::mint(at()), at())
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(&format!(
            "head {} does not contain base {}",
            r.base1, r.base2
        )),
        "{err}"
    );
}
