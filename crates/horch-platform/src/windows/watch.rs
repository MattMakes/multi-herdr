//! The concern watch for Windows: `ReadDirectoryChangesW` with overlapped
//! I/O on each folder, polled with no wait.

use std::collections::HashMap;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadDirectoryChangesW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
    FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

const FILTER: u32 =
    FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE;

/// The buffer of 1 read. Its content is not parsed: a completed read only
/// says that the folder changed, and an overflow completes it too.
#[repr(C, align(4))]
struct Buffer([u8; 1024]);

/// 1 watched folder with its read in flight.
struct Dir {
    handle: HANDLE,
    /// The OS writes these 2 while a read is in flight, so each is boxed
    /// and does not move.
    overlapped: Box<OVERLAPPED>,
    buffer: Box<Buffer>,
    /// A read is in flight.
    armed: bool,
}

impl std::fmt::Debug for Dir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dir").field("armed", &self.armed).finish()
    }
}

// SAFETY: the handle and the boxes belong to this `Dir` only, and the OS
// calls on them can come from any thread.
unsafe impl Send for Dir {}

impl Dir {
    fn open(dir: &Path) -> io::Result<Dir> {
        let mut wide: Vec<u16> = dir.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a path with a NUL character",
            ));
        }
        wide.push(0);
        // SAFETY: `wide` is a NUL-terminated wide string that lives for the
        // call. Null security attributes and template are allowed.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut watch = Dir {
            handle,
            // SAFETY: `OVERLAPPED` is plain data, and an all-zero value is
            // the documented start value.
            overlapped: Box::new(unsafe { std::mem::zeroed() }),
            buffer: Box::new(Buffer([0; 1024])),
            armed: false,
        };
        watch.arm()?;
        Ok(watch)
    }

    /// Start the next read.
    fn arm(&mut self) -> io::Result<()> {
        // SAFETY: `OVERLAPPED` is plain data, and no read is in flight.
        *self.overlapped = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is open with `FILE_FLAG_OVERLAPPED`, and the
        // buffer and the `OVERLAPPED` are boxed and live until `Drop` ends
        // the read.
        let ok = unsafe {
            ReadDirectoryChangesW(
                self.handle,
                self.buffer.0.as_mut_ptr().cast(),
                self.buffer.0.len() as u32,
                0,
                FILTER,
                std::ptr::null_mut(),
                &mut *self.overlapped,
                None,
            )
        };
        // SAFETY: `GetLastError` reads the error of this thread.
        if ok == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING {
            return Err(io::Error::last_os_error());
        }
        self.armed = true;
        Ok(())
    }

    /// The read completed: the folder changed. It starts the next read.
    /// A failed read, for example of a removed folder, also says "changed",
    /// and no next read starts.
    fn poll(&mut self) -> bool {
        if !self.armed {
            return false;
        }
        let mut bytes = 0u32;
        // SAFETY: the `OVERLAPPED` is of the read in flight on this handle,
        // and the last argument `FALSE` makes the call return at once.
        let ok = unsafe { GetOverlappedResult(self.handle, &*self.overlapped, &mut bytes, 0) };
        // SAFETY: `GetLastError` reads the error of this thread.
        if ok == 0 && unsafe { GetLastError() } == ERROR_IO_INCOMPLETE {
            return false;
        }
        self.armed = false;
        if ok != 0 {
            // A failed start is a missed hint: the pass scans each folder
            // anyway.
            let _ = self.arm();
        }
        true
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        if self.armed {
            let mut bytes = 0u32;
            // SAFETY: the `OVERLAPPED` is of the read in flight on this
            // handle. The wait makes sure that the OS no longer writes the
            // buffer before it is freed.
            unsafe {
                CancelIoEx(self.handle, &*self.overlapped);
                GetOverlappedResult(self.handle, &*self.overlapped, &mut bytes, 1);
            }
        }
        // SAFETY: the handle is open and this `Dir` owns it.
        unsafe { CloseHandle(self.handle) };
    }
}

#[derive(Debug)]
pub(crate) struct Watcher {
    dirs: HashMap<PathBuf, Dir>,
}

impl Watcher {
    pub(crate) fn new() -> io::Result<Watcher> {
        Ok(Watcher {
            dirs: HashMap::new(),
        })
    }

    pub(crate) fn add(&mut self, dir: &Path) -> io::Result<()> {
        if self.dirs.contains_key(dir) {
            return Ok(());
        }
        self.dirs.insert(dir.to_path_buf(), Dir::open(dir)?);
        Ok(())
    }

    pub(crate) fn remove(&mut self, dir: &Path) {
        self.dirs.remove(dir);
    }

    pub(crate) fn changed(&mut self) -> Vec<PathBuf> {
        self.dirs
            .iter_mut()
            .filter_map(|(path, dir)| dir.poll().then(|| path.clone()))
            .collect()
    }
}
