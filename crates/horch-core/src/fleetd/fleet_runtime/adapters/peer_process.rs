//! The fleet process fact (FR-10, SR-8): is a process a process of a fleet
//! attempt: yes, no or unknown. [`fleet_process`] is the 1 rule. The CLI
//! guard of each `operator` command and the peer process check of fleetd
//! both use it.
//!
//! The facts are facts of the OS that fleetd recorded, never an environment
//! variable: the PID and the start time of the wrapper and of the agent of
//! each attempt that is not ended, or that ended in the last
//! [`ENDED_COUNTS_S`]. A pane of a worker enters as its process: the pane
//! shell runs `exec horch worker`, so the pane process is the wrapper.
//!
//! - Yes: a process of the chain of the caller is a recorded process (PID
//!   and start time), or is in the process group or the session that a
//!   recorded process made. So a child shell of the agent, a child in a new
//!   group or a new session whose parent runs, and an agent that lives on
//!   after its attempt ended are each a fleet process.
//! - No: the chain reaches PID 1, or a process with no parent, with each
//!   fact read, and no fact matches.
//! - Unknown: a parent, a group, a session or a start time fails to read
//!   for a process that did not end, or the chain is longer than
//!   [`MAX_CHAIN`], and no process read before it matches. The caller
//!   refuses. A process of the chain that ends
//!   during the walk is not unknown: the walk goes on from the new parent
//!   of its child.
//!
//! A PID can go to a later process. Each compare uses the PID and the start
//! time, also for an ended attempt.
//!
//! Gap: a process that makes a new group and a new session and whose parent
//! then ends (a double fork) has no fact that ties it to its attempt. The
//! worker sandbox, which denies the operator token and the command channel,
//! guards that case.
//!
//! Windows gap: the platform gives no parent PID, no group and no session
//! on Windows (`NotSupported`). So on Windows each answer is unknown when
//! the ledger records a fleet process, and fleetd refuses each `operator`
//! request on the channel (Q11 of `docs/tickets/fleetd-p5/MAP.md`). The
//! request file path, with the token, still works for the operator on
//! Windows.

use anyhow::Result;
use horch_platform::{group, process};
use rusqlite::Connection;

use crate::fleetd::task_execution::adapters::sqlite::{self as attempts_store, AttemptProcesses};

/// The most processes of a chain that the walk reads, the peer included.
pub const MAX_CHAIN: usize = 64;

/// The answer: is the peer in a live attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fact {
    /// The ids of each live attempt that holds the peer, never empty: first
    /// the attempts whose group holds the peer, in the order of the input,
    /// then the attempts of the wrappers in its chain, nearest first.
    Yes(Vec<String>),
    No,
    Unknown,
}

/// How long an ended attempt counts for [`fleet_process`], from its end:
/// 24 hours. The stop ladder stops each process of an attempt in seconds,
/// so a process that lives on after its attempt ended is found in this
/// time. The bound also stops an old PID of a long-ended attempt from
/// counting against a later process for ever.
pub const ENDED_COUNTS_S: i64 = 24 * 60 * 60;

/// 1 recorded process of a fleet attempt, from the ledger: the wrapper or
/// the agent of an attempt that is not ended, or that ended in the last
/// [`ENDED_COUNTS_S`]. The launcher starts the wrapper as the leader of a
/// new process group, and a herdr pane starts it as the leader of a
/// session, so its PID also names its group and its session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetProcess {
    pub attempt: String,
    pub pid: u32,
    /// A start time of [`horch_platform::process::start_time`].
    pub start: u64,
}

/// The recorded processes of the fleet for [`fleet_process`], read now,
/// with the rows that hold them. `None` when an attempt that is not ended
/// has a process with no start time: its fact is unknown.
pub fn recorded_processes(
    conn: &Connection,
) -> Result<Option<(Vec<FleetProcess>, Vec<AttemptProcesses>)>> {
    let since = crate::clock::now() - chrono::Duration::seconds(ENDED_COUNTS_S);
    let rows = attempts_store::fleet_processes(conn, &crate::clock::stamp(since))?;
    Ok(processes_of(&rows).map(|out| (out, rows)))
}

