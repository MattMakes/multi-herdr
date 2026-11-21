use std::collections::BTreeMap;

use anyhow::{bail, ensure, Context, Result};

use crate::execution::SessionMode;
use crate::roster::{Base, ExecRule, Roster, Teammate};

pub fn render(template: &str, vars: &BTreeMap<&str, &str>) -> Result<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let close = match after.find('}') {
            Some(c) => c,
            None => bail!("unterminated '{{' in template"),
        };
        let name = &after[..close];
        match vars.get(name) {
            Some(value) => out.push_str(value),
            None => bail!(
                "unknown placeholder '{{{name}}}' (known: {})",
                vars.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

pub fn worker_prompt(
    roster: &Roster,
    teammate: &Teammate,
    role: &str,
    task: &str,
    session: &SessionMode,
) -> Result<String> {
    worker_prompt_with_context(
        roster,
        teammate,
        role,
        task,
        session,
        "(No project facts recorded.)",
        &crate::project_notes::host_line(),
    )
}

pub fn worker_prompt_with_context(
    roster: &Roster,
    teammate: &Teammate,
    role: &str,
    task: &str,
    session: &SessionMode,
    project_facts: &str,
    host: &str,
) -> Result<String> {
    let base_name = match &teammate.base {
        Some(b) => b.as_str(),
        None => return agent_prompt(roster, teammate, role),
    };
    let base = roster.require_base(base_name)?;

    let closing = closing_template(base, session, task);
    let closing = render(closing, &BTreeMap::from([("role", role), ("task", task)]))?;

    let mut vars = BTreeMap::from([
        ("role", role),
        ("project_facts", project_facts),
        ("host", host),
        ("persona", teammate.persona.as_str()),
        ("task_briefing", closing.as_str()),
    ]);
    let roster_lines = roster.roster_lines();
    vars.insert("roster", roster_lines.as_str());

    let mut out = render(&base.body, &vars)?;

    append_teammate_blocks(&mut out, teammate, (base_name, base), &vars)?;
    Ok(out)
}

fn append_teammate_blocks(
    out: &mut String,
    teammate: &Teammate,
    (base_name, base): (&str, &Base),
    vars: &BTreeMap<&str, &str>,
) -> Result<()> {
    let name = &teammate.name;
    if !teammate.skills.is_empty() {
        let joined = teammate.skills.join(", ");
        let mut skill_vars = vars.clone();
        skill_vars.insert("skills", joined.as_str());
        let missing = format!(
            "teammate '{name}' declares skills but base '{base_name}' has no \
             skills_instruction to render them into"
        );
        append_required_block(out, &base.skills_instruction, &skill_vars, missing)?;
    }

    if teammate.trains_on_input {
        let missing = format!(
            "teammate '{name}' sets trains_on_input but base '{base_name}' has no \
             trains_on_input block to render it into"
        );
        append_required_block(out, &base.trains_on_input, vars, missing)?;
    }

    if let Some(first) = &teammate.first_instruction {
        out.push_str("\n\n");
        out.push_str(&render(first, vars)?);
    }
    Ok(())
}

fn closing_template<'a>(base: &'a Base, session: &SessionMode, task: &str) -> &'a str {
    if session.is_resume() {
        &base.task_resume
    } else if !task.is_empty() {
        &base.task_fresh
    } else {
        &base.task_idle
    }
}

fn append_required_block(
    out: &mut String,
    block: &str,
    vars: &BTreeMap<&str, &str>,
    missing: String,
) -> Result<()> {
    ensure!(!block.trim().is_empty(), missing);
    out.push_str("\n\n");
    out.push_str(&render(block, vars)?);
    Ok(())
}

pub fn agent_prompt(roster: &Roster, teammate: &Teammate, role: &str) -> Result<String> {
    let roster_lines = roster.roster_lines();
    let mut vars = BTreeMap::from([("role", role), ("roster", roster_lines.as_str())]);
    let Some(base_name) = teammate.base.as_deref() else {
        return render(&teammate.persona, &vars);
    };
    let base = roster.require_base(base_name)?;
    vars.insert("persona", teammate.persona.as_str());
    render(&base.body, &vars)
}

