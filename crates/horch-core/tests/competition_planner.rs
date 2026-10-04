//! CMP-03, CMP-06 and EXP-02: the round state table, the candidate planner
//! and the budget rule. Every test is pure: a fixture roster and quota view,
//! no process, no herdr, no git.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use horch_core::competition::budget::{BudgetAction, BudgetPolicy};
use horch_core::competition::config::{BudgetConfig, Caps, DatasetConfig, JudgeConfig, Strategy};
use horch_core::competition::model::RoundState;
use horch_core::competition::planner::{
    candidate_planned_payload, plan_round, round_created_payload, round_seed, Baseline, PlanInput,
    RoundPlan,
};
use horch_core::competition::state::{transition, RoundEvent, TABLE};
use horch_core::ids::{RoundId, TeammateName};
use horch_core::measure::event::{EventKind, InterventionSource, SlotKind};
use horch_core::measure::testkit::{property, SplitMix64};
use horch_core::roster::Roster;
use horch_core::routing::decision::RoutingDecision;
use horch_core::routing::eligible::{EligibilityFilter, ExclusionReason, Verdict};
use horch_core::routing::policy::Policy;
use horch_core::routing::quota::{QuotaFile, QuotaView};
use horch_core::teacher::TeacherRef;
use horch_core::usage::money::MicroUsd;
use horch_core::usage::{builtin_prices, price_for};

// ─── CMP-03: the round state table ──────────────────────────────────────────

/// Every round event, each condition variant once.
fn all_events() -> Vec<RoundEvent> {
    use RoundEvent as E;
    let mut v = vec![
        E::ExperimentCreated,
        E::PreflightPassed,
        E::PreflightFailed,
        E::ExperimentAborted,
        E::RoundCreated,
        E::CandidateSpawned,
        E::CandidateFrozen,
        E::JudgeScheduled,
        E::JudgeStarted,
        E::JudgeCompleted,
        E::WinnerSelected,
        E::PromotionRequested,
        E::WinnerRejected,
        E::PromotionStarted,
        E::PromotionConflicted,
        E::PromotionCompleted,
        E::PromotionRolledBack,
        E::NeedsIntervention(InterventionSource::Judge),
        E::NeedsIntervention(InterventionSource::Promotion),
        E::NeedsIntervention(InterventionSource::Operator),
        E::OperatorPromote,
        E::CleanupStarted,
        E::RoundCompleted,
    ];
    for last in [false, true] {
        v.push(E::CandidatePlanned { last });
        v.push(E::WorktreeCreated { last });
        v.push(E::CandidateEnded { last });
        v.push(E::JudgeFailed { last_attempt: last });
        for any_eligible in [false, true] {
            v.push(E::ValidationCompleted { last, any_eligible });
        }
    }
    v
}

const ALL_STATES: [RoundState; 16] = [
    RoundState::Created,
    RoundState::Preflight,
    RoundState::Aborted,
    RoundState::Planned,
    RoundState::Provisioning,
    RoundState::Running,
    RoundState::Validating,
    RoundState::JudgingBackground,
    RoundState::Decided,
    RoundState::Revalidating,
    RoundState::Promoting,
    RoundState::Promoted,
    RoundState::NeedsIntervention,
    RoundState::Rejected,
    RoundState::Cleanup,
    RoundState::Complete,
];

/// Apply `events` from `from`; panic on the first invalid one.
fn walk(from: RoundState, events: &[RoundEvent]) -> Vec<RoundState> {
    let mut path = vec![from];
    for e in events {
        let next = transition(*path.last().unwrap(), e)
            .unwrap_or_else(|err| panic!("{err} on path {path:?}"));
        path.push(next);
    }
    path
}

