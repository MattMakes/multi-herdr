//! The concern files of Phase 1 (PL-2): the fleetd lock, the awake clock,
//! the service, file watching and the command channel. Tickets 05 to 08
//! fill them.

#[allow(unused_imports)]
use horch_platform::awake;
#[allow(unused_imports)]
use horch_platform::channel;
#[allow(unused_imports)]
use horch_platform::lock;
#[allow(unused_imports)]
use horch_platform::service;
#[allow(unused_imports)]
use horch_platform::watch;

/// The 5 concern modules exist. This test compiles only when they do.
#[test]
fn phase1_concern_modules_exist() {}
