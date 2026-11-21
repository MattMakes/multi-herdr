#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use horch_core::fleetd::fleet_runtime::adapters::peer_process::{
    fleet_process, Fact, FleetProcess, PlatformFacts, ProcessFacts,
};
use horch_platform::{group, process};

struct Wrapper {
    child: Child,
    printed: u32,
}

impl Wrapper {
    fn start(script: &str, arg: &str) -> Wrapper {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script, arg])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        group::new_group(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let printed = line.trim().parse().unwrap();
        Wrapper { child, printed }
    }

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

struct EndsInWalk {
    group_of_peer: u32,
    parents: std::collections::HashMap<u32, (Option<u32>, Option<u32>)>,
    ended: Vec<u32>,
    read: std::cell::RefCell<Vec<u32>>,
}

impl EndsInWalk {
    fn two_parents_end(group_of_peer: u32) -> EndsInWalk {
        EndsInWalk {
            group_of_peer,
            parents: [(10, (Some(20), Some(1))), (20, (Some(25), None))].into(),
            ended: vec![25],
            read: Default::default(),
        }
    }
}

impl ProcessFacts for EndsInWalk {
    fn parent(&self, pid: u32) -> Option<u32> {
        let (before, after) = *self.parents.get(&pid)?;
        let mut read = self.read.borrow_mut();
        if read.contains(&pid) {
            after
        } else {
            read.push(pid);
            before
        }
    }

    fn start_time(&self, _pid: u32) -> Option<u64> {
        None
    }

    fn in_group(&self, _pid: u32, _leader: u32, _leader_start: u64) -> Option<bool> {
        None
    }

    fn group_of(&self, pid: u32) -> Option<u32> {
        if self.ended.contains(&pid) {
            return None;
        }
        Some(if pid == 10 { self.group_of_peer } else { pid })
    }

    fn session_of(&self, pid: u32) -> Option<u32> {
        self.group_of(pid)
    }
}

fn ended_wrapper(id: &str) -> FleetProcess {
    FleetProcess {
        attempt: id.to_string(),
        pid: 30,
        start: 5,
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
fn parents_that_end_during_the_walk_give_the_answer_of_the_new_chain() {
    let attempts = [ended_wrapper("A-7")];
    let facts = EndsInWalk::two_parents_end(30);
    assert_eq!(fleet_process(10, &attempts, &facts), yes("A-7"));
    let facts = EndsInWalk::two_parents_end(10);
    assert_eq!(fleet_process(10, &attempts, &facts), Fact::No);
}

#[test]
fn failed_read_of_a_parent_that_did_not_end_is_unknown() {
    let attempts = [ended_wrapper("A-8")];
    let facts = EndsInWalk {
        group_of_peer: 10,
        parents: [(10, (Some(20), Some(20))), (20, (Some(25), Some(25)))].into(),
        ended: vec![25],
        read: Default::default(),
    };
    assert_eq!(fleet_process(10, &attempts, &facts), Fact::Unknown);
}

#[test]
fn failed_read_after_a_match_keeps_the_match() {
    let attempts = [ended_wrapper("A-10")];
    let facts = EndsInWalk {
        group_of_peer: 30,
        parents: [(10, (Some(20), Some(20))), (20, (Some(25), Some(25)))].into(),
        ended: vec![25],
        read: Default::default(),
    };
    assert_eq!(fleet_process(10, &attempts, &facts), yes("A-10"));
}

#[test]
fn failed_read_of_the_peer_is_unknown() {
    let attempts = [ended_wrapper("A-9")];
    let mut facts = EndsInWalk::two_parents_end(30);
    facts.ended.push(10);
    assert_eq!(fleet_process(10, &attempts, &facts), Fact::Unknown);
}

#[test]
fn orphan_in_the_group_of_an_ended_wrapper_is_a_fleet_process() {
    let mut w = Wrapper::start("sleep 30 & echo $!; read _", "");
    let attempt = w.attempt("A-6");
    drop(w.child.stdin.take());
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
