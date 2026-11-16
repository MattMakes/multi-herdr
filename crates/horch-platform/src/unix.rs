//! Code that `mac` and `linux` share. Only those 2 modules call it.
//! It is not a 4th OS module.

pub(crate) mod process;
pub(crate) mod signal;
pub(crate) mod group;
pub(crate) mod fs;
pub(crate) mod path;
pub(crate) mod ipc;
pub(crate) mod shell;
pub(crate) mod machine;
pub(crate) mod rest;
