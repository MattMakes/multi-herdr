//! The concern fs for the code of macOS and Linux.

use std::ffi::{CStr, CString};
use std::fs::{DirBuilder, File, Metadata, OpenOptions};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;

use crate::channel::User;
use crate::fs::OwnerAndMode;
use crate::NotSupported;

pub(crate) fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

pub(crate) fn create_mode(options: &mut OpenOptions, mode: u32) -> Result<(), NotSupported> {
    options.mode(mode);
    Ok(())
}

pub(crate) fn create_dir_mode(builder: &mut DirBuilder, mode: u32) -> Result<(), NotSupported> {
    builder.mode(mode);
    Ok(())
}

pub(crate) fn create_dir_owner_only(path: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(path)
}

pub(crate) fn create_new_owner_only(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

pub(crate) fn check_owner_only(_file: &File, meta: &Metadata, rule: u32) -> Result<(), String> {
    let mode = meta.mode() & 0o7777;
    if mode & 0o077 != 0 {
        return Err(format!("mode {mode:04o} is wider than {rule:04o}"));
    }
    Ok(())
}

pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    let f = std::fs::File::open(dir)?;
    f.sync_all()
}

pub(crate) fn open_no_block(options: &mut OpenOptions) -> Result<(), NotSupported> {
    options.custom_flags(libc::O_NONBLOCK);
    Ok(())
}

/// [`crate::fs::remove_dir_below`]: each folder is opened from the handle
/// of the folder before it, with `O_NOFOLLOW`, and each name is removed
/// with `unlinkat` on the handle of its folder.
pub(crate) fn remove_dir_below(root: &Path, names: &[&str]) -> io::Result<()> {
    let root_c = CString::new(root.as_os_str().as_bytes())?;
    // SAFETY: a NUL-terminated path that outlives the call.
    let mut dir = owned(unsafe {
        libc::open(
            root_c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    })?;
    let (last, way) = names.split_last().expect("the caller checks the names");
    for name in way {
        dir = open_below(&dir, &CString::new(*name)?)?;
    }
    let last = CString::new(*last)?;
    let folder = open_below(&dir, &last)?;
    empty_folder(&folder)?;
    unlink_at(&dir, &last, libc::AT_REMOVEDIR)
}

/// The folder `name` in the folder `dir`, opened with no link followed.
fn open_below(dir: &OwnedFd, name: &CStr) -> io::Result<OwnedFd> {
    // SAFETY: an open handle and a NUL-terminated name that outlive the call.
    owned(unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })
}

