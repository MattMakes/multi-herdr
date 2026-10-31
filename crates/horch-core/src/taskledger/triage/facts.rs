//! The git and disk facts of 1 triage row. Every call reads; none writes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;

use super::{Measure, Row};
use crate::taskledger::git::Git;

/// The ages of a row's work.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Age {
    pub created_at: Option<String>,
    pub last_commit_at: Option<String>,
    /// The newest file that git does not ignore.
    pub newest_mtime: Option<String>,
}

/// The disk a row's folder uses.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Disk {
    pub bytes: u64,
    /// The share that git ignores (build output).
    pub ignored_bytes: u64,
}

/// What the branch changes against its base.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Diff {
    pub files: usize,
    pub insertions: usize,
    pub deletions: usize,
    /// The 3 paths with the most changed lines.
    pub top_paths: Vec<String>,
}

/// The work in the folder that no commit holds.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Uncommitted {
    pub modified: usize,
    pub staged: usize,
    pub untracked: usize,
    /// The 3 largest untracked files: what `ARCHIVE` or `DELETE` commits.
    pub largest_untracked: Vec<String>,
}

fn stamp(t: SystemTime) -> String {
    DateTime::<Utc>::from(t).to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// When the folder was made, from its metadata.
pub(super) fn created_at(dir: &Path) -> Option<String> {
    let meta = std::fs::metadata(dir).ok()?;
    meta.created().or_else(|_| meta.modified()).ok().map(stamp)
}

/// The tip of the row's work: the folder's HEAD, else its branch.
fn tip(git: &Git, root: &Path, row: &Row) -> Option<String> {
    if row.on_disk {
        if let Ok(head) = git.rev(Path::new(&row.path), "HEAD") {
            return Some(head);
        }
    }
    row.branch.as_deref().and_then(|b| git.rev(root, b).ok())
}

/// Fill in `row`'s tip and, for [`Measure::Full`], every fact.
pub(super) fn measure(git: &Git, root: &Path, row: &mut Row, measure: Measure) {
    row.tip = tip(git, root, row);
    if measure == Measure::Ids {
        return;
    }
    if let Some(tip) = row.tip.clone() {
        history(git, root, row, &tip);
    }
    if row.on_disk {
        let dir = PathBuf::from(&row.path);
        row.uncommitted = uncommitted(git, &dir);
        let (disk, newest) = disk(git, &dir);
        row.disk = Some(disk);
        row.age.newest_mtime = newest;
    }
}

/// The facts that compare the tip with the base.
fn history(git: &Git, root: &Path, row: &mut Row, tip: &str) {
    let base = row.base.clone();
    let run = |args: &[&str]| {
        git.try_run(root, args)
            .ok()
            .filter(|o| o.ok)
            .map(|o| o.stdout)
    };
    if let Some(counts) = run(&[
        "rev-list",
        "--left-right",
        "--count",
        &format!("{base}...{tip}"),
    ]) {
        let mut it = counts.split_whitespace().map(|n| n.parse().unwrap_or(0));
        row.behind = it.next().unwrap_or(0);
        row.ahead = it.next().unwrap_or(0);
    }
    if let Some(numstat) = run(&["diff", "--numstat", &format!("{base}...{tip}")]) {
        row.diff = diff(&numstat);
    }
    if let Some(cherry) = run(&["cherry", &base, tip]) {
        row.superseded = row.ahead > 0 && !cherry.lines().any(|l| l.starts_with('+'));
    }
    row.conflicts = conflicts(git, root, &base, tip);
    row.age.last_commit_at = run(&["log", "-1", "--format=%cI", tip]).filter(|s| !s.is_empty());
}

/// The diff of `git diff --numstat` output.
fn diff(numstat: &str) -> Diff {
    let mut paths: Vec<(usize, String)> = Vec::new();
    let mut d = Diff::default();
    for line in numstat.lines() {
        let mut cols = line.splitn(3, '\t');
        let (Some(add), Some(del), Some(path)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let (add, del) = (add.parse().unwrap_or(0), del.parse().unwrap_or(0));
        d.files += 1;
        d.insertions += add;
        d.deletions += del;
        paths.push((add + del, path.to_string()));
    }
    paths.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    d.top_paths = paths.into_iter().take(3).map(|p| p.1).collect();
    d
}

/// The conflicting files of a merge of `tip` into `base`, from
/// `git merge-tree --write-tree` (it touches no ref, index or folder).
fn conflicts(git: &Git, root: &Path, base: &str, tip: &str) -> Option<usize> {
    let out = git
        .try_run(
            root,
            &[
                "merge-tree",
                "--write-tree",
                "--name-only",
                "--no-messages",
                base,
                tip,
            ],
        )
        .ok()?;
    if out.ok {
        return Some(0);
    }
    // Exit 1 is a conflict: the tree, then 1 conflicting path per line.
    let names: HashSet<&str> = out
        .stdout
        .lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .collect();
    (!names.is_empty()).then_some(names.len())
}

/// The modified, staged and untracked counts of `dir`.
fn uncommitted(git: &Git, dir: &Path) -> Uncommitted {
    let mut u = Uncommitted::default();
    let mut untracked: Vec<(u64, String)> = Vec::new();
    for line in git.dirty(dir).unwrap_or_default() {
        let (x, y) = (line.chars().next(), line.chars().nth(1));
        let path = line.get(3..).unwrap_or("").to_string();
        if x == Some('?') {
            u.untracked += 1;
            let size = std::fs::symlink_metadata(dir.join(&path))
                .map(|m| m.len())
                .unwrap_or(0);
            untracked.push((size, path));
            continue;
        }
        u.staged += usize::from(x.is_some_and(|c| c != ' '));
        u.modified += usize::from(y.is_some_and(|c| c != ' '));
    }
    untracked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    u.largest_untracked = untracked.into_iter().take(3).map(|p| p.1).collect();
    u
}

/// The paths git ignores in `dir` (a folder counts as 1 path).
fn ignored(git: &Git, dir: &Path) -> HashSet<PathBuf> {
    let out = git
        .try_run(
            dir,
            &[
                "status",
                "--porcelain",
                "--ignored=traditional",
                "--untracked-files=normal",
            ],
        )
        .ok()
        .filter(|o| o.ok)
        .map(|o| o.stdout)
        .unwrap_or_default();
    out.lines()
        .filter_map(|l| l.strip_prefix("!! "))
        .map(|p| dir.join(p.trim_end_matches('/')))
        .collect()
}

/// The folder's bytes, its ignored share, and its newest not-ignored file.
fn disk(git: &Git, dir: &Path) -> (Disk, Option<String>) {
    let ignored = ignored(git, dir);
    let mut walk = Walk {
        ignored: &ignored,
        disk: Disk::default(),
        newest: None,
    };
    walk.visit(dir, false);
    (walk.disk, walk.newest.map(stamp))
}

struct Walk<'a> {
    ignored: &'a HashSet<PathBuf>,
    disk: Disk,
    newest: Option<SystemTime>,
}

impl Walk<'_> {
    /// Add `p` (following no links); `in_ignored` when an ignored folder holds it.
    fn visit(&mut self, p: &Path, in_ignored: bool) {
        let Ok(meta) = std::fs::symlink_metadata(p) else {
            return;
        };
        let ignored = in_ignored || self.ignored.contains(p);
        if meta.is_dir() {
            for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
                self.visit(&e.path(), ignored);
            }
            return;
        }
        self.disk.bytes += meta.len();
        if ignored {
            self.disk.ignored_bytes += meta.len();
        } else if let Ok(m) = meta.modified() {
            self.newest = self.newest.max(Some(m));
        }
    }
}
