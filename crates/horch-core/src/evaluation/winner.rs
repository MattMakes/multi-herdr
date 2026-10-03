//! The winner policy (JDG-06, JDG-07): how a parsed judgment becomes a
//! winner, a rejection or a request for the operator.

use serde::{Deserialize, Serialize};

/// The judge's policy for accepting a winner. It enters the judge policy
/// digest as canonical JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WinnerPolicy {
    /// A `winner` verdict below this confidence needs the operator.
    pub min_confidence: f64,
    pub tie_break: TieBreak,
}

impl Default for WinnerPolicy {
    fn default() -> Self {
        Self {
            min_confidence: 0.7,
            tie_break: TieBreak::Disabled,
        }
    }
}

/// SPEC-TODO(Spec B §11): the utility definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TieBreak {
    Disabled,
    /// `v` is the version of the utility rule.
    Utility {
        v: String,
    },
}
