//! Fleet run facts (`docs/specs/fleet-dataset.md` §3-5).
//!
//! Every fleet spawn writes a start row, every finished worker or
//! orchestrator record gets one run row, and `horch verdict` writes a
//! verdict row. The rows live in `<dataset dir>/fleet/`, a stream of its own:
//! the round projection folds every event under `events/` into a round, and
//! a fleet run is not a round.
//!
//! | module      | owns                                                     |
//! |-------------|----------------------------------------------------------|
//! | [`store`]   | the `fleet/` files: locked appends, torn-line reads       |
//! | [`rows`]    | the 3 row schemas                                        |
//! | [`facts`]   | the plan rule, the start row and the run row             |
//! | [`sync`]    | run rows for finished records that have none             |
//! | [`verdict`] | the orchestrator's verdict on a record                   |
//!
//! Recording informs, it never decides (FD5): nothing that routes or picks
//! a teammate reads this module.

pub mod facts;
pub mod rows;
pub mod store;
pub mod sync;
pub mod verdict;
