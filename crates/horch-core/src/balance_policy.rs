//! Quota-aware balancing: which teammate a spawn actually runs as (design
//! section 13). Named apart from `balance.rs`, which evens grid columns.
//!
//! A teammate names `fallbacks`. When its usage pool cannot serve a spawn, a
//! fallback lends its LAUNCH settings (agent, model, args, env, ...) while the
//! persona, base, phase and skills stay the original's (D6). [`decide`] is
//! pure: no I/O, and the time arrives inside the [`QuotaView`].

use serde::Serialize;

use crate::policy::BalanceMode;
use crate::quota::{self, Assessment, QuotaView, State};
use crate::teammates::{effort_problem, Agent, Roster, Teammate};

/// Flags a spawn passes to the gate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GateFlags {
    /// Never substitute.
    pub exact: bool,
    /// Never refuse.
    pub force: bool,
}

/// One pool line for a REFUSED message and `horch route`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PoolLine {
    pub pool: String,
    pub state: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "decision", rename_all = "lowercase")]
pub enum Decision {
    Spawn {
        teammate: String,
        note: Option<String>,
    },
    Substitute {
        original: String,
        via: String,
        reason: String,
    },
    Refuse {
        teammate: String,
        reason: String,
        pools: Vec<PoolLine>,
    },
}

impl Decision {
    /// The line printed to stdout before the pane id, if any.
    pub fn line(&self) -> Option<String> {
        match self {
            Decision::Spawn { note, .. } => note.as_ref().map(|n| format!("NOTE: {n}")),
            Decision::Substitute {
                original,
                via,
                reason,
            } => Some(format!(
                "SUBSTITUTED: {original} runs on {via}. Reason: {reason}."
            )),
            Decision::Refuse {
                teammate, pools, ..
            } => {
                let pools: Vec<String> = pools
                    .iter()
                    .map(|p| format!("{} {} ({}).", p.pool, p.state, p.detail))
                    .collect();
                Some(format!(
                    "REFUSED: {teammate} cannot start. {} Options: wait, --force, or choose a teammate yourself.",
                    pools.join(" ")
                ))
            }
        }
    }
}

// ─── merge ──────────────────────────────────────────────────────────────────

/// `original` launched with `fallback`'s settings (section 13.2).
///
/// From the fallback: every field that says HOW to launch - agent, model,
/// args, env, tool lists, permission mode, plugins, MCP, settings, and the
/// rest of the Claude-shaped fields. From the original: name, description,
/// base, persona, phase, skills, generic, hidden. Effort stays the
/// original's when the fallback's agent and model accept it.
pub fn merge(original: &Teammate, fallback: &Teammate) -> Teammate {
    let mut m = fallback.clone();
    m.name = original.name.clone();
    m.brief_description = original.brief_description.clone();
    m.base = original.base.clone();
    m.persona = original.persona.clone();
    m.phase = original.phase;
    m.skills = original.skills.clone();
    m.generic = original.generic;
    m.hidden = original.hidden;
    m.first_instruction = original.first_instruction.clone();
    m.fallbacks = original.fallbacks.clone();
    m.effort = match &original.effort {
        Some(e) if effort_problem(fallback.agent, fallback.model.as_deref(), e).is_none() => {
            Some(e.clone())
        }
        _ => fallback.effort.clone(),
    };
    m
}

/// Whether a teammate's provider trains on its input: every OpenCode one
/// (BAL-09). Never chosen automatically.
pub fn trains_on_input(t: &Teammate) -> bool {
    t.trains_on_input || t.agent == Agent::Opencode
}

// ─── decide ─────────────────────────────────────────────────────────────────

