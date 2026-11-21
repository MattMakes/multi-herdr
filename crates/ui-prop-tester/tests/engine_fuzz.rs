mod engine_harness;

use engine_harness::{plain, run, Workspace};
use ui_prop_tester::trace::read_trace_file;

fn fuzz(workspace: &Workspace, body: &str, extra: &[&str]) -> engine_harness::Outcome {
    let spec = workspace.spec(body).to_string_lossy().into_owned();
    let mut args = vec!["terminal", "fuzz", "--specification", spec.as_str()];
    args.extend_from_slice(extra);
    args.push("fake-program");
    run(&plain(), &args)
}

fn run_lines(out: &str) -> Vec<String> {
    let mut lines: Vec<String> = out
        .lines()
        .filter(|line| line.starts_with("run "))
        .map(str::to_string)
        .collect();
    lines.sort();
    lines
}

#[test]
fn a_campaign_writes_numbered_traces_with_derived_seeds() {
    let workspace = Workspace::new();
    let base = workspace.path("fuzz.trace").to_string_lossy().into_owned();
    let outcome = fuzz(
        &workspace,
        "export const go = steps(\"a\", \"b\");",
        &[
            "--runs-max",
            "3",
            "--parallelism",
            "2",
            "--actions-max",
            "4",
            "--seed",
            "8",
            "--output-path",
            &base,
        ],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    assert!(
        outcome
            .out
            .contains("campaign seed 8: 3 runs, 0 with violations, 0 with errors"),
        "{}",
        outcome.out
    );
    let mut seeds = Vec::new();
    for number in 1..=3 {
        let trace = read_trace_file(&workspace.path(&format!("fuzz-{number}.trace"))).unwrap();
        assert_eq!(trace.actions().len(), 4);
        assert_eq!(trace.header.seed, ui_prop_tester::fuzz::run_seed(8, number));
        seeds.push(trace.header.seed);
    }
    seeds.dedup();
    assert_eq!(seeds.len(), 3);
    assert!(!workspace.path("fuzz-4.trace").exists());
}

#[test]
fn the_same_campaign_seed_repeats_the_same_runs() {
    let body = "export const go = steps(\"a\", \"b\", \"c\");";
    let first = fuzz(
        &Workspace::new(),
        body,
        &["--runs-max", "4", "--actions-max", "3", "--seed", "31"],
    );
    let second = fuzz(
        &Workspace::new(),
        body,
        &[
            "--runs-max",
            "4",
            "--actions-max",
            "3",
            "--seed",
            "31",
            "--parallelism",
            "3",
        ],
    );
    assert_eq!(first.code, 0, "{}", first.all());
    assert_eq!(run_lines(&first.out), run_lines(&second.out));
    assert_eq!(run_lines(&first.out).len(), 4);
}

#[test]
fn violations_give_exit_2_and_exit_on_violation_stops_the_campaign() {
    let body =
        "export const go = step(\"a\");\nexport const small = always(() => count.current < 2);";
    let outcome = fuzz(
        &Workspace::new(),
        body,
        &["--runs-max", "3", "--actions-max", "4", "--seed", "1"],
    );
    assert_eq!(outcome.code, 2, "{}", outcome.all());
    assert!(
        outcome.out.contains("3 runs, 3 with violations"),
        "{}",
        outcome.out
    );
    assert!(
        outcome.out.contains("violation of small at state 2"),
        "{}",
        outcome.out
    );
    let kept = outcome
        .out
        .lines()
        .find_map(|line| line.trim().strip_prefix("trace "))
        .unwrap()
        .to_string();
    assert!(read_trace_file(std::path::Path::new(&kept)).is_ok());

    let outcome = fuzz(
        &Workspace::new(),
        body,
        &[
            "--runs-max",
            "50",
            "--actions-max",
            "4",
            "--seed",
            "1",
            "--exit-on-violation",
        ],
    );
    assert_eq!(outcome.code, 2, "{}", outcome.all());
    assert_eq!(run_lines(&outcome.out).len(), 1, "{}", outcome.out);
}

#[test]
fn swarm_gives_each_run_a_stable_subset_of_categories() {
    let workspace = Workspace::new();
    let base = workspace.path("swarm.trace").to_string_lossy().into_owned();
    let body = "export const a = step(\"a\");\nexport const b = step(\"b\");\nexport const c = step(\"c\");\nexport const d = step(\"d\");\nexport const e = step(\"e\");";
    let args = [
        "--runs-max",
        "6",
        "--actions-max",
        "30",
        "--seed",
        "77",
        "--swarm",
        "--output-path",
        base.as_str(),
        "--output-path-overwrite",
    ];
    let first = fuzz(&workspace, body, &args);
    assert_eq!(first.code, 0, "{}", first.all());
    let second = fuzz(&workspace, body, &args);
    assert_eq!(run_lines(&first.out), run_lines(&second.out));
    let mut subsets = std::collections::BTreeSet::new();
    for line in run_lines(&first.out) {
        let number: u64 = line["run ".len()..]
            .split(' ')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let categories: Vec<String> = line
            .split("categories ")
            .nth(1)
            .unwrap()
            .split(')')
            .next()
            .unwrap()
            .split(',')
            .map(str::to_string)
            .collect();
        assert!(!categories.is_empty());
        let trace = read_trace_file(&workspace.path(&format!("swarm-{number}.trace"))).unwrap();
        for action in trace.actions() {
            let category = action["category"].as_str().unwrap().to_string();
            assert!(
                categories.contains(&category),
                "run {number} used {category} outside {categories:?}"
            );
        }
        subsets.insert(categories);
    }
    assert!(subsets.len() > 1, "every run had the same subset");
}

#[test]
fn run_time_limits_end_each_run_cleanly() {
    let workspace = Workspace::new();
    let base = workspace.path("timed.trace").to_string_lossy().into_owned();
    let outcome = fuzz(
        &workspace,
        "export const go = step(\"a\");",
        &[
            "--runs-max",
            "2",
            "--time-limit-run",
            "30ms",
            "--seed",
            "4",
            "--output-path",
            &base,
        ],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    for number in 1..=2 {
        let trace = read_trace_file(&workspace.path(&format!("timed-{number}.trace"))).unwrap();
        assert_eq!(
            trace.summary.unwrap().message.as_deref(),
            Some("the time limit was reached")
        );
    }
}

#[test]
fn a_campaign_time_limit_stops_new_runs() {
    let outcome = fuzz(
        &Workspace::new(),
        "export const go = step(\"a\");",
        &[
            "--time-limit-fuzz",
            "200ms",
            "--time-limit-run",
            "20ms",
            "--seed",
            "4",
        ],
    );
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    assert!(!run_lines(&outcome.out).is_empty());
}

#[test]
fn existing_numbered_traces_are_not_overwritten() {
    let workspace = Workspace::new();
    workspace.write("kept-1.trace", "old");
    let base = workspace.path("kept.trace").to_string_lossy().into_owned();
    let outcome = fuzz(
        &workspace,
        "export const go = step(\"a\");",
        &[
            "--runs-max",
            "2",
            "--actions-max",
            "2",
            "--output-path",
            &base,
        ],
    );
    assert_eq!(outcome.code, 1);
    assert!(
        outcome.err.contains("--output-path-overwrite"),
        "{}",
        outcome.err
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path("kept-1.trace")).unwrap(),
        "old"
    );
}

#[test]
fn a_broken_specification_stops_the_campaign_before_any_run() {
    let workspace = Workspace::new();
    let outcome = fuzz(&workspace, "export const = 1;", &["--runs-max", "2"]);
    assert_eq!(outcome.code, 1);
    assert!(run_lines(&outcome.out).is_empty());
}
