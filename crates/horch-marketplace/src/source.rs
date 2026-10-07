//! Fetch a resolved source into a staging directory. Git runs with hooks
//! off: a bare repository receives the pinned commit, and its tree is
//! checked out with `read-tree` and `checkout-index`, so no worktree, no
//! hook and no remote config is involved. The git listing is checked
//! before the checkout, so a rejected tree is never written.

use std::fs;
use std::path::{Path, PathBuf};

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
        ResolvedOrigin::Bundled(skill) => {
            for file in &skill.files {
                check_relative_path(&file.path)?;
                let path = tree.join(&file.path);
                let parent = path.parent().expect("joined path has a parent");
                fs::create_dir_all(parent)
                    .map_err(|e| MarketplaceError::io(parent.display().to_string(), e))?;
                fs::write(&path, &file.bytes)
                    .map_err(|e| MarketplaceError::io(format!("stage {}", file.path), e))?;
                // The store renames the staged tree into place, so this
                // mode is the store mode.
                #[cfg(unix)]
                if file.executable {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                        .map_err(|e| MarketplaceError::io(format!("chmod {}", file.path), e))?;
                }
            }
            Ok(tree)
        }
        ResolvedOrigin::Local(path) => {
            fsx::copy_tree(path, &tree)?;
            Ok(tree)
        }
        ResolvedOrigin::Git {
            url,
            commit,
            subdir,
        } => {
            fetch_git(git, url, commit, subdir.as_deref(), staging, &tree)?;
            Ok(tree)
        }
    }
}

fn fetch_git(
    git: &GitRunner,
    url: &str,
    commit: &str,
    subdir: Option<&str>,
    staging: &Path,
    tree: &Path,
) -> Result<()> {
    let repo = staging.join("repo.git");
    git.run(staging, &["init", "--quiet", "--bare", "repo.git"])?;
    // Fetching by commit is the existence check: a missing commit fails
    // here or in the `rev-parse` below.
    git.run(
        &repo,
        &["fetch", "--quiet", "--no-tags", "--depth=1", url, commit],
    )?;
    let verified = git
        .run(
            &repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{commit}^{{commit}}"),
            ],
        )
        .is_ok();
    if !verified {
        return Err(MarketplaceError::RevisionNotFound {
            url: url.to_owned(),
            revision: commit.to_owned(),
        });
    }
    let treeish = match subdir {
        Some(dir) => format!("{commit}:{dir}"),
        None => commit.to_owned(),
    };
    check_listing(git, &repo, &treeish)?;
    fs::create_dir_all(tree).map_err(|e| MarketplaceError::io(tree.display().to_string(), e))?;
    let work_tree = format!("--work-tree={}", tree.display());
    git.run(&repo, &["read-tree", &treeish])?;
    git.run(&repo, &[&work_tree, "checkout-index", "--all", "--force"])?;
    Ok(())
}

/// Apply the tree rules to the git listing before anything is checked out,
/// so a symlink, a submodule or an oversize tree is never written to disk.
/// `integrity::tree_digest` checks the written tree again.
fn check_listing(git: &GitRunner, repo: &Path, treeish: &str) -> Result<()> {
    let out = git.run(repo, &["ls-tree", "-r", "-l", "-z", "--full-tree", treeish])?;
    let mut count = 0usize;
    let mut total = 0u64;
    for record in out.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let record = String::from_utf8_lossy(record);
        let Some((meta, path)) = record.split_once('\t') else {
            return Err(MarketplaceError::Traversal(PathBuf::from(&*record)));
        };
        check_relative_path(path)?;
        let mut fields = meta.split_whitespace();
        let mode = fields.next().unwrap_or_default();
        match mode {
            "120000" => return Err(MarketplaceError::Symlink(PathBuf::from(path))),
            "100644" | "100755" => {}
            _ => return Err(MarketplaceError::NotRegularFile(PathBuf::from(path))),
        }
        let bytes = fields
            .nth(2)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| MarketplaceError::NotRegularFile(PathBuf::from(path)))?;
        count += 1;
        check_limits(path, bytes, count, &mut total)?;
    }
    Ok(())
}