/// `7d 88%, pace 1.4x, resets Fri 14:00Z`: an assessment in a few words.
pub fn summary(a: &Assessment, view: &QuotaView) -> String {
    let Some(w) = &a.worst else {
        return a.reason.clone();
    };
    let mut s = format!("{} {}", w.label(), quota::pct(w.used_at(view.now)));
    if let Some(pace) = w.pace(view.now) {
        if a.state == State::Tight && pace > view.policy.pace_factor {
            s.push_str(&format!(", pace {pace:.1}x"));
        }
    }
    if let Some(r) = &w.resets_at {
        s.push_str(&format!(", resets {}", quota::short_time(r)));
    }
    if a.state == State::Exhausted && !a.reason.contains('%') {
        return a.reason.clone();
    }
    s
}

/// The one-line reason a substitution records: `claude 7d 100%, resets
/// 2026-10-02T14:00Z`.
fn reason_line(a: &Assessment, view: &QuotaView) -> String {
    match &a.worst {
        Some(w) if a.state != State::Unknown && a.reason.contains('%') => {
            let mut s = format!(
                "{} {} {}",
                a.pool,
                w.label(),
                quota::pct(w.used_at(view.now))
            );
            if let Some(r) = w.resets_at.as_deref().and_then(crate::clock::parse) {
                let r = r + chrono::Duration::seconds(30);
                s.push_str(&format!(", resets {}", r.format("%Y-%m-%dT%H:%MZ")));
            }
            s
        }
        _ => format!("{} {}: {}", a.pool, a.state, a.reason),
    }
}

/// A fallback the gate may use, with its pool's state.
struct Candidate {
    name: String,
    state: State,
    assessment: Assessment,
}

fn candidates(req: &Teammate, roster: &Roster, view: &QuotaView) -> Vec<Candidate> {
    req.fallbacks
        .iter()
        .filter_map(|name| roster.get(name))
        .filter(|f| !f.hidden && Roster::is_spawnable(f).is_ok() && !trains_on_input(f))
        .map(|f| {
            let a = view.assess(f.agent.as_str(), f.model.as_deref().unwrap_or_default());
            Candidate {
                name: f.name.clone(),
                state: a.state,
                assessment: a,
            }
        })
        .collect()
}

fn pool_line(a: &Assessment, view: &QuotaView) -> PoolLine {
    PoolLine {
        pool: a.pool.clone(),
        state: a.state.to_string(),
        detail: summary(a, view),
    }
}

/// What the spawn gate does with a request (section 13.4, BAL-03).
pub fn decide(
    req: &Teammate,
    roster: &Roster,
    view: &QuotaView,
    mode: BalanceMode,
    flags: GateFlags,
) -> Decision {
    let spawn = |note: Option<String>| Decision::Spawn {
        teammate: req.name.clone(),
        note,
    };
    if mode == BalanceMode::Off {
        return spawn(None);
    }
    let a = view.assess(req.agent.as_str(), req.model.as_deref().unwrap_or_default());
    if a.state == State::Ok {
        return spawn(None);
    }
    let cands = candidates(req, roster, view);
    let first_ok = cands.iter().find(|c| c.state == State::Ok);
    let pool_note = |extra: Option<&Candidate>| {
        let mut n = format!("{} pool {} ({}).", a.pool, a.state, summary(&a, view));
        if let Some(c) = extra {
            n.push_str(&format!(" Fallback {} is {}.", c.name, c.state));
        }
        n
    };
    let substitute = |c: &Candidate| Decision::Substitute {
        original: req.name.clone(),
        via: c.name.clone(),
        reason: reason_line(&a, view),
    };
    let refuse = || {
        let mut pools = vec![pool_line(&a, view)];
        for c in &cands {
            if !pools.iter().any(|p| p.pool == c.assessment.pool) {
                pools.push(pool_line(&c.assessment, view));
            }
        }
        Decision::Refuse {
            teammate: req.name.clone(),
            reason: reason_line(&a, view),
            pools,
        }
    };
    let refuse_or_force = || {
        if flags.force {
            spawn(Some(format!(
                "{} Spawned anyway (--force).",
                pool_note(None)
            )))
        } else {
            refuse()
        }
    };

    if mode == BalanceMode::Advise {
        let hint = first_ok.or_else(|| cands.iter().find(|c| c.state == State::Tight));
        return spawn(Some(pool_note(hint)));
    }

    // mode auto
    match a.state {
        State::Ok => spawn(None),
        State::Tight => {
            let Some(c) = first_ok else {
                return spawn(Some(pool_note(None)));
            };
            let theirs = c.assessment.headroom_per_h.unwrap_or(0.0);
            let ours = a.headroom_per_h.unwrap_or(0.0);
            if theirs >= view.policy.headroom_ratio * ours && !flags.exact {
                substitute(c)
            } else {
                spawn(Some(pool_note(Some(c))))
            }
        }
        State::Exhausted | State::Broken | State::Cooling => {
            let usable = cands
                .iter()
                .find(|c| matches!(c.state, State::Ok | State::Tight));
            match usable {
                Some(c) if !flags.exact => substitute(c),
                _ => refuse_or_force(),
            }
        }
        State::Unknown => match first_ok {
            Some(c) if !flags.exact => substitute(c),
            c => spawn(Some(pool_note(c))),
        },
    }
}

