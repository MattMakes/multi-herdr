use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use horch_platform::fs::remove_dir_below;
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::application::jobs::Ran;
use crate::fleetd::fleet_runtime::domain::jobs::Outcome;
use crate::fleetd::worktree_custody::adapters::git::{
    failed, private_folder, repo_git_dir, CliWorktreeGit,
};
use crate::fleetd::worktree_custody::adapters::sqlite::WorktreeRow;
use crate::fleetd::worktree_custody::domain::rescue::{self, Facts};
use crate::runtime::process::which;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rescue {
    pub rescue_ref: String,
    pub commit: String,
    pub old_head: String,
    pub files: u64,
    pub bytes: u64,
    pub attempt: Option<String>,
}

pub fn rescue_key(path: &str, head: &str) -> String {
    serde_json::json!([path, head]).to_string()
}

const IDENTITY: [&str; 4] = ["-c", "user.name=horch", "-c", "user.email=horch@localhost"];

const REF_TRIES: usize = 16;

pub fn rescue(git: &CliWorktreeGit, fleet_root: &Path, row: &WorktreeRow) -> Ran {
    match save(git, fleet_root, row) {
        Ok(r) => Ran {
            outcome: Outcome {
                passed: true,
                verdict: true,
                text: format!(
                    "saved {} on {} ({} files, {} bytes)",
                    row.path, r.rescue_ref, r.files, r.bytes
                ),
            },
            data: Some(serde_json::to_value(&r).expect("a rescue serializes")),
        },
        Err(e) => failed(format!("the rescue of {}: {e:#}", row.worktree_id)),
    }
}

fn save(git: &CliWorktreeGit, fleet_root: &Path, row: &WorktreeRow) -> Result<Rescue> {
    let id = row.worktree_id.as_str();
    if !rescue::valid_id(id) {
        bail!("the worktree id {id:?} cannot name a rescue ref");
    }
    let tree = Path::new(&row.path);
    if !tree.is_dir() {
        bail!("no folder at {}", row.path);
    }
    let git = &git
        .clone()
        .writing(&[repo_git_dir(Path::new(&row.repo_root))?]);
    let run = run_folder(fleet_root, row)?;
    let snapshot_index = PrivateIndex::new(run.join(format!("rescue-{id}.index")))?;
    let verification_index = PrivateIndex::new(run.join(format!("rescue-{id}.verify.index")))?;

    let head = rev(git, tree, "HEAD")?;
    let saved_tree = snapshot(&git.with_index(&snapshot_index.path)?, tree, &head)?;
    let message = rescue::commit_message(id, row.owner_attempt.as_deref());
    let mut args = IDENTITY.to_vec();
    args.extend(["commit-tree", &saved_tree, "-p", &head, "-m", &message]);
    let commit = git.git(tree, &args)?.trim().to_string();
    let (n, rescue_ref) = create_ref(git, tree, id, &commit)?;
    let facts = Facts {
        read_back: rev(git, tree, &rescue_ref).ok(),
        commit_tree: rev(git, tree, &format!("{commit}^{{tree}}")).ok(),
        commit_parent: rev(git, tree, &format!("{commit}^")).ok(),
        disk_tree: git
            .with_index(&verification_index.path)
            .and_then(|g| snapshot(&g, tree, &head))
            .ok(),
        head_after: rev(git, tree, "HEAD").ok(),
        commit,
        saved_tree,
        head_before: head,
    };
    rescue::verify(&facts)
        .map_err(|e| anyhow!("the rescue ref {rescue_ref} does not verify: {e}"))?;
    let (files, bytes) = count(git, tree, &facts.commit)?;
    let r = Rescue {
        rescue_ref,
        commit: facts.commit,
        old_head: facts.head_before,
        files,
        bytes,
        attempt: row.owner_attempt.clone(),
    };
    write_manifest(&fleet_root.join("rescue"), &format!("{id}-{n}.json"), &r)?;
    Ok(r)
}

