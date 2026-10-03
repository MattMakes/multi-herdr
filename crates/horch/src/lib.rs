//! The parts of the `horch` binary that every entry point shares: reading the
//! process environment once ([`bootstrap`]), writing to stdout ([`output`]),
//! and the exit codes ([`exit`]). [`dataset`] holds the commands of the
//! second binary, `multi-herdr-dataset`.

pub mod bootstrap;
pub mod dataset;
pub mod exit;
pub mod output;
