//! Effort levels: which ones each agent's CLI takes, and why one is refused.

use super::reserved_tier;
use crate::harness::HarnessKind;

/// The effort levels each agent's CLI accepts, by name.
///
/// Same field, different mechanism per agent (see `_template.md`), and the
/// sets differ: codex has `none` but rejects `minimal`, pi has `off`, claude
/// tops out at `max`. Checked at `--check` and at `horch spawn --effort`, so a
/// typo fails before a pane starts rather than inside one nobody watches.
pub(crate) fn valid_efforts(agent: HarnessKind) -> &'static [&'static str] {
    agent.capabilities().effort
}

/// Whether `model` has any effort setting at all on `agent`.
///
/// Claude's Haiku 4.5 has none, so claude rejects `--effort` for it. The
/// OpenCode free-tier models report `variants: {}` (`opencode models
/// --verbose`), so an effort
/// there is silently a no-op - worse than an error, because the file then
/// claims a setting that is not happening. Each harness module owns its rule.
pub fn model_takes_effort(agent: HarnessKind, model: &str) -> bool {
    agent.model_takes_effort(model)
}

/// Why `effort` cannot be used with this agent and model, or `None` if it can.
///
/// The order of the three rules is the contract: the first message wins.
pub(crate) fn effort_problem(
    agent: HarnessKind,
    model: Option<&str>,
    effort: &str,
) -> Option<String> {
    let model = model.unwrap_or_default();
    model_effort_problem(agent, model)
        .or_else(|| {
            (agent == HarnessKind::Codex)
                .then(|| codex_effort_trap(model, effort))
                .flatten()
        })
        .or_else(|| level_problem(agent, effort))
}

/// Rule 1: the model has no effort setting on this agent.
fn model_effort_problem(agent: HarnessKind, model: &str) -> Option<String> {
    (!model.is_empty() && !model_takes_effort(agent, model)).then(|| {
        format!(
            "{model} has no effort setting on {agent}; remove effort (it would be \
             ignored, or refused)"
        )
    })
}

/// Rule 2: codex levels that some clients accept and that are traps.
fn codex_effort_trap(model: &str, effort: &str) -> Option<String> {
    match effort {
        "minimal" => {
            Some("codex effort 'minimal' is an API error on the gpt-5.6 models; use low".into())
        }
        "ultra" => Some(
            "codex effort 'ultra' fans out to parallel client-side agents and \
             multiplies spend; use max"
                .into(),
        ),
        "none" if reserved_tier(model).is_some() => {
            Some(format!("{model} does not accept effort 'none'; use low"))
        }
        _ => None,
    }
}

/// Rule 3: the level is not one this agent's CLI takes.
fn level_problem(agent: HarnessKind, effort: &str) -> Option<String> {
    let valid = valid_efforts(agent);
    (!valid.contains(&effort)).then(|| {
        format!(
            "effort '{effort}' is not a {agent} level (expected one of: {})",
            valid.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_rule_gives_its_exact_message_and_the_first_rule_wins() {
        assert_eq!(
            effort_problem(HarnessKind::Claude, Some("haiku"), "low").as_deref(),
            Some(
                "haiku has no effort setting on claude; remove effort (it would be \
                 ignored, or refused)"
            )
        );
        assert_eq!(
            effort_problem(HarnessKind::Codex, Some("gpt-5.6-sol"), "minimal").as_deref(),
            Some("codex effort 'minimal' is an API error on the gpt-5.6 models; use low")
        );
        assert_eq!(
            effort_problem(HarnessKind::Codex, Some("gpt-5.6-sol"), "ultra").as_deref(),
            Some(
                "codex effort 'ultra' fans out to parallel client-side agents and \
                 multiplies spend; use max"
            )
        );
        assert_eq!(
            effort_problem(HarnessKind::Codex, Some("gpt-6-astra"), "none").as_deref(),
            Some("gpt-6-astra does not accept effort 'none'; use low")
        );
        assert_eq!(
            effort_problem(HarnessKind::Pi, Some("ollama/qwen3.8"), "ultra").as_deref(),
            Some(
                format!(
                    "effort 'ultra' is not a pi level (expected one of: {})",
                    valid_efforts(HarnessKind::Pi).join(", ")
                )
                .as_str()
            )
        );
        // `none` on a codex model without a reserved tier is a valid level.
        assert_eq!(
            effort_problem(HarnessKind::Codex, Some("gpt-5.6-sol"), "none"),
            None
        );
        // Rule 1 beats rule 3: a no-effort model reports itself, not the level.
        assert!(effort_problem(HarnessKind::Claude, Some("haiku"), "bogus")
            .unwrap()
            .starts_with("haiku has no effort setting"));
    }
}
