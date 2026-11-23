use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Deserialize;

use crate::fleetd::fleet_runtime::adapters::fleetd_row;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::task_execution::domain::task::{GroupLeft, PaneProof, Recorded, Stopped};

const OLD_FILE_MAX_BYTES: u64 = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Polite {
    pub at: FleetSeconds,
    pub pane: Option<String>,
}

fn start_key(pid_start: u64) -> Result<i64> {
    i64::try_from(pid_start).with_context(|| format!("the start time {pid_start} is too large"))
}

pub fn get(conn: &Connection, pid: u32, pid_start: u64) -> Result<Option<Polite>> {
    conn.query_row(
        "SELECT at_s, pane FROM polite_stops WHERE pid = ?1 AND pid_start = ?2",
        params![pid, start_key(pid_start)?],
        |r| {
            Ok(Polite {
                at: FleetSeconds(r.get(0)?),
                pane: r.get(1)?,
            })
        },
    )
    .optional()
    .with_context(|| format!("reading the polite stop of process {pid}"))
}

pub fn record(
    conn: &mut Connection,
    guard: Generation,
    pid: u32,
    pid_start: u64,
    p: &Polite,
) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    fleetd_row::check_generation(&tx, guard)?;
    tx.execute(
        "INSERT OR REPLACE INTO polite_stops (pid, pid_start, at_s, pane) VALUES (?1, ?2, ?3, ?4)",
        params![pid, start_key(pid_start)?, p.at.0, p.pane],
    )?;
    tx.commit()
        .with_context(|| format!("recording the polite stop of process {pid}"))
}

fn rows(conn: &Connection) -> Result<Vec<(u32, u64, Option<String>)>> {
    let mut stmt =
        conn.prepare("SELECT pid, pid_start, pane FROM polite_stops ORDER BY pid, pid_start")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (pid, start, pane) = row?;
        if let (Ok(pid), Ok(start)) = (u32::try_from(pid), u64::try_from(start)) {
            out.push((pid, start, pane));
        }
    }
    Ok(out)
}

pub fn stopped(conn: &Connection, gone: impl Fn(u32, u64) -> bool) -> Result<Vec<Stopped>> {
    Ok(rows(conn)
        .context("reading the polite stops")?
        .into_iter()
        .filter(|&(pid, start, _)| gone(pid, start))
        .map(|(pid, start, pane)| Stopped {
            process: Recorded {
                pid,
                pid_start: Some(start),
            },
            proof: pane.map_or(PaneProof::None, PaneProof::Pane),
            group: GroupLeft::None,
        })
        .collect())
}

pub fn forget(
    conn: &mut Connection,
    guard: Generation,
    kept: impl Fn(u32, u64) -> bool,
) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    fleetd_row::check_generation(&tx, guard)?;
    for (pid, start, _) in rows(&tx)? {
        if !kept(pid, start) {
            tx.execute(
                "DELETE FROM polite_stops WHERE pid = ?1 AND pid_start = ?2",
                params![pid, start_key(start)?],
            )?;
        }
    }
    tx.commit()
        .context("deleting the polite stops of gone processes")
}

fn old_dir(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run").join("stops")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OldFile {
    polite_at: i64,
    #[serde(default)]
    pane: Option<OldPane>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OldPane {
    pane_id: String,
    shell_pid: u32,
    shell_start: u64,
}

fn old_key(name: &str) -> Option<(u32, u64)> {
    let (pid, start) = name.strip_suffix(".json")?.split_once('-')?;
    Some((pid.parse().ok()?, start.parse().ok()?))
}

fn read_old(path: &Path, (pid, start): (u32, u64)) -> std::result::Result<Polite, String> {
    let identity =
        horch_platform::fs::file_identity_at(path).map_err(|e| format!("not readable ({e})"))?;
    let meta = std::fs::symlink_metadata(path).map_err(|e| format!("not readable ({e})"))?;
    if !meta.file_type().is_file() {
        return Err("not a regular file".into());
    }
    let uid = horch_platform::fs::current_uid().map_err(|_| "owner unknown".to_string())?;
    let owner = horch_platform::fs::owner_and_mode(&meta).map_err(|_| "owner unknown")?;
    if owner.uid != uid {
        return Err(format!("owned by user {}, not by this user", owner.uid));
    }
    if meta.len() > OLD_FILE_MAX_BYTES {
        return Err(format!("larger than {OLD_FILE_MAX_BYTES} bytes"));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("not readable ({e})"))?;
    let opened =
        horch_platform::fs::file_identity_of(&file).map_err(|e| format!("not readable ({e})"))?;
    if opened != identity {
        return Err("another file replaced it while it was read".into());
    }
    let mut bytes = Vec::new();
    file.take(OLD_FILE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("not readable ({e})"))?;
    if bytes.len() as u64 > OLD_FILE_MAX_BYTES {
        return Err(format!("larger than {OLD_FILE_MAX_BYTES} bytes"));
    }
    let old: OldFile =
        serde_json::from_slice(&bytes).map_err(|_| "not a record of a polite stop".to_string())?;
    let pane = match old.pane {
        None => None,
        Some(p) if p.shell_pid == pid && p.shell_start == start => Some(p.pane_id),
        Some(_) => return Err("its pane names another process".into()),
    };
    Ok(Polite {
        at: FleetSeconds(old.polite_at),
        pane,
    })
}

pub fn import_old_files(
    conn: &mut Connection,
    guard: Generation,
    fleet_root: &Path,
) -> Result<Vec<String>> {
    let dir = old_dir(fleet_root);
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut files = Vec::new();
    for entry in entries {
        let path = entry
            .with_context(|| format!("reading {}", dir.display()))?
            .path();
        let key = path.file_name().and_then(|n| n.to_str()).and_then(old_key);
        let read = match key {
            Some(key) => read_old(&path, key).map(|p| (key, p)),
            None => Err("not named <pid>-<start>.json".into()),
        };
        files.push((path, read));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    fleetd_row::check_generation(&tx, guard)?;
    for ((pid, start), p) in files.iter().filter_map(|(_, r)| r.as_ref().ok()) {
        tx.execute(
            "INSERT OR IGNORE INTO polite_stops (pid, pid_start, at_s, pane) VALUES (?1, ?2, ?3, ?4)",
            params![pid, start_key(*start)?, p.at.0, p.pane],
        )?;
    }
    tx.commit()
        .context("writing the rows of the old polite stop files")?;
    let mut log = Vec::new();
    for (path, read) in files {
        let shown = path.display();
        if let Err(reason) = read {
            log.push(format!(
                "the old record of a polite stop {shown} fails a check: {reason}. fleetd \
                 removed it, and its process gets a new grace"
            ));
        }
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                log.push(format!(
                    "removing the old record of a polite stop {shown}: {e}"
                ));
            }
            _ => {}
        }
    }
    match std::fs::remove_dir(&dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            log.push(format!("removing the folder {}: {e}", dir.display()));
        }
        _ => {}
    }
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_key_reads_pid_and_start() {
        assert_eq!(old_key("12-345.json"), Some((12, 345)));
        assert_eq!(old_key("12-345.json.tmp"), None);
        assert_eq!(old_key("x-345.json"), None);
    }
}