/// The teammate a decision launches: the request, or the request merged with
/// its fallback.
pub fn resolve(req: &Teammate, roster: &Roster, decision: &Decision) -> Option<Teammate> {
    match decision {
        Decision::Spawn { .. } => Some(req.clone()),
        Decision::Substitute { via, .. } => roster.get(via).map(|f| merge(req, f)),
        Decision::Refuse { .. } => None,
    }
}

// ─── roster rules ───────────────────────────────────────────────────────────

/// Tools a Claude persona may lean on that no other agent has (rule 6).
const CLAUDE_ONLY_TOOLS: [&str; 7] = [
    "Agent",
    "Task",
    "TodoWrite",
    "WebFetch",
    "WebSearch",
    "NotebookEdit",
    "Skill",
];

/// Section 13.3 rules 1-5, as `horch teammates --check` errors (BAL-02).
pub fn fallback_problems(roster: &Roster) -> Vec<String> {
    let mut out = Vec::new();
    for name in roster.names() {
        let Some(t) = roster.get(name) else { continue };
        let pool = quota::pool_for(t.agent.as_str(), t.model.as_deref().unwrap_or_default());
        for fb in &t.fallbacks {
            let who = &t.name;
            let Some(f) = roster.get(fb) else {
                out.push(format!("{who}: fallback '{fb}' does not exist"));
                continue;
            };
            if f.hidden {
                out.push(format!("{who}: fallback '{fb}' is hidden"));
            }
            if let Err(e) = Roster::is_spawnable(f) {
                out.push(format!("{who}: fallback '{fb}' is not spawnable: {e:#}"));
            }
            let fpool = quota::pool_for(f.agent.as_str(), f.model.as_deref().unwrap_or_default());
            if fpool == pool {
                out.push(format!(
                    "{who}: fallback '{fb}' draws from the same pool ({pool}), so it cannot help"
                ));
            }
            if trains_on_input(f) {
                out.push(format!(
                    "{who}: fallback '{fb}' trains on its input; free tiers are chosen by \
                     the orchestrator, never automatically"
                ));
            }
            let merged = merge(t, f);
            for p in roster.check_teammate(&merged) {
                out.push(format!("{who} via {fb}: {p}"));
            }
        }
    }
    out
}

/// Rule 6: warnings, not errors. A persona that names a Claude-only tool,
/// on a fallback whose agent lacks it.
pub fn fallback_warnings(roster: &Roster) -> Vec<String> {
    let mut out = Vec::new();
    for name in roster.names() {
        let Some(t) = roster.get(name) else { continue };
        for fb in &t.fallbacks {
            let Some(f) = roster.get(fb) else { continue };
            if f.agent == Agent::Claude {
                continue;
            }
            for tool in CLAUDE_ONLY_TOOLS {
                if names_tool(&t.persona, tool) {
                    out.push(format!(
                        "{}: the persona names the {tool} tool, which {} (fallback {fb}) lacks",
                        t.name, f.agent
                    ));
                }
            }
        }
    }
    out
}

