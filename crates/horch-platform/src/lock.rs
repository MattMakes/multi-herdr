use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

#[derive(Debug)]
pub struct FleetdLock {
    _file: File,
}

impl FleetdLock {
    pub fn try_take(path: &Path) -> io::Result<Option<FleetdLock>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(FleetdLock { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(err)) => Err(err),
        }
    }
}
