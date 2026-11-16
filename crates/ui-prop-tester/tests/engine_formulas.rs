mod engine_harness;

use engine_harness::{fake, plain, Workspace, MACHINE};
use serde_json::json;

fn verdict(body: &str, actions: &str) -> (i32, Vec<ui_prop_tester::trace::Violation>, String) {
    let workspace = Workspace::new();
    let spec = workspace.spec(&format!("export const go = step(\"a\");\n{body}"));
    let (outcome, trace) =
        workspace.test(&plain(), &spec, &["--actions-max", actions, "--seed", "1"]);
    let summary = trace.summary.clone().unwrap();
    (outcome.code, summary.violations, outcome.all())
}

#[test]
fn always_fails_at_the_first_counterexample_and_reports_its_cells() {
    let (code, violations, all) =
        verdict("export const small = always(() => count.current < 3);", "6");
    assert_eq!(code, 2, "{all}");
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].property, "small");
    assert_eq!(violations[0].state_index, 3);
    assert_eq!(violations[0].path, "always > now");
    assert_eq!(violations[0].cells, json!({"count": 3}));
    assert!(all.contains("violation of small at state 3"), "{all}");
}

#[test]
fn a_bare_now_checks_only_the_first_state() {
    let (code, _, all) = verdict("export const first = now(() => count.current === 0);", "4");
    assert_eq!(code, 0, "{all}");
    let (code, violations, _) =
        verdict("export const first = now(() => count.current === 1);", "4");
    assert_eq!(code, 2);
    assert_eq!(violations[0].state_index, 0);
}

#[test]
fn always_that_never_fails_holds_at_the_end_of_the_trace() {
    let (code, violations, all) =
        verdict("export const fine = always(() => count.current >= 0);", "5");
    assert_eq!(code, 0, "{all}");
    assert!(violations.is_empty());
}

#[test]
fn eventually_passes_once_reached_and_fails_when_still_pending_at_the_end() {
    let (code, _, all) = verdict(
        "export const reach = eventually(() => count.current === 3);",
        "5",
    );
    assert_eq!(code, 0, "{all}");
    let (code, violations, all) = verdict(
        "export const reach = eventually(() => count.current === 9);",
        "5",
    );
    assert_eq!(code, 2, "{all}");
    assert_eq!(violations[0].state_index, 5);
    assert_eq!(
        violations[0].message.as_deref(),
        Some("still pending when the trace ended")
    );
    assert_eq!(violations[0].cells, json!({"count": 5}));
}

#[test]
fn next_checks_the_following_state() {
    let (code, _, all) = verdict(
        "export const steps1 = always(now(() => { const before = count.current; return next(() => count.current === before + 1); })).within(250, \"milliseconds\");",
        "5",
    );
    assert_eq!(code, 0, "{all}");
    let (code, violations, _) = verdict(
        "export const second = next(() => count.current === 7);",
        "3",
    );
    assert_eq!(code, 2);
    assert_eq!(violations[0].state_index, 1);
    assert_eq!(violations[0].path, "next > now");
}

#[test]
fn an_unmet_next_fails_at_the_end_only_when_its_obligation_was_active() {
    let (code, violations, all) = verdict(
        "export const guarded = always(implies(() => count.current === 3, next(() => count.current === 4)));",
        "3",
    );
    assert_eq!(code, 2, "{all}");
    assert_eq!(violations[0].state_index, 3);
    assert_eq!(
        violations[0].message.as_deref(),
        Some("the trace ended before the next state")
    );
    assert_eq!(violations[0].path, "always > implies > then > next");
    assert_eq!(violations[0].cells, json!({"count": 3}));
    let (code, _, all) = verdict(
        "export const guarded = always(implies(() => count.current === 7, next(() => count.current === 8)));",
        "3",
    );
    assert_eq!(code, 0, "{all}");
}

