//! The archive and trash refs of worktree triage (#308).
//!
//! `ARCHIVE` keeps its work at `refs/horch/archive/<name>`. `DELETE` keeps
//! its work at `refs/horch/trash/<name>/<UTC yyyymmddThhmmssZ>` until
//! [`gc`] deletes it, 30 days on. The time is in the name, so a folder that
//! never had a ledger row is dated too. Local refs only: no remote needed.

use std::path::Path;

use anyhow::{bail, Result};
use chrono::{DateTime, NaiveDateTime, Utc};

use crate::taskledger::git::Git;

/// The prefix of every archive ref.
pub const ARCHIVE: &str = "refs/horch/archive/";
/// The prefix of every trash ref.
pub const TRASH: &str = "refs/horch/trash/";
/// How long a trash ref stays before [`gc`] deletes it.
pub const TRASH_DAYS: i64 = 30;

const STAMP: &str = "%Y%m%dT%H%M%SZ";

/// The archive ref of the worktree `name`.
pub fn archive_ref(name: &str) -> String {
    format!("{ARCHIVE}{name}")
}

/// A trash ref of the worktree `name`, dated `at`.
pub fn trash_ref(name: &str, at: DateTime<Utc>) -> String {
    format!("{TRASH}{name}/{}", at.format(STAMP))
}

/// When a trash ref was written, from its name.
pub fn trashed_at(refname: &str) -> Option<DateTime<Utc>> {
    let stamp = refname.rsplit('/').next()?;
    NaiveDateTime::parse_from_str(stamp, STAMP)
        .ok()
        .map(|t| t.and_utc())
}

/// The commit `refname` holds, if it exists.
pub fn read(git: &Git, root: &Path, refname: &str) -> Option<String> {
    git.rev(root, refname).ok()
}

/// Point `refname` at `tip` and read it back. A new ref is created only
/// when none exists; an existing one moves only forward (its commit is in
/// `tip`'s history), so no work it held is lost.
pub fn write(git: &Git, root: &Path, refname: &str, tip: &str) -> Result<()> {
    let old = read(git, root, refname);
    match old.as_deref() {
        Some(o) if o == tip => return Ok(()),
        Some(o) if !git.is_ancestor(root, o, tip) => {
            bail!("{refname} holds other work ({o}); nothing removed")
        }
        _ => {}
    }
    let expect = old.unwrap_or_default();
    git.run(root, &["update-ref", refname, tip, &expect])?;
    if read(git, root, refname).as_deref() != Some(tip) {
        bail!("{refname} does not hold {tip} after the write; nothing removed");
    }
    Ok(())
}

/// The refs under `prefix`.
fn refs_under(git: &Git, root: &Path, prefix: &str) -> Result<Vec<String>> {
    let out = git.run(root, &["for-each-ref", "--format=%(refname)", prefix])?;
    Ok(out.lines().map(str::to_string).collect())
}

/// The ref that [`super::apply`] restores `name` from: its archive ref, else
/// its newest trash ref.
pub fn restorable(git: &Git, root: &Path, name: &str) -> Result<Option<String>> {
    let archive = archive_ref(name);
    if read(git, root, &archive).is_some() {
        return Ok(Some(archive));
    }
    let prefix = format!("{TRASH}{name}/");
    let mut trash: Vec<String> = refs_under(git, root, &prefix)?
        .into_iter()
        .filter(|r| r.strip_prefix(&prefix).is_some_and(|s| !s.contains('/')))
        .filter(|r| trashed_at(r).is_some())
        .collect();
    trash.sort();
    Ok(trash.pop())
}

/// `horch ledger gc`: delete every trash ref older than [`TRASH_DAYS`] at
/// `now`, and return them. A ref whose name has no time is kept. Only trash
/// refs are ever deleted here.
pub fn gc(git: &Git, root: &Path, now: DateTime<Utc>) -> Result<Vec<String>> {
    let mut deleted = Vec::new();
    for r in refs_under(git, root, TRASH)? {
        let Some(at) = trashed_at(&r) else {
            continue;
        };
        if now - at <= chrono::Duration::days(TRASH_DAYS) {
            continue;
        }
        let Some(commit) = read(git, root, &r) else {
            continue;
        };
        // The old value guards the delete: a ref that moved is kept.
        git.run(root, &["update-ref", "-d", &r, &commit])?;
        deleted.push(r);
    }
    Ok(deleted)
}
