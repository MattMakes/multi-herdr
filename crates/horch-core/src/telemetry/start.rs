//! The collector start handshake (section 12.1, SPC-03).
//!
//! `horch telemetry ensure` does not wait for the collector it starts: under
//! load the new process can take longer than any fixed wait to take the lock.
//! It writes `telemetry/start.json` (a [`Request`]) and returns. The process
//! it runs gets the request id in [`START_ENV`] and writes
//! `telemetry/starting.json` (a [`Marker`] with its pid) as its first step.
//! The process that then takes the lock closes the old collector
//! workspaces and removes both files; a process that finds the lock held by another
//! collector ends.
//!
//! A later call reads the two files while no collector holds the lock
//! ([`pending`]): a marker whose pid is dead is a failed start, and so is a
//! request with no marker after [`UNSEEN_AFTER`]. Either is reported, and
//! `ensure` starts the collector again. A wrong guess costs nothing: the lock
//! lets only 1 collector run.
//!
//! Nothing here holds telemetry: the collector reads ledgers and transcripts
//! from where they are, so a late start loses no event (section 14).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::procid;

/// The environment variable that gives the started process its request id.
pub const START_ENV: &str = "HORCH_TELEMETRY_START";

/// How long a request may wait for its process to write a marker before a
/// later call counts the start as failed. It decides only when to retry,
/// never whether `ensure` succeeds.
pub const UNSEEN_AFTER: Duration = Duration::from_secs(300);

/// What `ensure` started, and where.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub requested_at: String,
    pub workspace_id: String,
    pub pane_id: String,
    /// The herdr program `ensure` used; the collector closes the old
    /// workspaces with it.
    pub herdr: PathBuf,
}

/// The started process: written before it tries the lock.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub id: String,
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid_start: Option<u64>,
    pub started_at: String,
}

/// The state of the last start, while no collector holds the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    /// No start is outstanding.
    None,
    /// A start is in progress: its process runs, or has not been seen yet.
    Starting(Request),
    /// The start failed, for the reason given.
    Failed(Request, String),
}

pub fn request_path(state_root: &Path) -> PathBuf {
    super::dir(state_root).join("start.json")
}

pub fn marker_path(state_root: &Path) -> PathBuf {
    super::dir(state_root).join("starting.json")
}

fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

