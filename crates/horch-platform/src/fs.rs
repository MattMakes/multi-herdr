use std::fs::{DirBuilder, File, Metadata, OpenOptions};
use std::io;
use std::path::Path;

use crate::channel::User;
use crate::os::fs as os;
use crate::NotSupported;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerAndMode {
    pub uid: u32,
    pub mode: u32,
}

pub fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    os::set_mode(path, mode)
}

pub fn create_mode(options: &mut OpenOptions, mode: u32) -> Result<(), NotSupported> {
    os::create_mode(options, mode)
}

pub fn create_dir_mode(builder: &mut DirBuilder, mode: u32) -> Result<(), NotSupported> {
    os::create_dir_mode(builder, mode)
}

pub fn create_dir_owner_only(path: &Path) -> io::Result<()> {
    os::create_dir_owner_only(path)
}

pub fn create_new_owner_only(path: &Path) -> io::Result<File> {
    os::create_new_owner_only(path)
}

pub fn check_owner_only(file: &File, meta: &Metadata, rule: u32) -> Result<(), String> {
    os::check_owner_only(file, meta, rule)
}

pub fn sync_dir(dir: &Path) -> io::Result<()> {
    os::sync_dir(dir)
}

pub fn open_no_block(options: &mut OpenOptions) -> Result<(), NotSupported> {
    os::open_no_block(options)
}

pub fn open_no_follow(path: &Path) -> io::Result<File> {
    os::open_no_follow(path)
}

pub fn owner_of(file: &File) -> io::Result<User> {
    os::owner_of(file)
}

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

pub fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    os::rename_noreplace(from, to)
}

pub fn owner_and_mode(meta: &Metadata) -> Result<OwnerAndMode, NotSupported> {
    os::owner_and_mode(meta)
}

pub fn mode_of(meta: &Metadata) -> Result<u32, NotSupported> {
    os::mode_of(meta)
}

pub fn current_uid() -> Result<u32, NotSupported> {
    os::current_uid()
}

pub fn is_executable(meta: &Metadata) -> bool {
    os::is_executable(meta)
}

pub fn make_executable(path: &Path) -> io::Result<()> {
    os::make_executable(path)
}

pub fn file_identity_of(file: &File) -> io::Result<(u64, u64)> {
    os::file_identity_of(file)
}

pub fn file_identity_at(path: &Path) -> io::Result<(u64, u64)> {
    os::file_identity_at(path)
}

pub fn file_identity_of_target(path: &Path) -> io::Result<(u64, u64)> {
    os::file_identity_of_target(path)
}

pub fn device_and_inode(meta: &Metadata) -> Result<(u64, u64), NotSupported> {
    os::device_and_inode(meta)
}

#[derive(Debug)]
pub struct BreakerGuard {
    _held: os::BreakerGuard,
}

impl BreakerGuard {
    pub fn take(lock: &Path) -> Option<BreakerGuard> {
        os::BreakerGuard::take(lock).map(|held| BreakerGuard { _held: held })
    }
}
