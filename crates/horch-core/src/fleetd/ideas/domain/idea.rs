//! The idea entity: its 10 states and the exits of each state.
//!
//! The exits are the rows of `Entity::Idea` in the table of the 1 door
//! (`fleet_runtime::domain::ledger::EXITS`), and each state change goes
//! through `transition()` (`docs/specs/fleetd/design.md`, "The idea state
//! machine"; "Ledger", rule 1 "One door").

/// The 10 states of the design table, as the CHECK of `ideas.state` lists
/// them.
pub const STATES: [&str; 10] = [
    "captured",
    "clarifying",
    "awaiting_approval",
    "specifying",
    "ticketing",
    "in_progress",
    "integrating",
    "merged",
    "parked",
    "cancelled",
];

/// The state of a new idea: each `horch idea add` writes 1 row in it.
pub const FIRST: &str = "captured";

/// The states with no exit.
pub const TERMINAL: [&str; 2] = ["merged", "cancelled"];