/// From CREATED to JUDGING_BACKGROUND, for a round of 2 candidates.
fn to_judging() -> Vec<RoundEvent> {
    use RoundEvent as E;
    vec![
        E::ExperimentCreated,
        E::PreflightPassed,
        E::RoundCreated,
        E::CandidatePlanned { last: false },
        E::CandidatePlanned { last: true },
        E::WorktreeCreated { last: false },
        E::WorktreeCreated { last: true },
        E::CandidateSpawned,
        E::CandidateSpawned,
        E::CandidateEnded { last: false },
        E::CandidateEnded { last: true },
        E::CandidateFrozen,
        E::ValidationCompleted {
            last: false,
            any_eligible: false,
        },
        E::CandidateFrozen,
        E::ValidationCompleted {
            last: true,
            any_eligible: true,
        },
        E::JudgeScheduled,
        E::JudgeStarted,
        E::JudgeFailed {
            last_attempt: false,
        },
        E::JudgeScheduled,
        E::JudgeStarted,
        E::JudgeCompleted,
    ]
}

#[test]
fn cmp_03_transition_table() {
    use RoundEvent as E;
    use RoundState as S;

    // Every row is accepted and goes where it says.
    let events = all_events();
    for (from, name, to) in TABLE {
        let e = events
            .iter()
            .find(|e| e.name() == *name)
            .unwrap_or_else(|| panic!("no RoundEvent is named {name}"));
        assert_eq!(transition(*from, e), Ok(*to), "{from} {name}");
    }
    // Every pair without a row is rejected, and names the pair.
    let mut rejected = 0;
    for from in ALL_STATES {
        for e in &events {
            let row = TABLE.iter().any(|(f, n, _)| *f == from && *n == e.name());
            match transition(from, e) {
                Ok(_) => assert!(row, "{from} {e} accepted without a row"),
                Err(err) => {
                    assert!(!row, "{from} {e} rejected with a row");
                    assert_eq!(err.from, from);
                    assert!(err.to_string().contains(from.as_str()), "{err}");
                    rejected += 1;
                }
            }
        }
    }
    assert!(rejected > 500, "{rejected}");
    // A sample of invalid pairs, by hand.
    for (from, e) in [
        (S::Planned, E::PromotionCompleted),
        (S::Decided, E::PromotionStarted),
        (S::Decided, E::PromotionCompleted),
        (S::JudgingBackground, E::PromotionRequested),
        (S::Running, E::WinnerSelected),
        (S::Complete, E::CleanupStarted),
        (S::Aborted, E::RoundCreated),
        (S::NeedsIntervention, E::RoundCompleted),
        (S::Revalidating, E::PromotionCompleted),
    ] {
        assert!(transition(from, &e).is_err(), "{from} {e}");
    }

    // Default path: DECIDED → CLEANUP → COMPLETE.
    let mut default = to_judging();
    default.extend([E::WinnerSelected, E::CleanupStarted, E::RoundCompleted]);
    let path = walk(S::Created, &default);
    assert_eq!(
        &path[path.len() - 4..],
        &[S::JudgingBackground, S::Decided, S::Cleanup, S::Complete]
    );

    // Promote path: DECIDED → REVALIDATING → PROMOTING → PROMOTED → CLEANUP → COMPLETE.
    let mut promote = to_judging();
    promote.extend([
        E::WinnerSelected,
        E::PromotionRequested,
        E::PromotionStarted,
        E::PromotionCompleted,
        E::CleanupStarted,
        E::RoundCompleted,
        E::PromotionRolledBack,
    ]);
    let path = walk(S::Created, &promote);
    assert_eq!(
        &path[path.len() - 8..],
        &[
            S::JudgingBackground,
            S::Decided,
            S::Revalidating,
            S::Promoting,
            S::Promoted,
            S::Cleanup,
            S::Complete,
            S::Complete,
        ]
    );

    // Promotion ends REJECTED or NEEDS_INTERVENTION, then cleanup.
    for (end, mid) in [
        (E::WinnerRejected, S::Rejected),
        (E::PromotionConflicted, S::NeedsIntervention),
        (
            E::NeedsIntervention(InterventionSource::Promotion),
            S::NeedsIntervention,
        ),
    ] {
        let path = walk(
            S::Decided,
            &[
                E::PromotionRequested,
                end,
                E::CleanupStarted,
                E::RoundCompleted,
            ],
        );
        assert_eq!(path[2], mid);
        assert_eq!(*path.last().unwrap(), S::Complete);
    }

    // `promote <round>` re-enters at DECIDED from NEEDS_INTERVENTION and
    // from a completed default round.
    for from in [S::NeedsIntervention, S::Complete] {
        let path = walk(
            from,
            &[
                E::OperatorPromote,
                E::PromotionRequested,
                E::PromotionStarted,
                E::PromotionCompleted,
                E::CleanupStarted,
                E::RoundCompleted,
            ],
        );
        assert_eq!(path[1], S::Decided);
        assert_eq!(*path.last().unwrap(), S::Complete);
    }

    // 0 eligible: the judge is skipped.
    let path = walk(
        S::Validating,
        &[
            E::ValidationCompleted {
                last: true,
                any_eligible: false,
            },
            E::WinnerRejected,
            E::CleanupStarted,
            E::RoundCompleted,
        ],
    );
    assert_eq!(
        path[1..],
        [S::Validating, S::Rejected, S::Cleanup, S::Complete]
    );

    // A second failed judge attempt needs the operator.
    let path = walk(
        S::JudgingBackground,
        &[E::JudgeFailed { last_attempt: true }],
    );
    assert_eq!(path[1], S::NeedsIntervention);

    // A failed preflight aborts the experiment.
    let path = walk(
        S::Created,
        &[
            E::ExperimentCreated,
            E::PreflightFailed,
            E::ExperimentAborted,
        ],
    );
    assert_eq!(path[3], S::Aborted);
}

