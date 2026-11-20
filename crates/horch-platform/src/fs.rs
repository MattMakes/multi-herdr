//! The concern fs: file locks, file permissions and file identity.
//!
//! Each gap of 1 OS names its row of
//! `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6.

use std::fs::{DirBuilder, File, Metadata, OpenOptions};
use std::io;
use std::path::Path;

use crate::channel::User;
use crate::os::fs as os;
use crate::NotSupported;

/// The owner and the mode of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerAndMode {
    /// The user id of the owner.
    pub uid: u32,
    /// `st_mode`: the file type bits and the permission bits.
    pub mode: u32,
}

/// Set the permission bits of `path` to `mode`.
///
/// Windows: an `Unsupported` error that holds [`NotSupported`]. Row "File
/// permissions": modes ignored.
pub fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    os::set_mode(path, mode)
}

/// Make `options` create its file with `mode`, so the file never exists
/// with the permissions of the umask.
///
/// Windows: [`NotSupported`]. Row "File permissions": modes ignored.
pub fn create_mode(options: &mut OpenOptions, mode: u32) -> Result<(), NotSupported> {
    os::create_mode(options, mode)
}

/// Make `builder` create its dirs with `mode`.
///
/// Windows: [`NotSupported`]. Row "File permissions": modes ignored.
pub fn create_dir_mode(builder: &mut DirBuilder, mode: u32) -> Result<(), NotSupported> {
    os::create_dir_mode(builder, mode)
}

/// Make the folder `path` and each missing parent, each new one with access
/// for its owner only. A folder that exists keeps its access.
///
/// macOS and Linux: mode 0700. Windows: this user is the owner, and a
/// protected DACL allows only this user, for the folder and for each file
/// and folder made in it.
pub fn create_dir_owner_only(path: &Path) -> io::Result<()> {
    os::create_dir_owner_only(path)
}

/// Create the new file `path` for write, with access for its owner only
/// from the create on. A file at `path` is an `AlreadyExists` error.
///
/// macOS and Linux: mode 0600. Windows: this user is the owner, and a
/// protected DACL allows only this user.
pub fn create_new_owner_only(path: &Path) -> io::Result<File> {
    os::create_new_owner_only(path)
}

/// Check that only the owner of the open file or folder `file` has access
/// to it. `rule` is the unix mode of the rule, 0600 for a file or 0700 for
/// a folder. The error is the reason, as the user reads it. The caller
/// checks the owner with [`owner_of`].
///
/// macOS and Linux: the mode of `meta` has no bit of group or others, else
/// the reason is "mode <mode> is wider than <rule>". Windows: the DACL of
/// the handle has no allow rule for a user other than the owner, `SYSTEM`
/// and `Administrators`, and no rule of another type than allow or deny.
pub fn check_owner_only(file: &File, meta: &Metadata, rule: u32) -> Result<(), String> {
    os::check_owner_only(file, meta, rule)
}

/// Fsync the dir `dir`, so a rename or a create inside it is durable.
///
/// Windows: an `Unsupported` error that holds [`NotSupported`]. Windows
/// cannot open a dir for sync. Row "File permissions" (`fsx.rs:133`).
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    os::sync_dir(dir)
}

/// Make `options` open a FIFO at once instead of waiting for a writer.
///
/// Windows: [`NotSupported`]. Windows has no FIFO in the file system. Row
/// "Sockets and pipes" (`fsx.rs:186`).
pub fn open_no_block(options: &mut OpenOptions) -> Result<(), NotSupported> {
    os::open_no_block(options)
}

/// Open the file or the folder `path` for read, and refuse a link at its
/// last name. A link in a parent folder is followed: the caller checks the
/// real path. On macOS and Linux the open does not wait for the writer of
/// a FIFO.
///
/// macOS and Linux: `O_NOFOLLOW`, so a link is an `ELOOP` error. Windows:
/// `FILE_FLAG_OPEN_REPARSE_POINT`, and a reparse point of any tag is an
/// `InvalidInput` error.
pub fn open_no_follow(path: &Path) -> io::Result<File> {
    os::open_no_follow(path)
}