#[test]
fn closures_capture_cell_values_across_states() {
    let (code, _, all) = verdict(
        "export const grows = always(() => { const before = count.current; return implies(() => count.current < 4, next(() => count.current === before + 1)); });",
        "4",
    );
    assert_eq!(code, 0, "{all}");
    let (code, violations, all) = verdict(
        "export const jumps = always(() => { const before = count.current; return next(() => count.current === before + 2); });",
        "4",
    );
    assert_eq!(code, 2, "{all}");
    assert_eq!(violations[0].state_index, 1);
    assert_eq!(violations[0].path, "always > now > next > now");
    assert_eq!(violations[0].cells, json!({"count": 1, "count@0": 0}));
}

#[test]
fn logical_operators_combine_with_three_valued_results() {
    let cases = [
        ("and(() => true, () => count.current < 2)", 2),
        ("and(() => true, () => true)", 0),
        ("or(() => false, () => count.current < 9)", 0),
        ("or(() => false, () => count.current < 2)", 2),
        ("not(() => count.current === 2)", 2),
        ("not(() => false)", 0),
        ("implies(() => count.current === 2, () => false)", 2),
        ("implies(() => false, () => false)", 0),
    ];
    for (formula, expected) in cases {
        let (code, _, all) = verdict(&format!("export const p = always({formula});"), "4");
        assert_eq!(code, expected, "{formula}: {all}");
    }
    let (code, _, all) = verdict(
        "export const p = not(eventually(() => count.current === 9));",
        "3",
    );
    assert_eq!(code, 0, "{all}");
    let (code, _, all) = verdict(
        "export const p = or(eventually(() => count.current === 9), always(() => true));",
        "3",
    );
    assert_eq!(code, 0, "{all}");
}

#[test]
fn within_bounds_use_state_timestamps_in_milliseconds_and_seconds() {
    let (code, violations, all) = verdict(
        "export const quick = eventually(() => count.current === 3).within(250, \"milliseconds\");",
        "6",
    );
    assert_eq!(code, 2, "{all}");
    assert_eq!(violations[0].state_index, 3);
    assert_eq!(
        violations[0].message.as_deref(),
        Some("did not hold within 250 milliseconds")
    );
    assert_eq!(violations[0].path, "eventually within 250 milliseconds");
    let (code, _, all) = verdict(
        "export const slow = eventually(() => count.current === 3).within(1, \"seconds\");",
        "6",
    );
    assert_eq!(code, 0, "{all}");
    let (code, _, all) = verdict(
        "export const early = always(() => count.current < 3).within(250, \"milliseconds\");",
        "6",
    );
    assert_eq!(code, 0, "{all}");
    let (code, violations, all) = verdict(
        "export const early = always(() => count.current < 3).within(0.3, \"seconds\");",
        "6",
    );
    assert_eq!(code, 2, "{all}");
    assert_eq!(violations[0].state_index, 3);
}

#[test]
fn a_bad_within_unit_is_a_setup_error() {
    let (code, _, all) = verdict(
        "export const bad = always(() => true).within(3, \"hours\");",
        "2",
    );
    assert_eq!(code, 1);
    assert!(all.contains("unit must be"), "{all}");
}

#[test]
fn several_violations_are_all_reported_unless_exit_on_violation() {
    let workspace = Workspace::new();
    let spec = workspace.spec(
        "export const go = step(\"a\");\nexport const below2 = always(() => count.current < 2);\nexport const below4 = always(() => count.current < 4);",
    );
    let (outcome, trace) = workspace.test(&plain(), &spec, &["--actions-max", "6"]);
    assert_eq!(outcome.code, 2, "{}", outcome.all());
    let summary = trace.summary.unwrap();
    assert_eq!(summary.violations.len(), 2);
    assert_eq!(trace.states.len(), 7);
    assert_eq!(trace.states[2].violations.len(), 1);
    assert_eq!(trace.states[4].violations.len(), 1);

    let (outcome, trace) = workspace.test(
        &plain(),
        &spec,
        &["--actions-max", "6", "--exit-on-violation"],
    );
    assert_eq!(outcome.code, 2);
    let summary = trace.summary.unwrap();
    assert_eq!(summary.violations.len(), 1);
    assert_eq!(summary.violations[0].property, "below2");
    assert_eq!(trace.states.len(), 3);
}

