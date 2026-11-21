use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline, PassBudget};

pub struct ScanClock<'a> {
    pub deadline: Deadline,
    pub now: &'a mut dyn FnMut() -> Instant,
    pub stamp: &'a str,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Scan<F> {
    pub facts: Vec<F>,
    pub stopped_at_budget: bool,
}

#[derive(Clone, Debug)]
pub struct ProcessItem {
    pub key: String,
    pub pid: u32,
    pub pid_start: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessFact {
    pub key: String,
    pub liveness: Liveness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    Alive,
    Dead,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct FolderItem {
    pub key: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listing {
    Missing,
    Names(Vec<String>),
    Unreadable(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxFact {
    pub key: String,
    pub listing: Listing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderFact {
    pub key: String,
    pub presence: Presence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    Present,
    Missing,
    Unreadable(String),
}

pub fn outbox_dir(fleet_root: &Path, task_id: &str, attempt: &str) -> PathBuf {
    fleet_root
        .join("tasks")
        .join(task_id)
        .join("attempts")
        .join(attempt)
        .join("outbox")
}

pub fn requests_dir(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("requests")
}

pub fn scan_processes(
    items: &[ProcessItem],
    cursor: &mut Cursor,
    clock: &mut ScanClock<'_>,
) -> Scan<ProcessFact> {
    walk(
        items,
        |i| &i.key,
        &[],
        cursor,
        clock,
        |i| ProcessFact {
            key: i.key.clone(),
            liveness: process_liveness(i.pid, i.pid_start),
        },
    )
}

pub fn scan_outboxes(
    items: &[FolderItem],
    watched: &[PathBuf],
    cursor: &mut Cursor,
    clock: &mut ScanClock<'_>,
) -> Scan<OutboxFact> {
    let first = watched_first(items, watched);
    walk(
        items,
        |i| &i.key,
        &first,
        cursor,
        clock,
        |i| OutboxFact {
            key: i.key.clone(),
            listing: listing(&i.path),
        },
    )
}

pub fn scan_worktree_folders(
    items: &[FolderItem],
    watched: &[PathBuf],
    cursor: &mut Cursor,
    clock: &mut ScanClock<'_>,
) -> Scan<FolderFact> {
    let first = watched_first(items, watched);
    walk(
        items,
        |i| &i.key,
        &first,
        cursor,
        clock,
        |i| FolderFact {
            key: i.key.clone(),
            presence: presence(&i.path),
        },
    )
}

pub fn scan_requests(
    fleet_root: &Path,
    cursor: &mut Cursor,
    clock: &mut ScanClock<'_>,
) -> std::io::Result<Scan<String>> {
    let names = match listing(&requests_dir(fleet_root)) {
        Listing::Missing => Vec::new(),
        Listing::Names(names) => names,
        Listing::Unreadable(e) => return Err(std::io::Error::other(e)),
    };
    Ok(walk(&names, |n| n, &[], cursor, clock, Clone::clone))
}

pub fn scan_idea_folders<F>(
    ideas: &Path,
    cursor: &mut Cursor,
    clock: &mut ScanClock<'_>,
    read: impl FnMut(&String) -> F,
) -> std::io::Result<Scan<F>> {
    let names = match listing(ideas) {
        Listing::Missing => Vec::new(),
        Listing::Names(names) => names,
        Listing::Unreadable(e) => return Err(std::io::Error::other(e)),
    };
    Ok(walk(&names, |n| n, &[], cursor, clock, read))
}

fn process_liveness(pid: u32, pid_start: Option<u64>) -> Liveness {
    let alive = horch_platform::process::alive(pid);
    let read = || horch_platform::process::start_time(pid);
    liveness(alive, pid_start, read)
}

fn liveness(alive: bool, recorded: Option<u64>, read: impl FnOnce() -> Option<u64>) -> Liveness {
    if !alive {
        return Liveness::Dead;
    }
    let Some(recorded) = recorded else {
        return Liveness::Alive;
    };
    match read() {
        Some(start) if start == recorded => Liveness::Alive,
        Some(_) => Liveness::Dead,
        None => Liveness::Unknown,
    }
}

fn presence(path: &Path) -> Presence {
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => Presence::Present,
        Ok(_) => Presence::Unreadable("not a folder".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Presence::Missing,
        Err(e) => Presence::Unreadable(e.to_string()),
    }
}

fn watched_first(items: &[FolderItem], watched: &[PathBuf]) -> Vec<usize> {
    let mut first = Vec::new();
    for path in watched {
        for (i, item) in items.iter().enumerate() {
            if path.starts_with(&item.path) && !first.contains(&i) {
                first.push(i);
            }
        }
    }
    first
}

fn listing(dir: &Path) -> Listing {
    match std::fs::read_dir(dir) {
        Ok(entries) => {
            let mut names = Vec::new();
            for entry in entries {
                match entry {
                    Ok(e) => names.push(e.file_name().to_string_lossy().into_owned()),
                    Err(e) => return Listing::Unreadable(e.to_string()),
                }
            }
            names.sort();
            Listing::Names(names)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Listing::Missing,
        Err(e) => Listing::Unreadable(e.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliPath {
    pub cli: String,
    pub path: PathBuf,
}

pub fn scan_cli_binaries<F>(
    items: &[CliPath],
    budget: PassBudget,
    cursor: &mut Cursor,
    clock: &mut ScanClock<'_>,
    mut read: impl FnMut(&CliPath) -> Option<F>,
) -> Scan<F> {
    let mut facts = Vec::new();
    let lap = cursor.turns(items, |i| i.cli.as_str());
    for item in lap.into_iter().take(budget.rows()) {
        if clock.deadline.reached((clock.now)()) {
            return Scan {
                facts,
                stopped_at_budget: true,
            };
        }
        facts.extend(read(item));
        cursor.step(&item.cli, clock.stamp);
    }
    Scan {
        facts,
        stopped_at_budget: false,
    }
}

fn walk<T, F>(
    items: &[T],
    key: impl Fn(&T) -> &str,
    first: &[usize],
    cursor: &mut Cursor,
    clock: &mut ScanClock<'_>,
    mut read: impl FnMut(&T) -> F,
) -> Scan<F> {
    let mut facts = Vec::new();
    let done: Vec<&str> = first.iter().map(|&i| key(&items[i])).collect();
    for &i in first {
        if clock.deadline.reached((clock.now)()) {
            return Scan {
                facts,
                stopped_at_budget: true,
            };
        }
        facts.push(read(&items[i]));
    }
    for item in cursor.turns(items, &key) {
        let k = key(item);
        if done.contains(&k) {
            continue;
        }
        if clock.deadline.reached((clock.now)()) {
            return Scan {
                facts,
                stopped_at_budget: true,
            };
        }
        facts.push(read(item));
        cursor.step(k, clock.stamp);
    }
    Scan {
        facts,
        stopped_at_budget: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_time_that_cannot_be_read_is_not_a_dead_process() {
        assert_eq!(liveness(true, Some(7), || None), Liveness::Unknown);
    }

    #[test]
    fn liveness_compares_the_recorded_start_time() {
        assert_eq!(liveness(false, Some(7), || Some(7)), Liveness::Dead);
        assert_eq!(liveness(true, Some(7), || Some(7)), Liveness::Alive);
        assert_eq!(liveness(true, Some(7), || Some(8)), Liveness::Dead);
        assert_eq!(liveness(true, None, || None), Liveness::Alive);
    }

    #[test]
    fn presence_tells_a_missing_folder_from_a_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(presence(dir.path()), Presence::Present);
        assert_eq!(presence(&dir.path().join("gone")), Presence::Missing);
        let file = dir.path().join("f");
        std::fs::write(&file, "").unwrap();
        assert!(matches!(presence(&file), Presence::Unreadable(_)));
    }
}
