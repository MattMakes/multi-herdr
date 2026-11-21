use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_platform::signal::{stop_same, Stop as How};
use horch_platform::watch::Watcher;
use horch_platform::NotSupported;
use serde::{Deserialize, Serialize};

use crate::exeid::{exe_identity, ExeIdentity};
use crate::fleetd::fleet_runtime::domain::clock::{AwakeReading, FleetSeconds};
use crate::fleetd::fleet_runtime::domain::heartbeat::{Exe, Heartbeat};
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::task_execution::domain::task::{GroupLeft, ProcessScope, Recorded};
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, Launched, ProcessHost};
use crate::fleetd::worktree_custody::adapters::git::{
    repo_git_dir, CliWorktreeGit, FLEETD_GIT_LIMIT,
};
use crate::fsx;
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::ProcessEnv;

const MODE: u32 = 0o644;

pub const WATCH_LIMIT: usize = 64;

fn run_dir(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run")
}

pub fn stop_process(pid: u32, pid_start: u64, scope: ProcessScope, kill: bool) -> bool {
    stop_process_with(pid, pid_start, scope, kill, &mut || {})
}

pub fn stop_process_with(
    pid: u32,
    pid_start: u64,
    scope: ProcessScope,
    kill: bool,
    after_check: &mut dyn FnMut(),
) -> bool {
    let how = if kill { How::Kill } else { How::Term };
    if scope == ProcessScope::OrphanGroup {
        return stop_orphan_group(pid, how);
    }
    if scope == ProcessScope::LeaderGroup {
        if let Ok(Some(sent)) = stop_leader_group(pid, pid_start, how, after_check) {
            return sent;
        }
    }
    stop_same(pid, pid_start, how)
}

pub fn stop_leader_group(
    leader: u32,
    started: u64,
    how: How,
    after_check: &mut dyn FnMut(),
) -> Result<Option<bool>, NotSupported> {
    let mut snapshot: Vec<(u32, u64)> = horch_platform::process::group_members(leader)?
        .into_iter()
        .filter(|pid| *pid != leader)
        .filter_map(|pid| Some((pid, horch_platform::process::start_time(pid)?)))
        .collect();
    snapshot.push((leader, started));
    if !horch_platform::group::is_group_leader(leader, started)? {
        return Ok(None);
    }
    after_check();
    let mut sent = false;
    for (pid, start) in snapshot {
        sent |= stop_same(pid, start, how);
    }
    Ok(Some(sent))
}

fn stop_each(members: Vec<u32>, how: How) -> bool {
    let mut sent = false;
    for member in members {
        if let Some(start) = horch_platform::process::start_time(member) {
            sent |= stop_same(member, start, how);
        }
    }
    sent
}

fn stop_orphan_group(pgid: u32, how: How) -> bool {
    if horch_platform::process::alive(pgid) {
        return false;
    }
    match how {
        How::Kill => horch_platform::group::kill_group_id(pgid).unwrap_or(false),
        _ => stop_each(
            horch_platform::process::group_members(pgid).unwrap_or_default(),
            how,
        ),
    }
}

