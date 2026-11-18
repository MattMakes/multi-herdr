//! The effects of a pass on processes, panes and files.
//!
//! Part 3 of a pass does these effects after the commit of part 2. Each is
//! short by its nature, and each is keyed by the id of its row, so it is
//! safe to repeat (`docs/specs/fleetd/design.md`, "The reconcile pass"):
//!
//! - [`stop_process`]: a polite stop or a kill, only while the process has
//!   its recorded pid and start time.
//! - [`launch_attempt`]: the launch of an attempt in `launching`, at most
//!   once for each attempt id. Its launch record
//!   `<fleet root>/run/launches/<attempt>.json` carries the attempt id, the
//!   time of the launch on the fleet clock and the process. The record is
//!   written before the process starts, so a fleetd that dies within a
//!   launch leaves a record, and no later pass launches the attempt again.
//! - [`write_polite`]: the time of a polite stop, so that the kill comes
//!   when its grace ends, also after a restart of fleetd. When the process
//!   is the process in the pane of its row, the record also holds the pane:
//!   the proof with which a later pass closes the pane after the process
//!   ended on the polite stop ([`read_stopped`]). The proof is this file,
//!   not a ledger row.
//! - [`write_heartbeat`]: `<fleet root>/run/heartbeat.json`, written
//!   atomically after each full pass. [`read_heartbeat`] reads it for
//!   `horch fleetd check`.
//! - [`watchdog_ping`]: the systemd watchdog.
//!
//! [`Watch`] is the file watching of part 1: a hint that puts a changed
//! folder first in its scan.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use horch_platform::signal::{stop_same, Stop as How};
use horch_platform::watch::Watcher;
use serde::{Deserialize, Serialize};

use crate::exeid::{exe_identity, ExeIdentity};
use crate::fleetd::fleet_runtime::domain::clock::{AwakeReading, FleetSeconds};
use crate::fleetd::fleet_runtime::domain::heartbeat::{Exe, Heartbeat};
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::task_execution::domain::task::{PaneProof, ProcessScope, Recorded, Stopped};
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, Launched, ProcessHost};
use crate::fsx;

/// The file mode of the files of this module.
const MODE: u32 = 0o644;

/// The most folders that [`Watch`] watches at once. A watch on macOS holds
/// 1 open file for each folder, and the default soft limit of open files
/// there is 256, so fleetd keeps most of its open files for the ledger and
/// its processes. A folder past the limit is still scanned in turn.
pub const WATCH_LIMIT: usize = 64;

fn run_dir(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run")
}

// ─── stops ──────────────────────────────────────────────────────────────

/// Stop the process `pid` with the start time `pid_start`: the polite stop
/// that it can catch, or with `kill` the kill. With
/// [`ProcessScope::LeaderGroup`], the stop goes to each process of the group
/// of `pid` while `pid` leads it with that start. Whether a stop was sent.
///
/// Windows has no polite stop and no process groups: there each stop ends
/// the process at once, and the processes that it started live on
/// (`docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6).
pub fn stop_process(pid: u32, pid_start: u64, scope: ProcessScope, kill: bool) -> bool {
    use horch_platform::group;
    let how = if kill { How::Kill } else { How::Term };
    let leads = scope == ProcessScope::LeaderGroup
        && matches!(group::is_same_group(pid, pid_start), Ok(true));
    if !leads {
        return stop_same(pid, pid_start, how);
    }
    if kill {
        return group::kill_group_id(pid).unwrap_or(false);
    }
    let members = horch_platform::process::group_members(pid).unwrap_or_else(|_| vec![pid]);
    let mut sent = false;
    for member in members {
        if let Some(start) = horch_platform::process::start_time(member) {
            sent |= stop_same(member, start, how);
        }
    }
    sent
}

// ─── launches ───────────────────────────────────────────────────────────

/// The launch of 1 attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRecord {
    pub attempt: String,
    /// The time of the launch on the fleet clock: the start timeout counts
    /// from it.
    pub launched_at: FleetSeconds,
    /// The process; `None` while the launch is in progress, or when it
    /// failed or fleetd died within it.
    pub process: Option<Recorded>,
}

#[derive(Serialize, Deserialize)]
struct LaunchFile {
    attempt: String,
    launched_at: i64,
    pid: Option<u32>,
    pid_start: Option<u64>,
}

/// The launch record of `attempt`. Each byte of the id outside
/// `[A-Za-z0-9._-]` is written as `%XX`, so each id has its own file name
/// in the 1 folder.
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

/// The launch record of `attempt`; `None` when it has none. A record that
/// does not parse is a launch with no process: the attempt was launched.
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