pub fn context_message(roster: &Roster, key: &str, vars: &BTreeMap<&str, &str>) -> Result<String> {
    let base = roster
        .context_messages()
        .context("base 'context-messages' does not exist in _base/")?;
    let Some(message) = base.messages.get(key) else {
        bail!("base 'context-messages' has no message '{key}'");
    };
    let flat: BTreeMap<&str, String> = vars
        .iter()
        .map(|(k, v)| (*k, v.replace(['\r', '\n'], " ")))
        .collect();
    let flat: BTreeMap<&str, &str> = flat.iter().map(|(k, v)| (*k, v.as_str())).collect();
    render(message, &flat).with_context(|| format!("context message '{key}'"))
}

pub fn codex_rule_block(rule: &ExecRule) -> String {
    format!(
        "prefix_rule(\n    pattern = [{}],\n    decision = \"{}\",\n    \
         justification = \"herdr-fleet: {}\",\n)\n",
        rule.pattern,
        rule.decision.as_str(),
        rule.justification
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Roster {
        Roster::builtin().expect("built-in roster parses")
    }

    #[test]
    fn orchestrator_briefing_offers_a_domain_teammate_only_where_it_matches() {
        use crate::roster::ProjectFacts;
        let mut base = roster();
        base.insert_for_test(Teammate {
            name: "unreal-gameplay-engineer".into(),
            brief_description: "Unreal Engine gameplay code".into(),
            base: Some("fleet-worker".into()),
            offer_when: vec!["*.uproject".into()],
            ..Teammate::default()
        });
        let briefing = |facts: ProjectFacts| {
            let r = base.clone().with_project_facts(facts);
            agent_prompt(&r, r.require("orchestrator").unwrap(), "orchestrator").unwrap()
        };

        let rust = briefing(ProjectFacts::from_names(["Cargo.toml", "src"]));
        assert!(!rust.contains("unreal-gameplay-engineer"), "{rust}");
        assert!(!rust.contains("Unreal Engine gameplay code"));
        let game = briefing(ProjectFacts::from_names(["Game.uproject", "Source"]));
        assert!(game.contains("unreal-gameplay-engineer"), "{game}");

        for t in base.offered() {
            if t.offer_when.is_empty() {
                assert!(rust.contains(&t.name), "{} missing", t.name);
                assert!(game.contains(&t.name), "{} missing", t.name);
            }
        }
    }

    #[test]
    fn substitution_is_single_pass() {
        let vars = BTreeMap::from([("task", "look at {role}"), ("role", "sonnet-1")]);
        let out = render("A: {task}\nB: {role}", &vars).unwrap();
        assert_eq!(out, "A: look at {role}\nB: sonnet-1");
    }

    #[test]
    fn unknown_placeholder_is_an_error() {
        let vars = BTreeMap::from([("role", "r")]);
        let err = render("hi {nope}", &vars).unwrap_err().to_string();
        assert!(err.contains("unknown placeholder '{nope}'"), "{err}");
    }

    #[test]
    fn briefings_use_horch_subcommands() {
        let r = roster();
        let mut all = Vec::new();
        for t in r.names() {
            let t = r.require(t).unwrap();
            if t.base.is_some() {
                let resume = SessionMode::Resume("s-1".parse().unwrap());
                let fresh = SessionMode::Fresh(None);
                for (task, session) in [("do a thing", &fresh), ("", &fresh), ("more", &resume)] {
                    all.push(worker_prompt(&r, t, "r-1", task, session).unwrap());
                }
            } else {
                all.push(agent_prompt(&r, t, "r-1").unwrap());
            }
        }
        assert!(!all.is_empty());
        for text in &all {
            for stale in [
                "herdr-tell",
                "herdr-inbox",
                "horch-note",
                "horch-done",
                "horch-spawn",
                "horch-assign",
                "horch-sessions",
            ] {
                assert!(
                    !text.contains(stale),
                    "stale command {stale} in briefing:\n{text}"
                );
            }
        }
    }

    #[test]
    fn codex_rules_match_the_commands_workers_are_told_to_run() {
        let r = roster();
        let joined: String = r.exec_rules().iter().map(codex_rule_block).collect();
        assert!(joined.contains(r#"pattern = ["horch", "tell"],"#));
        assert!(joined.contains(r#"pattern = ["horch", "note"]"#));
        assert!(joined.contains(r#"pattern = ["horch", "done"]"#));
        assert!(r.check().is_empty(), "{:?}", r.check());
    }

    #[test]
    fn idle_worker_is_told_to_announce_readiness() {
        let r = roster();
        let p = worker_prompt(
            &r,
            r.require("opus").unwrap(),
            "opus-1",
            "",
            &SessionMode::Fresh(None),
        )
        .unwrap();
        assert!(
            p.contains(r#"horch tell orchestrator "[opus-1] ready""#),
            "{p}"
        );
    }

    #[test]
    fn declared_skills_reach_the_briefing() {
        let r = roster();
        let plain = worker_prompt(
            &r,
            r.require("sonnet").unwrap(),
            "s-1",
            "t",
            &SessionMode::Fresh(None),
        )
        .unwrap();
        assert!(!plain.contains("load these skills"), "{plain}");

        let mut with_skills = r.require("sonnet").unwrap().clone();
        with_skills.skills = vec!["herdr-worker".into(), "code-review".into()];
        let out = worker_prompt(&r, &with_skills, "s-1", "t", &SessionMode::Fresh(None)).unwrap();
        assert!(out.contains("herdr-worker, code-review"), "{out}");
        assert!(out.len() > plain.len());
    }

    #[test]
    fn a_free_tier_worker_is_told_what_it_is_running_on() {
        let r = roster();
        for name in ["opencode-ultra", "opencode-pickle", "opencode-lightning"] {
            let t = r.require(name).unwrap();
            assert!(t.trains_on_input, "{name} must declare it");
            let out =
                worker_prompt(&r, t, "oc-1", "do a thing", &SessionMode::Fresh(None)).unwrap();
            assert!(out.contains("trains on what it is sent"), "{name}:\n{out}");
            assert!(
                out.contains(r#"horch tell orchestrator "[oc-1] BLOCKED"#),
                "{name}"
            );
        }
        for quiet in ["opus", "sonnet", "codex-sol", "pi", "prime"] {
            let t = r.require(quiet).unwrap();
            assert!(!t.trains_on_input, "{quiet} must not declare it");
            let out = worker_prompt(&r, t, "w-1", "t", &SessionMode::Fresh(None)).unwrap();
            assert!(
                !out.contains("trains on what it is sent"),
                "{quiet} was warned:\n{out}"
            );
        }
    }

    #[test]
    fn the_orchestrator_sees_which_teammates_train_on_input() {
        let r = roster();
        let lines = r.roster_lines();
        for t in r.offered() {
            if t.trains_on_input {
                let line = lines
                    .lines()
                    .find(|l| l.trim_start().starts_with(&t.name))
                    .unwrap_or_else(|| panic!("{} missing from the roster", t.name));
                assert!(
                    line.contains("TRAINS ON YOUR INPUT") || line.contains("TRAINS ON INPUT"),
                    "{} is offered without warning the orchestrator: {line}",
                    t.name
                );
            }
        }
    }

    #[test]
    fn first_instruction_is_the_last_thing_a_worker_reads() {
        let r = roster();
        let mut t = r.require("opus").unwrap().clone();
        t.skills = vec!["planning".into()];
        t.first_instruction = Some("Read {role}'s plan file first.".into());
        let out = worker_prompt(&r, &t, "opus-1", "task", &SessionMode::Fresh(None)).unwrap();
        assert!(
            out.trim_end().ends_with("Read opus-1's plan file first."),
            "{out}"
        );
    }

    #[test]
    fn a_block_the_base_lacks_names_the_teammate_and_the_base() {
        let mut r = roster();
        let mut t = r.require("opus").unwrap().clone();
        let base_name = t.base.clone().unwrap();
        r.bases.get_mut(&base_name).unwrap().skills_instruction = "  ".into();
        r.bases.get_mut(&base_name).unwrap().trains_on_input = String::new();

        t.skills = vec!["planning".into()];
        let err = worker_prompt(&r, &t, "opus-1", "task", &SessionMode::Fresh(None)).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "teammate 'opus' declares skills but base '{base_name}' has no \
                 skills_instruction to render them into"
            )
        );

        t.skills.clear();
        t.trains_on_input = true;
        let err = worker_prompt(&r, &t, "opus-1", "task", &SessionMode::Fresh(None)).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "teammate 'opus' sets trains_on_input but base '{base_name}' has no \
                 trains_on_input block to render it into"
            )
        );
    }

    #[test]
    fn orchestrator_briefing_carries_the_roster() {
        let r = roster();
        let p = agent_prompt(&r, r.require("orchestrator").unwrap(), "orchestrator").unwrap();
        assert!(p.contains("sonnet"), "{p}");
        assert!(p.contains("codex-terra"), "{p}");
        assert!(!p.contains("Test-only fake agent"), "{p}");
        assert!(!p.contains("{roster}"), "placeholder survived substitution");
    }
}
