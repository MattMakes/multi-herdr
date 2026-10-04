//! The System One decision-model seam. Every implementation is inert in this
//! branch: nothing here makes a network call.
//!
//! Nothing outside this module calls [`DecisionModel`] yet. The seam is kept
//! on purpose: it is the inert Clef/Laya decision (OD4), see
//! `ai_docs/designs/2026-10-02-dataset-competition-design.md` §1.3. A later teacher implements the trait.

pub mod inert;
pub(crate) mod system_one;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use system_one::{DecisionRequest, DecisionResponse};

/// A decision model that answers typed questions about a state. The designed
/// seam for Clef/Laya (OD4, `ai_docs/designs/2026-10-02-dataset-competition-design.md` §1.3); [`inert::Inert`] is the only one.
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