/// The recorded processes of `rows` for [`fleet_process`]. `None` when an
/// attempt that is not ended has a process with no start time. A process
/// of an ended attempt with no start time cannot be compared, so it does
/// not count.
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

/// FR-10: the first attempt of `rows` that is not ended and has no recorded
/// process. Its agent can run before fleetd records its wrapper, and then
/// no fact ties the agent to its attempt: [`fleet_process`] gives `No` for
/// it. So an approval from a peer that is not proven inside an attempt is
/// refused while this gives `Some`. The start timeout
/// [`START_TIMEOUT_S`](crate::fleetd::fleet_runtime::domain::pass::START_TIMEOUT_S)
/// ends a launch with no process.
pub fn launching_attempt(rows: &[AttemptProcesses]) -> Option<&str> {
    rows.iter()
        .find(|r| !r.ended && r.processes.is_empty())
        .map(|r| r.attempt.as_str())
}

/// The process facts that the answer reads. `None` is a fact that is
/// `NotSupported` or that fails to read.
pub trait ProcessFacts {
    /// The parent PID of `pid`. 0 when `pid` has no parent.
    fn parent(&self, pid: u32) -> Option<u32>;
    /// The start time of `pid`.
    fn start_time(&self, pid: u32) -> Option<u64>;
    /// Whether `pid` is in the process group that `leader` leads with the
    /// start time `leader_start`. `Some(false)` when `leader` does not lead
    /// that group.
    fn in_group(&self, pid: u32, leader: u32, leader_start: u64) -> Option<bool>;
    /// The process group of `pid`. The default is a fact that is not
    /// supported.
    fn group_of(&self, _pid: u32) -> Option<u32> {
        None
    }
    /// The session of `pid`. The default is a fact that is not supported.
    fn session_of(&self, _pid: u32) -> Option<u32> {
        None
    }
}

/// The process facts of the OS, from `horch_platform`.
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
        // The group is the same, so it has a member: an empty list is a
        // read that failed.
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

/// FR-10, SR-8: is the process `peer` a fleet process. The CLI guard of
/// each `operator` command and the peer process check of fleetd both use
/// this rule; the check of fleetd is the one that counts.
///
/// The walk reads the chain of `peer` up to PID 1. Each process of the
/// chain is a fleet process when:
/// - its PID and start time are those of a process of `recorded`, or
/// - its process group or its session has the PID of a process of
///   `recorded`, and that PID still names that process (see
///   [`still_names`]).
///
/// A fact that cannot be read is [`Fact::Unknown`]: the caller refuses.
/// A process of the chain can end during the walk (the stop ladder stops
/// its group), and then a read of it fails. The OS gives the children of a
/// process that ends to another parent at once. So when a read fails, the
/// walk reads the parent of the child again: another parent proves that
/// the process ended, and the walk goes on from the new parent, as a later
/// walk would. When the child ended too, the walk goes 1 more step back.
/// The same parent, or a peer whose read fails, gives [`Fact::Unknown`].
/// A read adds a match and never removes one. So a fact that cannot be read
/// after a match does not change the answer: it is [`Fact::Yes`] with the
/// matches found.
/// With no recorded process, no process can be a fleet process.
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

/// Whether the group or the session with the id `r.pid`, which holds a
/// process of the chain, is the one that `r` made. While a group or a
/// session has a member, the OS gives its id to no new process. So a
/// process with that PID is the one that made it: compare its start time.
/// When no start time can be read, the leader has ended and the group or
/// the session lives on: it is the one of `r`. A start time that fails to
/// read for a process that runs also gives `true`: the rule refuses.
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
