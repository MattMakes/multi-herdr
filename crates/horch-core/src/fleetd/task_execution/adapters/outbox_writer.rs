//! The outbox writer of a worker command: find the own attempt by the
//! process chain, make the outbox when it is missing, and write each file
//! atomically.
//!
//! Requirements: FR-7, RR-5, RR-8, SR-6. Ticket 26 of fleetd-p5 fills it.
