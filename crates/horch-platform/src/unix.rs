//! Code that `mac` and `linux` share. Only those 2 modules call it.
//! It is not a 4th OS module.

mod process;
mod signal;
mod group;
mod fs;
mod path;
mod ipc;
mod shell;
mod machine;
mod rest;
