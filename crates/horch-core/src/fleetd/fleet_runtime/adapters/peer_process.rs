//! The peer process fact: is the process of a channel peer in a live
//! attempt (its process group, its job object, or a descendant of its
//! wrapper): yes, no or unknown.
//!
//! Requirements: SR-8. fleetd refuses an `operator` request on the channel
//! from a peer in a live attempt, and from a peer whose fact is unknown (a
//! guard whose fact is unknown does not hold).
//!
//! - Yes: the peer is in the process group that the wrapper of a live
//!   attempt leads, or the peer or a parent in its chain is that wrapper.
//! - No: the chain reaches PID 1, or a process with no parent, with each
//!   fact read, and no group and no process of it is of a live attempt.
//! - Unknown: a parent or a group fact is `NotSupported` or fails to read,
//!   the start time of a process with the PID of a wrapper fails to read, or
//!   the chain is longer than [`MAX_CHAIN`].
//!
//! A PID can go to a later process. Each compare with a wrapper uses the PID
//! and the start time.
//!
//! A process that leaves the group of its attempt and loses its parent chain
//! (an orphan in a new group) answers no. The worker sandbox, which denies
//! each write in `<fleet root>/run/`, and the operator token guard that case.
//!
//! Windows gap: the platform gives no job object fact and no parent PID on
//! Windows (`NotSupported`). So on Windows each answer is unknown, and
//! fleetd refuses each `operator` request on the channel (Q11 of
//! `docs/tickets/fleetd-p5/MAP.md`). The request file path, with the token,
//! still works for the operator on Windows.

use horch_platform::{group, process};

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

/// The process facts of 1 live attempt, from the ledger: the PID and the
/// start time of its wrapper. The launcher starts the wrapper as the leader
/// of a new process group, so that group is the group of the attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptProcess {
    pub attempt: String,
    pub wrapper_pid: u32,
    /// A start time of [`horch_platform::process::start_time`].
    pub wrapper_start: u64,
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
}

/// Is the process `peer` in a live attempt of `attempts`.
pub fn peer_in_live_attempt(
    peer: u32,
    attempts: &[AttemptProcess],
    facts: &dyn ProcessFacts,
) -> Fact {
    let mut found: Vec<String> = Vec::new();
    for a in attempts {
        match facts.in_group(peer, a.wrapper_pid, a.wrapper_start) {
            None => return Fact::Unknown,
            Some(true) => found.push(a.attempt.clone()),
            Some(false) => {}
        }
    }
    let mut pid = peer;
    for _ in 0..MAX_CHAIN {
        let wrappers = attempts.iter().filter(|a| a.wrapper_pid == pid);
        for a in wrappers {
            let Some(start) = facts.start_time(pid) else {
                return Fact::Unknown;
            };
            if a.wrapper_start == start && !found.contains(&a.attempt) {
                found.push(a.attempt.clone());
            }
        }
        if pid == 1 {
            return verdict(found);
        }
        match facts.parent(pid) {
            None => return Fact::Unknown,
            Some(0) => return verdict(found),
            Some(parent) => pid = parent,
        }
    }
    Fact::Unknown
}

fn verdict(found: Vec<String>) -> Fact {
    if found.is_empty() {
        Fact::No
    } else {
        Fact::Yes(found)
    }
}
