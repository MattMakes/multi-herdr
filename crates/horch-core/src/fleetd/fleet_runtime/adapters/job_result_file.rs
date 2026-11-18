//! The job result file `<fleet root>/run/jobs/<job id>-<run>.json`: a job
//! process writes it atomically, and fleetd reads it with the checks and
//! ignores a result of another run.
//!
//! Requirements: RR-5, RR-8, SR-6. Ticket 33 of fleetd-p5 fills it.