#[test]
fn cmp_03_prop_no_invalid_path_to_promoted() {
    let events = all_events();
    let mut reached = 0u32;
    property(0x0C3D_0003, 10_000, |rng: &mut SplitMix64| {
        let mut state = RoundState::Created;
        let mut path = vec![state];
        for _ in 0..120 {
            // Half the draws come from the current state's rows, so walks go
            // deep; the other half are any event, mostly invalid.
            let e = if rng.below(2) == 0 {
                let rows: Vec<&str> = TABLE
                    .iter()
                    .filter(|(f, _, _)| *f == state)
                    .map(|(_, n, _)| *n)
                    .collect();
                if rows.is_empty() {
                    break;
                }
                let name = rows[rng.below(rows.len() as u64) as usize];
                *events.iter().find(|e| e.name() == name).unwrap()
            } else {
                events[rng.below(events.len() as u64) as usize]
            };
            let Ok(next) = transition(state, &e) else {
                continue;
            };
            match next {
                RoundState::Promoted if state != RoundState::Promoted => {
                    assert_eq!(state, RoundState::Promoting, "{path:?}");
                }
                RoundState::Promoting => assert_eq!(state, RoundState::Revalidating, "{path:?}"),
                RoundState::Revalidating => assert_eq!(state, RoundState::Decided, "{path:?}"),
                _ => {}
            }
            state = next;
            path.push(state);
        }
        if let Some(p) = path.iter().position(|s| *s == RoundState::Promoted) {
            reached += 1;
            let before = &path[..p];
            let d = before.iter().rposition(|s| *s == RoundState::Decided);
            let r = before.iter().rposition(|s| *s == RoundState::Revalidating);
            let m = before.iter().rposition(|s| *s == RoundState::Promoting);
            assert!(
                matches!((d, r, m), (Some(d), Some(r), Some(m)) if d < r && r < m && m + 1 == p),
                "{path:?}"
            );
        }
    });
    assert!(reached > 100, "only {reached} walks reached PROMOTED");
}

// ─── CMP-06: the planner ────────────────────────────────────────────────────

