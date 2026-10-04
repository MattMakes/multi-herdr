//! The work phase that selects a worker's portable skill catalog.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Work phase selecting the portable skill catalog for a worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Research,
    Plan,
    Implementation,
    Validation,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Research => "research",
            Self::Plan => "plan",
            Self::Implementation => "implementation",
            Self::Validation => "validation",
        }
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Phase {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "research" => Ok(Self::Research),
            "plan" => Ok(Self::Plan),
            "implementation" => Ok(Self::Implementation),
            "validation" => Ok(Self::Validation),
            _ => Err(format!(
                "unknown phase '{s}' (research, plan, implementation, validation)"
            )),
        }
    }
}
