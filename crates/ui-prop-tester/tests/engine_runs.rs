mod engine_harness;

use engine_harness::{fake, names, plain, run, Workspace, MACHINE};
use serde_json::json;
use ui_prop_tester::trace::read_trace_file;

#[test]
fn an_action_limit_ends_a_clean_run_with_exit_0_and_a_full_trace() {
    let workspace = Workspace::new();
    let spec =
        workspace.spec("export const go = step(\"a\");\nexport const fine = always(() => true);");
    let drivers = plain();
    let (outcome, trace) = workspace.test(&drivers, &spec, &["--actions-max", "3", "--seed", "11"]);
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    assert!(outcome.out.contains("seed 11"));
    assert!(
        outcome
            .out
            .contains("4 states, 3 actions; the action limit was reached"),
        "{}",
        outcome.out
    );
    assert_eq!(trace.header.seed, 11);
    assert_eq!(trace.header.target["program"], "fake-program");
    let recorded = trace.header.specification.as_ref().unwrap();
    assert!(recorded.source.contains("export const go"));
    assert_eq!(trace.states.len(), 4);
    assert_eq!(trace.states[0].action, None);
    assert_eq!(
        trace.states[1].action,
        Some(json!({"kind": "step", "name": "a", "category": "go"}))
    );
    assert_eq!(trace.states[2].cells["count"], json!(2));
    assert_eq!(trace.states[3].timestamp_ms, 300);
    let summary = trace.summary.unwrap();
    assert_eq!(summary.outcome, "passed");
    assert_eq!(summary.states, 4);
    assert_eq!(drivers.opened(), 1);
    assert_eq!(drivers.shutdowns(), 1);
}

#[test]
fn a_time_limit_ends_the_run_cleanly() {
    let workspace = Workspace::new();
    let spec = workspace.spec("export const go = step(\"a\");");
    let (outcome, trace) = workspace.test(&plain(), &spec, &["--time-limit", "50ms"]);
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    assert!(
        outcome.out.contains("the time limit was reached"),
        "{}",
        outcome.out
    );
    assert!(!trace.states.is_empty());
}

#[test]
fn reproduce_replays_the_recorded_actions_and_keeps_the_verdict() {
    let workspace = Workspace::new();
    let drivers = fake(MACHINE);
    let spec = workspace.spec(
        "export const moves = steps(\"x\", \"y\", \"reset\", \"a\");\nexport const intact = always(() => screen.current !== \"broken\");",
    );
    let (outcome, original) =
        workspace.test(&drivers, &spec, &["--actions-max", "30", "--seed", "21"]);
    assert_eq!(outcome.code, 2, "{}", outcome.all());
    let recorded = workspace.path("recorded.trace");
    std::fs::rename(workspace.path("run.trace"), &recorded).unwrap();
    let recorded_text = recorded.to_string_lossy().into_owned();
    let (outcome, replayed) = workspace.test(&drivers, &spec, &["--reproduce", &recorded_text]);
    assert_eq!(outcome.code, 2, "{}", outcome.all());
    assert_eq!(names(&replayed), names(&original));
    assert_eq!(replayed.header.seed, 21);
    assert!(
        outcome.out.contains("every recorded action was replayed"),
        "{}",
        outcome.out
    );

    let output = workspace.path("again.trace");
    let output_text = output.to_string_lossy().into_owned();
    let replay_without_spec = run(
        &drivers,
        &[
            "terminal",
            "test",
            "--reproduce",
            &recorded_text,
            "--output-path",
            &output_text,
            "fake-program",
        ],
    );
    assert_eq!(replay_without_spec.code, 2, "{}", replay_without_spec.all());
    assert_eq!(names(&read_trace_file(&output).unwrap()), names(&original));
}

#[test]
fn reproducing_a_passing_trace_exits_0() {
    let workspace = Workspace::new();
    let spec = workspace.spec("export const go = steps(\"a\", \"b\");");
    let (outcome, _) = workspace.test(&plain(), &spec, &["--actions-max", "5"]);
    assert_eq!(outcome.code, 0);
    let recorded = workspace.path("recorded.trace");
    std::fs::rename(workspace.path("run.trace"), &recorded).unwrap();
    let recorded_text = recorded.to_string_lossy().into_owned();
    let (outcome, trace) = workspace.test(&plain(), &spec, &["--reproduce", &recorded_text]);
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    assert_eq!(trace.actions().len(), 5);
}

