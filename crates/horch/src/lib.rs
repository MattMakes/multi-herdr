//! The parts of the `horch` binary that every entry point shares: reading the
//! process environment once ([`bootstrap`]), writing to stdout ([`output`]),
//! and the exit codes ([`exit`]).

pub mod bootstrap;
pub mod exit;
pub mod output;
