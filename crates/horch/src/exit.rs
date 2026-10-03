//! The exit codes `horch` uses.

/// Success.
pub const SUCCESS: u8 = 0;
/// Any error that is not one of the codes below.
pub const FAILURE: u8 = 1;
/// `horch telemetry collect --once`: a live collector holds the lock.
pub const COLLECTOR_HELD: u8 = 2;
/// `horch spawn`: the usage-limit gate refused (design 13.4).
pub const REFUSED: u8 = 3;