#[test]
fn a_recorded_action_that_is_no_longer_offered_is_a_divergence() {
    let workspace = Workspace::new();
    let spec = workspace.spec("export const go = steps(\"a\", \"b\");");
    let (outcome, _) = workspace.test(&plain(), &spec, &["--actions-max", "8", "--seed", "2"]);
    assert_eq!(outcome.code, 0);
    let recorded = workspace.path("recorded.trace");
    std::fs::rename(workspace.path("run.trace"), &recorded).unwrap();
    let recorded_text = recorded.to_string_lossy().into_owned();
    let changed = workspace.spec("export const go = step(\"c\");");
    let (outcome, trace) = workspace.test(&plain(), &changed, &["--reproduce", &recorded_text]);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("the replay diverged in state 0"),
        "{}",
        outcome.err
    );
    assert_eq!(trace.summary.unwrap().outcome, "error");
}

#[test]
fn reproducing_into_the_input_file_is_refused() {
    let workspace = Workspace::new();
    let spec = workspace.spec("export const go = step(\"a\");");
    let (_, _) = workspace.test(&plain(), &spec, &["--actions-max", "2"]);
    let trace = workspace.path("run.trace").to_string_lossy().into_owned();
    let outcome = run(
        &plain(),
        &[
            "terminal",
            "test",
            "--reproduce",
            &trace,
            "--output-path",
            &trace,
            "--output-path-overwrite",
            "fake-program",
        ],
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("is the same file as the input trace"),
        "{}",
        outcome.err
    );
}

#[test]
fn the_output_path_is_protected() {
    let workspace = Workspace::new();
    let spec = workspace.spec("export const go = step(\"a\");");
    let spec_text = spec.to_string_lossy().into_owned();
    let existing = workspace.write("existing.trace", "keep me");
    let existing_text = existing.to_string_lossy().into_owned();
    let drivers = plain();
    let refused = run(
        &drivers,
        &[
            "terminal",
            "test",
            "--specification",
            &spec_text,
            "--actions-max",
            "1",
            "--output-path",
            &existing_text,
            "fake-program",
        ],
    );
    assert_eq!(refused.code, 1);
    assert!(
        refused.err.contains("--output-path-overwrite"),
        "{}",
        refused.err
    );
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "keep me");
    assert_eq!(drivers.opened(), 0);
    let allowed = run(
        &drivers,
        &[
            "terminal",
            "test",
            "--specification",
            &spec_text,
            "--actions-max",
            "1",
            "--output-path",
            &existing_text,
            "--output-path-overwrite",
            "fake-program",
        ],
    );
    assert_eq!(allowed.code, 0, "{}", allowed.all());
    assert!(read_trace_file(&existing).is_ok());
    let missing = workspace
        .path("absent/run.trace")
        .to_string_lossy()
        .into_owned();
    let refused = run(
        &drivers,
        &[
            "terminal",
            "test",
            "--specification",
            &spec_text,
            "--output-path",
            &missing,
            "fake-program",
        ],
    );
    assert_eq!(refused.code, 1);
    assert!(refused.err.contains("does not exist"), "{}", refused.err);
}

