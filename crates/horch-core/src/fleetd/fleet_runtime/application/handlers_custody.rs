//! The handlers of the custody, job and integration request kinds:
//! `job_requeue`, `worktree_scratch`, `worktree_drop`, `worktree_restore`,
//! `worktree_triage`, `feature_abandon`, `branch_abandon` and `rescue_drop`.
//!
//! Requirements: RR-5, RR-8, RR-13 to RR-18. Ticket 20 of fleetd-p5 fills it.
