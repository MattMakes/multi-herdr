//! The skill paragraph of a worker's briefing, rendered from an activation
//! plan.

use std::path::Path;

use super::activation::{InvocationPolicy, SkillActivationPlan};
use super::catalog::SkillCatalog;
use crate::teammates::Phase;

/// What the paragraph needs besides the plan.
pub struct BriefingContext<'a> {
    pub phase: Option<Phase>,
    /// The teammate's `skills:` as declared: the expected list keeps this
    /// order, not the plan's id order.
    pub declared: &'a [String],
    /// A prefix for skill names, such as `horch` for a Claude plugin.
    pub namespace: Option<&'a str>,
    /// `- <plugin>:<skill>: <description>` lines for the plan's
    /// `plugin_skills`, already resolved. Empty when there are none.
    pub plugin_lines: &'a [String],
    /// Where the materialized skill files are.
    pub skills_dir: &'a Path,
}

/// The teammate's own `skills:` and its `plugin_skills` are EXPECTED, and
/// each is named with its description, so the worker knows when its step
/// has come. A bare list of names read as optional, and workers skipped
/// them. The rest of the phase catalog stays available by name only, which
/// keeps the context cost of a broad phase small.
pub fn render(plan: &SkillActivationPlan, catalog: &SkillCatalog, ctx: &BriefingContext) -> String {
    let qualified = |s: &str| match ctx.namespace {
        Some(ns) => format!("{ns}:{s}"),
        None => s.to_string(),
    };
    let explicit = |s: &str| {
        plan.activated
            .iter()
            .any(|r| r.id.as_str() == s && r.policy == InvocationPolicy::Explicit)
    };
    let mut expected: Vec<String> = ctx
        .declared
        .iter()
        .filter(|s| explicit(s))
        .map(|s| {
            let description = catalog
                .lookup(s)
                .map(|e| {
                    e.description
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            format!("- {}: {description}", qualified(s))
        })
        .collect();
    expected.extend(ctx.plugin_lines.iter().cloned());
    let others: Vec<String> = plan
        .activated
        .iter()
        .filter(|r| r.policy != InvocationPolicy::Explicit)
        .map(|r| qualified(r.id.as_str()))
        .collect();

    let phase = ctx
        .phase
        .map(|p| p.to_string())
        .unwrap_or_else(|| "custom".into());
    let mut out = format!("\n\nFleet skill phase: {phase}.");
    if !expected.is_empty() {
        out.push_str(
            " Skills you are expected to use on this task. Load each one's body when its step comes up, not all at startup:\n",
        );
        out.push_str(&expected.join("\n"));
        out.push('\n');
        if !others.is_empty() {
            out.push_str(&format!(
                "Also available in this phase: {}. Load one only when your current step matches it.",
                others.join(", ")
            ));
        }
    } else {
        out.push_str(&format!(
            " Available native skills: {}. Load only the skill matching your current step; do not read every skill at startup.",
            others.join(", ")
        ));
    }
    out.push_str(&format!(
        " If a same-named ambient skill exists, use the fleet copy under {}. Skills do not change tool permissions. Report unresolved dependencies through horch tell orchestrator.\n",
        ctx.skills_dir.display()
    ));
    out
}
