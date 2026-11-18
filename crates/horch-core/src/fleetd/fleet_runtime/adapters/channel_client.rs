//! The client of the channel: send 1 request, check that the peer is the
//! fleetd of the `fleetd` row, ask for the result after a lost reply, or
//! write a request file when fleetd is down.
//!
//! Requirements: RR-8, SR-5, SR-8, FM-15. Ticket 15 of fleetd-p5 fills it.