/// Write the launch record `r` atomically.
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

/// Remove the launch record of `attempt`, when its attempt is no longer
/// in `launching`. A missing record is not an error.
pub fn remove_launch(fleet_root: &Path, attempt: &str) -> io::Result<()> {
    match std::fs::remove_file(launch_path(fleet_root, attempt)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Launch the attempt `attempt` at `clock`, once: `None` when it has a
/// launch record. Else write the record, take the spec from `spec`, start
/// the process through `host`, and write the record again with the
/// process. A failure after the first write leaves the record with no
/// process, and the start timeout ends the attempt.
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
        // No record names the process, so no later pass can find or stop
        // it: stop it now. The start timeout ends the attempt.
        horch_platform::group::kill_child_group(&mut launched.child);
        let _ = launched.child.wait();
        return Err(e.context(format!("recording the process of attempt {attempt}")));
    }
    Ok(Some(launched))
}

// ─── polite stops ───────────────────────────────────────────────────────

/// The record of the polite stop of the process `pid` with the start time
/// `pid_start`: `<fleet root>/run/stops/<pid>-<start>.json`. It keeps the
/// time of the polite stop over a restart of fleetd, so the kill comes when
/// the grace ends.
pub fn polite_path(fleet_root: &Path, pid: u32, pid_start: u64) -> PathBuf {
    stops_dir(fleet_root).join(format!("{pid}-{pid_start}.json"))
}

fn stops_dir(fleet_root: &Path) -> PathBuf {
    run_dir(fleet_root).join("stops")
}

/// The record of 1 polite stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Polite {
    /// The time of the polite stop, on the fleet clock.
    pub at: FleetSeconds,
    /// The pane that the process was in at the polite stop, by the pid and
    /// start time of the process in the pane.
    pub pane: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct PoliteFile {
    polite_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pane: Option<PaneFile>,
}

/// The proof of a pane: herdr named the process `shell_pid` with the start
/// time `shell_start` as the process in the pane `pane_id`. herdr 0.9.4
/// gives no identity of its server, so the record holds none.
#[derive(Serialize, Deserialize)]
struct PaneFile {
    pane_id: String,
    shell_pid: u32,
    shell_start: u64,
}

/// The record `bytes` of the polite stop of the process `pid` with the
/// start time `pid_start`. `Err` when it does not parse, or when its pane
/// names another process.
fn parse_polite(bytes: &[u8], pid: u32, pid_start: u64) -> std::result::Result<Polite, ()> {
    let f = serde_json::from_slice::<PoliteFile>(bytes).map_err(|_| ())?;
    let pane = match f.pane {
        None => None,
        Some(p) if p.shell_pid == pid && p.shell_start == pid_start => Some(p.pane_id),
        Some(_) => return Err(()),
    };
    Ok(Polite {
        at: FleetSeconds(f.polite_at),
        pane,
    })
}

/// The record of the polite stop of the process; `None` when it got none.
/// A record that does not parse counts as none.
pub fn read_polite(fleet_root: &Path, pid: u32, pid_start: u64) -> io::Result<Option<Polite>> {
    match std::fs::read(polite_path(fleet_root, pid, pid_start)) {
        Ok(b) => Ok(parse_polite(&b, pid, pid_start).ok()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Record the polite stop `p` of the process. The record is written to a
/// temp file, synced and renamed, before the stop is sent.
pub fn write_polite(fleet_root: &Path, pid: u32, pid_start: u64, p: &Polite) -> Result<()> {
    let pane = p.pane.as_ref().map(|pane_id| PaneFile {
        pane_id: pane_id.clone(),
        shell_pid: pid,
        shell_start: pid_start,
    });
    write_json(
        &polite_path(fleet_root, pid, pid_start),
        &PoliteFile {
            polite_at: p.at.0,
            pane,
        },
    )
}

/// The record of each polite stop whose process is `gone(pid, start)`. A
/// record that cannot be read or does not parse proves no pane.
pub fn read_stopped(
    fleet_root: &Path,
    gone: impl Fn(u32, u64) -> bool,
) -> io::Result<Vec<Stopped>> {
    let entries = match std::fs::read_dir(stops_dir(fleet_root)) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some((pid, start)) = polite_key(&path).filter(|&(p, s)| gone(p, s)) else {
            continue;
        };
        let parsed = std::fs::read(&path)
            .map_err(|_| ())
            .and_then(|b| parse_polite(&b, pid, start));
        let proof = match parsed {
            Ok(Polite { pane: Some(p), .. }) => PaneProof::Pane(p),
            Ok(Polite { pane: None, .. }) => PaneProof::None,
            Err(()) => PaneProof::Unreadable,
        };
        out.push(Stopped {
            process: Recorded {
                pid,
                pid_start: Some(start),
            },
            proof,
        });
    }
    out.sort_by_key(|s| s.process);
    Ok(out)
}

/// Remove the record of each polite stop for which `kept(pid, start)` is
/// false: the process is gone, or no row names it. A file whose name is not
/// a record of this module stays. Gives the path of each removed record
/// that did not parse, for the fleetd log.
pub fn forget_polite(
    fleet_root: &Path,
    kept: impl Fn(u32, u64) -> bool,
) -> io::Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(stops_dir(fleet_root)) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut unreadable = Vec::new();
    for entry in entries {
        let path = entry?.path();
        let Some((pid, start)) = polite_key(&path) else {
            continue;
        };
        if kept(pid, start) {
            continue;
        }
        let parses = std::fs::read(&path)
            .ok()
            .is_some_and(|b| parse_polite(&b, pid, start).is_ok());
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            Err(_) => {}
            Ok(()) if !parses => unreadable.push(path),
            Ok(()) => {}
        }
    }
    Ok(unreadable)
}

