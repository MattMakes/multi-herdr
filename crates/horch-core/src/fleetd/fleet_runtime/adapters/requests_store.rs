//! The `requests` rows in SQLite: insert a pending row, record its decision
//! and its result, and read the stored result of a seen `request_id`.
//!
//! Requirements: RR-8, SR-8, FM-15. Ticket 12 of fleetd-p5 fills it.
