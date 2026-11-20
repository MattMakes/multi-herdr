//! The concern fs for Windows. Each gap is documented in `crate::fs`.

use std::ffi::c_void;
use std::fs::{DirBuilder, File, Metadata, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{
    LocalFree, ERROR_SUCCESS, GENERIC_WRITE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    EqualSid, GetAce, IsWellKnownSid, WinBuiltinAdministratorsSid, WinLocalSystemSid,
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE,
};

use crate::channel::User;
use crate::fs::OwnerAndMode;
use crate::windows::channel::{current_user, sid_text};
use crate::NotSupported;

/// The `AceType` of an allow rule of a DACL.
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
/// The `AceType` of a deny rule of a DACL.
const ACCESS_DENIED_ACE_TYPE: u8 = 1;

const NOT_SUPPORTED: NotSupported = NotSupported::new("fs", "windows");

pub(crate) fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Err(NOT_SUPPORTED.into())
}

pub(crate) fn create_mode(_options: &mut OpenOptions, _mode: u32) -> Result<(), NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn create_dir_mode(_builder: &mut DirBuilder, _mode: u32) -> Result<(), NotSupported> {
    Err(NOT_SUPPORTED)
}

/// A security descriptor that makes this user the owner and allows only
/// this user, freed on drop.
struct OwnerOnly(PSECURITY_DESCRIPTOR);

impl OwnerOnly {
    /// `inherit` is the SDDL flags of the 1 rule: `OICI` for a folder, so
    /// each file and folder made in it gets the rule, and empty for a file.
    fn new(inherit: &str) -> io::Result<OwnerOnly> {
        let User::Sid(sid) = current_user()? else {
            unreachable!("a Windows user is a SID")
        };
        // `O:` sets the owner, also for an elevated token whose default
        // owner is `Administrators`. `D:P` is a protected DACL, so the
        // object takes no rule from its folder. `FA` is all file access.
        let text = wide_text(&format!("O:{sid}D:P(A;{inherit};FA;;;{sid})"));
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `text` ends with a NUL, and `descriptor` is valid for 1
        // write. The size pointer may be null.
        let made = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if made == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(OwnerOnly(descriptor))
    }

    /// The attributes of a create with this descriptor. They point into
    /// `self`, so `self` must live until the create returns.
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}

impl Drop for OwnerOnly {
    fn drop(&mut self) {
        // SAFETY: the system allocated the descriptor with LocalAlloc, and
        // only this drop frees it.
        unsafe { LocalFree(self.0) };
    }
}

/// `text` as UTF-16 with a NUL at the end.
fn wide_text(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// `path` as UTF-16 with a NUL at the end.
fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

pub(crate) fn create_dir_owner_only(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        create_dir_owner_only(parent)?;
    }
    let descriptor = OwnerOnly::new("OICI")?;
    let attributes = descriptor.attributes();
    let name = wide_path(path);
    // SAFETY: `name` ends with a NUL, and `attributes` and the descriptor
    // that it points to live until the call returns.
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 {
        let e = io::Error::last_os_error();
        // Another process made the folder after the check above.
        if e.kind() == io::ErrorKind::AlreadyExists && path.is_dir() {
            return Ok(());
        }
        return Err(e);
    }
    Ok(())
}

pub(crate) fn create_new_owner_only(path: &Path) -> io::Result<File> {
    let descriptor = OwnerOnly::new("")?;
    let attributes = descriptor.attributes();
    let name = wide_path(path);
    // SAFETY: `name` ends with a NUL, and `attributes` and the descriptor
    // that it points to live until the call returns. The template handle
    // may be null.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` is a new open handle that only the file owns.
    Ok(unsafe { File::from_raw_handle(handle) })
}

pub(crate) fn check_owner_only(file: &File, _meta: &Metadata, _rule: u32) -> Result<(), String> {
    let mut owner: PSID = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the handle of `file` is open for the call, and `owner`,
    // `dacl` and `descriptor` are valid for 1 write each. The other
    // outputs are null, because the call does not ask for them.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        let e = io::Error::from_raw_os_error(status as i32);
        return Err(format!("the access list is unknown: {e}"));
    }
    // SAFETY: `owner` and `dacl` point into `descriptor`, which lives
    // until the LocalFree below.
    let checked = unsafe { only_owner_allowed(owner, dacl) };
    // SAFETY: the system allocated `descriptor` with LocalAlloc, and
    // nothing reads it, `owner` or `dacl` after this call.
    unsafe { LocalFree(descriptor) };
    checked
}

