//! The apply of a request: its `requests` row goes from pending to applied
//! or refused in 1 transaction, and a seen `request_id` gets its stored
//! result with no second effect.
//!
//! Requirements: RR-8, SR-8, FM-15. Ticket 12 of fleetd-p5 fills it.
