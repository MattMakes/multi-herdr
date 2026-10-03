//! ARC-12 and ARC-13: routing is pure, matches the A0 oracle, and names why
//! each teammate is excluded.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use horch_core::harness::HarnessKind;
use horch_core::ids::TeammateName;
use horch_core::roster::{Roster, Teammate};
use horch_core::routing::balance::touched_pools;
use horch_core::routing::decision::{
    self, Decision, GateFlags, RoutingDecision, RoutingMode, RoutingProvenance,
};
use horch_core::routing::eligible::{
    eligible_fallbacks, roster_eligibility, EligibilityFilter, EligibleEntry, ExclusionReason,
    Verdict,
};
use horch_core::routing::policy::{BalanceMode, Policy};
use horch_core::routing::quota::{QuotaFile, QuotaView};
use serde_json::{json, Value};

/// The time and policy the A0 routing oracle pinned.
const ROUTING_NOW: &str = "2026-09-28T18:00:00Z";

fn core_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The repo roster without reading `HOME` or `HORCH_TEAMMATES_DIR`.
fn roster() -> Roster {
    let mut r = Roster::builtin().unwrap();
    r.overlay(&core_dir().join("../../teammates")).unwrap();
    r
}

fn view_at(path: &Path) -> QuotaView {
    let file = QuotaFile::read(path).unwrap();
    let now = horch_core::clock::parse(ROUTING_NOW).unwrap();
    QuotaView::new(file, now, Policy::default(), true)
}

fn fixture(name: &str) -> PathBuf {
    core_dir()
        .join("tests/fixtures/telemetry/quota")
        .join(format!("{name}.json"))
}

fn view(name: &str) -> QuotaView {
    view_at(&fixture(name))
}

// ─── ARC-12 ─────────────────────────────────────────────────────────────────

/// The object `horch route --json` prints, from the routing module.
fn route_json(
    t: &Teammate,
    roster: &Roster,
    view: &QuotaView,
    mode: BalanceMode,
    d: &Decision,
) -> Value {
    let (kind, via, reason) = match d {
        Decision::Spawn { note, .. } => ("spawn", None, note.clone()),
        Decision::Substitute { via, reason, .. } => {
            ("substitute", Some(via.clone()), Some(reason.clone()))
        }
        Decision::Refuse { reason, .. } => ("refuse", None, Some(reason.clone())),
    };
    json!({
        "decision": kind,
        "teammate": t.name,
        "via": via,
        "reason": reason,
        "line": d.line(),
        "mode": mode.as_str(),
        "pools": touched_pools(t, roster, view),
    })
}

fn flag(label: &str) -> GateFlags {
    match label {
        "none" => GateFlags::default(),
        "exact" => GateFlags {
            exact: true,
            force: false,
        },
        "force" => GateFlags {
            exact: false,
            force: true,
        },
        other => panic!("unknown flag {other}"),
    }
}