pub(crate) fn run_folder(fleet_root: &Path, row: &WorktreeRow) -> Result<PathBuf> {
    if fleet_root.components().any(|c| c == Component::ParentDir) {
        bail!("the fleet root {} has a `..`", fleet_root.display());
    }
    let id = row.worktree_id.as_str();
    if !rescue::valid_id(id) {
        bail!("the worktree id {id:?} cannot name a folder");
    }
    let mut projects = Vec::new();
    for project in [&row.path, &row.repo_root] {
        let canonical = Path::new(project)
            .canonicalize()
            .with_context(|| format!("reading {project}"))?;
        projects.push(canonical);
    }
    let folder = resolved(&fleet_root.join(TMP).join(id))?;
    outside(&folder, &projects)?;
    let made = !folder.exists();
    private_folder(&folder)?;
    let checked = folder
        .canonicalize()
        .context("reading the made folder")
        .and_then(|real| {
            let root = fleet_root
                .canonicalize()
                .context("reading the fleet root")?;
            if real != root.join(TMP).join(id) {
                bail!(
                    "the made folder {} is not in the fleet root {}",
                    real.display(),
                    root.display()
                );
            }
            outside(&real, &projects)?;
            Ok(real)
        });
    if checked.is_err() && made {
        let _ = std::fs::remove_dir(&folder);
    }
    checked
}

const TMP: &str = "tmp";

pub fn drop_index_folder(fleet_root: &Path, row: &WorktreeRow) {
    let id = row.worktree_id.as_str();
    if !rescue::valid_id(id) {
        return;
    }
    if let Ok(root) = fleet_root.canonicalize() {
        let _ = remove_dir_below(&root, &[TMP, id]);
    }
}

fn outside(folder: &Path, projects: &[PathBuf]) -> Result<()> {
    match projects.iter().find(|p| folder.starts_with(p)) {
        Some(project) => bail!(
            "the fleet folder {} is in the project {}",
            folder.display(),
            project.display()
        ),
        None => Ok(()),
    }
}

fn resolved(path: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(path).with_context(|| format!("reading {}", path.display()))?;
    let mut below = Vec::new();
    let mut base = path.as_path();
    loop {
        match base.canonicalize() {
            Ok(canonical) => {
                return Ok(below.iter().rev().fold(canonical, |p, part| p.join(part)));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("reading {}", base.display())),
        }
        let (Some(parent), Some(name)) = (base.parent(), base.file_name()) else {
            bail!("{} has no folder that exists", path.display());
        };
        below.push(name.to_os_string());
        base = parent;
    }
}

pub(crate) struct PrivateIndex {
    pub(crate) path: PathBuf,
}

impl PrivateIndex {
    pub(crate) fn new(path: PathBuf) -> Result<PrivateIndex> {
        let index = PrivateIndex { path };
        for file in index.files() {
            match std::fs::remove_file(&file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("deleting {}", file.display())),
            }
        }
        Ok(index)
    }

    fn files(&self) -> [PathBuf; 2] {
        let mut lock = self.path.clone().into_os_string();
        lock.push(".lock");
        [self.path.clone(), lock.into()]
    }
}

impl Drop for PrivateIndex {
    fn drop(&mut self) {
        for file in self.files() {
            let _ = std::fs::remove_file(file);
        }
    }
}

pub(crate) fn rev(git: &CliWorktreeGit, dir: &Path, rev: &str) -> Result<String> {
    Ok(git
        .git(dir, &["rev-parse", "--verify", "--end-of-options", rev])?
        .trim()
        .to_string())
}

pub(crate) fn snapshot(git: &CliWorktreeGit, dir: &Path, head: &str) -> Result<String> {
    let version = git.git(dir, &["version"])?;
    if !has_attr_source(&version) {
        bail!(
            "{}: a snapshot with the filters off needs git 2.41 or newer \
             (`--attr-source`), so the job saved no file; install a newer git",
            version.trim()
        );
    }
    let empty = git.git(dir, &["hash-object", "-t", "tree", "/dev/null"])?;
    let source = format!("--attr-source={}", empty.trim());
    let raw = |args: &[&str]| {
        let mut full = vec![source.as_str(), "-c", "core.autocrlf=false"];
        full.extend_from_slice(args);
        git.git(dir, &full)
    };
    raw(&["read-tree", head])?;
    raw(&["add", "-A"])?;
    Ok(raw(&["write-tree"])?.trim().to_string())
}

