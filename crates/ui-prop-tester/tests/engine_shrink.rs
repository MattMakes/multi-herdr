mod engine_harness;

use std::path::PathBuf;

use engine_harness::{fake, names, run, Workspace, MACHINE};
use ui_prop_tester::fake::FakeDrivers;
use ui_prop_tester::trace::read_trace_file;

const MOVES: &str = "export const moves = steps(\"x\", \"y\", \"a\", \"b\", \"reset\");\n";

fn failing_trace(
    workspace: &Workspace,
    drivers: &FakeDrivers,
    body: &str,
    extra: &[&str],
) -> PathBuf {
    let spec = workspace.spec(&format!("{MOVES}{body}"));
    for seed in 1..200u64 {
        let seed_text = seed.to_string();
        let mut args = vec!["--actions-max", "60", "--seed", seed_text.as_str()];
        args.extend_from_slice(extra);
        let (outcome, trace) = workspace.test(drivers, &spec, &args);
        if outcome.code == 2 && trace.actions().len() > 4 {
            let path = workspace.path("failing.trace");
            std::fs::rename(workspace.path("run.trace"), &path).unwrap();
            return path;
        }
    }
    panic!("no seed produced a failing trace");
}

fn shrink(
    drivers: &FakeDrivers,
    input: &PathBuf,
    output: &PathBuf,
    extra: &[&str],
) -> engine_harness::Outcome {
    let input = input.to_string_lossy().into_owned();
    let output = output.to_string_lossy().into_owned();
    let mut args = vec![
        "terminal",
        "shrink",
        "--trace",
        input.as_str(),
        "--output-path",
        output.as_str(),
    ];
    args.extend_from_slice(extra);
    run(drivers, &args)
}

const INTACT: &str = "export const intact = always(() => screen.current !== \"broken\");";

#[test]
fn shrinking_finds_the_two_actions_that_break_the_machine() {
    let workspace = Workspace::new();
    let drivers = fake(MACHINE);
    let input = failing_trace(&workspace, &drivers, INTACT, &["--exit-on-violation"]);
    let original = read_trace_file(&input).unwrap();
    let output = workspace.path("small.trace");
    let outcome = shrink(&drivers, &input, &output, &[]);
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    let small = read_trace_file(&output).unwrap();
    assert_eq!(names(&small), vec!["x", "y"]);
    assert_eq!(small.header.seed, original.header.seed);
    assert_eq!(small.header.target, original.header.target);
    let summary = small.summary.unwrap();
    assert_eq!(summary.violations[0].property, "intact");
    assert!(
        outcome.out.contains(&format!(
            "shrunk intact from {} to 2 actions",
            original.actions().len()
        )),
        "{}",
        outcome.out
    );
}

#[test]
fn parallel_candidates_reach_the_same_minimum() {
    let workspace = Workspace::new();
    let drivers = fake(MACHINE);
    let input = failing_trace(&workspace, &drivers, INTACT, &["--exit-on-violation"]);
    let output = workspace.path("small.trace");
    let outcome = shrink(
        &drivers,
        &input,
        &output,
        &["--parallelism", "3", "--confirmation-runs-max", "2"],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    assert_eq!(names(&read_trace_file(&output).unwrap()), vec!["x", "y"]);
}

#[test]
fn several_violated_properties_need_a_property_choice() {
    let workspace = Workspace::new();
    let drivers = fake(MACHINE);
    let input = failing_trace(
        &workspace,
        &drivers,
        &format!("{INTACT}\nexport const calm = always(() => screen.current !== \"armed\");"),
        &[],
    );
    let violated = read_trace_file(&input).unwrap().violated_properties();
    assert_eq!(violated.len(), 2, "{violated:?}");
    let output = workspace.path("small.trace");
    let outcome = shrink(&drivers, &input, &output, &[]);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("choose one with --property"),
        "{}",
        outcome.err
    );
    assert!(!output.exists());
    let outcome = shrink(&drivers, &input, &output, &["--property", "calm"]);
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    assert_eq!(names(&read_trace_file(&output).unwrap()), vec!["x"]);
}

#[test]
fn calibration_must_reproduce_every_time() {
    let workspace = Workspace::new();
    let flaky = "const run = extract((s) => s.run);\nexport const intact = always(() => !(screen.current === \"broken\" && run.current % 2 === 1));";
    let input = failing_trace(&workspace, &fake(MACHINE), flaky, &["--exit-on-violation"]);
    let output = workspace.path("small.trace");
    let outcome = shrink(
        &fake(MACHINE),
        &input,
        &output,
        &["--calibration-runs", "3"],
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome
            .err
            .contains("calibration run 2 of 3 did not reproduce intact"),
        "{}",
        outcome.err
    );
    assert!(!output.exists());
}

