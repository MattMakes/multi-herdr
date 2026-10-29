//! Fetch a resolved source into a staging directory. Git runs with hooks
//! off: a bare repository receives the pinned commit, and its tree is
//! checked out with `read-tree` and `checkout-index`, so no worktree, no
//! hook and no remote config is involved. The git listing is checked
//! before the checkout, so a rejected tree is never written.

use std::fs;
use std::path::{Path, PathBuf};

use crate::catalog::{BundledFile, BundledSkill};
use crate::error::{MarketplaceError, Result};
use crate::fsx;
use crate::git::GitRunner;
use crate::integrity::{check_limits, check_relative_path};
use crate::resolver::{ResolvedOrigin, ResolvedSource};

/// Write the skill tree under `staging` and return the staged skill
/// directory.
pub fn fetch(git: &GitRunner, resolved: &ResolvedSource, staging: &Path) -> Result<PathBuf> {
    let tree = staging.join("tree");
    match &resolved.origin {
        ResolvedOrigin::Bundled(skill) => stage_bundled(skill, &tree)?,
        ResolvedOrigin::Local(path) => fsx::copy_tree(path, &tree)?,
        ResolvedOrigin::Git {
            url,
            commit,
            subdir,
        } => fetch_git(git, url, commit, subdir.as_deref(), staging, &tree)?,
    }
    Ok(tree)
}

fn stage_bundled(skill: &BundledSkill, tree: &Path) -> Result<()> {
    skill
        .files
        .iter()
        .try_for_each(|file| stage_file(tree, file))
}

fn stage_file(tree: &Path, file: &BundledFile) -> Result<()> {
    check_relative_path(&file.path)?;
    let path = tree.join(&file.path);
    let parent = path.parent().expect("joined path has a parent");
    fs::create_dir_all(parent)
        .map_err(|e| MarketplaceError::io(parent.display().to_string(), e))?;
    fs::write(&path, &file.bytes)
        .map_err(|e| MarketplaceError::io(format!("stage {}", file.path), e))?;
    set_executable(&path, file)
}

/// The store renames the staged tree into place, so this mode is the store
/// mode.
#[cfg(unix)]
fn set_executable(path: &Path, file: &BundledFile) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if !file.executable {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|e| MarketplaceError::io(format!("chmod {}", file.path), e))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _file: &BundledFile) -> Result<()> {
    Ok(())
}

fn fetch_git(
    git: &GitRunner,
    url: &str,
    commit: &str,
    subdir: Option<&str>,
    staging: &Path,
    tree: &Path,
) -> Result<()> {
    let repo = fetch_commit(git, staging, url, commit)?;
    let treeish = subdir.map_or_else(|| commit.to_owned(), |dir| format!("{commit}:{dir}"));
    check_listing(git, &repo, &treeish)?;
    checkout(git, &repo, &treeish, tree)
}

/// Fetch `commit` into a bare repository under `staging` and return it.
fn fetch_commit(git: &GitRunner, staging: &Path, url: &str, commit: &str) -> Result<PathBuf> {
    let repo = staging.join("repo.git");
    git.run(staging, &["init", "--quiet", "--bare", "repo.git"])?;
    // Fetching by commit is the existence check: a missing commit fails
    // here or in the `rev-parse` below.
    git.run(
        &repo,
        &["fetch", "--quiet", "--no-tags", "--depth=1", url, commit],
    )?;
    let verify = format!("{commit}^{{commit}}");
    git.run(&repo, &["rev-parse", "--verify", "--quiet", &verify])
        .is_ok()
        .then_some(repo)
        .ok_or_else(|| MarketplaceError::RevisionNotFound {
            url: url.to_owned(),
            revision: commit.to_owned(),
        })
}

fn checkout(git: &GitRunner, repo: &Path, treeish: &str, tree: &Path) -> Result<()> {
    fs::create_dir_all(tree).map_err(|e| MarketplaceError::io(tree.display().to_string(), e))?;
    let work_tree = format!("--work-tree={}", tree.display());
    git.run(repo, &["read-tree", treeish])?;
    git.run(repo, &[&work_tree, "checkout-index", "--all", "--force"])?;
    Ok(())
}