/// Whether `text` names `tool` as a tool: the name in backticks, or followed
/// by the word "tool". Plain English "Task" or "Agent" does not count.
fn names_tool(text: &str, tool: &str) -> bool {
    text.contains(&format!("`{tool}`"))
        || text.contains(&format!("{tool} tool"))
        || text.contains(&format!("{tool}("))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Policy;
    use crate::quota::QuotaFile;

    fn roster() -> Roster {
        Roster::builtin().unwrap()
    }

    fn view(name: &str) -> QuotaView {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/telemetry/quota")
            .join(format!("{name}.json"));
        QuotaView::new(
            QuotaFile::read(&path).unwrap(),
            crate::clock::parse("2026-09-28T18:00:00Z").unwrap(),
            Policy::default(),
            true,
        )
    }

    fn t(name: &str) -> Teammate {
        roster().require(name).unwrap().clone()
    }

    #[test]
    fn bal_01_merge_rule() {
        let r = t("researcher");
        let f = t("codex-sol");
        let m = merge(&r, &f);
        // From the fallback: how to launch.
        assert_eq!(m.agent, Agent::Codex);
        assert_eq!(m.model, f.model);
        assert_eq!(m.args, f.args);
        assert_eq!(m.env, f.env);
        assert_eq!(m.disallowed_tools, f.disallowed_tools);
        assert_eq!(m.permission_mode, f.permission_mode);
        assert_eq!(m.inherit_plugins, f.inherit_plugins);
        assert_eq!(m.mcp_servers, f.mcp_servers);
        assert_eq!(m.disabled_skills, f.disabled_skills);
        assert_eq!(m.subagent_model, f.subagent_model);
        assert_eq!(m.plugin_skills, f.plugin_skills);
        // From the original: who it is.
        assert_eq!(m.name, "researcher");
        assert_eq!(m.brief_description, r.brief_description);
        assert_eq!(m.base, r.base);
        assert_eq!(m.persona, r.persona);
        assert_eq!(m.phase, r.phase);
        assert_eq!(m.skills, r.skills);
        assert_eq!((m.generic, m.hidden), (r.generic, r.hidden));
        // researcher's `medium` is a codex level too, so it stays.
        assert_eq!(m.effort.as_deref(), Some("medium"));
        // An effort the fallback cannot take comes from the fallback.
        let mut xhigh_claude = r.clone();
        xhigh_claude.effort = Some("bogus".into());
        assert_eq!(merge(&xhigh_claude, &f).effort, f.effort);
    }

    #[test]
    fn bal_02_repo_roster_passes() {
        let problems = fallback_problems(&roster());
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn bal_02_roster_rules() {
        let base = roster();
        let with = |name: &str, fallbacks: &[&str], edit: &dyn Fn(&mut Roster)| {
            let mut r = base.clone();
            let mut tm = r.require(name).unwrap().clone();
            tm.fallbacks = fallbacks.iter().map(|s| s.to_string()).collect();
            r.insert_for_test(tm);
            edit(&mut r);
            fallback_problems(&r)
        };
        let none = |_: &mut Roster| {};
        // 1: missing, hidden, not spawnable.
        assert!(with("researcher", &["ghost"], &none)
            .iter()
            .any(|p| p.contains("does not exist")));
        assert!(with("researcher", &["smoke"], &none)
            .iter()
            .any(|p| p.contains("is hidden")));
        let reserved = |r: &mut Roster| {
            let mut a = r.require("codex-sol").unwrap().clone();
            a.name = "astra-worker".into();
            a.model = Some("gpt-6-astra".into());
            r.insert_for_test(a);
        };
        assert!(with("researcher", &["astra-worker"], &reserved)
            .iter()
            .any(|p| p.contains("not spawnable")));
        // 2: same pool.
        assert!(with("researcher", &["sonnet"], &none)
            .iter()
            .any(|p| p.contains("same pool")));
        // 3: trains on input.
        assert!(with("researcher", &["opencode-pickle"], &none)
            .iter()
            .any(|p| p.contains("trains on its input")));
        // 4: the merged teammate must pass the existing rules. A claude
        // fallback without `disallowed_tools: [Agent]` breaks the
        // no-subagents rule once merged.
        let open_claude = |r: &mut Roster| {
            let mut s = r.require("sonnet").unwrap().clone();
            s.name = "loose".into();
            s.disallowed_tools.clear();
            r.insert_for_test(s);
        };
        assert!(with("codex-sol", &["loose"], &open_claude)
            .iter()
            .any(|p| p.contains("via loose") && p.contains("subagents")));
        // 5: a fallback with fallbacks of its own is fine.
        assert!(with("researcher", &["codex-sol"], &none).is_empty());
    }

    #[test]
    fn bal_03_decision_table() {
        let r = roster();
        let opus = t("opus");
        let auto = BalanceMode::Auto;
        let d = |v: &str, req: &Teammate, mode, flags| decide(req, &r, &view(v), mode, flags);
        let plain = GateFlags::default();
        let exact = GateFlags {
            exact: true,
            force: false,
        };
        let force = GateFlags {
            exact: false,
            force: true,
        };

        // ok: spawn, every mode.
        for mode in [BalanceMode::Off, BalanceMode::Advise, auto] {
            assert_eq!(
                d("all-ok", &opus, mode, plain),
                Decision::Spawn {
                    teammate: "opus".into(),
                    note: None
                }
            );
        }
        // off: always a plain spawn.
        assert_eq!(
            d("all-exhausted", &opus, BalanceMode::Off, plain),
            Decision::Spawn {
                teammate: "opus".into(),
                note: None
            }
        );
        // advise: spawn with a note, never substitute or refuse.
        let adv = d("all-exhausted", &opus, BalanceMode::Advise, plain);
        assert!(
            matches!(&adv, Decision::Spawn { note: Some(n), .. } if n.starts_with("claude pool exhausted")),
            "{adv:?}"
        );

        // exhausted -> the first ok-or-tight fallback.
        let sub = d("claude-exhausted-codex-ok", &opus, auto, plain);
        assert!(
            matches!(&sub, Decision::Substitute { via, .. } if via == "codex-sol"),
            "{sub:?}"
        );
        assert_eq!(
            sub.line().unwrap(),
            "SUBSTITUTED: opus runs on codex-sol. Reason: claude 7d 100%, resets 2026-10-02T14:00Z."
        );
        // ... --exact refuses instead; nothing usable refuses; --force spawns.
        assert!(matches!(
            d("claude-exhausted-codex-ok", &opus, auto, exact),
            Decision::Refuse { .. }
        ));
        let refused = d("all-exhausted", &opus, auto, plain);
        assert!(matches!(refused, Decision::Refuse { .. }));
        let line = refused.line().unwrap();
        assert!(line.starts_with("REFUSED: opus cannot start. claude exhausted (7d 100%, resets Fri 14:00Z). codex exhausted (7d 99%, resets Sat 19:16Z)."), "{line}");
        assert!(line.ends_with("Options: wait, --force, or choose a teammate yourself."));
        assert!(matches!(
            d("all-exhausted", &opus, auto, force),
            Decision::Spawn { note: Some(_), .. }
        ));

        // tight: substitute only with >= 2x the headroom.
        let sonnet = t("sonnet");
        let tight_sub = d("claude-tight-codex-ok", &sonnet, auto, plain);
        assert!(
            matches!(&tight_sub, Decision::Substitute { via, .. } if via == "codex-terra"),
            "{tight_sub:?}"
        );
        let close = d("claude-tight-codex-close", &sonnet, auto, plain);
        assert!(
            matches!(&close, Decision::Spawn { note: Some(n), .. } if n.contains("Fallback codex-terra is ok")),
            "{close:?}"
        );
        assert!(matches!(
            d("claude-tight-codex-ok", &sonnet, auto, exact),
            Decision::Spawn { note: Some(_), .. }
        ));
        assert!(matches!(
            d("claude-pace-tight", &opus, auto, plain),
            Decision::Substitute { .. } | Decision::Spawn { note: Some(_), .. }
        ));

        // unknown: substitute with an ok fallback, never refuse.
        assert!(matches!(
            d("claude-unknown-codex-ok", &opus, auto, plain),
            Decision::Substitute { .. }
        ));
        assert!(matches!(
            d("claude-unknown-codex-ok", &opus, auto, exact),
            Decision::Spawn { note: Some(_), .. }
        ));
        let mut lonely = opus.clone();
        lonely.fallbacks.clear();
        assert!(matches!(
            d("claude-unknown-codex-ok", &lonely, auto, plain),
            Decision::Spawn { note: Some(_), .. }
        ));
        // No fallbacks: the else branch of each cell.
        assert!(matches!(
            d("claude-exhausted-codex-ok", &lonely, auto, plain),
            Decision::Refuse { .. }
        ));
        assert!(matches!(
            d("claude-tight-codex-ok", &lonely, auto, plain),
            Decision::Spawn { note: Some(_), .. }
        ));

        // broken and cooling behave like exhausted.
        let pi = t("pi");
        assert!(matches!(
            d("local-broken", &pi, auto, plain),
            Decision::Refuse { .. }
        ));
        let luna = t("codex-luna");
        assert!(
            matches!(d("codex-not-allowed", &luna, auto, plain), Decision::Substitute { via, .. } if via == "sonnet")
        );
    }

    #[test]
    fn bal_09_never_trains_on_input() {
        let base = roster();
        let free: Vec<String> = base
            .names()
            .into_iter()
            .filter(|n| base.get(n).is_some_and(trains_on_input))
            .map(str::to_owned)
            .collect();
        assert!(!free.is_empty());
        // Every permutation of fallbacks with a free teammate in it fails --check.
        for name in ["opus", "sonnet", "codex-sol", "pi", "prime"] {
            for f in &free {
                for order in [
                    vec![f.clone(), "codex-sol".into()],
                    vec!["codex-terra".into(), f.clone()],
                ] {
                    let mut r = base.clone();
                    let mut t = r.require(name).unwrap().clone();
                    t.fallbacks = order.clone();
                    r.insert_for_test(t);
                    assert!(
                        fallback_problems(&r)
                            .iter()
                            .any(|p| p.contains("trains on its input")),
                        "{name} {order:?}"
                    );
                }
            }
        }
        // And decide() never hands out one, whatever the pools say.
        for fixture in [
            "all-ok",
            "all-exhausted",
            "claude-exhausted-codex-ok",
            "claude-unknown-codex-ok",
            "opencode-cooling",
        ] {
            for name in base.names() {
                let mut t = base.require(name).unwrap().clone();
                t.fallbacks = free
                    .iter()
                    .cloned()
                    .chain(["codex-sol".to_string()])
                    .collect();
                if let Decision::Substitute { via, .. } = decide(
                    &t,
                    &base,
                    &view(fixture),
                    BalanceMode::Auto,
                    GateFlags::default(),
                ) {
                    assert!(!free.contains(&via), "{fixture}: {name} -> {via}");
                }
            }
        }
    }

    #[test]
    fn rule_6_warns_on_claude_only_tools() {
        let mut r = roster();
        let mut t = r.require("researcher").unwrap().clone();
        t.persona.push_str("\nUse the `WebSearch` tool freely.\n");
        r.insert_for_test(t);
        let w = fallback_warnings(&r);
        assert!(
            w.iter()
                .any(|x| x.contains("researcher") && x.contains("WebSearch")),
            "{w:?}"
        );
        assert!(!names_tool("Finish the Task.", "Task"));
    }
}
