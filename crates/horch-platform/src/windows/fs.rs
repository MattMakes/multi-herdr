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
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    EqualSid, GetAce, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, IsWellKnownSid,
    WinBuiltinAdministratorsSid, WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL,
    DACL_SECURITY_INFORMATION, OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, READ_CONTROL, WRITE_DAC, WRITE_OWNER,
};

use crate::channel::User;
use crate::fs::OwnerAndMode;
use crate::windows::channel::{current_user, sid_text};
use crate::NotSupported;

const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
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
        OwnerOnly::from_sddl(&format!("O:{sid}D:P(A;{inherit};FA;;;{sid})"))
    }

    fn from_sddl(text: &str) -> io::Result<OwnerOnly> {
        let text = wide_text(text);
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

    /// Give the existing folder or file `path` the owner and the protected
    /// DACL of this descriptor. The DACL goes first: an owner holds
    /// `WRITE_DAC` but not always `WRITE_OWNER`, and the new DACL gives this
    /// user all access, `WRITE_OWNER` too.
    fn set_on(&self, path: &Path) -> io::Result<()> {
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl: *mut ACL = ptr::null_mut();
        let mut owner: PSID = ptr::null_mut();
        // SAFETY: `self.0` is a valid descriptor, and each output is valid
        // for 1 write. `dacl` and `owner` point into `self.0`, which lives
        // until the end of this function.
        let read = unsafe {
            GetSecurityDescriptorDacl(self.0, &mut present, &mut dacl, &mut defaulted) != 0
                && GetSecurityDescriptorOwner(self.0, &mut owner, &mut defaulted) != 0
        };
        if !read {
            return Err(io::Error::last_os_error());
        }
        let info = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
        set_security(path, WRITE_DAC, info, ptr::null_mut(), dacl)?;
        set_security(
            path,
            WRITE_OWNER,
            OWNER_SECURITY_INFORMATION,
            owner,
            ptr::null(),
        )
    }
}

fn set_security(
    path: &Path,
    access: u32,
    info: OBJECT_SECURITY_INFORMATION,
    owner: PSID,
    dacl: *const ACL,
) -> io::Result<()> {
    let file = OpenOptions::new()
        .access_mode(access | READ_CONTROL | FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is a reparse point", path.display()),
        ));
    }
    // SAFETY: the handle of `file` is open for the call, and `owner` and
    // `dacl` are null or valid for the call. The group and SACL are null,
    // because `info` does not name them.
    let status = unsafe {
        SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            info,
            owner,
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    Ok(())
}

impl Drop for OwnerOnly {
    fn drop(&mut self) {
        // SAFETY: the system allocated the descriptor with LocalAlloc, and
        // only this drop frees it.
        unsafe { LocalFree(self.0) };
    }
}

fn wide_text(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain([0]).collect()
}

pub(crate) fn create_dir_owner_only(path: &Path) -> io::Result<()> {
    if !path.is_dir() {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            make_missing_dirs(parent)?;
        }
        if make_dir(path)? {
            return Ok(());
        }
    }
    keep_owner_only(path)
}

fn make_missing_dirs(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        make_missing_dirs(parent)?;
    }
    make_dir(path).map(|_| ())
}

fn make_dir(path: &Path) -> io::Result<bool> {
    let descriptor = OwnerOnly::new("OICI")?;
    let attributes = descriptor.attributes();
    let name = wide_path(path);
    // SAFETY: `name` ends with a NUL, and `attributes` and the descriptor
    // that it points to live until the call returns.
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 {
        let e = io::Error::last_os_error();
        if e.kind() == io::ErrorKind::AlreadyExists && path.is_dir() {
            return Ok(false);
        }
        return Err(e);
    }
    Ok(true)
}

