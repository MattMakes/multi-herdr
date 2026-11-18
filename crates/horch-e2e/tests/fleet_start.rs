//! End-to-end: what `horch fleet` and `horch orchestration` leave behind when
//! they fail to start (#217), the task ledger of a scratch fleet (#199), and
//! a fleet with no orchestrator pane (FM-16).

use horch_core::execution::records::SessionLedger;
use horch_e2e::harness::{fixtures, Harness};
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

/// Whether the fake herdr was asked to create a workspace.
fn created_a_workspace(h: &Harness) -> bool {
    h.calls_of("herdr").iter().any(|c| {
        let argv: Vec<String> = serde_json::from_value(c["argv"].clone()).unwrap_or_default();
        argv.get(..2) == Some(&["workspace".to_string(), "create".to_string()][..])
    })
}

/// FM-16 (ticket 45 of fleetd-p6): `horch fleet` starts its workspace with
/// no orchestrator pane and no orchestrator record, and names `horch idea
/// add`. A failed pane run of the telemetry space is only a warning. With
/// `HORCH_LEDGER_DB` set, no ledger warning prints (#199).
#[test]
fn a_fleet_starts_no_orchestrator_pane() {
    let h = harness("f45-none", "fail_run");
    let o = h.run(&["fleet", "opus", "--no-update", "--cwd", &cwd(&h)]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    assert!(
        out(&o).contains("Add an idea: horch idea add \"...\""),
        "{}",
        out(&o)
    );
    assert!(!err(&o).contains("the task ledger is still"), "{}", err(&o));
    assert!(created_a_workspace(&h), "the fleet creates its workspace");
    let launches: Vec<String> = h
        .calls_of("herdr")
        .iter()
        .filter_map(|c| {
            let argv: Vec<String> = serde_json::from_value(c["argv"].clone()).ok()?;
            let line = argv.join(" ");
            line.contains("pane-launch").then_some(line)
        })
        .collect();
    assert_eq!(launches, Vec::<String>::new(), "no orchestrator pane");
    let records = SessionLedger::for_project(&h.state, &cwd(&h))
        .read()
        .unwrap_or_default();
    assert!(
        !records.iter().any(|r| r.is_orchestrator()),
        "{records:#?}"
    );
}

#[test]
fn an_orchestration_whose_pane_run_fails_closes_its_workspace() {
    let h = harness("f217-orch", "fail_run");
    let o = h.run(&["orchestration", "--no-update", "--cwd", &cwd(&h)]);
    assert!(!o.status.success(), "{}{}", out(&o), err(&o));
    assert!(err(&o).contains("pane run refused"), "{}", err(&o));
    let calls: Vec<Vec<String>> = h
        .calls_of("herdr")
        .iter()
        .map(|c| serde_json::from_value(c["argv"].clone()).unwrap_or_default())
        .collect();
    let count = |verb: &str| {
        calls
            .iter()
            .filter(|a| {
                a.first().map(String::as_str) == Some("workspace")
                    && a.get(1).map(String::as_str) == Some(verb)
            })
            .count()
    };
    assert_eq!((count("create"), count("close")), (1, 1), "{calls:?}");
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
    // The operator's real ledger, with 1 active run of this project. fleetd
    // makes a ledger, so the test writes it with the library.
    std::fs::create_dir_all(default_ledger(&h).parent().unwrap()).unwrap();
    let store = horch_core::taskledger::Store::open(&default_ledger(&h)).unwrap();
    let new = horch_core::taskledger::runs::NewRun {
        slug: "real".into(),
        ..Default::default()
    };
    horch_core::taskledger::runs::create(store.conn(), &h.project, &new).unwrap();
    drop(store);
    let before = std::fs::read(default_ledger(&h)).unwrap();
    let o = h.run(&["fleet", "opus", "--no-update", "--cwd", &cwd(&h)]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    assert_eq!(
        std::fs::read(default_ledger(&h)).unwrap(),
        before,
        "the fleet changed the default ledger"
    );
}