/// The time the A0 routing oracle pinned.
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

fn fixture(name: &str) -> PathBuf {
    core_dir()
        .join("tests/fixtures/telemetry/quota")
        .join(format!("{name}.json"))
}

fn view(name: &str) -> QuotaView {
    let file = QuotaFile::read(&fixture(name)).unwrap();
    let now = horch_core::clock::parse(ROUTING_NOW).unwrap();
    QuotaView::new(file, now, Policy::default(), true)
}

fn config() -> DatasetConfig {
    DatasetConfig {
        candidates: 4,
        strategy: Strategy::Diverse,
        budget: BudgetConfig {
            soft_usd_micro: 40_000_000,
            hard_usd_micro: 50_000_000,
            judge_reserve_usd_micro: 5_000_000,
            ..BudgetConfig::default()
        },
        judge: JudgeConfig::default(),
        baseline: None,
        gates: Vec::new(),
        caps: Caps::default(),
        exclude: Vec::new(),
        promote_to: None,
        worktree_root: None,
        allow_dirty: false,
        prune_branches: false,
        retain_transcripts: false,
    }
}

fn rid(s: &str) -> RoundId {
    RoundId::new(s).unwrap()
}

fn tm(s: &str) -> TeammateName {
    TeammateName::new(s).unwrap()
}

struct Fixture {
    roster: Roster,
    view: QuotaView,
    filter: EligibilityFilter,
    config: DatasetConfig,
}

impl Fixture {
    fn new(quota: &str) -> Self {
        Fixture {
            roster: roster(),
            view: view(quota),
            filter: EligibilityFilter::default(),
            config: config(),
        }
    }

    fn plan(&self, round: &str, n: u32, baseline: Option<&str>) -> RoundPlan {
        let round_id = rid(round);
        plan_round(&PlanInput {
            round_id: &round_id,
            index: 0,
            base_sha: "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
            n,
            baseline: baseline.map(tm),
            roster: &self.roster,
            view: &self.view,
            filter: &self.filter,
            config: &self.config,
        })
    }
}

fn slots(plan: &RoundPlan) -> Vec<SlotKind> {
    plan.candidates.iter().map(|c| c.slot).collect()
}

#[test]
fn cmp_06_planner_deterministic() {
    let f = Fixture::new("all-ok");
    let a = f.plan("rnd-1", 4, Some("sonnet"));
    assert_eq!(a, f.plan("rnd-1", 4, Some("sonnet")));
    assert_eq!(
        slots(&a),
        [
            SlotKind::Baseline,
            SlotKind::Diversity,
            SlotKind::Diversity,
            SlotKind::Exploration
        ]
    );
    let labels: Vec<&str> = a.candidates.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["A", "B", "C", "D"]);
    assert_eq!(a.candidates[0].teammate.as_str(), "sonnet");
    assert_eq!(a.seed, round_seed(&rid("rnd-1")));

    // No teammate twice; every non-baseline slot is eligible.
    let mut names: Vec<&str> = a.candidates.iter().map(|c| c.teammate.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 4);
    for c in &a.candidates[1..] {
        let e = a
            .eligible_set
            .iter()
            .find(|e| e.teammate == c.teammate)
            .unwrap();
        assert!(e.is_eligible(), "{}", c.teammate);
    }
    // The diversity slots add a new harness each while one is left.
    assert_ne!(a.candidates[1].harness, a.candidates[0].harness);

    // Another round id moves only the exploration slot.
    let mut moved = false;
    for i in 2..50 {
        let b = f.plan(&format!("rnd-{i}"), 4, Some("sonnet"));
        assert_eq!(a.candidates[..3], b.candidates[..3]);
        assert_eq!(a.eligible_set, b.eligible_set);
        assert_eq!(b.candidates[3].slot, SlotKind::Exploration);
        moved |= a.candidates[3] != b.candidates[3];
    }
    assert!(moved, "the exploration slot never moved");

    // n = 1 is the baseline alone; with no baseline, diversity fills in.
    assert_eq!(
        slots(&f.plan("rnd-1", 1, Some("sonnet"))),
        [SlotKind::Baseline]
    );
    let none = f.plan("rnd-1", 3, None);
    assert_eq!(none.baseline, Baseline::NotRequested);
    assert_eq!(
        slots(&none),
        [
            SlotKind::Diversity,
            SlotKind::Diversity,
            SlotKind::Exploration
        ]
    );
    // The config's baseline is used when there is no flag.
    let mut g = Fixture::new("all-ok");
    g.config.baseline = Some(tm("sonnet"));
    assert_eq!(g.plan("rnd-1", 4, None), a);
}

