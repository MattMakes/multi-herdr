use super::system_one::{DecisionRequest, DecisionResponse};
use super::DecisionModel;

/// The teacher that is always on in this branch: it never answers. No caller
/// uses it yet; it is the OD4 seam, see `ai_docs/designs/2026-10-02-dataset-competition-design.md` §1.3.
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