fn owned(fd: libc::c_int) -> io::Result<OwnedFd> {
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` is a new open handle that only this value owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn unlink_at(dir: &OwnedFd, name: &CStr, flags: libc::c_int) -> io::Result<()> {
    // SAFETY: an open handle and a NUL-terminated name that outlive the call.
    if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), flags) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Remove each entry of the open folder `dir`: a folder with all in it, and
/// each other entry, a link too, as 1 name.
fn empty_folder(dir: &OwnedFd) -> io::Result<()> {
    for name in entries(dir)? {
        let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: an open handle, a NUL-terminated name and a stat buffer
        // that outlive the call.
        let rc = unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                name.as_ptr(),
                st.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: fstatat filled the buffer.
        let mode = unsafe { st.assume_init() }.st_mode;
        if mode & libc::S_IFMT == libc::S_IFDIR {
            empty_folder(&open_below(dir, &name)?)?;
            unlink_at(dir, &name, libc::AT_REMOVEDIR)?;
        } else {
            unlink_at(dir, &name, 0)?;
        }
    }
    Ok(())
}

/// The names in the open folder `dir`, without `.` and `..`.
fn entries(dir: &OwnedFd) -> io::Result<Vec<CString>> {
    // SAFETY: `dir` is an open handle; the copy is owned by the stream.
    let copy = unsafe { libc::dup(dir.as_raw_fd()) };
    if copy < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `copy` is an open folder handle that the stream takes.
    let stream = unsafe { libc::fdopendir(copy) };
    if stream.is_null() {
        let e = io::Error::last_os_error();
        // SAFETY: the stream did not take `copy`.
        unsafe { libc::close(copy) };
        return Err(e);
    }
    let mut names = Vec::new();
    loop {
        // SAFETY: `stream` is open until the closedir below.
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            break;
        }
        // SAFETY: readdir gives an entry with a NUL-terminated name.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() != b"." && name.to_bytes() != b".." {
            names.push(name.to_owned());
        }
    }
    // SAFETY: `stream` is open and closed only here.
    unsafe { libc::closedir(stream) };
    Ok(names)
}

/// [`crate::fs::rename_noreplace`] with `rename`, the call of the OS that
/// fails when the target exists. `rename` returns 0 or sets `errno`.
pub(crate) fn rename_noreplace(
    from: &Path,
    to: &Path,
    rename: fn(&CStr, &CStr) -> libc::c_int,
) -> io::Result<()> {
    let from_c = CString::new(from.as_os_str().as_bytes())?;
    let to_c = CString::new(to.as_os_str().as_bytes())?;
    if rename(&from_c, &to_c) == 0 {
        return Ok(());
    }
    let e = io::Error::last_os_error();
    // A file system without the flag: the plain rename below.
    if !matches!(
        e.raw_os_error(),
        Some(libc::EINVAL | libc::ENOSYS | libc::ENOTSUP)
    ) {
        return Err(e);
    }
    std::fs::rename(from, to)
}

pub(crate) fn open_no_follow(path: &Path) -> io::Result<File> {
    // `O_NONBLOCK`: a FIFO that replaces the file after a check of the
    // caller does not block the open. A read of a regular file ignores it.
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

pub(crate) fn owner_of(file: &File) -> io::Result<User> {
    Ok(User::Uid(file.metadata()?.uid()))
}

pub(crate) fn owner_and_mode(meta: &Metadata) -> Result<OwnerAndMode, NotSupported> {
    Ok(OwnerAndMode {
        uid: meta.uid(),
        mode: meta.mode(),
    })
}

pub(crate) fn mode_of(meta: &Metadata) -> Result<u32, NotSupported> {
    Ok(meta.permissions().mode())
}

pub(crate) fn current_uid() -> Result<u32, NotSupported> {
    // SAFETY: getuid has no preconditions and cannot fail.
    Ok(unsafe { libc::getuid() })
}

pub(crate) fn is_executable(meta: &Metadata) -> bool {
    meta.is_file() && meta.permissions().mode() & 0o111 != 0
}

pub(crate) fn make_executable(path: &Path) -> io::Result<()> {
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)
}

pub(crate) fn file_identity(meta: &Metadata) -> (u64, u64) {
    (meta.dev(), meta.ino())
}

/// [`crate::fs::BreakerGuard`]: the open dir that holds the `flock`, or
/// nothing where `flock` is not available.
#[derive(Debug)]
pub(crate) struct BreakerGuard {
    dir: Option<std::fs::File>,
}

impl BreakerGuard {
    pub(crate) fn take(lock: &Path) -> Option<BreakerGuard> {
        match lock.parent().and_then(|d| std::fs::File::open(d).ok()) {
            // SAFETY: `dir` is an open directory `File` that outlives the call, so its raw fd is valid;
            // `flock` touches no memory.
            Some(dir) => match unsafe {
                libc::flock(dir.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB)
            } {
                0 => Some(BreakerGuard { dir: Some(dir) }),
                _ if io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) => None,
                _ => Some(BreakerGuard { dir: None }),
            },
            None => Some(BreakerGuard { dir: None }),
        }
    }
}

impl Drop for BreakerGuard {
    /// Unlock before the close. A child forked by another thread shares the
    /// open file until it execs, and a close alone leaves the `flock` held
    /// while any copy is open.
    fn drop(&mut self) {
        if let Some(dir) = &self.dir {
            // SAFETY: `dir` is the open directory `File` this guard owns, so its raw fd is valid until drop;
            // `flock` touches no memory.
            unsafe { libc::flock(dir.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}
