use std::fmt;

pub mod awake;
pub mod channel;
pub mod fs;
pub mod group;
pub mod ipc;
pub mod lock;
pub mod machine;
pub mod path;
pub mod process;
pub mod rest;
pub mod sandbox;
pub mod service;
pub mod shell;
pub mod signal;
pub mod start;
pub mod watch;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
compile_error!("horch-platform supports macOS, Linux and Windows");

#[cfg(target_os = "linux")]
#[allow(unused_imports)]
use linux as os;
#[cfg(target_os = "macos")]
#[allow(unused_imports)]
use mac as os;
#[cfg(windows)]
#[allow(unused_imports)]
use windows as os;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct NotSupported {
    pub concern: &'static str,
    pub os: &'static str,
    pub reason: Option<&'static str>,
}

impl NotSupported {
    pub const fn new(concern: &'static str, os: &'static str) -> Self {
        NotSupported {
            concern,
            os,
            reason: None,
        }
    }

    pub const fn because(self, reason: &'static str) -> Self {
        NotSupported {
            reason: Some(reason),
            ..self
        }
    }
}

impl fmt::Display for NotSupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is not supported on {}", self.concern, self.os)?;
        match self.reason {
            Some(reason) => write!(f, ": {reason}"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for NotSupported {}

impl From<NotSupported> for std::io::Error {
    fn from(value: NotSupported) -> Self {
        std::io::Error::new(std::io::ErrorKind::Unsupported, value)
    }
}
