//! The fact scans of a pass, each in its budget.
//!
//! Part 1 of a pass runs 4 scans (`docs/tickets/fleetd-p1/MAP.md`, Q4):
//! processes, outboxes, worktree folders and requests. Each scan walks its
//! items in turn order from its [`Cursor`], looks at its [`Deadline`]
//! between 2 items, and stops there. The next pass continues at the
//! cursor. A folder that file watching reports goes first; it does not move
//! the cursor.
//!
//! The caller gives the items of the processes, outboxes and worktree
//! folders scans: the rows that name them belong to other contexts
//! (Q13). The requests scan lists its folder itself.

use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline};

/// The time of a scan: its deadline, the monotonic clock it reads between
/// 2 items, and the calendar stamp of a new lap.
pub struct ScanClock<'a> {
    /// Where the scan stops.
    pub deadline: Deadline,
    /// The monotonic clock.
    pub now: &'a mut dyn FnMut() -> Instant,
    /// The ledger stamp of this pass, for `lap_started_at`.
    pub stamp: &'a str,
}

/// The facts that 1 scan read in 1 pass.
#[derive(Debug, PartialEq, Eq)]
pub struct Scan<F> {
    /// 1 fact for each item read, in the order read.
    pub facts: Vec<F>,
    /// True when the deadline stopped the scan before the end of its lap.
    pub stopped_at_budget: bool,
}

/// A process that a row names.
#[derive(Clone, Debug)]
pub struct ProcessItem {
    /// The key of the row, for the cursor.
    pub key: String,
    /// The recorded pid.
    pub pid: u32,
    /// The recorded start time; `None` checks the pid alone.
    pub pid_start: Option<u64>,
}

/// Whether the process of a row is alive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessFact {
    /// The key of the row.
    pub key: String,
    /// What the scan knows of the process.
    pub liveness: Liveness,
}

/// What a scan knows of a recorded process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    /// A process has the pid and, when one is recorded, the start time.
    Alive,
    /// No process has the pid, or a process with another start time has it.
    Dead,
    /// A process has the pid, and its start time cannot be read. The scan
    /// does not know whether it is the recorded process, so it is not dead.
    Unknown,
}

/// A folder that a row names: an outbox or a worktree folder.
#[derive(Clone, Debug)]
pub struct FolderItem {
    /// The key of the row, for the cursor.
    pub key: String,
    /// The folder.
    pub path: PathBuf,
}

/// The listing of an outbox folder. Ingestion comes in Phase 4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listing {
    /// The folder does not exist.
    Missing,
    /// The names in the folder, in order.
    Names(Vec<String>),
    /// The folder exists and cannot be read; the text of the error.
    Unreadable(String),
}

/// The listing of 1 outbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxFact {
    /// The key of the row.
    pub key: String,
    /// The folder listing.
    pub listing: Listing,
}

/// Whether the folder of 1 worktree exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderFact {
    /// The key of the row.
    pub key: String,
    /// Whether the folder exists.
    pub presence: Presence,
}

/// Whether a folder exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    /// The folder exists.
    Present,
    /// Nothing exists at the path.
    Missing,
    /// The path cannot be read, or it is not a folder; the text of the
    /// error. The folder is not known to be missing.
    Unreadable(String),
}

/// The outbox folder of attempt `attempt` of task `task_id`:
/// `<fleet root>/tasks/<task id>/attempts/<attempt>/outbox/`.
pub fn outbox_dir(fleet_root: &Path, task_id: &str, attempt: &str) -> PathBuf {
    fleet_root
        .join("tasks")
        .join(task_id)
        .join("attempts")
        .join(attempt)
        .join("outbox")
}

/// The request folder: `<fleet root>/run/requests/`.
pub fn requests_dir(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("requests")
}

/// Whether each process is alive, by its pid and start time.
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

/// The folder listing of each outbox. A watched folder goes first.
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

/// Whether each worktree folder exists. A watched folder goes first.
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

/// The names of the request files in `<fleet root>/run/requests/`, in turn
/// order. No folder gives no requests. The requests themselves come in
/// Phase 5.
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

/// The liveness of process `pid` with the recorded start time `pid_start`.
fn process_liveness(pid: u32, pid_start: Option<u64>) -> Liveness {
    let alive = horch_platform::process::alive(pid);
    // The start time is read only when it is compared.
    let read = || horch_platform::process::start_time(pid);
    liveness(alive, pid_start, read)
}

/// The liveness from the facts of the OS: whether the pid exists, the
/// recorded start time, and the start time that the OS gives (`None` when
/// the query fails).
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

/// Whether folder `path` exists. An error other than "not found" is not a
/// missing folder.
fn presence(path: &Path) -> Presence {
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => Presence::Present,
        Ok(_) => Presence::Unreadable("not a folder".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Presence::Missing,
        Err(e) => Presence::Unreadable(e.to_string()),
    }
}

/// The indexes of the items in or under a watched path, in the order
/// watched, each once.
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

/// The names in folder `dir`, in order.
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

/// Read the items at `first`, then 1 lap of the others from the cursor,
/// until the deadline. Only the lap moves the cursor.
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