#[test]
fn a_trace_without_violations_cannot_be_shrunk() {
    let workspace = Workspace::new();
    let spec = workspace.spec("export const go = step(\"a\");");
    let drivers = fake(MACHINE);
    let (outcome, _) = workspace.test(&drivers, &spec, &["--actions-max", "3"]);
    assert_eq!(outcome.code, 0);
    let input = workspace.path("run.trace");
    let outcome = shrink(&drivers, &input, &workspace.path("small.trace"), &[]);
    assert_eq!(outcome.code, 1);
    assert!(outcome.err.contains("nothing to shrink"), "{}", outcome.err);
}

#[test]
fn the_output_must_differ_from_the_input_by_path_symlink_and_hard_link() {
    let workspace = Workspace::new();
    let drivers = fake(MACHINE);
    let input = failing_trace(&workspace, &drivers, INTACT, &["--exit-on-violation"]);
    let before = std::fs::read(&input).unwrap();
    let outcome = shrink(&drivers, &input, &input, &["--output-path-overwrite"]);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("is the same file as the input trace"),
        "{}",
        outcome.err
    );
    #[cfg(unix)]
    {
        let symlink = workspace.path("link.trace");
        std::os::unix::fs::symlink(&input, &symlink).unwrap();
        let outcome = shrink(&drivers, &input, &symlink, &["--output-path-overwrite"]);
        assert_eq!(outcome.code, 1);
        assert!(outcome.err.contains("is the same file"), "{}", outcome.err);
        let hard = workspace.path("hard.trace");
        std::fs::hard_link(&input, &hard).unwrap();
        let outcome = shrink(&drivers, &input, &hard, &["--output-path-overwrite"]);
        assert_eq!(outcome.code, 1);
        assert!(outcome.err.contains("is the same file"), "{}", outcome.err);
    }
    assert_eq!(std::fs::read(&input).unwrap(), before);
    let existing = workspace.write("existing.trace", "x");
    let outcome = shrink(&drivers, &input, &existing, &[]);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("--output-path-overwrite"),
        "{}",
        outcome.err
    );
}

#[test]
fn a_trace_from_the_other_driver_is_refused() {
    let workspace = Workspace::new();
    let drivers = fake(MACHINE);
    let input = failing_trace(&workspace, &drivers, INTACT, &["--exit-on-violation"]);
    let input_text = input.to_string_lossy().into_owned();
    let output = workspace.path("small.trace").to_string_lossy().into_owned();
    let outcome = run(
        &drivers,
        &[
            "browser",
            "shrink",
            "--trace",
            &input_text,
            "--output-path",
            &output,
        ],
    );
    assert_eq!(outcome.code, 1);
    assert!(outcome.err.contains("terminal shrink"), "{}", outcome.err);
}

#[test]
fn a_zero_time_limit_keeps_the_calibrated_trace() {
    let workspace = Workspace::new();
    let drivers = fake(MACHINE);
    let input = failing_trace(&workspace, &drivers, INTACT, &["--exit-on-violation"]);
    let original = read_trace_file(&input).unwrap();
    let output = workspace.path("small.trace");
    let outcome = shrink(
        &drivers,
        &input,
        &output,
        &["--time-limit", "0ms", "--calibration-runs", "1"],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    assert_eq!(names(&read_trace_file(&output).unwrap()), names(&original));
}

#[test]
fn a_changed_specification_can_be_given_for_shrinking() {
    let workspace = Workspace::new();
    let drivers = fake(MACHINE);
    let input = failing_trace(&workspace, &drivers, INTACT, &["--exit-on-violation"]);
    let other = workspace.write(
        "other.js",
        &format!(
            "import {{ always, extract }} from \"ui-prop-tester\";\nimport {{ steps }} from \"ui-prop-tester/fake\";\nconst screen = extract((s) => s.screen);\n{MOVES}export const intact = always(() => screen.current !== \"armed\");\n"
        ),
    );
    let output = workspace.path("small.trace");
    let other_text = other.to_string_lossy().into_owned();
    let outcome = shrink(&drivers, &input, &output, &["--specification", &other_text]);
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    assert_eq!(names(&read_trace_file(&output).unwrap()), vec!["x"]);
}