/// The same round id gives the same plan, and every planned slot has a
/// price. An eligible teammate whose model the price table does not know
/// is excluded with the reason `unpriced`, so the exploration slot never
/// draws it, and as a baseline it is not planned.
#[test]
fn cmp_06_plan_is_deterministic_and_priced() {
    let mut f = Fixture::new("all-ok");
    let mut nova = f.roster.get("codex-terra").unwrap().clone();
    nova.name = "codex-nova".into();
    nova.model = Some("gpt-9-nova".into());
    f.roster.insert_for_test(nova);
    let prices = builtin_prices();
    assert!(price_for(&prices, "gpt-9-nova").is_none());

    for i in 0..200 {
        let round = format!("rnd-{i}");
        let plan = f.plan(&round, 4, Some("sonnet"));
        assert_eq!(plan, f.plan(&round, 4, Some("sonnet")), "{round}");
        for c in &plan.candidates {
            assert!(
                price_for(&prices, c.model.as_str()).is_some(),
                "{round}: {} on {} has no price",
                c.teammate,
                c.model
            );
        }
        let nova = plan
            .eligible_set
            .iter()
            .find(|e| e.teammate.as_str() == "codex-nova")
            .unwrap();
        assert_eq!(nova.verdict, Verdict::Excluded(ExclusionReason::Unpriced));
    }

    // The reason is in the recorded payload.
    let created = round_created_payload(&f.plan("rnd-1", 4, Some("sonnet")));
    let json = EventKind::RoundCreated(created).payload();
    let nova = json["eligible_set"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["teammate"] == "codex-nova")
        .unwrap()
        .clone();
    assert_eq!(nova["verdict"], "excluded");
    assert_eq!(nova["reason"], "unpriced");

    // An unpriced baseline is not planned; the plan says why.
    let plan = f.plan("rnd-1", 3, Some("codex-nova"));
    assert!(plan
        .candidates
        .iter()
        .all(|c| c.teammate.as_str() != "codex-nova"));
    assert_eq!(
        plan.baseline.missing_reason().unwrap(),
        "codex-nova: model gpt-9-nova has no price"
    );
}

#[test]
fn cmp_06_propensities() {
    let f = Fixture::new("all-ok");
    let first = f.plan("rnd-0", 3, Some("sonnet"));
    let fixed: Vec<&str> = first.candidates[..2]
        .iter()
        .map(|c| c.teammate.as_str())
        .collect();
    let open: Vec<String> = first
        .eligible_set
        .iter()
        .filter(|e| e.is_eligible() && e.model.is_some())
        .map(|e| e.teammate.to_string())
        .filter(|n| !fixed.contains(&n.as_str()))
        .collect();
    let k = open.len();
    assert!(k >= 3, "{open:?}");
    let p = 1.0 / k as f64;

    let rounds = 10_000;
    let mut picks: BTreeMap<String, u32> = BTreeMap::new();
    for i in 0..rounds {
        let plan = f.plan(&format!("rnd-{i}"), 3, Some("sonnet"));
        assert_eq!(plan.candidates[0].propensity, 1.0);
        assert_eq!(plan.candidates[1].propensity, 1.0);
        let x = &plan.candidates[2];
        assert_eq!(x.slot, SlotKind::Exploration);
        assert!((x.propensity - p).abs() < 1e-12, "{} vs {p}", x.propensity);
        assert_eq!(plan.propensities["C"], x.propensity);
        *picks.entry(x.teammate.to_string()).or_default() += 1;
    }
    assert_eq!(picks.keys().cloned().collect::<Vec<_>>(), {
        let mut o = open.clone();
        o.sort();
        o
    });
    for (name, n) in &picks {
        let freq = f64::from(*n) / f64::from(rounds);
        assert!((freq - p).abs() <= 0.03, "{name}: {freq} vs {p}");
    }
}

