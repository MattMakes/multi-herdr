//! The concern group for Windows. Windows has no process groups. A job
//! object holds a contained child and its descendants.

use crate::NotSupported;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetHandleInformation, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE,
};
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

/// No polite stop on Windows.
pub(crate) fn term_child_group(_child: &mut Child) -> bool {
    false
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
///
/// It fails closed: when the flag of a valid std handle cannot be read or
/// cleared, nothing spawns, because the child would hold the pipe of the
/// caller. A failed restore after the spawn returns the child and writes the
/// failure to stderr: the child already runs, and an error would make the
/// caller lose it. A flag that stays cleared changes no later std spawn,
/// which passes its own inheritable duplicate.
pub(crate) fn spawn_without_inherited_stdio(cmd: &mut Command) -> std::io::Result<Child> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let std_handles: [HANDLE; 3] = [
        std::io::stdin().as_raw_handle() as HANDLE,
        std::io::stdout().as_raw_handle() as HANDLE,
        std::io::stderr().as_raw_handle() as HANDLE,
    ];
    spawn_with_cleared(&SystemFlag, std_handles, || cmd.spawn())
}

trait InheritFlag {
    fn inherits(&self, handle: HANDLE) -> std::io::Result<bool>;
    fn set(&self, handle: HANDLE, on: bool) -> std::io::Result<()>;
}

struct SystemFlag;

impl InheritFlag for SystemFlag {
    fn inherits(&self, handle: HANDLE) -> std::io::Result<bool> {
        let mut flags = 0u32;
        // SAFETY: `handle` is a std handle of this process; `flags` is live.
        if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(flags & HANDLE_FLAG_INHERIT != 0)
    }

    fn set(&self, handle: HANDLE, on: bool) -> std::io::Result<()> {
        let value = if on { HANDLE_FLAG_INHERIT } else { 0 };
        // SAFETY: as above; only the inherit flag changes.
        if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, value) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

fn spawn_with_cleared<T>(
    flag: &impl InheritFlag,
    handles: [HANDLE; 3],
    spawn: impl FnOnce() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let cleared = clear_inherit(flag, handles)?;
    let spawned = spawn();
    if let Err(e) = restore_inherit(flag, &cleared) {
        eprintln!("horch: a std handle of this process stays not inheritable: {e}");
    }
    spawned
}

fn clear_inherit(flag: &impl InheritFlag, handles: [HANDLE; 3]) -> std::io::Result<Vec<HANDLE>> {
    let mut cleared: Vec<HANDLE> = Vec::new();
    for handle in handles {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE || cleared.contains(&handle) {
            continue;
        }
        let step = flag.inherits(handle).and_then(|on| {
            if on {
                flag.set(handle, false).map(|()| on)
            } else {
                Ok(on)
            }
        });
        match step {
            Ok(true) => cleared.push(handle),
            Ok(false) => {}
            Err(e) => {
                if let Err(r) = restore_inherit(flag, &cleared) {
                    eprintln!("horch: a std handle of this process stays not inheritable: {r}");
                }
                return Err(e);
            }
        }
    }
    Ok(cleared)
}

fn restore_inherit(flag: &impl InheritFlag, cleared: &[HANDLE]) -> std::io::Result<()> {
    let mut first = Ok(());
    for &handle in cleared {
        if let Err(e) = flag.set(handle, true) {
            if first.is_ok() {
                first = Err(e);
            }
        }
    }
    first
}

pub(crate) fn is_same_group(_pgid: u32, _started: u64) -> Result<bool, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn is_group_leader(_pgid: u32, _started: u64) -> Result<bool, NotSupported> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fail {
        query: Option<HANDLE>,
        clear: Option<HANDLE>,
        restore: Option<HANDLE>,
    }

    struct FakeFlag {
        fail: Fail,
        off: RefCell<Vec<HANDLE>>,
        calls: RefCell<Vec<(HANDLE, &'static str)>>,
    }

    impl FakeFlag {
        fn new(fail: Fail) -> Self {
            Self {
                fail,
                off: RefCell::new(Vec::new()),
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    fn denied() -> std::io::Error {
        std::io::Error::from(std::io::ErrorKind::PermissionDenied)
    }

    impl InheritFlag for FakeFlag {
        fn inherits(&self, handle: HANDLE) -> std::io::Result<bool> {
            self.calls.borrow_mut().push((handle, "query"));
            if self.fail.query == Some(handle) {
                return Err(denied());
            }
            Ok(!self.off.borrow().contains(&handle))
        }

        fn set(&self, handle: HANDLE, on: bool) -> std::io::Result<()> {
            let (call, fails) = if on {
                ("restore", self.fail.restore)
            } else {
                ("clear", self.fail.clear)
            };
            self.calls.borrow_mut().push((handle, call));
            if fails == Some(handle) {
                return Err(denied());
            }
            let mut off = self.off.borrow_mut();
            off.retain(|h| *h != handle);
            if !on {
                off.push(handle);
            }
            Ok(())
        }
    }

    fn h(n: usize) -> HANDLE {
        std::ptr::without_provenance_mut(n)
    }

    const HANDLES: [usize; 3] = [4, 8, 12];

    fn handles() -> [HANDLE; 3] {
        HANDLES.map(h)
    }

    #[test]
    fn a_failed_query_spawns_nothing_and_restores_the_cleared_handles() {
        let flag = FakeFlag::new(Fail {
            query: Some(h(8)),
            ..Fail::default()
        });
        let mut spawned = false;
        let got = spawn_with_cleared(&flag, handles(), || {
            spawned = true;
            Ok(())
        });
        assert_eq!(
            got.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(!spawned);
        assert!(flag.off.borrow().is_empty());
        assert!(flag.calls.borrow().contains(&(h(4), "restore")));
    }

    #[test]
    fn a_failed_clear_spawns_nothing_and_restores_the_cleared_handles() {
        let flag = FakeFlag::new(Fail {
            clear: Some(h(12)),
            ..Fail::default()
        });
        let mut spawned = false;
        let got = spawn_with_cleared(&flag, handles(), || {
            spawned = true;
            Ok(())
        });
        assert_eq!(
            got.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(!spawned);
        assert!(flag.off.borrow().is_empty());
    }

    #[test]
    fn a_failed_restore_returns_the_child_and_restores_every_other_handle() {
        let flag = FakeFlag::new(Fail {
            restore: Some(h(4)),
            ..Fail::default()
        });
        let got = spawn_with_cleared(&flag, handles(), || {
            assert_eq!(
                flag.off.borrow().len(),
                3,
                "each handle is cleared at the spawn"
            );
            Ok("child")
        });
        assert_eq!(got.unwrap(), "child");
        assert_eq!(*flag.off.borrow(), vec![h(4)]);
        let calls = flag.calls.borrow();
        assert!(calls.contains(&(h(8), "restore")));
        assert!(calls.contains(&(h(12), "restore")));
    }

    #[test]
    fn a_handle_that_is_not_valid_is_not_a_failure() {
        let flag = FakeFlag::new(Fail {
            query: Some(std::ptr::null_mut()),
            ..Fail::default()
        });
        let got = spawn_with_cleared(
            &flag,
            [std::ptr::null_mut(), INVALID_HANDLE_VALUE, h(4)],
            || Ok(()),
        );
        assert!(got.is_ok());
        assert_eq!(
            *flag.calls.borrow(),
            vec![(h(4), "query"), (h(4), "clear"), (h(4), "restore")]
        );
    }
}