fn write<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    super::store::replace_private(path, &serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

/// A fresh request id.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Record a new start; it replaces any earlier request and its marker.
pub fn record(state_root: &Path, request: &Request) -> anyhow::Result<()> {
    let _ = std::fs::remove_file(marker_path(state_root));
    write(&request_path(state_root), request)
}

/// The request id this process was started for, if any.
pub fn started_for() -> Option<String> {
    use crate::runtime::EnvSource;
    crate::runtime::ProcessEnv
        .var(START_ENV)
        .filter(|v| !v.is_empty())
}

/// Write this process's marker for request `id`.
pub fn mark(state_root: &Path, id: &str, now: &str) -> anyhow::Result<()> {
    let pid = std::process::id();
    let marker = Marker {
        id: id.to_string(),
        pid,
        pid_start: procid::start_time(pid),
        started_at: now.to_string(),
    };
    write(&marker_path(state_root), &marker)
}

/// The outstanding request, whichever start it is for. The new lock
/// holder reads it to close the old workspaces, then calls [`clear`].
pub fn current(state_root: &Path) -> Option<Request> {
    read(&request_path(state_root))
}

/// A started process found another collector holding the lock: its start
/// is over. Removes request `id` and its marker, and no later one.
pub fn withdraw(state_root: &Path, id: &str) {
    if read::<Request>(&request_path(state_root)).is_some_and(|r| r.id == id) {
        let _ = std::fs::remove_file(request_path(state_root));
    }
    if read::<Marker>(&marker_path(state_root)).is_some_and(|m| m.id == id) {
        let _ = std::fs::remove_file(marker_path(state_root));
    }
}

/// The start is over: the new collector holds the lock, or a failed start
/// is replaced by the next one.
pub fn clear(state_root: &Path) {
    let _ = std::fs::remove_file(request_path(state_root));
    let _ = std::fs::remove_file(marker_path(state_root));
}

/// The outstanding start, if any. Only meaningful while no collector holds
/// the lock: the lock holder removes both files.
pub fn pending(state_root: &Path) -> Pending {
    let path = request_path(state_root);
    let Some(request) = read::<Request>(&path) else {
        return Pending::None;
    };
    let marker = read::<Marker>(&marker_path(state_root)).filter(|m| m.id == request.id);
    match marker {
        Some(m) if procid::alive(m.pid, m.pid_start) => Pending::Starting(request),
        Some(m) => {
            let why = format!("collector pid {} ended before it took the lock", m.pid);
            Pending::Failed(request, why)
        }
        None if unseen_too_long(&path) => {
            let why = format!(
                "no collector process started in pane {} within {} s",
                request.pane_id,
                UNSEEN_AFTER.as_secs()
            );
            Pending::Failed(request, why)
        }
        None => Pending::Starting(request),
    }
}

/// Whether the request file is older than [`UNSEEN_AFTER`]. Its modification
/// time, not `HORCH_NOW`: a pinned clock must not hold a start forever.
fn unseen_too_long(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age >= UNSEEN_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str) -> Request {
        Request {
            id: id.into(),
            requested_at: "2026-10-30T05:00:00Z".into(),
            workspace_id: "w2".into(),
            pane_id: "w2:p1".into(),
            herdr: PathBuf::from("/bin/herdr"),
        }
    }

    fn age(path: &Path, by: Duration) {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(std::time::SystemTime::now() - by).unwrap();
    }

    /// SPC-03: a request whose process has not written a marker is a start
    /// in progress, until it is [`UNSEEN_AFTER`] old: then it failed.
    #[test]
    fn spc_03_an_unseen_start_fails_only_after_the_bound() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(pending(tmp.path()), Pending::None);
        record(tmp.path(), &request("a")).unwrap();
        assert_eq!(pending(tmp.path()), Pending::Starting(request("a")));
        age(
            &request_path(tmp.path()),
            UNSEEN_AFTER - Duration::from_secs(5),
        );
        assert_eq!(pending(tmp.path()), Pending::Starting(request("a")));
        age(&request_path(tmp.path()), UNSEEN_AFTER);
        let Pending::Failed(r, why) = pending(tmp.path()) else {
            panic!("{:?}", pending(tmp.path()));
        };
        assert_eq!(r.id, "a");
        assert!(why.contains("pane w2:p1"), "{why}");
    }

    /// SPC-03: a marker with a live pid is a start in progress at any age; a
    /// marker whose pid ended is a failed start at once. A marker of another
    /// request counts as no marker.
    #[cfg(unix)]
    #[test]
    fn spc_03_a_marker_decides_by_its_pid() {
        let tmp = tempfile::tempdir().unwrap();
        record(tmp.path(), &request("a")).unwrap();
        age(&request_path(tmp.path()), UNSEEN_AFTER * 2);
        mark(tmp.path(), "a", "now").unwrap();
        assert_eq!(pending(tmp.path()), Pending::Starting(request("a")));

        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();
        let marker = Marker {
            id: "a".into(),
            pid: dead,
            pid_start: Some(1),
            started_at: "now".into(),
        };
        write(&marker_path(tmp.path()), &marker).unwrap();
        let Pending::Failed(_, why) = pending(tmp.path()) else {
            panic!("{:?}", pending(tmp.path()));
        };
        assert!(why.contains(&format!("pid {dead} ended")), "{why}");

        let other = Marker {
            id: "b".into(),
            ..marker
        };
        write(&marker_path(tmp.path()), &other).unwrap();
        assert!(matches!(pending(tmp.path()), Pending::Failed(_, w) if w.contains("no collector")));
    }

    /// The lock holder ends any start; a process that lost the lock
    /// withdraws only its own, never a later start's.
    #[test]
    fn clear_ends_any_start_and_withdraw_only_its_own() {
        let tmp = tempfile::tempdir().unwrap();
        record(tmp.path(), &request("b")).unwrap();
        mark(tmp.path(), "b", "now").unwrap();
        withdraw(tmp.path(), "a");
        assert!(request_path(tmp.path()).is_file());
        assert!(marker_path(tmp.path()).is_file());
        withdraw(tmp.path(), "b");
        assert_eq!(pending(tmp.path()), Pending::None);
        assert!(!marker_path(tmp.path()).exists());

        record(tmp.path(), &request("c")).unwrap();
        mark(tmp.path(), "c", "now").unwrap();
        assert_eq!(current(tmp.path()), Some(request("c")));
        clear(tmp.path());
        assert_eq!(pending(tmp.path()), Pending::None);
        assert!(!marker_path(tmp.path()).exists());
        assert_eq!(current(tmp.path()), None);
    }
}
