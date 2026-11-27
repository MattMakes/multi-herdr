use super::reserved_tier;
use crate::harness::HarnessKind;

pub(crate) fn valid_efforts(agent: HarnessKind) -> &'static [&'static str] {
    agent.capabilities().effort
}

pub fn model_takes_effort(agent: HarnessKind, model: &str) -> bool {
    agent.model_takes_effort(model)
}

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

fn model_effort_problem(agent: HarnessKind, model: &str) -> Option<String> {
    (!model.is_empty() && !model_takes_effort(agent, model)).then(|| {
        format!(
            "{model} has no effort setting on {agent}; remove effort (it would be \
             ignored, or refused)"
        )
    })
}

fn codex_effort_trap(model: &str, effort: &str) -> Option<String> {
    match effort {
        "minimal" => Some("codex effort 'minimal' is an API error on gpt-* models; use low".into()),
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
            effort_problem(HarnessKind::Codex, Some("gpt-6.2-sol"), "minimal").as_deref(),
            Some("codex effort 'minimal' is an API error on gpt-* models; use low")
        );
        assert_eq!(
            effort_problem(HarnessKind::Codex, Some("gpt-6.2-sol"), "ultra").as_deref(),
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
        assert_eq!(
            effort_problem(HarnessKind::Codex, Some("gpt-6.2-sol"), "none"),
            None
        );
        assert!(effort_problem(HarnessKind::Claude, Some("haiku"), "bogus")
            .unwrap()
            .starts_with("haiku has no effort setting"));
    }
}