/// The pid and start time of the record at `path`, from its name
/// `<pid>-<start>.json`.
fn polite_key(path: &Path) -> Option<(u32, u64)> {
    let name = path.file_name()?.to_str()?.strip_suffix(".json")?;
    let (pid, start) = name.split_once('-')?;
    Some((pid.parse().ok()?, start.parse().ok()?))
}

// ─── the heartbeat ──────────────────────────────────────────────────────

/// `<fleet root>/run/heartbeat.json`.
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

/// The [`Exe`] of the identity `id`.
pub fn exe_of(id: ExeIdentity) -> Exe {
    Exe {
        path: id.path,
        len: id.len,
        mtime_ns: id.mtime_ns,
    }
}

/// The binary of this process now. A binary that cannot be read has an
/// empty identity.
pub fn this_exe() -> Exe {
    let id = std::env::current_exe()
        .ok()
        .and_then(|p| exe_identity(&p))
        .unwrap_or_default();
    exe_of(id)
}

/// Write `hb` to the heartbeat file atomically.
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

/// The heartbeat file; `None` when fleetd never wrote one.
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

/// Tell the watchdog of the service manager that fleetd is alive. macOS and
/// Windows have none: there `horch fleetd check` reads the heartbeat. A
/// failed ping is not an error of the pass: the watchdog restarts a fleetd
/// that stops its pings.
pub fn watchdog_ping() {
    let _ = horch_platform::service::watchdog_ping();
}

// ─── activity ───────────────────────────────────────────────────────────

/// The activity signals of 1 live attempt, as 1 pass saw them
/// (`docs/tickets/fleetd-p2/MAP.md`, Q7). The next pass compares its
/// signals with these: a signal that moved is activity.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    /// The last progress that the agent recorded itself.
    pub progress_at: Option<String>,
    pub transcript_bytes: Option<i64>,
    pub transcript_mtime: Option<String>,
    /// The state counter of herdr for the pane.
    pub herdr_seq: Option<i64>,
    /// The seconds of the working timer on the screen.
    pub timer_s: Option<i64>,
}

impl Activity {
    /// Whether a signal of `self` moved since `before`. A signal that the
    /// pass did not read is no move.
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

/// `<fleet root>/run/activity.json`: the [`Activity`] of each live attempt
/// at the last pass, by attempt id. A file, not a ledger row, so that
/// `horch ledger watch --once` compares with the pass before it.
pub fn activity_path(fleet_root: &Path) -> PathBuf {
    run_dir(fleet_root).join("activity.json")
}

/// The activity file. A missing file, or a file that does not parse, is
/// no record: the next pass sees no activity for 1 pass.
pub fn read_activity(fleet_root: &Path) -> BTreeMap<String, Activity> {
    std::fs::read(activity_path(fleet_root))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Write the activity file atomically.
pub fn write_activity(fleet_root: &Path, seen: &BTreeMap<String, Activity>) -> Result<()> {
    write_json(&activity_path(fleet_root), seen)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let dir = path.parent().expect("a file of the run folder");
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let bytes = serde_json::to_vec_pretty(value)?;
    fsx::write_atomic(path, &bytes, MODE)?;
    Ok(())
}

// ─── file watching ──────────────────────────────────────────────────────

/// The file watching of the scans: at most [`WATCH_LIMIT`] folders. With
/// no watcher of the OS, it reports nothing, and the scans go on in turn.
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

    /// Watch the folders of `dirs`, and no other. A folder that does not
    /// exist yet is tried again on the next call.
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

    /// The watched folders that changed since the last call.
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