#[test]
fn without_an_output_path_the_trace_goes_to_a_temporary_file() {
    let workspace = Workspace::new();
    let spec = workspace.spec("export const go = step(\"a\");");
    let spec_text = spec.to_string_lossy().into_owned();
    let outcome = run(
        &plain(),
        &[
            "terminal",
            "test",
            "--specification",
            &spec_text,
            "--actions-max",
            "1",
            "--seed",
            "5",
            "fake-program",
        ],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    let path = outcome
        .out
        .lines()
        .find_map(|line| line.strip_prefix("ui-prop-tester: trace "))
        .unwrap()
        .to_string();
    let trace = read_trace_file(std::path::Path::new(&path)).unwrap();
    assert_eq!(trace.header.seed, 5);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn driver_failures_exit_1_and_always_shut_the_driver_down() {
    let workspace = Workspace::new();
    let spec = workspace.spec("export const go = step(\"a\");");
    let drivers = fake(r#"{"fail_after_actions": 2}"#);
    let (outcome, trace) = workspace.test(&drivers, &spec, &["--actions-max", "9"]);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("ui-prop-tester: error: driver failed"),
        "{}",
        outcome.err
    );
    assert_eq!(trace.states.len(), 3);
    assert_eq!(trace.summary.unwrap().outcome, "error");
    assert_eq!(drivers.shutdowns(), 1);

    let drivers = fake(r#"{"stall_after_actions": 1}"#);
    let (outcome, _) = workspace.test(&drivers, &spec, &["--actions-max", "9"]);
    assert_eq!(outcome.code, 1);
    assert!(outcome.err.contains("target stalled"), "{}", outcome.err);

    let drivers = fake(r#"{"fail_start": "no such program"}"#);
    let (outcome, trace) = workspace.test(&drivers, &spec, &[]);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("setup failed: no such program"),
        "{}",
        outcome.err
    );
    assert!(trace.states.is_empty());
    assert_eq!(drivers.shutdowns(), 1);
}

#[test]
fn a_broken_specification_exits_1_before_the_driver_opens() {
    let workspace = Workspace::new();
    let spec = workspace.write("spec.js", "export const = ;\n");
    let drivers = plain();
    let (outcome, _) = workspace.test(&drivers, &spec, &[]);
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("ui-prop-tester: error:"),
        "{}",
        outcome.err
    );
    assert_eq!(drivers.opened(), 0);
    let missing = workspace.path("missing.js").to_string_lossy().into_owned();
    let outcome = run(
        &drivers,
        &[
            "terminal",
            "test",
            "--specification",
            &missing,
            "fake-program",
        ],
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("cannot read the specification"),
        "{}",
        outcome.err
    );
}

#[test]
fn bad_arguments_exit_1_and_help_exits_0() {
    let outcome = run(&plain(), &["terminal", "test", "--no-such-flag", "x"]);
    assert_eq!(outcome.code, 1);
    let outcome = run(&plain(), &["--help"]);
    assert_eq!(outcome.code, 0);
    assert!(outcome.out.contains("terminal"));
}

#[test]
fn the_driver_defaults_module_is_the_specification_when_none_is_given() {
    let drivers = plain();
    let outcome = run(
        &drivers,
        &[
            "terminal",
            "test",
            "--actions-max",
            "5",
            "--seed",
            "3",
            "--output-path",
            &Workspace::new().path("x.trace").to_string_lossy(),
            "fake-program",
        ],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    let applied = drivers.log.applied.lock().unwrap();
    assert_eq!(applied[0].len(), 5);
    assert!(applied[0]
        .iter()
        .all(|action| action["kind"] == "write" && action["category"] == "typing"));
}

#[test]
fn custom_browser_actions_reach_the_driver_and_carry_cells() {
    let workspace = Workspace::new();
    let spec = workspace.write(
        "spec.js",
        "import { extract } from \"ui-prop-tester\";\nimport { custom, defineAction } from \"ui-prop-tester/browser\";\nexport const steps = extract((s) => s.steps);\ndefineAction(\"fill\", async ({ document, args }) => { document.title = args.title; });\nexport const filling = custom(\"fill\", { title: \"x\" });\n",
    );
    let output = workspace.path("b.trace");
    let drivers = plain();
    let outcome = run(
        &drivers,
        &[
            "browser",
            "test",
            "--specification",
            &spec.to_string_lossy(),
            "--actions-max",
            "2",
            "--output-path",
            &output.to_string_lossy(),
            "http://localhost:1/",
        ],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    let prepared = drivers.log.prepared.lock().unwrap();
    assert_eq!(prepared[0].len(), 1);
    assert_eq!(prepared[0][0].name, "fill");
    assert!(prepared[0][0].source.contains("document.title"));
    let trace = read_trace_file(&output).unwrap();
    assert_eq!(trace.header.target["origin"], "http://localhost:1/");
    let actions = trace.actions();
    assert_eq!(actions[0]["kind"], "custom");
    assert_eq!(actions[0]["args"], json!({"title": "x"}));
    assert_eq!(actions[1]["cells"]["steps"], json!(1));
}

#[test]
fn a_typescript_specification_runs_end_to_end() {
    let workspace = Workspace::new();
    let spec = workspace.write(
        "spec.ts",
        "import { always, extract } from \"ui-prop-tester\";\nimport { step } from \"ui-prop-tester/fake\";\ninterface Shape { steps: number }\ntype Count = number;\nexport const count = extract((s: Shape): Count => s.steps as Count);\nexport const go = step(\"a\");\nexport const small = always((): boolean => count.current! < 2);\n",
    );
    let (outcome, trace) = workspace.test(&plain(), &spec, &["--actions-max", "4"]);
    assert_eq!(outcome.code, 2, "{}", outcome.all());
    assert_eq!(trace.summary.unwrap().violations[0].state_index, 2);
}
