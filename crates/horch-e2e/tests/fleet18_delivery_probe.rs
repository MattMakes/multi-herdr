//! #61: the real CLI verifies delivery rather than assuming a successful write.
#![cfg(unix)]

use horch_core::messaging::mail::Mail;
use horch_e2e::harness::{fixtures, Harness};

fn world(name: &str, scenario: &str) -> Harness {
    let mut h = Harness::new(name);
    h.set("HORCH_FAKE_SCENARIO", scenario);
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_FAULT", "delivery-probe-ms:4000");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h
}

/// The budget of a probe that must confirm. Each fake herdr call loads and
/// saves a state file, and on a loaded machine one call took 2.9 s: past
/// the 2 s call bound and the 4 s budget. A probe that confirms ends at
/// once, so the bounds cost nothing when delivery works.
const CONFIRM_FAULTS: &str = "delivery-probe-ms:30000,delivery-probe-call-ms:30000";

#[test]
fn g61_doctor_confirms_executed_echo_and_cleans_up() {
    let mut h = world("g61-doctor", "exec,shell");
    h.set("HORCH_FAULT", CONFIRM_FAULTS);
    let out = h.run(&["doctor"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("delivery: confirmed"), "{text}");
    let calls = h.calls_of("herdr");
    assert!(calls
        .iter()
        .any(|c| c["argv"][0] == "workspace" && c["argv"][1] == "close"));
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(h.log.with_extension("log.state.json")).unwrap())
            .unwrap();
    assert!(
        state["workspaces"].as_array().unwrap().is_empty(),
        "{state}"
    );
}

/// `delivery-probe-call-ms` bounds each herdr call of the probe. The fake
/// `pane split` never answers, so the probe fails at that bound and never
/// at the 2000 ms default. A loaded machine can make another call (the
/// create, the cleanup) miss the bound first; the warning names it then.
#[test]
fn g61_doctor_call_bound_follows_the_fault() {
    let mut h = world("g61-call-ms", "exec,shell,hang_split");
    h.set(
        "HORCH_FAULT",
        "delivery-probe-ms:30000,delivery-probe-call-ms:300",
    );
    let out = h.run(&["doctor"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{}\n{stderr}", out.status);
    assert!(
        stderr.contains("warning: delivery test failed: ")
            && stderr.contains("herdr did not answer within 300 ms"),
        "{}{stderr}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// The typing fails, and the probe still closes its workspace. The cleanup
/// gets the long call bound of [`CONFIRM_FAULTS`]: on a loaded machine one
/// fake herdr call takes seconds, and a close cut off at the 2 s default is
/// never recorded. The bound itself is g61_doctor_call_bound_follows_the_fault.
#[test]
fn g61_doctor_typing_failure_warns_without_failing_or_leaking_workspace() {
    let mut h = world("g61-no-type", "exec,shell,fail_typing");
    h.set("HORCH_FAULT", CONFIRM_FAULTS);
    let out = h.run(&["doctor"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("warning: delivery test failed"), "{stderr}");
    assert!(
        h.calls_of("herdr")
            .iter()
            .any(|c| c["argv"][0] == "workspace" && c["argv"][1] == "close"),
        "{stderr}"
    );
}

#[test]
fn g61_fleet_confirms_only_after_orchestrator_runs_inbox() {
    let mut h = world("g61-fleet-ack", "exec,fleet_probe_ack");
    h.set("HORCH_FAULT", CONFIRM_FAULTS);
    let out = h.run(&["fleet", "--no-update"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("fleet delivery: confirmed"), "{text}");
    assert!(Mail::under(&h.state, "w1").pending().unwrap().is_empty());
}

#[test]
fn g61_fleet_without_ack_warns_and_keeps_probe_pending() {
    let mut h = world("g61-fleet-no", "");
    h.set("HORCH_FAULT", "delivery-probe-ms:100");
    let out = h.run(&["fleet", "--no-update"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("warning: fleet delivery"), "{text}");
    assert!(
        text.contains("not acknowledged") || text.contains("no relay") || text.contains("not idle"),
        "{text}"
    );
    let mail = Mail::under(&h.state, "w1").pending().unwrap();
    assert!(mail
        .iter()
        .any(|m| m.text.contains("run horch inbox to confirm delivery")));
}

#[test]
fn g61_fleet_probe_never_falls_back_to_separate_text_and_enter() {
    let h = world("g61-atomic", "fleet_probe_ack,fail_typing");
    let out = h.run(&["fleet", "--no-update"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(text.contains("warning: fleet delivery"), "{text}");
    let calls = h.calls_of("herdr");
    assert!(calls
        .iter()
        .any(|c| c["argv"][0] == "agent" && c["argv"][1] == "prompt"));
    let non_atomic = calls
        .iter()
        .filter(|c| {
            c["argv"][0] == "pane" && (c["argv"][1] == "send-text" || c["argv"][1] == "send-keys")
        })
        .count();
    assert_eq!(
        non_atomic, 0,
        "a failed probe must leave the live TUI input untouched"
    );
    assert!(Mail::under(&h.state, "w1")
        .pending()
        .unwrap()
        .iter()
        .any(|m| m.text.contains("confirm delivery")));
}