#[test]
fn cmp_06_eligible_set_persisted() {
    let mut f = Fixture::new("claude-exhausted-codex-ok");
    f.config.exclude = vec![tm("codex-terra")];
    let plan = f.plan("rnd-1", 3, Some("sonnet"));
    let created = round_created_payload(&plan);
    assert_eq!(created.eligible_set, plan.eligible_set);
    assert_eq!(created.eligible_set.len(), f.roster.names().len());
    assert_eq!(created.labels, ["A", "B", "C"][..plan.candidates.len()]);
    assert_eq!(created.propensities, plan.propensities);
    assert_eq!(created.seed, round_seed(&rid("rnd-1")));

    // The payload on disk: every entry with its verdict and reason.
    let kind = EventKind::RoundCreated(created.clone());
    let json = kind.payload();
    let set = json["eligible_set"].as_array().unwrap();
    assert_eq!(set.len(), created.eligible_set.len());
    let reason = |name: &str| {
        let e = set.iter().find(|e| e["teammate"] == name).unwrap();
        (e["verdict"].clone(), e["reason"].clone())
    };
    assert_eq!(reason("codex-terra").1, "excluded_by_config");
    assert_eq!(reason("smoke").0, "excluded");
    assert!(set.iter().any(|e| e["reason"] == "pool_blocked"));
    assert!(set.iter().any(|e| e["verdict"] == "eligible"));
    for label in &created.labels {
        assert!(json["propensities"][label].is_f64(), "{label}");
    }
    let back = EventKind::from_parts(kind.name(), &json).unwrap();
    assert_eq!(back, kind);

    // Every candidate.planned matches its slot.
    for c in &plan.candidates {
        let planned = candidate_planned_payload(c);
        assert_eq!(planned.label, c.label.as_str());
        assert_eq!(planned.propensity, plan.propensities[c.label.as_str()]);
        assert_eq!(
            planned.config_id,
            format!(
                "{}|{}|{}|{}",
                c.teammate,
                c.harness.as_str(),
                c.model,
                c.effort.as_deref().unwrap_or("-")
            )
        );
        let v = EventKind::CandidatePlanned(planned).payload();
        assert!(v["slot"].is_string() && v["config_id"].is_string());
    }
    // No ineligible teammate is planned outside the baseline.
    for c in plan
        .candidates
        .iter()
        .filter(|c| c.slot != SlotKind::Baseline)
    {
        let e = plan
            .eligible_set
            .iter()
            .find(|e| e.teammate == c.teammate)
            .unwrap();
        assert_eq!(e.verdict, Verdict::Eligible);
    }
}