/// Make sure that this user owns the existing folder `path` and that its
/// DACL is owner-only, as [`check_owner_only`] checks it. An older horch
/// made the folder with an inherited DACL, or with the owner
/// `Administrators` on an elevated token (D266-F1). When the check fails,
/// this user becomes the owner and the folder gets the protected DACL of a
/// new folder, then the check runs again. A folder that this user may not
/// change, a link, or a check that still fails is a `PermissionDenied`
/// error that names the folder.
fn keep_owner_only(path: &Path) -> io::Result<()> {
    let refused = |reason: String| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "folder {} is not owner-only, and horch cannot make it owner-only: {reason}",
                path.display()
            ),
        )
    };
    let me = current_user()?;
    if owner_only_gap(path, &me).map_err(refused)?.is_none() {
        return Ok(());
    }
    OwnerOnly::new("OICI")?.set_on(path).map_err(|e| {
        refused(format!(
            "the change of its owner and access list failed: {e}"
        ))
    })?;
    match owner_only_gap(path, &me).map_err(refused)? {
        None => Ok(()),
        Some(gap) => Err(refused(gap)),
    }
}

fn owner_only_gap(path: &Path, me: &User) -> Result<Option<String>, String> {
    let folder =
        open_no_follow(path).map_err(|e| format!("the open without a link failed: {e}"))?;
    let meta = folder.metadata().map_err(|e| format!("stat failed: {e}"))?;
    if !meta.is_dir() {
        return Err("not a folder".into());
    }
    let owner = owner_of(&folder).map_err(|e| format!("the owner is unknown: {e}"))?;
    if owner != *me {
        let found = match &owner {
            User::Sid(sid) => sid.clone(),
            User::Uid(uid) => uid.to_string(),
        };
        return Ok(Some(format!("its owner is {found}, not this user")));
    }
    Ok(check_owner_only(&folder, &meta, 0o700).err())
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

pub(crate) fn device_and_inode(_meta: &Metadata) -> Result<(u64, u64), NotSupported> {
    Err(NOT_SUPPORTED.because("stable Rust gives no volume serial number and file index"))
}

/// [`crate::fs::BreakerGuard`]: it holds nothing on Windows.
#[derive(Debug)]
pub(crate) struct BreakerGuard;

impl BreakerGuard {
    pub(crate) fn take(_lock: &Path) -> Option<BreakerGuard> {
        Some(BreakerGuard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dir_with(path: &Path, sddl: &str) {
        let descriptor = OwnerOnly::from_sddl(sddl).unwrap();
        let attributes = descriptor.attributes();
        let name = wide_path(path);
        // SAFETY: `name` ends with a NUL, and `attributes` and the
        // descriptor that it points to live until the call returns.
        let made = unsafe { CreateDirectoryW(name.as_ptr(), &attributes) };
        assert_ne!(made, 0, "{}", io::Error::last_os_error());
    }

    fn gap(path: &Path) -> Option<String> {
        owner_only_gap(path, &current_user().unwrap()).unwrap()
    }

    #[test]
    fn an_existing_folder_with_an_open_dacl_becomes_owner_only() {
        let root = tempfile::tempdir().unwrap();
        let jobs = root.path().join("jobs");
        // Each user has all access, and each file made in it inherits it.
        make_dir_with(&jobs, "D:(A;OICI;FA;;;WD)");
        assert!(gap(&jobs).is_some(), "the old folder is not owner-only");

        create_dir_owner_only(&jobs).unwrap();
        assert_eq!(gap(&jobs), None);

        let file = jobs.join("result.json");
        drop(create_new_owner_only(&file).unwrap());
        let opened = File::open(&file).unwrap();
        check_owner_only(&opened, &opened.metadata().unwrap(), 0o600).unwrap();
    }

    #[test]
    fn an_owner_only_folder_is_kept_and_a_second_call_changes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let jobs = root.path().join("run").join("jobs");
        create_dir_owner_only(&jobs).unwrap();
        create_dir_owner_only(&jobs).unwrap();
        assert_eq!(gap(&jobs), None);
        assert_eq!(gap(&root.path().join("run")), None);
    }

    #[test]
    fn an_existing_parent_keeps_its_access() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("shared");
        make_dir_with(&parent, "D:(A;OICI;FA;;;WD)");
        create_dir_owner_only(&parent.join("jobs")).unwrap();
        assert_eq!(gap(&parent.join("jobs")), None);
        assert!(gap(&parent).is_some(), "only the asked folder changes");
    }

    #[test]
    fn a_file_at_the_path_is_refused_with_the_folder_name() {
        let root = tempfile::tempdir().unwrap();
        let jobs = root.path().join("jobs");
        std::fs::write(&jobs, b"").unwrap();
        let e = create_dir_owner_only(&jobs).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists, "{e}");
    }
}
