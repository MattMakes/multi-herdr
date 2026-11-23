use std::path::Path;

use anyhow::{bail, Result};
use chrono::{DateTime, NaiveDateTime, Utc};

use crate::runtime::context::{EnvSource, ProcessEnv};
use crate::runtime::fault::Faults;
use crate::taskledger::git::Git;

pub const ARCHIVE: &str = "refs/horch/archive/";
pub const TRASH: &str = "refs/horch/trash/";
pub const RESCUE: &str = "refs/horch/rescue/";
pub const READ_BACK_FAULT: &str = "triage-ref-read-back";
pub const TRASH_DAYS: i64 = 30;

const STAMP: &str = "%Y%m%dT%H%M%SZ";

pub fn archive_ref(name: &str) -> String {
    format!("{ARCHIVE}{name}")
}

pub fn trash_ref(name: &str, at: DateTime<Utc>) -> String {
    format!("{TRASH}{name}/{}", at.format(STAMP))
}

pub fn trashed_at(refname: &str) -> Option<DateTime<Utc>> {
    let stamp = refname.rsplit('/').next()?;
    NaiveDateTime::parse_from_str(stamp, STAMP)
        .ok()
        .map(|t| t.and_utc())
}

pub fn read(git: &Git, root: &Path, refname: &str) -> Option<String> {
    git.rev(root, refname).ok()
}

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
    let faults = Faults::parse(ProcessEnv.var("HORCH_FAULT").as_deref());
    let back = if faults.has(READ_BACK_FAULT) {
        None
    } else {
        read(git, root, refname)
    };
    if back.as_deref() != Some(tip) {
        bail!("{refname} does not hold {tip} after the write; nothing removed");
    }
    Ok(())
}

fn refs_under(git: &Git, root: &Path, prefix: &str) -> Result<Vec<String>> {
    let out = git.run(root, &["for-each-ref", "--format=%(refname)", prefix])?;
    Ok(out.lines().map(str::to_string).collect())
}

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
        git.run(root, &["update-ref", "-d", &r, &commit])?;
        deleted.push(r);
    }
    Ok(deleted)
}