#[test]
fn arc_12_decisions_match_baseline() {
    let roster = roster();
    let dir = core_dir().join("tests/oracles/routing");
    let mut files = 0;
    let mut cells = 0;
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if stem == "fallback_problems" {
            continue;
        }
        files += 1;
        let oracle: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(oracle["_now"], ROUTING_NOW);
        let view = view(&stem);
        let decisions = oracle["decisions"].as_object().unwrap();
        for (key, want) in decisions {
            let parts: Vec<&str> = key.split('|').collect();
            let [name, flag_label, mode_label] = parts[..] else {
                panic!("bad key {key}");
            };
            let t = roster.require(name).unwrap();
            let mode: BalanceMode = mode_label.parse().unwrap();
            let flags = flag(flag_label);
            let d = decision::decide(t, &roster, &view, mode, flags);
            let resolved = decision::resolve(t, &roster, &d).map(|r| {
                json!({
                    "name": r.name,
                    "agent": r.agent.as_str(),
                    "model": r.model,
                    "effort": r.effort,
                })
            });
            let got = json!({
                "route_json": route_json(t, &roster, &view, mode, &d),
                "decision": serde_json::to_value(&d).unwrap(),
                "resolved": resolved,
            });
            assert_eq!(&got, want, "{stem}: {key}");

            // The typed forms agree with the decision.
            let typed = RoutingDecision::from(&d);
            let prov = RoutingProvenance::from_decision(
                t,
                &roster,
                &view,
                &d,
                RoutingMode::for_gate(mode, flags),
            )
            .unwrap();
            match (&d, &typed, &prov) {
                (Decision::Spawn { .. }, RoutingDecision::Spawn { teammate }, Some(p)) => {
                    assert_eq!(teammate, name);
                    assert_eq!(p.resolved.as_str(), name);
                    assert_eq!(p.fallback_index, None);
                }
                (
                    Decision::Substitute { via, .. },
                    RoutingDecision::Substitute { resolved, .. },
                    Some(p),
                ) => {
                    assert_eq!(resolved, via);
                    assert_eq!(p.resolved.as_str(), via);
                    let i = p.fallback_index.unwrap() as usize;
                    assert_eq!(&t.fallbacks[i], via);
                }
                (Decision::Refuse { .. }, RoutingDecision::Refuse { .. }, None) => {}
                other => panic!("{stem}: {key}: {other:?}"),
            }
            cells += 1;
        }
    }
    assert_eq!(files, 12);
    assert!(cells >= 12 * 33 * 9, "{cells}");
}

// ─── ARC-13 ─────────────────────────────────────────────────────────────────

/// The source of `routing/` before its test module.
fn production_source(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap();
    match text.find("#[cfg(test)]") {
        Some(i) => text[..i].to_string(),
        None => text,
    }
}

#[test]
fn arc_13_routing_never_launches() {
    let dir = core_dir().join("src/routing");
    let mut scanned = 0;
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name == "quota_probe.rs" || name == "snapshot.rs" {
            continue;
        }
        let src = production_source(&path);
        for bad in [
            "std::process",
            "Command::new",
            "std::fs",
            "herdr",
            "launch::",
            "std::env::var",
        ] {
            assert!(!src.contains(bad), "routing/{name} contains `{bad}`");
        }
        scanned += 1;
    }
    // mod, policy, quota, balance, eligible, decision.
    assert_eq!(scanned, 6);
}

fn with(roster: &mut Roster, from: &str, edit: impl FnOnce(&mut Teammate)) {
    let mut t = roster.require(from).unwrap().clone();
    edit(&mut t);
    roster.insert_for_test(t);
}

fn verdict_of<'a>(entries: &'a [EligibleEntry], name: &str) -> &'a EligibleEntry {
    entries
        .iter()
        .find(|e| e.teammate.as_str() == name)
        .unwrap_or_else(|| panic!("{name} not in {entries:#?}"))
}

