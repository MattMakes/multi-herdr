//! The fleet process fact (ticket 11 of fleetd-p5, SR-8; FR-10) with real
//! child processes: is the peer a fleet process, by the group or by the
//! parent chain of its wrapper.
#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use horch_core::fleetd::fleet_runtime::adapters::peer_process::{
    fleet_process, Fact, FleetProcess, PlatformFacts, ProcessFacts,
};
use horch_platform::{group, process};

/// A wrapper `sh` that leads its own group, as the launcher starts it, with
/// the pid of the process that it printed first. Drop kills both.
struct Wrapper {
    child: Child,
    printed: u32,
}

impl Wrapper {
    /// Start `sh -c <script> <arg>`. The script prints 1 pid on its first
    /// line.
    fn start(script: &str, arg: &str) -> Wrapper {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script, arg]).stdout(Stdio::piped());
        group::new_group(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let printed = line.trim().parse().unwrap();
        Wrapper { child, printed }
    }

    /// A wrapper that starts `sleep` as its child.
    fn with_sleep() -> Wrapper {
        Wrapper::start("sleep 30 & echo $!; wait", "")
    }

    fn attempt(&self, id: &str) -> FleetProcess {
        let pid = self.child.id();
        FleetProcess {
            attempt: id.to_string(),
            pid,
            start: process::start_time(pid).unwrap(),
        }
    }
}

impl Drop for Wrapper {
    fn drop(&mut self) {
        kill(self.printed);
        group::kill_child_group(&mut self.child);
        let _ = self.child.wait();
    }
}

fn kill(pid: u32) {
    // SAFETY: plain syscall on a pid that this test started.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
}

/// The platform facts, except that the parent of 1 pid cannot be read.
struct UnreadableParent {
    of: u32,
}

impl ProcessFacts for UnreadableParent {
    fn parent(&self, pid: u32) -> Option<u32> {
        if pid == self.of {
            return None;
        }
        PlatformFacts.parent(pid)
    }

    fn start_time(&self, pid: u32) -> Option<u64> {
        PlatformFacts.start_time(pid)
    }

    fn in_group(&self, pid: u32, leader: u32, leader_start: u64) -> Option<bool> {
        PlatformFacts.in_group(pid, leader, leader_start)
    }

    fn group_of(&self, pid: u32) -> Option<u32> {
        PlatformFacts.group_of(pid)
    }

    fn session_of(&self, pid: u32) -> Option<u32> {
        PlatformFacts.session_of(pid)
    }
}

fn yes(id: &str) -> Fact {
    Fact::Yes(vec![id.to_string()])
}

#[test]
fn child_of_the_wrapper_is_in_the_attempt() {
    let w = Wrapper::with_sleep();
    let attempts = [w.attempt("A-1")];
    assert_eq!(
        fleet_process(w.printed, &attempts, &PlatformFacts),
        yes("A-1")
    );
}

#[test]
fn grandchild_in_another_group_is_in_the_attempt_by_its_parent_chain() {
    // The child of the wrapper leads a new group, then starts `sleep`.
    const PERL: &str = r#"$| = 1; setpgrp(0, 0); my $p = fork(); if ($p == 0) { exec("sleep", "30"); } print "$p\n"; waitpid($p, 0);"#;
    let w = Wrapper::start(r#"perl -e "$0" & wait"#, PERL);
    let grandchild = w.printed;
    let middle = process::parent(grandchild).unwrap().unwrap();
    assert_eq!(process::parent(middle).unwrap(), Some(w.child.id()));
    assert!(!process::group_members(w.child.id())
        .unwrap()
        .contains(&grandchild));
    let attempts = [w.attempt("A-2")];
    let got = fleet_process(grandchild, &attempts, &PlatformFacts);
    kill(middle);
    assert_eq!(got, yes("A-2"));
}

#[test]
fn process_outside_each_attempt_is_not() {
    let w = Wrapper::with_sleep();
    let outside = Wrapper::with_sleep();
    let attempts = [w.attempt("A-3")];
    assert_eq!(
        fleet_process(outside.printed, &attempts, &PlatformFacts),
        Fact::No
    );
}

#[test]
fn reused_pid_with_another_start_time_is_not_the_wrapper() {
    let w = Wrapper::with_sleep();
    let mut attempt = w.attempt("A-4");
    attempt.start += 1;
    assert_eq!(
        fleet_process(w.printed, &[attempt], &PlatformFacts),
        Fact::No
    );
}

#[test]
fn unreadable_parent_is_unknown() {
    let w = Wrapper::with_sleep();
    let outside = Wrapper::with_sleep();
    let attempts = [w.attempt("A-5")];
    let facts = UnreadableParent {
        of: outside.child.id(),
    };
    assert_eq!(
        fleet_process(outside.printed, &attempts, &facts),
        Fact::Unknown
    );
}

#[test]
fn orphan_in_the_group_of_an_ended_wrapper_is_a_fleet_process() {
    // The wrapper starts `sleep` in its group and exits: `sleep` lives on
    // with no parent chain to the wrapper, as an agent that outlives its
    // attempt.
    let mut w = Wrapper::start("sleep 30 & echo $!", "");
    let attempt = w.attempt("A-6");
    let _ = w.child.wait();
    assert_eq!(
        process::group_of(w.printed).unwrap(),
        Some(attempt.pid),
        "sleep stays in the group of the wrapper"
    );
    assert_eq!(
        fleet_process(w.printed, &[attempt], &PlatformFacts),
        yes("A-6")
    );
}

#[test]
fn no_recorded_process_is_no_fleet_process() {
    let outside = Wrapper::with_sleep();
    assert_eq!(
        fleet_process(outside.printed, &[], &PlatformFacts),
        Fact::No
    );
}
