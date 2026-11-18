//! Reads the outbox files of an attempt through the protected file reader,
//! with the owner and mode checks, and refuses a foreign-owned outbox.
//!
//! Requirements: SR-6, RR-8, FM-23. Ticket 29 of fleetd-p5 fills it.
