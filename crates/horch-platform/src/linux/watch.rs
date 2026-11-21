use std::collections::{HashMap, HashSet};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use crate::unix::watch::c_path;

const MASK: u32 =
    libc::IN_CREATE | libc::IN_MODIFY | libc::IN_MOVED_TO | libc::IN_CLOSE_WRITE | libc::IN_ONLYDIR;

const HEADER: usize = std::mem::size_of::<libc::inotify_event>();

const MAX_READS: usize = 16;

#[derive(Debug)]
pub(crate) struct Watcher {
    fd: OwnedFd,
    dirs: HashMap<libc::c_int, PathBuf>,
}

impl Watcher {
    pub(crate) fn new() -> io::Result<Watcher> {
        // SAFETY: `inotify_init1` takes only flags.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is a new inotify fd that nothing else owns.
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        Ok(Watcher {
            fd,
            dirs: HashMap::new(),
        })
    }

    pub(crate) fn add(&mut self, dir: &Path) -> io::Result<()> {
        let path = c_path(dir)?;
        // SAFETY: `path` is a valid C string that lives for the call.
        let wd = unsafe { libc::inotify_add_watch(self.fd.as_raw_fd(), path.as_ptr(), MASK) };
        if wd < 0 {
            return Err(io::Error::last_os_error());
        }
        self.dirs.insert(wd, dir.to_path_buf());
        Ok(())
    }

    pub(crate) fn remove(&mut self, dir: &Path) {
        let Some(wd) = self.dirs.iter().find(|(_, d)| *d == dir).map(|(wd, _)| *wd) else {
            return;
        };
        self.dirs.remove(&wd);
        // SAFETY: `wd` is a descriptor of this fd. An error means that the
        // kernel removed the watch already, and that is not an error here.
        unsafe { libc::inotify_rm_watch(self.fd.as_raw_fd(), wd) };
    }

    pub(crate) fn changed(&mut self) -> Vec<PathBuf> {
        let mut wds = HashSet::new();
        let mut overflow = false;
        let mut gone = Vec::new();
        let mut buf = [0u8; 4096];
        for _ in 0..MAX_READS {
            // SAFETY: `buf` is writable for its full length, and the fd is
            // non-blocking, so the read returns at once.
            let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            let Ok(n) = usize::try_from(n) else { break };
            if n == 0 {
                break;
            }
            let mut at = 0;
            while at + HEADER <= n {
                // SAFETY: the kernel wrote a whole event at `at`, and
                // `read_unaligned` needs no alignment.
                let event: libc::inotify_event =
                    unsafe { std::ptr::read_unaligned(buf.as_ptr().add(at).cast()) };
                if event.mask & libc::IN_Q_OVERFLOW != 0 {
                    overflow = true;
                } else if event.mask & libc::IN_IGNORED != 0 {
                    gone.push(event.wd);
                } else {
                    wds.insert(event.wd);
                }
                at += HEADER + event.len as usize;
            }
        }
        let changed = self
            .dirs
            .iter()
            .filter(|(wd, _)| overflow || wds.contains(*wd))
            .map(|(_, dir)| dir.clone())
            .collect();
        for wd in gone {
            self.dirs.remove(&wd);
        }
        changed
    }
}
