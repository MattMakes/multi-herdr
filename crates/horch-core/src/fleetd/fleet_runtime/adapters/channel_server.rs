//! The command channel of fleetd: bind `<fleet root>/run/fleetd.sock` (a
//! pipe on Windows), check the peer user, read 1 request line with a
//! deadline, and write the reply of the recorded decision.
//!
//! Requirements: SR-5, SR-8, RR-8, FM-15. Ticket 13 of fleetd-p5 fills it.