pub fn group_left(p: Recorded) -> GroupLeft {
    use horch_platform::process;
    if !process::group_members(p.pid).is_ok_and(|m| !m.is_empty()) {
        return GroupLeft::None;
    }
    if !process::alive(p.pid) {
        return GroupLeft::Members;
    }
    if p.pid_start.is_some() && process::start_time(p.pid) == p.pid_start {
        GroupLeft::None
    } else {
        GroupLeft::NewLeader
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRecord {
    pub attempt: String,
    pub launched_at: FleetSeconds,
    pub process: Option<Recorded>,
}

#[derive(Serialize, Deserialize)]
struct LaunchFile {
    attempt: String,
    launched_at: i64,
    pid: Option<u32>,
    pid_start: Option<u64>,
}

pub fn launch_path(fleet_root: &Path, attempt: &str) -> PathBuf {
    let mut name = String::new();
    for b in attempt.bytes() {
        if b.is_ascii_alphanumeric() || b"._-".contains(&b) {
            name.push(char::from(b));
        } else {
            name.push_str(&format!("%{b:02X}"));
        }
    }
    run_dir(fleet_root)
        .join("launches")
        .join(format!("{name}.json"))
}

pub fn read_launch(fleet_root: &Path, attempt: &str) -> io::Result<Option<LaunchRecord>> {
    let bytes = match std::fs::read(launch_path(fleet_root, attempt)) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let record = match serde_json::from_slice::<LaunchFile>(&bytes) {
        Ok(f) => LaunchRecord {
            attempt: attempt.to_string(),
            launched_at: FleetSeconds(f.launched_at),
            process: f.pid.map(|pid| Recorded {
                pid,
                pid_start: f.pid_start,
            }),
        },
        Err(_) => LaunchRecord {
            attempt: attempt.to_string(),
            launched_at: FleetSeconds(0),
            process: None,
        },
    };
    Ok(Some(record))
}

pub fn write_launch(fleet_root: &Path, r: &LaunchRecord) -> Result<()> {
    let path = launch_path(fleet_root, &r.attempt);
    let file = LaunchFile {
        attempt: r.attempt.clone(),
        launched_at: r.launched_at.0,
        pid: r.process.map(|p| p.pid),
        pid_start: r.process.and_then(|p| p.pid_start),
    };
    write_json(&path, &file)
}

pub fn remove_launch(fleet_root: &Path, attempt: &str) -> io::Result<()> {
    match std::fs::remove_file(launch_path(fleet_root, attempt)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

pub fn launch_attempt(
    fleet_root: &Path,
    attempt: &str,
    clock: FleetSeconds,
    spec: impl FnOnce() -> Result<LaunchSpec>,
    host: &dyn ProcessHost,
) -> Result<Option<Launched>> {
    if read_launch(fleet_root, attempt)?.is_some() {
        return Ok(None);
    }
    let mut record = LaunchRecord {
        attempt: attempt.to_string(),
        launched_at: clock,
        process: None,
    };
    write_launch(fleet_root, &record)?;
    let spec = spec().with_context(|| format!("the launch of attempt {attempt}"))?;
    let mut launched = host
        .launch(&spec)
        .with_context(|| format!("launching attempt {attempt}"))?;
    record.process = Some(Recorded {
        pid: launched.pid,
        pid_start: launched.pid_start,
    });
    if let Err(e) = write_launch(fleet_root, &record) {
        horch_platform::group::kill_child_group(&mut launched.child);
        let _ = launched.child.wait();
        return Err(e.context(format!("recording the process of attempt {attempt}")));
    }
    Ok(Some(launched))
}

pub fn heartbeat_path(fleet_root: &Path) -> PathBuf {
    run_dir(fleet_root).join("heartbeat.json")
}

#[derive(Serialize, Deserialize)]
struct HeartbeatFile {
    pid: u32,
    pid_start: u64,
    generation: i64,
    awake: AwakeFile,
    at: String,
    exe: ExeIdentity,
    version: String,
}

#[derive(Serialize, Deserialize)]
struct AwakeFile {
    boot_id: String,
    awake_ms: u64,
}

pub fn exe_of(id: ExeIdentity) -> Exe {
    Exe {
        path: id.path,
        len: id.len,
        mtime_ns: id.mtime_ns,
    }
}

pub fn this_exe() -> Exe {
    let id = std::env::current_exe()
        .ok()
        .and_then(|p| exe_identity(&p))
        .unwrap_or_default();
    exe_of(id)
}

pub fn write_heartbeat(fleet_root: &Path, hb: &Heartbeat) -> Result<()> {
    let file = HeartbeatFile {
        pid: hb.pid,
        pid_start: hb.pid_start,
        generation: hb.generation.0,
        awake: AwakeFile {
            boot_id: hb.awake.boot_id.clone(),
            awake_ms: hb.awake.awake_ms,
        },
        at: hb.at.clone(),
        exe: ExeIdentity {
            path: hb.exe.path.clone(),
            len: hb.exe.len,
            mtime_ns: hb.exe.mtime_ns,
        },
        version: hb.version.clone(),
    };
    write_json(&heartbeat_path(fleet_root), &file)
}

pub fn read_heartbeat(fleet_root: &Path) -> Result<Option<Heartbeat>> {
    let path = heartbeat_path(fleet_root);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let f: HeartbeatFile =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(Heartbeat {
        pid: f.pid,
        pid_start: f.pid_start,
        generation: Generation(f.generation),
        awake: AwakeReading {
            boot_id: f.awake.boot_id,
            awake_ms: f.awake.awake_ms,
        },
        at: f.at,
        exe: exe_of(f.exe),
        version: f.version,
    }))
}

pub fn watchdog_ping() {
    let _ = horch_platform::service::watchdog_ping();
}

pub fn integration_git(fleet_root: &Path, repos: &[PathBuf]) -> CliWorktreeGit {
    let git_dirs: Vec<PathBuf> = repos.iter().filter_map(|r| repo_git_dir(r).ok()).collect();
    CliWorktreeGit::new(git_bin(&BinOverrides::from_env(&ProcessEnv)), fleet_root)
        .reading(repos)
        .writing(&git_dirs)
        .with_time_limit(FLEETD_GIT_LIMIT)
}

pub fn judge_index_folder(fleet_root: &Path) -> PathBuf {
    fleet_root.join("tmp").join("judge-branch")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub progress_at: Option<String>,
    pub transcript_bytes: Option<i64>,
    pub transcript_mtime: Option<String>,
    pub herdr_seq: Option<i64>,
    pub timer_s: Option<i64>,
}

impl Activity {
    pub fn moved_since(&self, before: &Activity) -> bool {
        fn moved<T: PartialEq>(now: &Option<T>, before: &Option<T>) -> bool {
            now.is_some() && now != before
        }
        moved(&self.progress_at, &before.progress_at)
            || moved(&self.transcript_bytes, &before.transcript_bytes)
            || moved(&self.transcript_mtime, &before.transcript_mtime)
            || moved(&self.herdr_seq, &before.herdr_seq)
            || moved(&self.timer_s, &before.timer_s)
    }
}

pub fn activity_path(fleet_root: &Path) -> PathBuf {
    run_dir(fleet_root).join("activity.json")
}

pub fn read_activity(fleet_root: &Path) -> BTreeMap<String, Activity> {
    std::fs::read(activity_path(fleet_root))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn write_activity(fleet_root: &Path, seen: &BTreeMap<String, Activity>) -> Result<()> {
    write_json(&activity_path(fleet_root), seen)
}

pub fn doorbells_path(fleet_root: &Path) -> PathBuf {
    run_dir(fleet_root).join("doorbells.json")
}

pub fn read_doorbells(fleet_root: &Path) -> BTreeMap<String, BTreeSet<i64>> {
    std::fs::read(doorbells_path(fleet_root))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn write_doorbells(fleet_root: &Path, sent: &BTreeMap<String, BTreeSet<i64>>) -> Result<()> {
    write_json(&doorbells_path(fleet_root), sent)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let dir = path.parent().expect("a file of the run folder");
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let bytes = serde_json::to_vec_pretty(value)?;
    fsx::write_atomic(path, &bytes, MODE)?;
    Ok(())
}

#[derive(Debug)]
pub struct Watch {
    watcher: Option<Watcher>,
    watched: BTreeSet<PathBuf>,
}

impl Default for Watch {
    fn default() -> Self {
        Self::new()
    }
}

impl Watch {
    pub fn new() -> Watch {
        Watch {
            watcher: Watcher::new().ok(),
            watched: BTreeSet::new(),
        }
    }

    pub fn follow(&mut self, dirs: &[PathBuf]) {
        let Some(w) = self.watcher.as_mut() else {
            return;
        };
        let wanted: BTreeSet<&PathBuf> = dirs.iter().collect();
        self.watched.retain(|d| {
            let keep = wanted.contains(d);
            if !keep {
                w.remove(d);
            }
            keep
        });
        for d in dirs {
            if self.watched.len() >= WATCH_LIMIT {
                break;
            }
            if !self.watched.contains(d) && w.add(d).is_ok() {
                self.watched.insert(d.clone());
            }
        }
    }

    pub fn changed(&mut self) -> Vec<PathBuf> {
        self.watcher
            .as_mut()
            .map(Watcher::changed)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_attempt_id_has_its_own_launch_file() {
        let root = Path::new("/f");
        let a = launch_path(root, "T-1/A2");
        let b = launch_path(root, "T-1%2FA2");
        assert_eq!(a, Path::new("/f/run/launches/T-1%2FA2.json"));
        assert_ne!(a, b);
    }

    #[test]
    fn a_launch_record_that_does_not_parse_is_a_launch() {
        let dir = tempfile::tempdir().unwrap();
        let path = launch_path(dir.path(), "a");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{").unwrap();
        let r = read_launch(dir.path(), "a").unwrap().unwrap();
        assert_eq!(r.process, None);
    }
}
