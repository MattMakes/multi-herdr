mod engine_harness;

use engine_harness::{names, plain, Workspace};

fn walk(body: &str, extra: &[&str]) -> (engine_harness::Outcome, ui_prop_tester::trace::Trace) {
    let workspace = Workspace::new();
    let spec = workspace.spec(body);
    workspace.test(&plain(), &spec, extra)
}

fn count(names: &[String], wanted: &str) -> usize {
    names.iter().filter(|name| name.as_str() == wanted).count()
}

#[test]
fn weighted_choices_follow_their_weights() {
    let (outcome, trace) = walk(
        "export const pick = weighted([[3, step(\"a\")], [1, step(\"b\")]]);",
        &["--actions-max", "400", "--seed", "3"],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    let names = names(&trace);
    let a = count(&names, "a");
    assert_eq!(a + count(&names, "b"), 400);
    assert!((250..=350).contains(&a), "a was chosen {a} times");
}

#[test]
fn top_level_categories_use_their_weight_and_zero_is_never_chosen() {
    let (outcome, trace) = walk(
        "export const heavy = step(\"a\").weight(9);\nexport const light = step(\"b\");\nexport const never = step(\"z\").weight(0);",
        &["--actions-max", "300", "--seed", "4"],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    let names = names(&trace);
    assert_eq!(count(&names, "z"), 0);
    let a = count(&names, "a");
    assert!((240..=295).contains(&a), "a was chosen {a} times");
    let categories: Vec<String> = trace
        .actions()
        .iter()
        .map(|action| action["category"].as_str().unwrap().to_string())
        .collect();
    assert!(categories.iter().all(|c| c == "heavy" || c == "light"));
    assert_eq!(trace.actions()[0]["kind"], "step");
}

#[test]
fn map_filter_and_flat_map_transform_values() {
    let (outcome, trace) = walk(
        "export const evens = from([1, 2, 3, 4, 5, 6]).filter((n) => n % 2 === 0).map((n) => ({ kind: \"step\", name: \"n\" + n }));\n\
         export const nested = integer(1, 3).flatMap((n) => constant({ kind: \"step\", name: \"f\" + n }));",
        &["--actions-max", "200", "--seed", "5"],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    let names = names(&trace);
    for name in &names {
        assert!(
            ["n2", "n4", "n6", "f1", "f2", "f3"].contains(&name.as_str()),
            "unexpected {name}"
        );
    }
    for wanted in ["n2", "n4", "n6", "f1", "f2", "f3"] {
        assert!(count(&names, wanted) > 0, "{wanted} never appeared");
    }
}

#[test]
fn when_guards_accept_booleans_cells_and_functions() {
    let (outcome, trace) = walk(
        "const allow = extract((s) => s.steps < 3);\n\
         export const byFunction = step(\"early\").when(() => count.current < 2);\n\
         export const byCell = step(\"cell\").when(allow);\n\
         export const byBoolean = step(\"never\").when(false);\n\
         export const always1 = step(\"late\");",
        &["--actions-max", "60", "--seed", "6"],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    let names = names(&trace);
    assert_eq!(count(&names, "never"), 0);
    assert!(names[2..].iter().all(|name| name != "early"), "{names:?}");
    assert!(names[3..].iter().all(|name| name != "cell"), "{names:?}");
}

#[test]
fn actions_callbacks_offer_a_generator_an_array_or_nothing() {
    let (outcome, trace) = walk(
        "export const parity = actions(() => (count.current % 2 === 0 ? [step(\"even\")] : step(\"odd\")));",
        &["--actions-max", "6"],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    assert_eq!(
        names(&trace),
        vec!["even", "odd", "even", "odd", "even", "odd"]
    );
}

#[test]
fn parameter_generators_stay_in_their_ranges() {
    let (outcome, trace) = walk(
        "export const params = action({ kind: \"step\", name: \"p\", i: integer(-2, 2), f: float(0.5, 1.5), r: regex(/^[a-c]{2}x$/), e: email(), t: text({ min: 2, max: 4 }), c: chars(\"xyz\", { min: 3, max: 3 }), nested: { list: [from([\"u\", \"v\"])] } });",
        &["--actions-max", "150", "--seed", "7"],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    let mut seen_integers = std::collections::BTreeSet::new();
    for action in trace.actions() {
        let i = action["i"].as_i64().unwrap();
        assert!((-2..=2).contains(&i));
        seen_integers.insert(i);
        let f = action["f"].as_f64().unwrap();
        assert!((0.5..1.5).contains(&f), "{f}");
        let r = action["r"].as_str().unwrap();
        assert!(
            r.len() == 3 && r.ends_with('x') && r[..2].chars().all(|c| ('a'..='c').contains(&c)),
            "{r}"
        );
        let e = action["e"].as_str().unwrap();
        let (local, domain) = e.split_once('@').unwrap();
        assert!(!local.is_empty() && domain.contains('.'), "{e}");
        let t = action["t"].as_str().unwrap();
        assert!((2..=4).contains(&t.chars().count()), "{t:?}");
        let c = action["c"].as_str().unwrap();
        assert!(
            c.len() == 3 && c.chars().all(|ch| "xyz".contains(ch)),
            "{c}"
        );
        let list = action["nested"]["list"][0].as_str().unwrap();
        assert!(list == "u" || list == "v");
    }
    assert_eq!(seen_integers.len(), 5);
}

#[test]
fn no_available_action_is_an_error_naming_the_state() {
    let (outcome, trace) = walk(
        "export const none = actions(() => null);",
        &["--actions-max", "5"],
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("no actions are available in state 0"),
        "{}",
        outcome.err
    );
    assert_eq!(trace.summary.unwrap().outcome, "error");

    let (outcome, _) = walk(
        "export const few = step(\"a\").when(() => count.current < 2);\nexport const empty = from([]);",
        &["--actions-max", "5"],
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("no actions are available in state 2"),
        "{}",
        outcome.err
    );

    let (outcome, _) = walk(
        "export const p = always(() => true);",
        &["--actions-max", "5"],
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("no actions are available in state 0"),
        "{}",
        outcome.err
    );
}

#[test]
fn a_generator_that_yields_a_non_action_is_an_error() {
    let (outcome, _) = walk(
        "export const numbers = integer(1, 3);",
        &["--actions-max", "2"],
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("not an action object"),
        "{}",
        outcome.err
    );
}

#[test]
fn the_same_seed_gives_the_same_actions() {
    let body = "export const pick = oneOf(step(\"a\"), step(\"b\"), step(\"c\"), step(\"d\"));\nexport const typed = action({ kind: \"step\", name: \"t\", value: text({ min: 1, max: 9 }) });";
    let workspace = Workspace::new();
    let spec = workspace.spec(body);
    let (first_outcome, first) =
        workspace.test(&plain(), &spec, &["--actions-max", "40", "--seed", "99"]);
    assert!(
        first_outcome.out.contains("seed 99"),
        "{}",
        first_outcome.out
    );
    let (_, second) = workspace.test(&plain(), &spec, &["--actions-max", "40", "--seed", "99"]);
    let (_, other) = workspace.test(&plain(), &spec, &["--actions-max", "40", "--seed", "100"]);
    assert_eq!(first.actions(), second.actions());
    assert_ne!(first.actions(), other.actions());
    assert_eq!(first.header.seed, 99);
}
