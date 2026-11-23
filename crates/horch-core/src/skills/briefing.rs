use std::path::Path;

use super::activation::{InvocationPolicy, SkillActivationPlan};
use super::catalog::SkillCatalog;
use crate::roster::Phase;

pub struct BriefingContext<'a> {
    pub phase: Option<Phase>,
    pub declared: &'a [String],
    pub namespace: Option<&'a str>,
    pub plugin_lines: &'a [String],
    pub skills_dir: &'a Path,
}

pub fn render(plan: &SkillActivationPlan, catalog: &SkillCatalog, ctx: &BriefingContext) -> String {
    let expected = expected_lines(plan, catalog, ctx);
    let others = other_names(plan, ctx.namespace);
    let mut out = format!("\n\nFleet skill phase: {}.", phase_label(ctx.phase));
    out.push_str(&skill_lines(&expected, &others));
    for skipped in catalog.skipped_operator() {
        out.push_str(&format!(" Skipped: {}.", skipped.note()));
    }
    if !plan.activated.is_empty() {
        out.push_str(&format!(
            " If a same-named ambient skill exists, use the fleet copy under {}. Skills do not change tool permissions.",
            ctx.skills_dir.display()
        ));
    }
    let activated: Vec<&str> = plan.activated.iter().map(|r| r.id.as_str()).collect();
    out.push_str(&outside_note(catalog, &activated));
    out.push_str(" Report unresolved dependencies through horch tell orchestrator.\n");
    out
}

fn qualified(namespace: Option<&str>, name: &str) -> String {
    match namespace {
        Some(ns) => format!("{ns}:{name}"),
        None => name.to_string(),
    }
}

fn expected_lines(
    plan: &SkillActivationPlan,
    catalog: &SkillCatalog,
    ctx: &BriefingContext,
) -> Vec<String> {
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
            format!("- {}: {description}", qualified(ctx.namespace, s))
        })
        .collect();
    expected.extend(ctx.plugin_lines.iter().cloned());
    expected
}

fn other_names(plan: &SkillActivationPlan, namespace: Option<&str>) -> Vec<String> {
    plan.activated
        .iter()
        .filter(|r| r.policy != InvocationPolicy::Explicit)
        .map(|r| qualified(namespace, r.id.as_str()))
        .collect()
}

fn phase_label(phase: Option<Phase>) -> String {
    phase
        .map(|p| p.to_string())
        .unwrap_or_else(|| "custom".into())
}

fn skill_lines(expected: &[String], others: &[String]) -> String {
    let mut out = String::new();
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
    } else if !others.is_empty() {
        out.push_str(&format!(
            " Available native skills: {}. Load only the skill matching your current step; do not read every skill at startup.",
            others.join(", ")
        ));
    }
    out
}

