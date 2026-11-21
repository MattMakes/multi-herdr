//! The concern group for the code of macOS and Linux.

use crate::NotSupported;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

pub(crate) fn new_group(cmd: &mut Command) {
    cmd.process_group(0);
}

/// `SIGKILL` to group `pgid`. 0 names the group of the caller, and a value
/// above `i32::MAX` becomes a negative `pid_t`, so both send nothing.
fn kill_pgid(pgid: u32) -> bool {
    signal_pgid(pgid, libc::SIGKILL)
}

/// `signal` to group `pgid`, with the rule of [`kill_pgid`].
fn signal_pgid(pgid: u32, signal: libc::c_int) -> bool {
    let Some(group) = i32::try_from(pgid).ok().filter(|p| *p > 0) else {
        return false;
    };
    // SAFETY: killpg takes plain integers and touches no memory.
    unsafe { libc::killpg(group, signal) == 0 }
}

pub(crate) fn kill_group_id(pgid: u32) -> Result<bool, NotSupported> {
    Ok(kill_pgid(pgid))
}

/// The pid of the child names its group. The child is unreaped, so the
/// group is ours.
pub(crate) fn kill_child_group(child: &mut Child) -> bool {
    kill_pgid(child.id())
}

/// `SIGTERM` to the group of `child`, which is ours as above.
pub(crate) fn term_child_group(child: &mut Child) -> bool {
    signal_pgid(child.id(), libc::SIGTERM)
}

/// `waitid` on `child` with `WNOWAIT`: it does not reap. The caller reads
/// the pid field, which is 0 while the child runs. The layout of that field
/// differs between macOS and Linux.
pub(crate) fn wait_without_reap(child: &Child) -> std::io::Result<libc::siginfo_t> {
    // SAFETY: an all-zero `siginfo_t` is valid; `waitid` fills it in.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: waits on our own unreaped child without reaping it.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id() as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(info)
}

pub(crate) fn detach(cmd: &mut Command) -> Result<(), NotSupported> {
    // SAFETY: runs in the forked child between fork and exec. `setsid` is
    // async-signal-safe and touches no state this process shares.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    Ok(())
}

/// std opens each descriptor close-on-exec: the child inherits only its
/// stdio.
pub(crate) fn spawn_without_inherited_stdio(cmd: &mut Command) -> std::io::Result<Child> {
    cmd.spawn()
}

pub(crate) fn is_same_group(pgid: u32, started: u64) -> Result<bool, NotSupported> {
    let Some(group) = i32::try_from(pgid).ok().filter(|p| *p > 0) else {
        return Ok(false);
    };
    Ok(match crate::process::start_time(pgid) {
        // SAFETY: plain syscall; a pid that has gone answers -1.
        Some(now) => now == started && (unsafe { libc::getpgid(group) }) == group,
        // SAFETY: signal 0 checks that the group has a member; nothing is sent.
        None => (unsafe { libc::kill(-group, 0) }) == 0,
    })
}

pub(crate) fn is_group_leader(pgid: u32, started: u64) -> Result<bool, NotSupported> {
    let Some(group) = i32::try_from(pgid).ok().filter(|p| *p > 0) else {
        return Ok(false);
    };
    // SAFETY: plain syscall; a pid that has gone answers -1.
    Ok(crate::process::start_time(pgid) == Some(started)
        && (unsafe { libc::getpgid(group) }) == group)
}

/// The process group of the child holds it: nothing to set up or to end
/// here. `kill_child_group` kills the group.
pub(crate) struct Contained;

impl Contained {
    pub(crate) fn prepare(cmd: &mut Command) {
        new_group(cmd);
    }

    pub(crate) fn start(_child: &Child) -> std::io::Result<Self> {
        Ok(Self)
    }

    pub(crate) fn terminate(&self) -> bool {
        true
    }
}
