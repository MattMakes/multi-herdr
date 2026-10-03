//! The System One decision-model seam. Every implementation is inert in this
//! branch: nothing here makes a network call.

pub mod inert;
pub(crate) mod system_one;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use system_one::{DecisionRequest, DecisionResponse};

/// A decision model that answers typed questions about a state.
pub trait DecisionModel {
    fn id(&self) -> &str;
    fn decide(&self, req: &DecisionRequest) -> Option<DecisionResponse>;
}

/// Which teacher, if any, produced the probabilities recorded on a row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeacherRef {
    pub id: String,
    pub probabilities: Option<BTreeMap<String, f64>>,
}

impl TeacherRef {
    pub fn none() -> Self {
        Self {
            id: "none".to_string(),
            probabilities: None,
        }
    }
}
