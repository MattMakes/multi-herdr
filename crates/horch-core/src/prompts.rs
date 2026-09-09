//! Rendering briefings from `teammates/`.
//!
//! This module holds no prompt prose. Every word an agent reads comes from a
//! markdown file in the roster; the only transformation applied is literal
//! `{placeholder}` substitution, performed in a single pass so that text
//! substituted in is never rescanned. There is no conditional rewriting, no
//! summarising, and no assembling of sentences in Rust. To change what an agent
//! is told, edit the file.
//!
//! The one piece of composition that remains is *selection*: which of the three
//! closing paragraphs in `_base/fleet-worker.md` applies (fresh task, resume, or
//! idle). All three are verbatim from the file.

use std::collections::BTreeMap;

use anyhow::{bail, Result};

use crate::teammates::{ExecRule, Roster, Teammate};

/// Substitute `{name}` spans in `template`.
///
/// Single pass: a value containing `{...}` is emitted as-is rather than being
/// re-substituted, so a task description that happens to mention `{role}`
/// cannot rewrite the briefing around it. An unrecognised placeholder is an
/// error, never a silently empty string - a worker briefed with a hole in it
/// misbehaves in a pane nobody is watching.
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

/// Full briefing for a fleet worker pane: `_base/<base>.md` with this
/// teammate's persona and the applicable closing paragraph substituted in.
pub fn worker_prompt(
    roster: &Roster,
    teammate: &Teammate,
    role: &str,
    task: &str,
    resume: bool,
) -> Result<String> {
    let base_name = match &teammate.base {
        Some(b) => b.as_str(),
        None => return agent_prompt(roster, teammate, role),
    };
    let base = roster.require_base(base_name)?;

    let closing = if resume {
        &base.task_resume
    } else if !task.is_empty() {
        &base.task_fresh
    } else {
        &base.task_idle
    };
    let closing = render(closing, &BTreeMap::from([("role", role), ("task", task)]))?;

    let mut vars = BTreeMap::from([
        ("role", role),
        ("persona", teammate.persona.as_str()),
        ("task_briefing", closing.as_str()),
    ]);
    let roster_lines = roster.roster_lines();
    vars.insert("roster", roster_lines.as_str());

    let mut out = render(&base.body, &vars)?;

    // Skills are not a launch flag: there is no `claude --skill`. They are
    // discovered from plugin directories and invoked as /name, so the only way
    // to make a teammate use one is to tell it to. The wording lives in the
    // base file's `skills_instruction`, not here.
    if !teammate.skills.is_empty() {
        if base.skills_instruction.trim().is_empty() {
            bail!(
                "teammate '{}' declares skills but base '{base_name}' has no \
                 skills_instruction to render them into",
                teammate.name
            );
        }
        let joined = teammate.skills.join(", ");
        let mut skill_vars = vars.clone();
        skill_vars.insert("skills", joined.as_str());
        out.push_str("\n\n");
        out.push_str(&render(&base.skills_instruction, &skill_vars)?);
    }

    if let Some(first) = &teammate.first_instruction {
        let first = render(first, &vars)?;
        out.push_str("\n\n");
        out.push_str(&first);
    }
    Ok(out)
}

/// Briefing for a teammate whose body is the entire prompt - an orchestrator,
/// or a worker in the fixed `orchestration` recipe.
pub fn agent_prompt(roster: &Roster, teammate: &Teammate, role: &str) -> Result<String> {
    let roster_lines = roster.roster_lines();
    let vars = BTreeMap::from([("role", role), ("roster", roster_lines.as_str())]);
    render(&teammate.persona, &vars)
}

/// Render one execpolicy `prefix_rule` block for `~/.codex/execpolicy`.
pub fn codex_rule_block(rule: &ExecRule) -> String {
    format!(
        "prefix_rule(\n    pattern = [{}],\n    decision = \"allow\",\n    \
         justification = \"herdr-fleet: {}\",\n)\n",
        rule.pattern, rule.justification
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Roster {
        Roster::builtin().expect("built-in roster parses")
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

    /// The bash implementation referred to `herdr-tell` / `horch-note` /
    /// `horch-done` as separate executables. Every briefing must now name the
    /// `horch` subcommands instead, or workers silently lose their only channel.
    #[test]
    fn briefings_use_horch_subcommands() {
        let r = roster();
        let mut all = Vec::new();
        for t in r.names() {
            let t = r.require(t).unwrap();
            if t.base.is_some() {
                for (task, resume) in [("do a thing", false), ("", false), ("more", true)] {
                    all.push(worker_prompt(&r, t, "r-1", task, resume).unwrap());
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
                assert!(!text.contains(stale), "stale command {stale} in briefing:\n{text}");
            }
        }
    }

    #[test]
    fn codex_rules_match_the_commands_workers_are_told_to_run() {
        let r = roster();
        let joined: String = r.exec_rules().iter().map(codex_rule_block).collect();
        assert!(joined.contains(r#"pattern = ["horch", "tell", "orchestrator"]"#));
        assert!(joined.contains(r#"pattern = ["horch", "note"]"#));
        assert!(joined.contains(r#"pattern = ["horch", "done"]"#));
        // And the roster agrees they are all actually used in the briefing.
        assert!(r.check().is_empty(), "{:?}", r.check());
    }

    #[test]
    fn idle_worker_is_told_to_announce_readiness() {
        let r = roster();
        let p = worker_prompt(&r, r.require("opus").unwrap(), "opus-1", "", false).unwrap();
        assert!(p.contains(r#"horch tell orchestrator "[opus-1] ready""#), "{p}");
    }

    /// `skills` has to reach the prompt, or a teammate that declares one
    /// launches identically to one that does not - a field that reads as
    /// configuration but changes nothing.
    #[test]
    fn declared_skills_reach_the_briefing() {
        let r = roster();
        let plain = worker_prompt(&r, r.require("sonnet").unwrap(), "s-1", "t", false).unwrap();
        assert!(!plain.contains("load these skills"), "{plain}");

        let mut with_skills = r.require("sonnet").unwrap().clone();
        with_skills.skills = vec!["herdr-worker".into(), "code-review".into()];
        let out = worker_prompt(&r, &with_skills, "s-1", "t", false).unwrap();
        assert!(out.contains("herdr-worker, code-review"), "{out}");
        assert!(out.len() > plain.len());
    }

    #[test]
    fn first_instruction_is_the_last_thing_a_worker_reads() {
        let r = roster();
        let mut t = r.require("opus").unwrap().clone();
        t.skills = vec!["planning".into()];
        t.first_instruction = Some("Read {role}'s plan file first.".into());
        let out = worker_prompt(&r, &t, "opus-1", "task", false).unwrap();
        assert!(out.trim_end().ends_with("Read opus-1's plan file first."), "{out}");
    }

    #[test]
    fn orchestrator_briefing_carries_the_roster() {
        let r = roster();
        let p = agent_prompt(&r, r.require("orchestrator").unwrap(), "orchestrator").unwrap();
        assert!(p.contains("sonnet"), "{p}");
        assert!(p.contains("codex-terra"), "{p}");
        // Hidden teammates cost the orchestrator nothing.
        assert!(!p.contains("Test-only fake agent"), "{p}");
        assert!(!p.contains("{roster}"), "placeholder survived substitution");
    }
}