#[test]
fn cmp_06_baseline_routed_through_gate() {
    // Claude is exhausted and codex is ok: `horch spawn opus` runs on its
    // fallback codex-sol, and so does the baseline slot.
    let f = Fixture::new("claude-exhausted-codex-ok");
    let plan = f.plan("rnd-1", 3, Some("opus"));
    let a = &plan.candidates[0];
    assert_eq!(a.slot, SlotKind::Baseline);
    assert_eq!(a.teammate.as_str(), "codex-sol");
    assert_eq!(a.harness.as_str(), "codex");
    assert_eq!(a.propensity, 1.0);
    match &plan.baseline {
        Baseline::Routed(RoutingDecision::Substitute {
            requested,
            resolved,
            reason,
        }) => {
            assert_eq!(requested, "opus");
            assert_eq!(resolved, "codex-sol");
            assert!(!reason.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(plan.baseline.missing_reason(), None);
    // Neither opus nor codex-sol is planned twice.
    assert!(plan.candidates[1..]
        .iter()
        .all(|c| c.teammate.as_str() != "opus" && c.teammate.as_str() != "codex-sol"));

    // Every pool exhausted: the gate refuses, and no baseline slot exists.
    let f = Fixture::new("all-exhausted");
    let plan = f.plan("rnd-1", 3, Some("opus"));
    assert!(matches!(
        plan.baseline,
        Baseline::Routed(RoutingDecision::Refuse { .. })
    ));
    assert!(plan.baseline.missing_reason().unwrap().contains("opus"));
    assert!(plan.candidates.iter().all(|c| c.slot != SlotKind::Baseline));

    // A name the roster does not know.
    let f = Fixture::new("all-ok");
    let plan = f.plan("rnd-1", 3, Some("ghost"));
    assert!(matches!(plan.baseline, Baseline::Unusable { .. }));
    assert!(plan.candidates.iter().all(|c| c.slot != SlotKind::Baseline));
}

// ─── EXP-02 ─────────────────────────────────────────────────────────────────

#[test]
fn exp_02_round_records_teacher_none() {
    let f = Fixture::new("all-ok");
    let plan = f.plan("rnd-1", 3, Some("sonnet"));
    assert_eq!(plan.teacher, TeacherRef::none());
    let json = EventKind::RoundCreated(round_created_payload(&plan)).payload();
    assert_eq!(json["teacher"]["id"], "none");
    assert!(json["eligible_set"].is_array());
    assert!(json["propensities"].is_object());
    assert!(!json["label_policy_version"].as_str().unwrap().is_empty());
}

// ─── budget ─────────────────────────────────────────────────────────────────

#[test]
fn budget_policy_table() {
    // hard 50, reserve 5: the candidates may use 45.
    let config = config().budget;
    let usd = |d: i64| MicroUsd(d * 1_000_000);
    for (spent, committed, want) in [
        (0, 0, BudgetAction::Continue),
        (30, 14, BudgetAction::Continue),
        (30, 15, BudgetAction::StopLaunches),
        (44, 0, BudgetAction::Continue),
        (44, 1, BudgetAction::StopLaunches),
        (45, 0, BudgetAction::CancelRunning),
        (50, 0, BudgetAction::CancelRunning),
        (60, 10, BudgetAction::CancelRunning),
    ] {
        assert_eq!(
            BudgetPolicy::check(usd(spent), usd(committed), &config),
            want,
            "spent {spent}, committed {committed}"
        );
    }
    // 1 µ$ below the limit still continues.
    assert_eq!(
        BudgetPolicy::check(MicroUsd(44_999_999), MicroUsd(0), &config),
        BudgetAction::Continue
    );
    // No ceiling: preflight refuses the run; the rule cancels.
    assert_eq!(
        BudgetPolicy::check(MicroUsd(0), MicroUsd(0), &BudgetConfig::default()),
        BudgetAction::CancelRunning
    );
}

// ─── CMP-02 for the B3 files ────────────────────────────────────────────────

#[test]
fn cmp_02_planner_files_have_no_adapter_imports() {
    let src = core_dir().join("src/competition");
    for file in ["state.rs", "planner.rs", "diversity.rs", "budget.rs"] {
        let text = std::fs::read_to_string(src.join(file)).unwrap();
        for word in [
            "std::process",
            "herdr::",
            "workspace::",
            "launch::",
            "vcs::",
            "std::env",
            "std::fs",
        ] {
            assert!(!text.contains(word), "{file} contains {word:?}");
        }
    }
}
