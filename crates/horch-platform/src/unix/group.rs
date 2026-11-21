use crate::NotSupported;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

pub(crate) fn new_group(cmd: &mut Command) {
    cmd.process_group(0);
}

fn kill_pgid(pgid: u32) -> bool {
    signal_pgid(pgid, libc::SIGKILL)
}

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

pub(crate) fn kill_child_group(child: &mut Child) -> bool {
    kill_pgid(child.id())
}

pub(crate) fn term_child_group(child: &mut Child) -> bool {
    signal_pgid(child.id(), libc::SIGTERM)
}

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
