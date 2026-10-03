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

pub mod effort;
pub mod operator;
pub mod parser;
pub mod permission;
pub mod phase;
pub mod repository;
pub mod teammate;
pub mod validation;

/// The harness enum under its pre-A1 name. Phase A12 removes this alias.
pub use crate::harness::HarnessKind as Agent;

pub use effort::{effort_problem, model_takes_effort, valid_efforts, Effort};
pub use operator::{
    effort_override_warnings, expand_home, operator_effort_warnings, operator_enabled_plugins,
    operator_status_line,
};
pub use permission::PermissionMode;
pub use phase::Phase;
pub use repository::{Roster, TEMPLATE};
pub use teammate::{
    reserved_tier, Base, ExecRule, Teammate, BRIEF_DESCRIPTION_MAX, FLEET_ORCHESTRATORS,
    ORCHESTRATOR_DENIED_TOOLS, ORCHESTRATOR_ONLY_SKILLS, ORCHESTRATOR_TIERS,
};
pub use validation::{fallback_problems, fallback_warnings};
