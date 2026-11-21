mod engine_harness;

use engine_harness::{fake, Workspace};

#[test]
fn an_interrupt_stops_cleanly_writes_the_trace_and_shuts_down() {
    let workspace = Workspace::new();
    let drivers = fake(r#"{"interrupt_after_actions": 3}"#);
    let spec = workspace.spec("export const go = step(\"a\");\nexport const reach = eventually(() => count.current === 99);");
    let (outcome, trace) = workspace.test(&drivers, &spec, &["--actions-max", "50"]);
    assert_eq!(outcome.code, 2, "{}", outcome.all());
    assert!(
        outcome.out.contains("4 states, 3 actions; interrupted"),
        "{}",
        outcome.out
    );
    assert_eq!(trace.states.len(), 4);
    assert_eq!(
        trace.summary.unwrap().message.as_deref(),
        Some("interrupted")
    );
    assert_eq!(drivers.shutdowns(), 1);

    let spec = workspace.spec("export const go = step(\"a\");");
    let drivers = fake(r#"{"interrupt_after_actions": 2}"#);
    let (outcome, trace) = workspace.test(&drivers, &spec, &["--actions-max", "50"]);
    assert_eq!(outcome.code, 0, "{}", outcome.all());
    assert_eq!(trace.states.len(), 3);
}
