pub(crate) mod ceiling;
pub(crate) mod effort;
pub(crate) mod offer;
pub(crate) mod operator;
pub(crate) mod parser;
pub(crate) mod permission;
pub(crate) mod phase;
pub(crate) mod repository;
pub(crate) mod teammate;
pub mod validation;

pub(crate) use effort::effort_problem;
pub use effort::model_takes_effort;
pub use offer::{offered_in, project_skills, with_project_skills, ProjectFacts, Requirement};
pub use operator::operator_effort_warnings;
pub(crate) use operator::{expand_home, operator_enabled_plugins, operator_status_line};
pub use permission::PermissionMode;
pub use phase::Phase;
pub use repository::{Roster, TEMPLATE};
pub use teammate::{
    reserved_tier, Base, ExecDecision, ExecRule, HarnessDefault, HorchAccess, ModelSource,
    OperatorSkills, Teammate,
};
pub(crate) use teammate::{
    BRIEF_DESCRIPTION_MAX, FLEET_ORCHESTRATORS, ORCHESTRATOR_DENIED_TOOLS,
    ORCHESTRATOR_ONLY_SKILLS, SEATS_WITH_FALLBACKS, TOP_TIER_SEATS,
};
pub use validation::{fallback_problems, fallback_warnings};

#[cfg(test)]
mod tests;