/// Apply the tree rules to the git listing before anything is checked out,
/// so a symlink, a submodule or an oversize tree is never written to disk.
/// `integrity::tree_digest` checks the written tree again.
fn check_listing(git: &GitRunner, repo: &Path, treeish: &str) -> Result<()> {
    let out = git.run(repo, &["ls-tree", "-r", "-l", "-z", "--full-tree", treeish])?;
    let mut total = 0u64;
    let records = out.stdout.split(|b| *b == 0).filter(|r| !r.is_empty());
    for (index, record) in records.enumerate() {
        let record = String::from_utf8_lossy(record);
        let (path, bytes) = listing_entry(&record)?;
        check_limits(path, bytes, index + 1, &mut total)?;
    }
    Ok(())
}

/// Parse one `ls-tree -l` record, `<mode> <type> <sha> <size>\t<path>`, and
/// return the path and size of a regular file.
fn listing_entry(record: &str) -> Result<(&str, u64)> {
    let (meta, path) = record
        .split_once('\t')
        .ok_or_else(|| MarketplaceError::Traversal(PathBuf::from(record)))?;
    check_relative_path(path)?;
    let mut fields = meta.split_whitespace();
    check_mode(fields.next().unwrap_or_default(), path)?;
    let bytes = fields
        .nth(2)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| MarketplaceError::NotRegularFile(PathBuf::from(path)))?;
    Ok((path, bytes))
}

