//! Moved to [`crate::routing`] (A5): the gate to `routing::decision`, the
//! eligible set to `routing::eligible`. This shim keeps old paths compiling
//! until A12. The roster rules live in `roster::validation` (A3).

pub use crate::routing::decision::*;
pub use crate::routing::eligible::trains_on_input;

#[allow(unused_imports)]
use crate::policy::BalanceMode;
#[allow(unused_imports)]
use crate::quota::{self, QuotaView};
#[allow(unused_imports)]
use crate::teammates::{Agent, Roster, Teammate};

// ─── roster rules ───────────────────────────────────────────────────────────

#[cfg(test)]
use crate::roster::validation::names_tool;
pub use crate::roster::validation::{fallback_problems, fallback_warnings};

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
