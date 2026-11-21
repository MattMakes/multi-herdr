//! Test support shared by the CLI tests of `horch`.

pub mod bind;
// Not each test file that uses `support` starts a test fleetd.
#[allow(dead_code)]
pub mod fleetd;
