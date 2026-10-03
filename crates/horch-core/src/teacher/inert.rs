use super::system_one::{DecisionRequest, DecisionResponse};
use super::DecisionModel;

/// The teacher that is always on in this branch: it never answers.
#[derive(Debug, Clone, Copy, Default)]
pub struct Inert;

impl DecisionModel for Inert {
    fn id(&self) -> &str {
        "none"
    }

    fn decide(&self, _req: &DecisionRequest) -> Option<DecisionResponse> {
        None
    }
}
