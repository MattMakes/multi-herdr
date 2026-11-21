//! The protected file reader: the 1 function that reads an untrusted file
//! and refuses a link, a file that is not regular, another owner, a wide
//! mode, a real path outside its root, or a file over its size limit.
//!
//! Requirements: SR-5, SR-6, FM-23. The checks run in the order of
//! Protocol rule 7 of `docs/specs/fleetd/design.md`. A guard whose fact is
//! unknown does not hold. The access rule is the unix mode on macOS and
//! Linux, and the DACL on Windows ([`platform::check_owner_only`]).

use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::Path;

use horch_platform::channel::User;
use horch_platform::fs as platform;

/// The rule that a protected file must meet. The caller gives it.
#[derive(Debug, Clone, Copy)]
pub struct Rule<'a> {
    /// The folder that holds the file, for example the outbox of 1 attempt
    /// or the request folder. The real path of the file is inside it, and
    /// each folder from this root to the file has the owner and mode rule.
    pub root: &'a Path,
    /// The user that owns the ledger.
    pub owner: &'a User,
    /// The size limit of the type of the file, in bytes. A file of this
    /// size is read; a larger one is refused.
    pub limit: u64,
}

/// A file that failed 1 check. The caller records it in `ingested` and does
/// not read the path again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// Why the file failed, as the user reads it.
    pub reason: String,
}

/// Read the untrusted file `path` under `rule`. Returns its bytes, or the
/// reason of the first check that fails.
pub fn read_protected(path: &Path, rule: &Rule<'_>) -> Result<Vec<u8>, Rejected> {
    let before = fs::symlink_metadata(path).map_err(|e| reject(format!("stat failed: {e}")))?;
    if before.file_type().is_symlink() {
        return Err(reject("the file is a link".into()));
    }
    if !before.is_file() {
        return Err(reject("the file is not a regular file".into()));
    }
    let mut file = platform::open_no_follow(path)
        .map_err(|e| reject(format!("the open without a link failed: {e}")))?;
    let meta = file
        .metadata()
        .map_err(|e| reject(format!("stat of the open file failed: {e}")))?;
    if !meta.is_file() || platform::file_identity(&meta) != platform::file_identity(&before) {
        return Err(reject(
            "the file changed between the check and the open".into(),
        ));
    }
    let real_root = fs::canonicalize(rule.root)
        .map_err(|e| reject(format!("the real path of the root failed: {e}")))?;
    let real = fs::canonicalize(path)
        .map_err(|e| reject(format!("the real path of the file failed: {e}")))?;
    if !real.starts_with(&real_root) {
        return Err(reject(format!(
            "real path {} is outside the root {}",
            real.display(),
            real_root.display()
        )));
    }
    check_owner_and_mode(&file, &meta, rule.owner, 0o600).map_err(reject)?;
    check_folders(&real, &real_root, rule.owner)?;
    read_to_limit(&mut file, &meta, rule.limit)
}

/// Check each folder from the parent of `real` up to `real_root`.
fn check_folders(real: &Path, real_root: &Path, owner: &User) -> Result<(), Rejected> {
    for folder in real.ancestors().skip(1) {
        let in_folder = |text: String| reject(format!("folder {}: {text}", folder.display()));
        let handle = platform::open_no_follow(folder)
            .map_err(|e| in_folder(format!("the open without a link failed: {e}")))?;
        let meta = handle
            .metadata()
            .map_err(|e| in_folder(format!("stat failed: {e}")))?;
        if !meta.is_dir() {
            return Err(in_folder("not a folder".into()));
        }
        check_owner_and_mode(&handle, &meta, owner, 0o700).map_err(in_folder)?;
        if folder == real_root {
            break;
        }
    }
    Ok(())
}

/// Check that `owner` owns `file` and that only its owner has access to
/// it: on unix, its mode is `rule` or narrower. The error is the reason
/// text.
fn check_owner_and_mode(
    file: &File,
    meta: &Metadata,
    owner: &User,
    rule: u32,
) -> Result<(), String> {
    let found = platform::owner_of(file).map_err(|_| "owner unknown".to_string())?;
    if found != *owner {
        return Err(format!(
            "owner {} is not the ledger owner {}",
            user_text(&found),
            user_text(owner)
        ));
    }
    platform::check_owner_only(file, meta, rule)
}

/// Read `file` when its size is at most `limit`. A size over the limit is
/// refused before a read, and a read stops 1 byte after the limit, so a
/// file that grows after the check is refused without a read of all of it.
fn read_to_limit(file: &mut File, meta: &Metadata, limit: u64) -> Result<Vec<u8>, Rejected> {
    if meta.len() > limit {
        return Err(reject(format!(
            "size {} bytes is over the limit of {limit} bytes",
            meta.len()
        )));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| reject(format!("the read failed: {e}")))?;
    if bytes.len() as u64 > limit {
        return Err(reject(format!(
            "the file grew over the limit of {limit} bytes while it was read"
        )));
    }
    Ok(bytes)
}

fn user_text(user: &User) -> String {
    match user {
        User::Uid(uid) => format!("uid {uid}"),
        User::Sid(sid) => format!("sid {sid}"),
    }
}

fn reject(reason: String) -> Rejected {
    Rejected { reason }
}
