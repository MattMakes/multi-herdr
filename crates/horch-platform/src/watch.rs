use std::io;
use std::path::{Path, PathBuf};

use crate::os::watch as os;

#[derive(Debug)]
pub struct Watcher {
    inner: os::Watcher,
}

impl Watcher {
    pub fn new() -> io::Result<Watcher> {
        Ok(Watcher {
            inner: os::Watcher::new()?,
        })
    }

    pub fn add(&mut self, dir: &Path) -> io::Result<()> {
        self.inner.add(dir)
    }

    pub fn remove(&mut self, dir: &Path) {
        self.inner.remove(dir)
    }

    pub fn changed(&mut self) -> Vec<PathBuf> {
        self.inner.changed()
    }
}