fn outside_note(catalog: &SkillCatalog, activated: &[&str]) -> String {
    let outside = super::reading::outside_bundle(catalog, activated);
    if outside.is_empty() {
        String::new()
    } else {
        let examples: Vec<&str> = outside.iter().take(3).map(|(id, _)| id.as_str()).collect();
        let (noun, verb) = if outside.len() == 1 {
            ("skill", "is")
        } else {
            ("skills", "are")
        };
        format!(
            " Your skills name {} {noun} that {verb} not in this bundle, for example {}. Read one with horch skills read <id> only when your step needs it.",
            outside.len(),
            examples.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use horch_marketplace::SkillVersion;

    use super::super::activation::ResolvedSkillRef;
    use super::*;
    use crate::digest::Digest;
    use crate::ids::SkillId;
    use crate::roster::{OperatorSkills, Teammate};

    const REPORT: &str = " Report unresolved dependencies through horch tell orchestrator.\n";
    const FLEET_COPY: &str = " If a same-named ambient skill exists, use the fleet copy under /bundle/skills. Skills do not change tool permissions.";

    fn plan(named: &[(&str, InvocationPolicy)]) -> SkillActivationPlan {
        SkillActivationPlan {
            activated: named
                .iter()
                .map(|(id, policy)| ResolvedSkillRef {
                    id: SkillId::new(*id).unwrap(),
                    version: SkillVersion("test".into()),
                    digest: Digest([0; 32]),
                    source: "bundled".into(),
                    policy: *policy,
                })
                .collect(),
            available: Vec::new(),
            plugin_skills: BTreeMap::new(),
        }
    }

    fn render_with(
        plan: &SkillActivationPlan,
        catalog: &SkillCatalog,
        phase: Option<Phase>,
        declared: &[&str],
        namespace: Option<&str>,
        plugin_lines: &[&str],
    ) -> String {
        let declared: Vec<String> = declared.iter().map(|s| (*s).to_owned()).collect();
        let plugin_lines: Vec<String> = plugin_lines.iter().map(|s| (*s).to_owned()).collect();
        render(
            plan,
            catalog,
            &BriefingContext {
                phase,
                declared: &declared,
                namespace,
                plugin_lines: &plugin_lines,
                skills_dir: Path::new("/bundle/skills"),
            },
        )
    }

    #[test]
    fn a_plan_that_activates_nothing_has_the_phase_and_the_report_line() {
        let catalog = SkillCatalog::bundled().unwrap();
        let text = render_with(&plan(&[]), &catalog, None, &["declared"], None, &[]);
        assert_eq!(text, format!("\n\nFleet skill phase: custom.{REPORT}"));
        let text = render_with(&plan(&[]), &catalog, Some(Phase::Plan), &[], None, &[]);
        assert_eq!(text, format!("\n\nFleet skill phase: plan.{REPORT}"));
    }

    #[test]
    fn expected_skills_come_first_in_declared_order_and_the_others_follow() {
        let catalog = SkillCatalog::bundled().unwrap();
        let plan = plan(&[
            ("zz-a", InvocationPolicy::Explicit),
            ("zz-b", InvocationPolicy::Explicit),
            ("zz-c", InvocationPolicy::Deterministic),
            ("zz-d", InvocationPolicy::Offered),
        ]);
        let text = render_with(
            &plan,
            &catalog,
            Some(Phase::Implementation),
            &["zz-b", "zz-c", "zz-a", "zz-unplanned"],
            Some("horch"),
            &["- plug:one: A plugin skill."],
        );
        assert_eq!(
            text,
            format!(
                "\n\nFleet skill phase: implementation. Skills you are expected to use on this task. Load each one's body when its step comes up, not all at startup:\n\
                 - horch:zz-b: \n\
                 - horch:zz-a: \n\
                 - plug:one: A plugin skill.\n\
                 Also available in this phase: horch:zz-c, horch:zz-d. Load one only when your current step matches it.{FLEET_COPY}{REPORT}"
            )
        );

        let alone = self::plan(&[("zz-a", InvocationPolicy::Explicit)]);
        let text = render_with(&alone, &catalog, None, &["zz-a"], None, &[]);
        assert_eq!(
            text,
            format!(
                "\n\nFleet skill phase: custom. Skills you are expected to use on this task. Load each one's body when its step comes up, not all at startup:\n\
                 - zz-a: \n\
                 {FLEET_COPY}{REPORT}"
            )
        );
    }

    #[test]
    fn plugin_lines_alone_are_the_expected_list() {
        let catalog = SkillCatalog::bundled().unwrap();
        let text = render_with(
            &plan(&[("zz-c", InvocationPolicy::Deterministic)]),
            &catalog,
            Some(Phase::Plan),
            &[],
            Some("horch"),
            &["- plug:one: A plugin skill."],
        );
        assert_eq!(
            text,
            format!(
                "\n\nFleet skill phase: plan. Skills you are expected to use on this task. Load each one's body when its step comes up, not all at startup:\n\
                 - plug:one: A plugin skill.\n\
                 Also available in this phase: horch:zz-c. Load one only when your current step matches it.{FLEET_COPY}{REPORT}"
            )
        );
    }

    #[test]
    fn with_nothing_expected_the_activated_skills_are_the_native_skills() {
        let catalog = SkillCatalog::bundled().unwrap();
        let plan = plan(&[
            ("zz-c", InvocationPolicy::Deterministic),
            ("zz-d", InvocationPolicy::Offered),
        ]);
        let text = render_with(&plan, &catalog, None, &[], None, &[]);
        assert_eq!(
            text,
            format!(
                "\n\nFleet skill phase: custom. Available native skills: zz-c, zz-d. Load only the skill matching your current step; do not read every skill at startup.{FLEET_COPY}{REPORT}"
            )
        );
    }

    #[test]
    fn skipped_operator_skills_are_named_in_order_before_the_fleet_copy() {
        let teammate = Teammate {
            operator_skills: Some(OperatorSkills {
                dir: "/no/such/dir".into(),
                names: vec!["ghost-b".into(), "ghost-a".into()],
            }),
            ..Teammate::default()
        };
        let catalog = SkillCatalog::bundled()
            .unwrap()
            .with_host_skills(&teammate, None)
            .unwrap();
        let skipped: Vec<String> = catalog
            .skipped_operator()
            .iter()
            .map(|s| s.note())
            .collect();
        assert_eq!(skipped.len(), 2);
        assert!(
            skipped[0].starts_with("operator skill ghost-b "),
            "{skipped:?}"
        );

        let notes = format!(" Skipped: {}. Skipped: {}.", skipped[0], skipped[1]);
        let text = render_with(&plan(&[]), &catalog, None, &[], None, &[]);
        assert_eq!(
            text,
            format!("\n\nFleet skill phase: custom.{notes}{REPORT}")
        );
        let with_skill = plan(&[("zz-c", InvocationPolicy::Deterministic)]);
        let text = render_with(&with_skill, &catalog, None, &[], None, &[]);
        assert_eq!(
            text,
            format!(
                "\n\nFleet skill phase: custom. Available native skills: zz-c. Load only the skill matching your current step; do not read every skill at startup.{notes}{FLEET_COPY}{REPORT}"
            )
        );
    }

    #[test]
    fn the_outside_sentence_counts_in_the_singular_and_the_plural() {
        let catalog = SkillCatalog::bundled().unwrap();
        let (mut one, mut many) = (None, None);
        for entry in catalog.entries().filter(|e| !e.is_plugin()) {
            let id = entry.id.as_str();
            let outside = super::super::reading::outside_bundle(&catalog, &[id]);
            match outside.len() {
                0 => {}
                1 => one = one.or(Some((id.to_owned(), outside))),
                _ => many = many.or(Some((id.to_owned(), outside))),
            }
        }
        let (id, outside) = one.expect("a bundled skill names exactly 1 outside skill");
        let sentence = format!(
            " Your skills name 1 skill that is not in this bundle, for example {}. Read one with horch skills read <id> only when your step needs it.",
            outside[0].0
        );
        let text = render_with(
            &plan(&[(id.as_str(), InvocationPolicy::Deterministic)]),
            &catalog,
            None,
            &[],
            None,
            &[],
        );
        assert!(text.ends_with(&format!("{sentence}{REPORT}")), "{text}");

        let (id, outside) = many.expect("a bundled skill names 2 or more outside skills");
        let examples: Vec<&str> = outside.iter().take(3).map(|(n, _)| n.as_str()).collect();
        let sentence = format!(
            " Your skills name {} skills that are not in this bundle, for example {}. Read one with horch skills read <id> only when your step needs it.",
            outside.len(),
            examples.join(", ")
        );
        let text = render_with(
            &plan(&[(id.as_str(), InvocationPolicy::Deterministic)]),
            &catalog,
            None,
            &[],
            None,
            &[],
        );
        assert!(text.ends_with(&format!("{sentence}{REPORT}")), "{text}");
    }
}
