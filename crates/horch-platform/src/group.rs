//! The concern group: process groups (the stop and the kill of a group) and
//! the detach of a child process.
//!
//! On macOS and Linux a group is a process group. On Windows it is a job
//! object, which only [`Contained`] makes. Each Windows gap is a
//! [`NotSupported`] value or is stated in the doc of its function, with its
//! row of `docs/specs/fleetd/preservation-06-wiring-and-platform.md`,
//! section 6.

use crate::os::group as os;
use crate::start::{Start, TOKEN};
use crate::NotSupported;
use std::process::Child;

/// Start the child of `cmd` in a new process group that it leads, so that
/// [`kill_child_group`] also ends its descendants.
///
/// Windows: no change to `cmd`. Windows has no process groups, so the child
/// stays in the group of this process. Section 6, row "Process groups".
pub fn new_group(cmd: &mut impl Start) {
    os::new_group(cmd.std_mut(TOKEN));
}

/// Send `SIGKILL` to process group `pgid`. Whether the signal was sent. A
/// group that has gone gives `false` (`ESRCH`).
///
/// 0 names the group of this process, and a value above `i32::MAX` names
/// no single group: both give `false` and send no signal. The caller passes
/// the pid of a group leader that it started with [`new_group`].
///
/// Windows gives [`NotSupported`]: it has no process groups. Section 6, row
/// "Process groups".
pub fn kill_group_id(pgid: u32) -> Result<bool, NotSupported> {
    os::kill_group_id(pgid)
}

/// Kill `child` and the group it leads (a child started with
/// [`new_group`]). Whether the kill was sent. `child` must be unreaped, so
/// that its pid still names its group and no other process.
///
/// Windows kills the child only: its descendants live on. Section 6, row
/// "Process groups". Use [`Contained`] to kill a whole tree on Windows.
pub fn kill_child_group(child: &mut Child) -> bool {
    os::kill_child_group(child)
}

/// Whether `child` has exited. It stays unreaped, so its pid and process
/// group cannot go to another process before the caller reaps it.
///
/// Windows has no wait without a reap: there it is `Child::try_wait`, which
/// keeps the exit status in `child`. A later `wait` still gives it, and
/// the handle in `child` keeps the pid from reuse.
pub fn exited_without_reap(child: &mut Child) -> std::io::Result<bool> {
    os::exited_without_reap(child)
}

/// Make the child of `cmd` the leader of a new session (`setsid`), so that
/// it outlives the close of the pane that starts it. A new process group
/// alone does not do that: a pane close ends its whole session.
///
/// Windows gives [`NotSupported`] and leaves `cmd` unchanged: there is no
/// detach, and a detached child dies with the pane. Section 6, row "Service
/// or daemon start".
pub fn detach(cmd: &mut impl Start) -> Result<(), NotSupported> {
    os::detach(cmd.std_mut(TOKEN))
}

/// Start a detached child of `cmd` ([`detach`]) that inherits no handle of
/// this process: only the stdin, stdout and stderr that `cmd` sets. Each
/// name of [`crate::start::FORBIDDEN_ENV`] is removed before the start.
///
/// macOS and Linux: std opens each descriptor close-on-exec, so this is
/// [`detach`] and a spawn. Windows: std always starts a child with handle
/// inheritance on, so a detached child also gets the stdout and stderr of
/// this process. A caller that reads the output of this process through a
/// pipe then waits until the detached child ends (CI run 4: the mail courier
/// held the pipe of `horch tell` for hours). Windows clears the inherit flag
/// of the 3 std handles of this process for the spawn, then sets it back.
pub fn spawn_detached(cmd: &mut impl Start) -> std::io::Result<Child> {
    let cmd = cmd.std_mut(TOKEN);
    for key in crate::start::FORBIDDEN_ENV {
        cmd.env_remove(key);
    }
    let _ = os::detach(cmd);
    os::spawn_without_inherited_stdio(cmd)
}

/// Whether process group `pgid` is still the one whose leader started at
/// `started` (a start time of [`crate::process::start_time`]). True when the
/// leader runs with that start time and still leads the group, or when the
/// leader has ended and the group still has a member, which keeps the id
/// from going to another group. False for 0 and for a value above
/// `i32::MAX`.
///
/// Windows gives [`NotSupported`]: it has no process groups. Section 6, row
/// "Process groups".
pub fn is_same_group(pgid: u32, started: u64) -> Result<bool, NotSupported> {
    os::is_same_group(pgid, started)
}

/// A child and every process that it starts, held so that 1 call kills them
/// all. On Windows it is a job object that kills its processes when it is
/// terminated or dropped. On macOS and Linux it is the process group of the
/// child, and [`kill_child_group`] kills it.
///
/// Use: [`Contained::prepare`] the command, spawn it, then
/// [`Contained::start`] the child. If `start` fails, kill and reap the
/// child with [`kill_child_group`].
pub struct Contained(os::Contained);

impl Contained {
    /// Prepare `cmd` to be contained. macOS and Linux: [`new_group`].
    /// Windows: the child starts suspended, so it cannot start a descendant
    /// before the job holds it.
    pub fn prepare(cmd: &mut impl Start) {
        os::Contained::prepare(cmd.std_mut(TOKEN));
    }

    /// Contain `child`, spawned from a command of [`Contained::prepare`],
    /// and let it run. Windows: a new job with kill-on-close holds the child,
    /// then its main thread resumes. When no thread resumes, the error holds
    /// a [`NotResumed`]. macOS and Linux: the process group holds it already,
    /// and this does nothing.
    pub fn start(child: &Child) -> std::io::Result<Self> {
        os::Contained::start(child).map(Self)
    }

    /// Kill every process in the job. Whether the OS did it.
    ///
    /// macOS and Linux: this does nothing and gives `true`. The caller
    /// kills the process group with [`kill_child_group`], as it does after
    /// this call on every OS.
    pub fn terminate(&self) -> bool {
        self.0.terminate()
    }
}

/// The error of [`Contained::start`] on Windows when the main thread of the
/// child does not resume. The `std::io::Error` holds it, so the caller finds
/// it with `get_ref` and can name the child in its own message.
#[derive(Debug)]
pub struct NotResumed;

impl std::fmt::Display for NotResumed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("could not resume the child process")
    }
}

impl std::error::Error for NotResumed {}