/// The owner of the open file or folder `file`.
///
/// macOS and Linux: [`User::Uid`]. Windows: [`User::Sid`], the owner SID
/// of the security descriptor of the handle.
pub fn owner_of(file: &File) -> io::Result<User> {
    os::owner_of(file)
}

/// Remove the folder `root/<names[0]>/.../<names[n]>` and each file and
/// folder in it. No link below `root` is followed: a link on the way is an
/// error, and a link in the folder is removed, not followed. The folders on
/// the way are opened 1 by 1 from `root`, so a swap of 1 of them for a link
/// after a check of the caller cannot make this remove a folder elsewhere.
/// Each name must be 1 folder name: not empty, not `.` or `..`, no `/`.
///
/// Windows: an `Unsupported` error that holds [`NotSupported`]. Windows
/// has no sandbox, so no job of fleetd makes a folder to remove there.
pub fn remove_dir_below(root: &Path, names: &[&str]) -> io::Result<()> {
    let valid =
        |name: &&str| !name.is_empty() && *name != "." && *name != ".." && !name.contains('/');
    if names.is_empty() || !names.iter().all(valid) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{names:?} is not a list of folder names"),
        ));
    }
    os::remove_dir_below(root, names)
}

/// Rename `from` to `to`, and fail when `to` exists, where the OS and the
/// file system can refuse an existing target. A plain `rename` replaces an
/// empty dir.
///
/// Where the file system has no no-replace flag (`EINVAL`, `ENOSYS` or
/// `ENOTSUP`), this falls back to a plain `rename`, which can replace an
/// existing target.
///
/// Windows: always a plain `rename`, which can replace an existing target.
/// Row "File locks".
pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    os::rename_noreplace(from, to)
}

/// The owner and the mode of the file of `meta`.
///
/// Windows: [`NotSupported`]. Row "File permissions": owner and mode
/// checks skipped.
pub fn owner_and_mode(meta: &Metadata) -> Result<OwnerAndMode, NotSupported> {
    os::owner_and_mode(meta)
}

/// The mode of the file of `meta`: the file type bits and the permission
/// bits.
///
/// Windows: [`NotSupported`]. Row "File permissions": modes ignored.
pub fn mode_of(meta: &Metadata) -> Result<u32, NotSupported> {
    os::mode_of(meta)
}

/// The real user id of this process.
///
/// Windows: [`NotSupported`]. Row "File permissions": owner checks skipped.
pub fn current_uid() -> Result<u32, NotSupported> {
    os::current_uid()
}

/// Whether the file of `meta` is a regular file that a user can execute.
///
/// Windows: every regular file is executable. Row "File permissions":
/// executable = exists.
pub fn is_executable(meta: &Metadata) -> bool {
    os::is_executable(meta)
}

/// Make the file `path` executable: mode 0755.
///
/// Windows: does nothing. The extension decides. Row "File permissions":
/// executable = exists.
pub fn make_executable(path: &Path) -> io::Result<()> {
    os::make_executable(path)
}

/// The identity of the file of `meta`: `(dev, ino)`.
///
/// Windows: `(0, creation time)`. Stable Rust has no file index on Windows,
/// so the creation time stands in for the inode. Row "Other"
/// (`telemetry/cursor.rs:84`).
pub fn file_identity(meta: &Metadata) -> (u64, u64) {
    os::file_identity(meta)
}

/// An exclusive `flock` on the dir of a lock, held while 1 process breaks
/// that lock. The kernel drops it when the process dies, so it is never
/// left behind. The drop unlocks before the close: a child that another
/// thread forks shares the open file until it execs.
///
/// Windows: the guard holds nothing. Row "File locks": lock break race not
/// guarded.
#[derive(Debug)]
pub struct BreakerGuard {
    _held: os::BreakerGuard,
}

impl BreakerGuard {
    /// Take the guard of the lock at `lock`. `None` while another holder
    /// has it. Where `flock` is not available (a file system without it, or
    /// Windows), the guard holds nothing.
    pub fn take(lock: &Path) -> Option<BreakerGuard> {
        os::BreakerGuard::take(lock).map(|held| BreakerGuard { _held: held })
    }
}
