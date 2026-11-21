use anyhow::Result;
use horch_platform::{group, process};
use rusqlite::Connection;

use crate::fleetd::task_execution::adapters::sqlite::{self as attempts_store, AttemptProcesses};

pub const MAX_CHAIN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fact {
    Yes(Vec<String>),
    No,
    Unknown,
}

pub const ENDED_COUNTS_S: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetProcess {
    pub attempt: String,
    pub pid: u32,
    pub start: u64,
}

pub fn recorded_processes(
    conn: &Connection,
) -> Result<Option<(Vec<FleetProcess>, Vec<AttemptProcesses>)>> {
    let since = crate::clock::now() - chrono::Duration::seconds(ENDED_COUNTS_S);
    let rows = attempts_store::fleet_processes(conn, &crate::clock::stamp(since))?;
    Ok(processes_of(&rows).map(|out| (out, rows)))
}

pub fn processes_of(rows: &[AttemptProcesses]) -> Option<Vec<FleetProcess>> {
    let mut out = Vec::new();
    for row in rows {
        for p in &row.processes {
            match p.pid_start {
                Some(start) => out.push(FleetProcess {
                    attempt: row.attempt.clone(),
                    pid: p.pid,
                    start,
                }),
                None if !row.ended => return None,
                None => {}
            }
        }
    }
    Some(out)
}

pub trait ProcessFacts {
    fn parent(&self, pid: u32) -> Option<u32>;
    fn start_time(&self, pid: u32) -> Option<u64>;
    fn in_group(&self, pid: u32, leader: u32, leader_start: u64) -> Option<bool>;
    fn group_of(&self, _pid: u32) -> Option<u32> {
        None
    }
    fn session_of(&self, _pid: u32) -> Option<u32> {
        None
    }
}

pub struct PlatformFacts;

impl ProcessFacts for PlatformFacts {
    fn parent(&self, pid: u32) -> Option<u32> {
        process::parent(pid).ok().flatten()
    }

    fn start_time(&self, pid: u32) -> Option<u64> {
        process::start_time(pid)
    }

    fn in_group(&self, pid: u32, leader: u32, leader_start: u64) -> Option<bool> {
        if !group::is_same_group(leader, leader_start).ok()? {
            return Some(false);
        }
        let members = process::group_members(leader).ok()?;
        if members.is_empty() {
            return None;
        }
        Some(members.contains(&pid))
    }

    fn group_of(&self, pid: u32) -> Option<u32> {
        process::group_of(pid).ok().flatten()
    }

    fn session_of(&self, pid: u32) -> Option<u32> {
        process::session_of(pid).ok().flatten()
    }
}

pub fn fleet_process(peer: u32, recorded: &[FleetProcess], facts: &dyn ProcessFacts) -> Fact {
    if recorded.is_empty() {
        return Fact::No;
    }
    let mut found: Vec<String> = Vec::new();
    let mut chain = vec![peer];
    for _ in 0..4 * MAX_CHAIN {
        if chain.len() > MAX_CHAIN {
            return unknown_unless_found(found);
        }
        let Some(&pid) = chain.last() else {
            return unknown_unless_found(found);
        };
        if pid <= 1 {
            return verdict(found);
        }
        match read_one(pid, recorded, facts, &mut found) {
            Some(parent) => chain.push(parent),
            None => {
                if !step_back(&mut chain, facts) {
                    return unknown_unless_found(found);
                }
            }
        }
    }
    unknown_unless_found(found)
}

fn unknown_unless_found(found: Vec<String>) -> Fact {
    if found.is_empty() {
        Fact::Unknown
    } else {
        Fact::Yes(found)
    }
}

fn read_one(
    pid: u32,
    recorded: &[FleetProcess],
    facts: &dyn ProcessFacts,
    found: &mut Vec<String>,
) -> Option<u32> {
    let mut add = |id: &str| {
        if !found.iter().any(|f| f == id) {
            found.push(id.to_string());
        }
    };
    for r in recorded.iter().filter(|r| r.pid == pid) {
        if r.start == facts.start_time(pid)? {
            add(&r.attempt);
        }
    }
    let group = facts.group_of(pid)?;
    let session = facts.session_of(pid)?;
    for r in recorded
        .iter()
        .filter(|r| r.pid == group || r.pid == session)
    {
        if still_names(r, facts) {
            add(&r.attempt);
        }
    }
    facts.parent(pid)
}

fn step_back(chain: &mut Vec<u32>, facts: &dyn ProcessFacts) -> bool {
    let Some(mut failed) = chain.pop() else {
        return false;
    };
    while let Some(&child) = chain.last() {
        match facts.parent(child) {
            Some(parent) if parent != failed => {
                chain.push(parent);
                return true;
            }
            Some(_) => return false,
            None => {
                failed = child;
                chain.pop();
            }
        }
    }
    false
}

fn still_names(r: &FleetProcess, facts: &dyn ProcessFacts) -> bool {
    facts.start_time(r.pid).is_none_or(|start| start == r.start)
}

fn verdict(found: Vec<String>) -> Fact {
    if found.is_empty() {
        Fact::No
    } else {
        Fact::Yes(found)
    }
}