#[test]
fn arc_13_exclusion_reasons() {
    let mut r = roster();
    with(&mut r, "codex-sol", |t| {
        t.name = "astra-worker".into();
        t.model = Some("gpt-6-astra".into());
    });
    with(&mut r, "sonnet", |t| {
        t.name = "modelless".into();
        t.model = None;
    });
    with(&mut r, "codex-sol", |t| {
        t.name = "codex-minimal".into();
        t.effort = Some("minimal".into());
    });
    with(&mut r, "smoke", |t| {
        t.name = "smoke-visible".into();
        t.hidden = false;
    });
    with(&mut r, "opus", |t| {
        t.name = "asker".into();
        t.fallbacks = vec![
            "ghost".into(),
            "smoke".into(),
            "astra-worker".into(),
            "opencode-pickle".into(),
            "codex-sol".into(),
        ];
    });
    let mut seen = BTreeMap::new();
    let mut expect = |entries: &[EligibleEntry], name: &str, reason: ExclusionReason| {
        let e = verdict_of(entries, name);
        assert_eq!(e.verdict, Verdict::Excluded(reason), "{name}: {e:?}");
        seen.insert(reason, name.to_string());
    };

    // The fallback rules, from a request's `fallbacks:`.
    let ok = view("all-ok");
    let asker = r.require("asker").unwrap().clone();
    let fb = eligible_fallbacks(&asker, &r, &ok);
    expect(&fb, "ghost", ExclusionReason::NotInRoster);
    expect(&fb, "smoke", ExclusionReason::Hidden);
    expect(&fb, "astra-worker", ExclusionReason::ReservedTier);
    expect(&fb, "opencode-pickle", ExclusionReason::TrainsOnInput);
    assert_eq!(verdict_of(&fb, "codex-sol").verdict, Verdict::Eligible);
    assert_eq!(verdict_of(&fb, "asker").fallback_index, None);
    assert_eq!(verdict_of(&fb, "codex-sol").fallback_index, Some(4));

    // The planner's rules, over the whole roster.
    let all = roster_eligibility(&r, &ok, &EligibilityFilter::default());
    expect(&all, "modelless", ExclusionReason::Unspawnable);
    expect(&all, "codex-minimal", ExclusionReason::EffortUnsupported);
    expect(&all, "smoke-visible", ExclusionReason::AgentNone);
    assert_eq!(verdict_of(&all, "sonnet").verdict, Verdict::Eligible);
    let names: Vec<&str> = all.iter().map(|e| e.teammate.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "sorted by name");

    let exhausted = roster_eligibility(&r, &view("all-exhausted"), &EligibilityFilter::default());
    expect(&exhausted, "opus", ExclusionReason::PoolBlocked);

    let sonnet = TeammateName::new("sonnet").unwrap();
    let filter = EligibilityFilter {
        excluded: [TeammateName::new("opus").unwrap()].into(),
        max_cost_microusd: Some(100),
        estimated_cost_microusd: [(sonnet, 200)].into(),
        available_harnesses: Some(vec![HarnessKind::Claude]),
    };
    let filtered = roster_eligibility(&r, &ok, &filter);
    expect(&filtered, "opus", ExclusionReason::ExcludedByConfig);
    expect(&filtered, "sonnet", ExclusionReason::OverBudget);
    expect(&filtered, "codex-sol", ExclusionReason::HarnessUnavailable);

    assert_eq!(seen.len(), 11, "every reason produced: {seen:?}");

    // The wire form: snake_case reasons, the verdict flattened.
    let e = serde_json::to_value(verdict_of(&fb, "ghost")).unwrap();
    assert_eq!(e["verdict"], "excluded");
    assert_eq!(e["reason"], "not_in_roster");
    let back: EligibleEntry = serde_json::from_value(e).unwrap();
    assert_eq!(&back, verdict_of(&fb, "ghost"));
    let e = serde_json::to_value(verdict_of(&fb, "codex-sol")).unwrap();
    assert_eq!(e["verdict"], "eligible");
    assert_eq!(e["pool_state"], "ok");
}

// ─── ARC-14 ─────────────────────────────────────────────────────────────────

#[test]
fn arc_14_legacy_record_resumes_with_provenance() {
    let r = roster();
    let researcher = r.require("researcher").unwrap();
    let i = researcher
        .fallbacks
        .iter()
        .position(|f| f == "codex-sol")
        .unwrap() as u32;
    let p = RoutingProvenance::legacy(
        "researcher",
        Some("codex-sol"),
        "codex",
        "gpt-5.6-sol",
        Some("claude 7d 100%"),
        Some(researcher),
    )
    .unwrap()
    .resumed();
    assert_eq!(p.requested.as_str(), "researcher");
    assert_eq!(p.resolved.as_str(), "codex-sol");
    assert_eq!(p.fallback_index, Some(i));
    assert_eq!(p.pool, "codex");
    assert_eq!(p.mode, RoutingMode::Resume);
    let wire = serde_json::to_value(&p).unwrap();
    assert_eq!(wire["mode"], "resume");
    assert_eq!(wire["pool_state"], "unknown");

    // A record from before A5 has no `routing` key and saves without one.
    let rec: horch_core::ledger::Record = serde_json::from_value(json!({
        "record_id": "r1", "session_id": null, "agent": "claude", "tier": "opus",
        "model": "opus", "role": "opus-1", "status": "done", "task": "",
        "history": [], "created_at": "", "updated_at": ""
    }))
    .unwrap();
    assert!(rec.routing.is_none());
    assert!(serde_json::to_value(&rec).unwrap().get("routing").is_none());
}
