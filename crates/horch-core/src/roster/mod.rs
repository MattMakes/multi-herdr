//! The roster: one markdown file per team member, read at run time.
//!
//! Every briefing this fleet sends to an agent originates in `teammates/`. This
//! module reads those files and substitutes placeholders; it never composes,
//! paraphrases, or conditionally rewrites their prose. A prompt that appears in
//! a pane is byte-for-byte what the `.md` file says, with `{name}` spans
//! replaced. If a briefing needs to change, the file changes.
//!
//! Layering, lowest precedence first:
//!
//! | layer                        | why |
//! |------------------------------|-----|
//! | compiled-in (`build.rs`)     | `horch install` is a plain binary copy, so a fleet must spawn with no repo checked out |
//! | `~/.config/horch/teammates`  | user-wide additions |
//! | `$HORCH_TEAMMATES_DIR`       | this repo's folder, exported by the justfile; tests and one-offs |
//!
//! Overlay is by whole entry, keyed on name. `<cwd>/teammates` is deliberately
//! NOT searched: a worker's cwd is the target project, and a stray folder there
//! would silently re-brief the fleet.
//!
//! | module         | holds |
//! |----------------|-------|
//! | [`teammate`]   | the frontmatter types and the orchestrator-only constants |
//! | [`phase`]      | [`Phase`] |
//! | [`effort`]     | [`Effort`] and the per-agent effort checks |
//! | [`permission`] | [`PermissionMode`] |
//! | [`parser`]     | frontmatter splitting and parsing |
//! | [`repository`] | [`Roster`]: built-ins and overlays |
//! | [`validation`] | `--check`: [`Roster::check`] and the fallback rules |
//! | [`operator`]   | the operator's `~/.claude/settings.json` and effort overrides |
//!
//! Nothing here reads the process environment: the home directory and the
//! roster override arrive as parameters.

pub(crate) mod effort;
pub(crate) mod operator;
pub(crate) mod parser;
pub(crate) mod permission;
pub(crate) mod phase;
pub(crate) mod repository;
pub(crate) mod teammate;
pub mod validation;

pub(crate) use effort::effort_problem;
pub use effort::{model_takes_effort, Effort};
pub use operator::operator_effort_warnings;
pub(crate) use operator::{expand_home, operator_enabled_plugins, operator_status_line};
pub use permission::PermissionMode;
pub use phase::Phase;
pub use repository::{Roster, TEMPLATE};
pub use teammate::{reserved_tier, Base, ExecRule, Teammate};
pub(crate) use teammate::{
    BRIEF_DESCRIPTION_MAX, FLEET_ORCHESTRATORS, ORCHESTRATOR_DENIED_TOOLS, ORCHESTRATOR_ONLY_SKILLS,
};
pub use validation::{fallback_problems, fallback_warnings};

#[cfg(test)]
mod tests;
