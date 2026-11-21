use std::ffi::CStr;
use std::io;
use std::path::Path;

pub(crate) use crate::unix::fs::{
    check_owner_only, create_dir_mode, create_dir_owner_only, create_mode, create_new_owner_only,
    current_uid, device_and_inode, file_identity, is_executable, make_executable, mode_of,
    open_no_block, open_no_follow, owner_and_mode, owner_of, remove_dir_below, set_mode, sync_dir,
    BreakerGuard,
};

pub(crate) fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    crate::unix::fs::rename_noreplace(from, to, rename_noreplace_at)
}

fn rename_noreplace_at(from: &CStr, to: &CStr) -> libc::c_int {
    // SAFETY: both arguments are NUL-terminated paths that outlive the call.
    unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    }
}
