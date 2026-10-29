//! End-to-end: what `horch fleet` and `horch orchestration` leave behind when
//! they fail to start (#217), and what the orchestrator pane line carries
//! (#199).

use horch_core::execution::records::Ledger;
use horch_e2e::harness::{copy_tree, fixtures, repo_root, Harness};
use serde_json::Value;

fn err(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn out(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn harness(name: &str, scenario: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_FAULT", "delivery-probe-ms:100");
    h.set("HORCH_NO_WAKE", "1").set("HORCH_NO_WATCHDOG", "1");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_FAKE_SCENARIO", scenario);
    h
}

/// The ids of the workspaces fake herdr lists now.
fn workspaces(h: &Harness) -> Vec<String> {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let o = cmd.args(["workspace", "list"]).output().unwrap();
    assert!(o.status.success(), "{}", err(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    v["result"]["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["workspace_id"].as_str().unwrap().to_string())
        .collect()
}

fn cwd(h: &Harness) -> String {
    h.project.to_string_lossy().into_owned()
}

#[test]
fn a_fleet_whose_orchestrator_skill_is_missing_creates_no_workspace() {
    let mut h = harness("f217-skill", "");
    // The repo roster, with an orchestrator that names a skill no store has.
    let teammates = h.root.join("teammates");
    copy_tree(&repo_root().join("teammates"), &teammates);
    let file = teammates.join("orchestrator.md");
    let text = std::fs::read_to_string(&file).unwrap();
    let broken = text.replace(
        "skills: [orchestrate, skill-creator]",
        "skills: [orchestrate, skill-creator, no-such-skill-217]",
    );
    assert_ne!(text, broken, "the orchestrator's skills line changed");
    std::fs::write(&file, broken).unwrap();
    h.set("HORCH_TEAMMATES_DIR", teammates.to_string_lossy());
    let o = h.run(&["fleet", "opus", "--no-update", "--cwd", &cwd(&h)]);
    assert!(!o.status.success(), "{}{}", out(&o), err(&o));
    assert_eq!(workspaces(&h), Vec::<String>::new(), "{}", out(&o));
}

#[test]
fn a_fleet_whose_pane_run_fails_closes_its_workspace_and_ends_its_record() {
    let h = harness("f217-run", "fail_run");
    let o = h.run(&["fleet", "opus", "--no-update", "--cwd", &cwd(&h)]);
    assert!(!o.status.success(), "{}{}", out(&o), err(&o));
    assert!(
        err(&o).contains("pane run refused"),
        "the pane run error is the one reported: {}",
        err(&o)
    );
    assert_eq!(workspaces(&h), Vec::<String>::new(), "{}", out(&o));
    let records = Ledger::for_project(&h.state, &cwd(&h)).read().unwrap();
    let orchestrators: Vec<_> = records.iter().filter(|r| r.is_orchestrator()).collect();
    assert_eq!(orchestrators.len(), 1, "{records:#?}");
    let status = orchestrators[0].execution_status();
    assert!(!status.is_live(), "{status:?}");
    assert!(
        matches!(
            status,
            horch_core::execution::ExecutionStatus::LaunchFailed { .. }
        ),
        "{status:?}"
    );
}

#[test]
fn an_orchestration_whose_pane_run_fails_closes_its_workspace() {
    let h = harness("f217-orch", "fail_run");
    let o = h.run(&["orchestration", "--no-update", "--cwd", &cwd(&h)]);
    assert!(!o.status.success(), "{}{}", out(&o), err(&o));
    assert_eq!(workspaces(&h), Vec::<String>::new(), "{}", out(&o));
}
