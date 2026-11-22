use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::procid;

pub const START_ENV: &str = "HORCH_TELEMETRY_START";

pub const UNSEEN_AFTER: Duration = Duration::from_secs(300);

fn is_herdr(b: &crate::runtime::Backend) -> bool {
    *b == crate::runtime::Backend::Herdr
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub requested_at: String,
    pub workspace_id: String,
    pub pane_id: String,
    pub herdr: PathBuf,
    #[serde(default, skip_serializing_if = "is_herdr")]
    pub backend: crate::runtime::Backend,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub id: String,
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid_start: Option<u64>,
    pub started_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    None,
    Starting(Request),
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

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub fn record(state_root: &Path, request: &Request) -> anyhow::Result<()> {
    let _ = std::fs::remove_file(marker_path(state_root));
    write(&request_path(state_root), request)
}

pub fn started_for() -> Option<String> {
    use crate::runtime::EnvSource;
    crate::runtime::ProcessEnv
        .var(START_ENV)
        .filter(|v| !v.is_empty())
}

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

pub fn current(state_root: &Path) -> Option<Request> {
    read(&request_path(state_root))
}

pub fn withdraw(state_root: &Path, id: &str) {
    if read::<Request>(&request_path(state_root)).is_some_and(|r| r.id == id) {
        let _ = std::fs::remove_file(request_path(state_root));
    }
    if read::<Marker>(&marker_path(state_root)).is_some_and(|m| m.id == id) {
        let _ = std::fs::remove_file(marker_path(state_root));
    }
}

pub fn clear(state_root: &Path) {
    let _ = std::fs::remove_file(request_path(state_root));
    let _ = std::fs::remove_file(marker_path(state_root));
}

pub fn pending(state_root: &Path) -> Pending {
    pending_with(state_root, |_| false)
}

pub fn pending_with(state_root: &Path, pane_ended: impl Fn(&Request) -> bool) -> Pending {
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
        None if pane_ended(&request) => {
            let why = format!(
                "pane {} finished before a collector started",
                request.pane_id
            );
            Pending::Failed(request, why)
        }
        None => Pending::Starting(request),
    }
}

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
            backend: crate::runtime::Backend::Herdr,
        }
    }

    fn age(path: &Path, by: Duration) {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(std::time::SystemTime::now() - by).unwrap();
    }

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

    #[test]
    fn an_ended_pane_fails_only_a_start_without_a_marker() {
        let tmp = tempfile::tempdir().unwrap();
        record(tmp.path(), &request("a")).unwrap();
        assert_eq!(
            pending_with(tmp.path(), |_| false),
            Pending::Starting(request("a"))
        );
        let Pending::Failed(r, why) = pending_with(tmp.path(), |_| true) else {
            panic!("{:?}", pending_with(tmp.path(), |_| true));
        };
        assert_eq!(r.id, "a");
        assert!(why.contains("pane w2:p1 finished"), "{why}");

        mark(tmp.path(), "a", "now").unwrap();
        assert_eq!(
            pending_with(tmp.path(), |_| true),
            Pending::Starting(request("a"))
        );
    }

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
