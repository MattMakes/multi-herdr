//! The concern watch for macOS: kqueue with `EVFILT_VNODE` on each open
//! folder. kqueue needs no run loop, unlike FSEvents.

use std::collections::{HashMap, HashSet};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};

use crate::unix::watch::c_path;

/// The events of 1 read of the queue. A full read reads again.
const BATCH: usize = 64;

/// The most reads of the queue in 1 call of `changed`. A busy folder
/// cannot hold the call: the events after the last read wait for the next
/// call.
const MAX_READS: usize = 16;

#[derive(Debug)]
pub(crate) struct Watcher {
    queue: OwnedFd,
    /// Each watched folder with its open fd. The fd is the ident of its
    /// kevent, and its close removes the kevent.
    dirs: HashMap<PathBuf, OwnedFd>,
}

impl Watcher {
    pub(crate) fn new() -> io::Result<Watcher> {
        // SAFETY: `kqueue` takes no argument.
        let fd = unsafe { libc::kqueue() };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is a new kqueue fd that nothing else owns.
        let queue = unsafe { OwnedFd::from_raw_fd(fd) };
        Ok(Watcher {
            queue,
            dirs: HashMap::new(),
        })
    }

    pub(crate) fn add(&mut self, dir: &Path) -> io::Result<()> {
        if self.dirs.contains_key(dir) {
            return Ok(());
        }
        let path = c_path(dir)?;
        // SAFETY: `path` is a valid C string that lives for the call.
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_EVTONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is a new fd that nothing else owns.
        let file = unsafe { OwnedFd::from_raw_fd(fd) };
        let change = libc::kevent {
            ident: fd as libc::uintptr_t,
            filter: libc::EVFILT_VNODE,
            flags: libc::EV_ADD | libc::EV_CLEAR,
            fflags: libc::NOTE_WRITE | libc::NOTE_EXTEND | libc::NOTE_DELETE | libc::NOTE_RENAME,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: `change` is 1 valid kevent, the event list is empty, and
        // a null timeout is allowed when no event is read.
        let n = unsafe {
            libc::kevent(
                self.queue.as_raw_fd(),
                &change,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        self.dirs.insert(dir.to_path_buf(), file);
        Ok(())
    }

    pub(crate) fn remove(&mut self, dir: &Path) {
        // The close of the fd removes its kevent.
        self.dirs.remove(dir);
    }

    pub(crate) fn changed(&mut self) -> Vec<PathBuf> {
        let mut idents: HashSet<RawFd> = HashSet::new();
        let zero = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        for _ in 0..MAX_READS {
            // SAFETY: `kevent` is plain data, and an all-zero value is valid.
            let mut events: [libc::kevent; BATCH] = unsafe { std::mem::zeroed() };
            // SAFETY: the change list is empty, `events` holds `BATCH`
            // kevents, and the zero timeout makes the call return at once.
            let n = unsafe {
                libc::kevent(
                    self.queue.as_raw_fd(),
                    std::ptr::null(),
                    0,
                    events.as_mut_ptr(),
                    BATCH as libc::c_int,
                    &zero,
                )
            };
            // An error is a missed hint: the pass scans each folder anyway.
            let Ok(n) = usize::try_from(n) else { break };
            idents.extend(events[..n].iter().map(|e| e.ident as RawFd));
            if n < BATCH {
                break;
            }
        }
        self.dirs
            .iter()
            .filter(|(_, fd)| idents.contains(&fd.as_raw_fd()))
            .map(|(dir, _)| dir.clone())
            .collect()
    }
}