/// Check that the DACL `dacl` allows no user other than `owner`, `SYSTEM`
/// and `Administrators`, and has only allow and deny rules. A null DACL
/// allows each user.
///
/// # Safety
///
/// `owner` is a valid SID, and `dacl` is null or a valid ACL, for the
/// length of the call.
unsafe fn only_owner_allowed(owner: PSID, dacl: *const ACL) -> Result<(), String> {
    if dacl.is_null() {
        return Err("the access list is missing, so each user has access".into());
    }
    // SAFETY: the caller gives a valid ACL.
    let count = unsafe { (*dacl).AceCount };
    for index in 0..u32::from(count) {
        let mut ace: *mut c_void = ptr::null_mut();
        // SAFETY: `dacl` is a valid ACL, `index` is below its count, and
        // `ace` is valid for 1 write.
        if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
            let e = io::Error::last_os_error();
            return Err(format!("rule {index} of the access list is unknown: {e}"));
        }
        // SAFETY: GetAce gives a pointer to a rule inside `dacl`, and each
        // rule starts with an ACE_HEADER.
        let kind = unsafe { (*ace.cast::<ACE_HEADER>()).AceType };
        match kind {
            ACCESS_DENIED_ACE_TYPE => {}
            ACCESS_ALLOWED_ACE_TYPE => {
                // SAFETY: an allow rule is an ACCESS_ALLOWED_ACE, and its SID
                // starts at `SidStart`.
                let sid: PSID =
                    unsafe { ptr::addr_of_mut!((*ace.cast::<ACCESS_ALLOWED_ACE>()).SidStart) }
                        .cast();
                // SAFETY: `sid` and `owner` are valid SIDs.
                let allowed = unsafe {
                    EqualSid(sid, owner) != 0
                        || IsWellKnownSid(sid, WinLocalSystemSid) != 0
                        || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
                };
                if !allowed {
                    // SAFETY: `sid` is a valid SID.
                    let text = unsafe { sid_text(sid) }
                        .unwrap_or_else(|e| format!("an unknown user ({e})"));
                    return Err(format!(
                        "the access list allows {text} too, not only the owner"
                    ));
                }
            }
            other => {
                return Err(format!(
                    "the access list has a rule of type {other}, not only allow and deny rules"
                ))
            }
        }
    }
    Ok(())
}

pub(crate) fn sync_dir(_dir: &Path) -> io::Result<()> {
    Err(NOT_SUPPORTED.into())
}

pub(crate) fn open_no_block(_options: &mut OpenOptions) -> Result<(), NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn remove_dir_below(_root: &Path, _names: &[&str]) -> io::Result<()> {
    Err(NOT_SUPPORTED.into())
}

pub(crate) fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

pub(crate) fn open_no_follow(path: &Path) -> io::Result<File> {
    // `FILE_FLAG_OPEN_REPARSE_POINT` opens a link itself, not its target.
    // `FILE_FLAG_BACKUP_SEMANTICS` lets the call open a folder too.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is a reparse point", path.display()),
        ));
    }
    Ok(file)
}

pub(crate) fn owner_of(file: &File) -> io::Result<User> {
    let mut owner: PSID = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: the handle of `file` is open for the call, and `owner` and
    // `descriptor` are valid for 1 write each. The other outputs are null,
    // because the call does not ask for them.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    // SAFETY: `owner` points into `descriptor`, which lives until the
    // LocalFree below.
    let sid = unsafe { sid_text(owner) };
    // SAFETY: the system allocated `descriptor` with LocalAlloc, and
    // nothing reads it or `owner` after this call.
    unsafe { LocalFree(descriptor) };
    sid.map(User::Sid)
}

pub(crate) fn owner_and_mode(_meta: &Metadata) -> Result<OwnerAndMode, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn mode_of(_meta: &Metadata) -> Result<u32, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn current_uid() -> Result<u32, NotSupported> {
    Err(NOT_SUPPORTED)
}

pub(crate) fn is_executable(meta: &Metadata) -> bool {
    meta.is_file()
}

pub(crate) fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

pub(crate) fn file_identity(meta: &Metadata) -> (u64, u64) {
    (0, meta.creation_time())
}

/// [`crate::fs::BreakerGuard`]: it holds nothing on Windows.
#[derive(Debug)]
pub(crate) struct BreakerGuard;

impl BreakerGuard {
    pub(crate) fn take(_lock: &Path) -> Option<BreakerGuard> {
        Some(BreakerGuard)
    }
}
