//! The concern watch: file watching (PL-2).
//!
//! A [`Watcher`] tells which watched folders changed since the last read,
//! with no wait. Its events are only a hint (RR-4): a fleetd pass scans each
//! folder in turn anyway, and file watching only puts a changed folder at
//! the front of the order. So [`Watcher::changed`] can give a folder that
//! did not change, and it can miss a change.
//!
//! - macOS: kqueue with `EVFILT_VNODE` on an open folder. It reports a file
//!   that is created, removed or renamed in the folder, not a write to an
//!   existing file.
//! - Linux: `inotify` with `IN_CREATE`, `IN_MODIFY`, `IN_MOVED_TO` and
//!   `IN_CLOSE_WRITE`. When the event queue overflows, each watched folder
//!   is reported.
//! - Windows: `ReadDirectoryChangesW` with overlapped I/O, polled with no
//!   wait.
//!
//! Each OS can give a watch, so no function here returns [`NotSupported`].
//! A watch of many folders can hit a limit of the OS (open files for
//! kqueue, `max_user_watches` for inotify). Then [`Watcher::add`] returns
//! the error, and the caller goes on without a watch for that folder.
//!
//! [`NotSupported`]: crate::NotSupported

use std::io;
use std::path::{Path, PathBuf};

use crate::os::watch as os;

/// A set of watched folders.
#[derive(Debug)]
pub struct Watcher {
    inner: os::Watcher,
}

impl Watcher {
    /// A watcher with no folder.
    pub fn new() -> io::Result<Watcher> {
        Ok(Watcher {
            inner: os::Watcher::new()?,
        })
    }

    /// Watch the folder `dir`. A folder that is watched already stays
    /// watched. An error when `dir` is not a folder that exists, or when
    /// the OS has no watch left.
    pub fn add(&mut self, dir: &Path) -> io::Result<()> {
        self.inner.add(dir)
    }

    /// Stop the watch of `dir`. A folder that is not watched, or that no
    /// longer exists, is not an error.
    pub fn remove(&mut self, dir: &Path) {
        self.inner.remove(dir)
    }

    /// The watched folders with a change since the last call, each 1 time,
    /// as they were given to [`Watcher::add`]. It never blocks. It can give
    /// a folder that did not change, and it can miss a change.
    pub fn changed(&mut self) -> Vec<PathBuf> {
        self.inner.changed()
    }
}
