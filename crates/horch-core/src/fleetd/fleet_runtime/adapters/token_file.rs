//! The operator token `<fleet root>/run/operator.token`: fleetd makes it 1
//! time with mode 0600, a `horch` command reads it, and fleetd compares it
//! in constant time.
//!
//! Requirements: SR-8. Ticket 10 of fleetd-p5 fills it.
