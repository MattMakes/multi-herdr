//! The ingest of the outbox files of each live attempt: `progress.jsonl` to
//! its last complete line, and the handoff result, each 1 time.
//!
//! Requirements: SR-6, RR-8, FM-23. Ticket 29 of fleetd-p5 fills it.
