//! Effort levels: which ones each agent's CLI takes, and why one is refused.

use serde::{Deserialize, Serialize};

use super::{reserved_tier, Agent};

/// One effort level as a teammate file writes it, for example `medium`.
///
/// Additive: the checks below still take `&str`, so a caller may use either.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Effort(String);

impl Effort {
    pub fn new(level: impl Into<String>) -> Self {
        Effort(level.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Effort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The effort levels each agent's CLI accepts, by name.
///
/// Same field, different mechanism per agent (see `_template.md`), and the
/// sets differ: codex has `none` but rejects `minimal`, pi has `off`, claude
/// tops out at `max`. Checked at `--check` and at `horch spawn --effort`, so a
/// typo fails before a pane starts rather than inside one nobody watches.
pub fn valid_efforts(agent: Agent) -> &'static [&'static str] {
    agent.capabilities().effort
}

/// Whether `model` has any effort setting at all on `agent`.
///
/// Claude's Haiku 4.5 has none, so claude rejects `--effort` for it. The
/// OpenCode free-tier models report `variants: {}` (`opencode models
/// --verbose`, ai_docs/reports/env-research/codex-opencode.md), so an effort
/// there is silently a no-op - worse than an error, because the file then
/// claims a setting that is not happening. Each harness module owns its rule.
pub fn model_takes_effort(agent: Agent, model: &str) -> bool {
    agent.model_takes_effort(model)
}

/// Why `effort` cannot be used with this agent and model, or `None` if it can.
pub fn effort_problem(agent: Agent, model: Option<&str>, effort: &str) -> Option<String> {
    let model = model.unwrap_or_default();
    if !model.is_empty() && !model_takes_effort(agent, model) {
        return Some(format!(
            "{model} has no effort setting on {agent}; remove effort (it would be \
             ignored, or refused)"
        ));
    }
    if agent == Agent::Codex {
        // Both are accepted by some codex clients and both are traps.
        match effort {
            "minimal" => {
                return Some(
                    "codex effort 'minimal' is an API error on the gpt-5.6 models; use low".into(),
                )
            }
            "ultra" => {
                return Some(
                    "codex effort 'ultra' fans out to parallel client-side agents and \
                     multiplies spend; use max"
                        .into(),
                )
            }
            "none" if reserved_tier(model).is_some() => {
                return Some(format!("{model} does not accept effort 'none'; use low"))
            }
            _ => {}
        }
    }
    let valid = valid_efforts(agent);
    if valid.contains(&effort) {
        None
    } else {
        Some(format!(
            "effort '{effort}' is not a {agent} level (expected one of: {})",
            valid.join(", ")
        ))
    }
}