fn check_mode(mode: &str, path: &str) -> Result<()> {
    match mode {
        "120000" => Err(MarketplaceError::Symlink(PathBuf::from(path))),
        "100644" | "100755" => Ok(()),
        _ => Err(MarketplaceError::NotRegularFile(PathBuf::from(path))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{BundledFile, BundledSkill};
    use crate::model::{SkillId, SkillSource};

    fn resolved(origin: ResolvedOrigin) -> ResolvedSource {
        ResolvedSource {
            source: SkillSource::Bundled {
                name: "demo".into(),
            },
            origin,
            expected_name: None,
            requested_revision: None,
            resolved_commit: None,
        }
    }

    fn bundled(files: Vec<BundledFile>) -> ResolvedOrigin {
        ResolvedOrigin::Bundled(BundledSkill {
            id: SkillId::parse("demo").unwrap(),
            files,
        })
    }

    fn file(path: &str, bytes: &'static str, executable: bool) -> BundledFile {
        BundledFile {
            path: path.into(),
            bytes: bytes.as_bytes().into(),
            executable,
        }
    }

    #[test]
    fn fetch_bundled_writes_files_and_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let origin = bundled(vec![
            file("SKILL.md", "doc", false),
            file("bin/run.sh", "#!/bin/sh\n", true),
        ]);
        let tree = fetch(&GitRunner::new("git"), &resolved(origin), tmp.path()).unwrap();
        assert_eq!(tree, tmp.path().join("tree"));
        assert_eq!(fs::read_to_string(tree.join("SKILL.md")).unwrap(), "doc");
        assert_eq!(
            fs::read_to_string(tree.join("bin/run.sh")).unwrap(),
            "#!/bin/sh\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &str| fs::metadata(tree.join(p)).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode("bin/run.sh"), 0o755);
            assert_ne!(mode("SKILL.md") & 0o111, 0o111);
        }
    }

    #[test]
    fn fetch_bundled_refuses_an_unclean_path() {
        let tmp = tempfile::tempdir().unwrap();
        let origin = bundled(vec![file("../escape", "x", false)]);
        let err = fetch(&GitRunner::new("git"), &resolved(origin), tmp.path()).unwrap_err();
        assert!(matches!(err, MarketplaceError::Traversal(_)), "{err:?}");
        assert!(!tmp.path().join("escape").exists());
    }

    #[test]
    fn fetch_local_copies_the_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let from = tmp.path().join("skill");
        fs::create_dir_all(from.join("refs")).unwrap();
        fs::write(from.join("SKILL.md"), "doc").unwrap();
        fs::write(from.join("refs/a.md"), "a").unwrap();
        let staging = tmp.path().join("staging");
        fs::create_dir(&staging).unwrap();
        let tree = fetch(
            &GitRunner::new("git"),
            &resolved(ResolvedOrigin::Local(from)),
            &staging,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(tree.join("SKILL.md")).unwrap(), "doc");
        assert_eq!(fs::read_to_string(tree.join("refs/a.md")).unwrap(), "a");
    }

    /// A repo whose index holds `entries` as `(mode, blob text, path)`, and
    /// the tree they make. A mode of `160000` is a submodule: no blob.
    fn tree_of(entries: &[(&str, &str, &str)]) -> (tempfile::TempDir, GitRunner, String) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("gitconfig");
        fs::write(&config, "").unwrap();
        let git = GitRunner::new("git").with_env("GIT_CONFIG_GLOBAL", &config);
        let repo = tmp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git.run(&repo, &["init", "--quiet"]).unwrap();
        for (mode, text, path) in entries {
            let sha = if *mode == "160000" {
                "1".repeat(40)
            } else {
                fs::write(repo.join("blob"), text).unwrap();
                git.run(&repo, &["hash-object", "-w", "blob"])
                    .unwrap()
                    .stdout_text()
            };
            let info = format!("{mode},{sha},{path}");
            git.run(&repo, &["update-index", "--add", "--cacheinfo", &info])
                .unwrap();
        }
        let tree = git.run(&repo, &["write-tree"]).unwrap().stdout_text();
        (tmp, git, tree)
    }

    fn listing(entries: &[(&str, &str, &str)]) -> Result<()> {
        let (tmp, git, tree) = tree_of(entries);
        check_listing(&git, &tmp.path().join("repo"), &tree)
    }

    #[test]
    fn listing_accepts_regular_files() {
        listing(&[("100644", "a", "SKILL.md"), ("100755", "b", "bin/run.sh")]).unwrap();
    }

    #[test]
    fn listing_refuses_a_symlink() {
        let err = listing(&[("120000", "target", "link")]).unwrap_err();
        assert!(matches!(err, MarketplaceError::Symlink(p) if p == Path::new("link")));
    }

    #[test]
    fn listing_refuses_a_submodule() {
        let err = listing(&[("160000", "", "sub")]).unwrap_err();
        assert!(matches!(err, MarketplaceError::NotRegularFile(p) if p == Path::new("sub")));
    }

    #[test]
    fn listing_refuses_an_oversize_file() {
        let big = "x".repeat(crate::integrity::MAX_FILE_BYTES as usize + 1);
        let (tmp, git, tree) = tree_of(&[("100644", &big, "big.bin")]);
        let err = check_listing(&git, &tmp.path().join("repo"), &tree).unwrap_err();
        assert!(
            matches!(err, MarketplaceError::FileTooLarge { ref path, .. } if path == Path::new("big.bin")),
            "{err:?}"
        );
    }

    #[test]
    fn listing_entry_reads_path_and_size() {
        let record = format!("100644 blob {} 42\tdir/a.md", "a".repeat(40));
        assert_eq!(listing_entry(&record).unwrap(), ("dir/a.md", 42));
    }

    #[test]
    fn listing_entry_refuses_a_record_with_no_tab() {
        let err = listing_entry("100644 blob abc 3 a.md").unwrap_err();
        assert!(matches!(err, MarketplaceError::Traversal(_)), "{err:?}");
    }

    #[test]
    fn listing_entry_refuses_a_record_with_no_size() {
        let err = listing_entry("100644 blob\ta.md").unwrap_err();
        assert!(
            matches!(err, MarketplaceError::NotRegularFile(_)),
            "{err:?}"
        );
    }
}