#[test]
fn an_extractor_reading_another_cell_aborts_with_exit_1() {
    let workspace = Workspace::new();
    let spec = workspace
        .spec("export const go = step(\"a\");\nexport const bad = extract((s) => count.current);");
    let (outcome, _) = workspace.test(&plain(), &spec, &["--actions-max", "2"]);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome
            .err
            .contains("an extractor cannot read another extractor's cell"),
        "{}",
        outcome.err
    );
}

#[test]
fn an_extractor_exception_aborts_with_its_javascript_stack() {
    let workspace = Workspace::new();
    let spec = workspace.spec(
        "export const go = step(\"a\");\nfunction failAt(s) {\n  if (s.steps === 2) { throw new RangeError(\"too far\"); }\n  return s.steps;\n}\nexport const risky = extract((s) => failAt(s));",
    );
    let (outcome, trace) = workspace.test(&plain(), &spec, &["--actions-max", "5"]);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome
            .err
            .contains("the extractor risky threw RangeError: too far"),
        "{}",
        outcome.err
    );
    assert!(outcome.err.contains("failAt"), "{}", outcome.err);
    assert!(outcome.err.contains("spec.js:8"), "{}", outcome.err);
    assert_eq!(trace.summary.unwrap().outcome, "error");
    assert_eq!(trace.states.len(), 2);
}

#[test]
fn remote_extraction_fills_cells_and_reports_page_side_exceptions() {
    let workspace = Workspace::new();
    let drivers = fake(r#"{"remote": true}"#);
    let spec = workspace.spec("export const go = step(\"a\");\nexport const doubled = extract((s) => s.steps * 2);\nexport const small = always(() => doubled.current < 4);");
    let (outcome, trace) = workspace.test(&drivers, &spec, &["--actions-max", "4"]);
    assert_eq!(outcome.code, 2, "{}", outcome.all());
    assert_eq!(trace.states[1].cells["doubled"], json!(2));
    assert_eq!(trace.summary.unwrap().violations[0].state_index, 2);

    let spec = workspace.spec("export const go = step(\"a\");\nconst limit = 3;\nexport const closes = extract((s) => s.steps < limit);");
    let (outcome, _) = workspace.test(&drivers, &spec, &["--actions-max", "4"]);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("the extractor closes threw"),
        "{}",
        outcome.err
    );
    assert!(outcome.err.contains("limit"), "{}", outcome.err);
}

#[test]
fn a_predicate_that_throws_aborts_and_names_the_property() {
    let workspace = Workspace::new();
    let spec = workspace.spec("export const go = step(\"a\");\nexport const broken = always(() => { if (count.current === 1) { throw new Error(\"nope\"); } return true; });");
    let (outcome, _) = workspace.test(&plain(), &spec, &["--actions-max", "3"]);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome
            .err
            .contains("the property broken threw Error: nope"),
        "{}",
        outcome.err
    );
}

#[test]
fn the_ended_state_stops_the_walk_and_resolves_obligations() {
    let workspace = Workspace::new();
    let drivers = fake(MACHINE);
    let spec = workspace.spec("export const go = step(\"quit\");\nexport const fixes = eventually(() => screen.current === \"broken\");");
    let (outcome, trace) = workspace.test(&drivers, &spec, &[]);
    assert_eq!(outcome.code, 2, "{}", outcome.all());
    assert_eq!(trace.states.len(), 2);
    assert!(outcome.out.contains("the target ended"), "{}", outcome.out);
}