pub fn rescue_git_version(
    git: &Path,
    path: Option<&OsStr>,
    pathext: Option<&str>,
) -> std::result::Result<String, String> {
    let bin = if git.components().count() > 1 {
        git.to_path_buf()
    } else {
        which(path, pathext, &git.to_string_lossy())
            .ok_or_else(|| format!("{} not found on PATH. {RESCUE_GIT_FIX}", git.display()))?
    };
    let out = horch_platform::start::command(&bin)
        .arg("version")
        .output()
        .map_err(|e| {
            format!(
                "could not run `{} version`: {e}. {RESCUE_GIT_FIX}",
                bin.display()
            )
        })?;
    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() {
        return Err(format!(
            "`{} version` failed ({}). {RESCUE_GIT_FIX}",
            bin.display(),
            out.status
        ));
    }
    if !has_attr_source(&version) {
        return Err(format!(
            "{} is {version:?}, older than 2.41. {RESCUE_GIT_FIX}",
            bin.display()
        ));
    }
    Ok(format!("{}: {version}", bin.display()))
}

const RESCUE_GIT_FIX: &str = "The rescue of a worktree saves its files with the \
     filters off, and that needs git 2.41 or newer (`--attr-source`). Install git \
     2.41 or newer, or set HORCH_GIT_BIN to one.";

fn has_attr_source(version: &str) -> bool {
    let number = version.trim().strip_prefix("git version ").unwrap_or("");
    let mut parts = number.split('.').map(|p| p.parse::<u32>().ok());
    match (parts.next().flatten(), parts.next().flatten()) {
        (Some(major), Some(minor)) => (major, minor) >= (2, 41),
        _ => false,
    }
}

fn create_ref(git: &CliWorktreeGit, dir: &Path, id: &str, commit: &str) -> Result<(u64, String)> {
    let folder = rescue::ref_folder(id);
    for _ in 0..REF_TRIES {
        let out = git.git(dir, &["for-each-ref", "--format=%(refname)", &folder])?;
        let refs: Vec<String> = out.lines().map(str::to_string).collect();
        let n = rescue::next_n(id, &refs);
        let name = rescue::ref_name(id, n);
        let message = format!("horch rescue {id}");
        let out = git.checked(dir, &["update-ref", "-m", &message, &name, commit, ""])?;
        if out.status == 0 {
            return Ok((n, name));
        }
        if !ref_exists(git, dir, &name)? {
            bail!(
                "git update-ref {name} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
    }
    bail!("each of {REF_TRIES} new rescue refs of {id} existed before its write")
}

fn ref_exists(git: &CliWorktreeGit, dir: &Path, name: &str) -> Result<bool> {
    let out = git.checked(dir, &["show-ref", "--verify", "--quiet", name])?;
    match out.status {
        0 => Ok(true),
        1 => Ok(false),
        code => bail!(
            "git show-ref of {name} failed ({code}): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
    }
}

fn count(git: &CliWorktreeGit, dir: &Path, commit: &str) -> Result<(u64, u64)> {
    let out = git.git(dir, &["ls-tree", "-r", "-l", "-z", commit])?;
    let (mut files, mut bytes) = (0u64, 0u64);
    for entry in out.split('\0').filter(|e| !e.is_empty()) {
        let head = entry.split('\t').next().unwrap_or("");
        let fields: Vec<&str> = head.split_whitespace().collect();
        if fields.get(1) != Some(&"blob") {
            continue;
        }
        let size = fields
            .get(3)
            .and_then(|s| s.parse::<u64>().ok())
            .with_context(|| format!("git ls-tree gave no size in {entry:?}"))?;
        files += 1;
        bytes = bytes.saturating_add(size);
    }
    Ok((files, bytes))
}

fn write_manifest(folder: &Path, name: &str, r: &Rescue) -> Result<()> {
    std::fs::create_dir_all(folder).with_context(|| format!("making {}", folder.display()))?;
    let path = folder.join(name);
    let tmp = folder.join(format!(".{name}.tmp"));
    let text = serde_json::to_string_pretty(r).expect("a rescue serializes");
    std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::has_attr_source;

    #[test]
    fn attr_source_is_in_git_2_41_and_newer() {
        assert!(has_attr_source("git version 2.41.0\n"));
        assert!(has_attr_source("git version 2.50.1 (Apple Git-155)"));
        assert!(has_attr_source("git version 3.0.0"));
        assert!(!has_attr_source("git version 2.39.5"));
        assert!(!has_attr_source("git version 2.40.1.windows.1"));
        assert!(has_attr_source("git version 2.51.2.windows.1"));
        assert!(!has_attr_source("git version 2.39.5 (Apple Git-154)"));
        assert!(!has_attr_source("not git"));
    }
}
