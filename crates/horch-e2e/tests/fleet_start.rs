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
    assert!(err(&o).contains("no-such-skill-217"), "{}", err(&o));
    // The failure comes before herdr creates anything, not after a close.
    let created = h.calls_of("herdr").iter().any(|c| {
        let argv: Vec<String> = serde_json::from_value(c["argv"].clone()).unwrap_or_default();
        argv.get(..2) == Some(&["workspace".to_string(), "create".to_string()][..])
    });
    assert!(!created, "the fleet created a workspace");
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

/// The default task ledger of the sealed home: what a scratch fleet must
/// not create or change.
fn default_ledger(h: &Harness) -> std::path::PathBuf {
    h.home.join(".horch").join("ledger.sqlite3")
}

/// #199: a scratch fleet with its own `HORCH_STATE_DIR` and no
/// `HORCH_LEDGER_DB` says which task ledger it still uses, and does not
/// create that ledger.
#[test]
fn a_scratch_fleet_warns_about_the_shared_ledger_and_does_not_create_it() {
    let mut h = harness("f199-new", "");
    h.unset("HORCH_LEDGER_DB");
    h.unset("HORCH_NO_WATCHDOG").unset("HORCH_NO_WAKE");
    let o = h.run(&["fleet", "opus", "--no-update", "--cwd", &cwd(&h)]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    let warning = format!(
        "the task ledger is still {}; set HORCH_LEDGER_DB to isolate it",
        default_ledger(&h).display()
    );
    assert_eq!(err(&o).matches(&warning).count(), 1, "{}", err(&o));
    assert!(
        !default_ledger(&h).exists(),
        "the fleet created the default ledger"
    );

    // `horch doctor` and `horch spawn` give the same warning, once each.
    let o = h.run(&["doctor"]);
    let doctor = format!("warning: {warning}");
    assert_eq!(err(&o).matches(&doctor).count(), 1, "{}", err(&o));
    assert!(
        !default_ledger(&h).exists(),
        "doctor created the default ledger"
    );
    h.set("HORCH_WORKSPACE_ID", "w1");
    let o = h.run(&[
        "spawn",
        "sonnet",
        "task",
        "--from-pane",
        "w1:p1",
        "--no-tile",
        "--force",
    ]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    let note = format!("horch: NOTE: {warning}");
    assert_eq!(err(&o).matches(&note).count(), 1, "{}", err(&o));
}

/// #199: with an existing default ledger, a scratch fleet leaves its bytes
/// as they were.
#[test]
fn a_scratch_fleet_does_not_change_an_existing_default_ledger() {
    let mut h = harness("f199-old", "");
    h.unset("HORCH_LEDGER_DB");
    h.unset("HORCH_NO_WATCHDOG").unset("HORCH_NO_WAKE");
    // The operator's real ledger, with 1 active run of this project.
    let o = h.run(&["ledger", "run", "new", "real"]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    let before = std::fs::read(default_ledger(&h)).unwrap();
    let o = h.run(&["fleet", "opus", "--no-update", "--cwd", &cwd(&h)]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    assert_eq!(
        std::fs::read(default_ledger(&h)).unwrap(),
        before,
        "the fleet changed the default ledger"
    );
}

/// #199: with `HORCH_LEDGER_DB` set, the orchestrator pane of a real fleet
/// start carries it, and no warning prints.
#[test]
fn a_fleet_with_a_ledger_override_passes_it_to_the_orchestrator_pane() {
    let h = harness("f199-pane", "");
    let ledger = h.root.join("ledger.sqlite3");
    let o = h.run(&["fleet", "opus", "--no-update", "--cwd", &cwd(&h)]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    assert!(!err(&o).contains("the task ledger is still"), "{}", err(&o));
    let runs: Vec<String> = h
        .calls_of("herdr")
        .iter()
        .filter_map(|c| {
            let argv: Vec<String> = serde_json::from_value(c["argv"].clone()).ok()?;
            let line = argv.join(" ");
            (argv.first().map(String::as_str) == Some("pane")
                && argv.get(1).map(String::as_str) == Some("run")
                && line.contains("pane-launch"))
            .then_some(line)
        })
        .collect();
    assert_eq!(runs.len(), 1, "{runs:?}");
    let set = format!("HORCH_LEDGER_DB={}", ledger.display());
    assert!(runs[0].contains(&set), "{}", runs[0]);
    assert!(!default_ledger(&h).exists());
}
