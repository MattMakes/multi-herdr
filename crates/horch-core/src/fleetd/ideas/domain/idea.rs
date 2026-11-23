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

pub const FIRST: &str = "captured";

pub const TERMINAL: [&str; 2] = ["merged", "cancelled"];
