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
        .or_else(|| level_problem(agent, model, effort))
}

fn model_efforts(agent: HarnessKind, model: &str) -> &'static [&'static str] {
    if agent == HarnessKind::Antigravity && model.starts_with("gemini-4-neon") {
        &["xhigh"]
    } else {
        &[]
    }
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

fn level_problem(agent: HarnessKind, model: &str, effort: &str) -> Option<String> {
    let valid = [valid_efforts(agent), model_efforts(agent, model)].concat();
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
        assert_eq!(
            effort_problem(HarnessKind::Codex, Some("gpt-5.6-sol"), "none"),
            None
        );
        assert!(effort_problem(HarnessKind::Claude, Some("haiku"), "bogus")
            .unwrap()
            .starts_with("haiku has no effort setting"));
    }

    #[test]
    fn antigravity_takes_xhigh_only_for_gemini_4_neon() {
        let agy = HarnessKind::Antigravity;
        for effort in ["low", "medium", "high", "xhigh"] {
            assert_eq!(effort_problem(agy, Some("gemini-4-neon"), effort), None);
        }
        assert_eq!(
            effort_problem(agy, Some("gemini-4-neon"), "max").as_deref(),
            Some("effort 'max' is not a antigravity level (expected one of: low, medium, high, xhigh)")
        );
        for model in ["gemini-4-argon", "gemini-3.8-flash", "claude-opus-5-5"] {
            assert_eq!(
                effort_problem(agy, Some(model), "xhigh").as_deref(),
                Some("effort 'xhigh' is not a antigravity level (expected one of: low, medium, high)"),
                "{model}"
            );
        }
        assert_eq!(
            effort_problem(agy, None, "xhigh").as_deref(),
            Some("effort 'xhigh' is not a antigravity level (expected one of: low, medium, high)")
        );
    }
}
