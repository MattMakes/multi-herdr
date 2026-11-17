//! The platform layer: the one place in the workspace that holds OS code
//! (PL-2). It has 1 interface file for each concern and 1 module for each
//! OS: `mac`, `linux` and `windows`. `unix` holds the code that `mac` and
//! `linux` share. Where an OS has no equivalent, a function of a concern
//! returns the explicit value [`NotSupported`] (PL-6), and core code decides
//! what to do with it.
//!
//! This crate depends on no other crate of the workspace.

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
pub mod service;
pub mod shell;
pub mod signal;
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

// The module of the build target. The concern files call the OS through
// this alias only. No concern uses it until tickets 03 to 09 fill them.
#[cfg(target_os = "linux")]
#[allow(unused_imports)]
use linux as os;
#[cfg(target_os = "macos")]
#[allow(unused_imports)]
use mac as os;
#[cfg(windows)]
#[allow(unused_imports)]
use windows as os;

/// A concern that the OS of the build target has no equivalent for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotSupported {
    /// The concern, for example `"signal"`.
    pub concern: &'static str,
    /// The OS, for example `"windows"`.
    pub os: &'static str,
}

impl fmt::Display for NotSupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is not supported on {}", self.concern, self.os)
    }
}

impl std::error::Error for NotSupported {}

impl From<NotSupported> for std::io::Error {
    fn from(value: NotSupported) -> Self {
        std::io::Error::new(std::io::ErrorKind::Unsupported, value)
    }
}
