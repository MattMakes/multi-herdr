use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::Path;

use horch_platform::channel::User;
use horch_platform::fs as platform;

#[derive(Debug, Clone, Copy)]
pub struct Rule<'a> {
    pub root: &'a Path,
    pub owner: &'a User,
    pub limit: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    pub reason: String,
}

pub fn read_protected(path: &Path, rule: &Rule<'_>) -> Result<Vec<u8>, Rejected> {
    let identity =
        platform::file_identity_at(path).map_err(|e| reject(format!("stat failed: {e}")))?;
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
    let opened = platform::file_identity_of(&file)
        .map_err(|e| reject(format!("stat of the open file failed: {e}")))?;
    if !meta.is_file() || opened != identity {
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
