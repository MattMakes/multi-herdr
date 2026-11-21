//! The concern group for Windows. Windows has no process groups. A job
//! object holds a contained child and its descendants.

use crate::NotSupported;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    OpenThread, ResumeThread, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
};

const NOT_SUPPORTED: NotSupported = NotSupported::new("group", "windows");

/// No process groups: the command is unchanged.
pub(crate) fn new_group(_cmd: &mut Command) {}

pub(crate) fn kill_group_id(_pgid: u32) -> Result<bool, NotSupported> {
    Err(NOT_SUPPORTED)
}

/// The child only.
pub(crate) fn kill_child_group(child: &mut Child) -> bool {
    child.kill().is_ok()
}

/// `try_wait`: Windows has no wait without a reap.
pub(crate) fn exited_without_reap(child: &mut Child) -> std::io::Result<bool> {
    Ok(child.try_wait()?.is_some())
}

pub(crate) fn detach(_cmd: &mut Command) -> Result<(), NotSupported> {
    Err(NOT_SUPPORTED)
}

/// Spawn `cmd` while the std handles of this process are not inheritable.
///
/// std calls `CreateProcessW` with handle inheritance on, so a child gets
/// every inheritable handle of this process. A std handle that this process
/// got from its parent is inheritable. The handle list of the start info
/// (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`) would name only the 3 handles of
/// the child, but stable std has no way to pass it (`raw_attribute` is
/// unstable), and a direct `CreateProcessW` gives no `std::process::Child`.
/// So the flag of the parent handles changes for the spawn. Another spawn
/// of this process is not changed by it: std passes a duplicate of an
/// inherited std handle, made inheritable, to each child. The lock keeps 2
/// spawns of this function from restoring a flag that the other cleared.
pub(crate) fn spawn_without_inherited_stdio(cmd: &mut Command) -> std::io::Result<Child> {
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, SetHandleInformation, HANDLE_FLAG_INHERIT,
    };
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let std_handles: [HANDLE; 3] = [
        std::io::stdin().as_raw_handle() as HANDLE,
        std::io::stdout().as_raw_handle() as HANDLE,
        std::io::stderr().as_raw_handle() as HANDLE,
    ];
    let mut cleared: Vec<HANDLE> = Vec::new();
    for handle in std_handles {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE || cleared.contains(&handle) {
            continue;
        }
        let mut flags = 0u32;
        // SAFETY: `handle` is a std handle of this process; `flags` is live.
        let inherits = unsafe { GetHandleInformation(handle, &mut flags) } != 0
            && flags & HANDLE_FLAG_INHERIT != 0;
        // SAFETY: as above; only the inherit flag changes.
        if inherits && unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } != 0 {
            cleared.push(handle);
        }
    }
    let child = cmd.spawn();
    for handle in cleared {
        // SAFETY: as above; the flag goes back to what it was.
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
    }
    child
}

pub(crate) fn is_same_group(_pgid: u32, _started: u64) -> Result<bool, NotSupported> {
    Err(NOT_SUPPORTED)
}

/// The process tree of a child. The job kills every process in it when the
/// last handle closes, so a descendant that holds a pipe cannot outlive the
/// child on any path.
pub(crate) struct Contained(HANDLE);

impl Contained {
    pub(crate) fn prepare(cmd: &mut Command) {
        cmd.creation_flags(CREATE_SUSPENDED);
    }

    pub(crate) fn start(child: &Child) -> std::io::Result<Self> {
        let job = Self::contain(child)?;
        resume_main_thread(child)?;
        Ok(job)
    }

    fn contain(child: &Child) -> std::io::Result<Self> {
        // SAFETY: a null name and attributes make an unnamed job.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Self(handle);
        // SAFETY: an all-zero limit structure is valid.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` is live and its size is passed with it.
        let set = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        };
        // SAFETY: the child handle stays valid while `child` is alive.
        if set == 0 || unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle() as _) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(job)
    }

    pub(crate) fn terminate(&self) -> bool {
        // SAFETY: `self.0` is a live job handle.
        unsafe { TerminateJobObject(self.0, 1) != 0 }
    }
}

impl Drop for Contained {
    fn drop(&mut self) {
        // SAFETY: closing the handle we own; kill-on-close ends the tree.
        unsafe { CloseHandle(self.0) };
    }
}

/// Let the suspended `child` run: resume its only thread. std keeps the main
/// thread handle to itself, so find the thread by its owner process.
fn resume_main_thread(child: &Child) -> std::io::Result<()> {
    // SAFETY: a snapshot of all threads; the handle is closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: an all-zero entry is valid; `dwSize` is set before use.
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
    let mut resumed = false;
    // SAFETY: `snapshot` is live and `entry` is sized.
    let mut more = unsafe { Thread32First(snapshot, &mut entry) } != 0;
    while more {
        if entry.th32OwnerProcessID == child.id() {
            // SAFETY: opens the thread for resume only; closed right after.
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !thread.is_null() {
                // SAFETY: `thread` is a live handle we own.
                resumed |= unsafe { ResumeThread(thread) } != u32::MAX;
                // SAFETY: closing the thread handle we opened above.
                unsafe { CloseHandle(thread) };
            }
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snapshot, &mut entry) } != 0;
    }
    // SAFETY: closing the snapshot we own.
    unsafe { CloseHandle(snapshot) };
    if resumed {
        Ok(())
    } else {
        Err(std::io::Error::other(crate::group::NotResumed))
    }
}
