//! End-to-end: `horch fleet` and `horch spawn` on the `hc` (`herdr-core` +
//! `herdr-app`) pane host backend.

use horch_core::execution::records::SessionLedger;
use horch_core::messaging::mailbox::Mailbox;
use horch_core::runtime::Backend;
use horch_e2e::harness::{fixtures, Harness};
use serde_json::Value;

fn err(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn out(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn harness(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_FAULT", "delivery-probe-ms:100");
    h.set("HORCH_NO_WAKE", "1")
        .set("HORCH_NO_WATCHDOG", "1")
        .set("HORCH_SPAWN_WAIT_MS", "0");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h
}

fn cwd(h: &Harness) -> String {
    h.project.to_string_lossy().into_owned()
}

/// Find the fleet space (ignoring the `horch telemetry` space if started).
fn fleet_space(h: &Harness) -> Value {
    let state = h.hc_state();
    let spaces: Vec<Value> = state["spaces"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s["name"].as_str() != Some("horch telemetry"))
        .collect();
    assert_eq!(spaces.len(), 1, "expected 1 fleet space, got: {spaces:#?}");
    spaces.into_iter().next().unwrap()
}

#[test]
fn hc_fleet_start_makes_a_space_and_records_the_backend() {
    let h = harness("hc-fleet-start");
    let o = h.run(&[
        "--backend",
        "hc",
        "fleet",
        "opus",
        "--no-update",
        "--cwd",
        &cwd(&h),
    ]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));
    let herdr_mutations: Vec<Value> = h
        .calls_of("herdr")
        .into_iter()
        .filter(|c| {
            let argv: Vec<String> = serde_json::from_value(c["argv"].clone()).unwrap_or_default();
            argv.as_slice() != ["workspace", "list"]
        })
        .collect();
    assert!(
        herdr_mutations.is_empty(),
        "an hc fleet must not mutate or lay out on herdr: {herdr_mutations:?}"
    );

    let space = fleet_space(&h);
    let sp_id = space["id"].as_str().unwrap().to_string();
    assert_eq!(space["folder"].as_str(), Some(cwd(&h).as_str()));
    let root_term = space["tabs"][0]["terminals"][0]["terminal"]
        .as_str()
        .unwrap()
        .to_string();

    let records = SessionLedger::for_project(&h.state, &cwd(&h))
        .read()
        .unwrap();
    let orch: Vec<_> = records.iter().filter(|r| r.is_orchestrator()).collect();
    assert_eq!(orch.len(), 1, "{records:#?}");
    assert_eq!(orch[0].backend, Backend::Hc);
    assert_eq!(orch[0].workspace_id.as_deref(), Some(sp_id.as_str()));

    let sends: Vec<Vec<String>> = h
        .hc_calls()
        .into_iter()
        .filter(|c| c["program"].as_str() == Some("hc"))
        .filter_map(|c| serde_json::from_value::<Vec<String>>(c["argv"].clone()).ok())
        .filter(|argv| {
            argv.first().map(String::as_str) == Some("send")
                && argv.iter().any(|a| a.contains("pane-launch"))
        })
        .collect();
    assert_eq!(sends.len(), 1, "{sends:?}");
    assert!(
        sends[0].iter().any(|a| a == &root_term),
        "pane-launch was not sent to root terminal {root_term}: {:?}",
        sends[0]
    );
    assert!(
        sends[0].iter().any(|a| a.contains("HORCH_BACKEND=hc")),
        "pane-launch command should carry HORCH_BACKEND=hc: {:?}",
        sends[0]
    );

    // Execute the exact pane-launch command line inside the root terminal
    // (HC_TERMINAL_ID set, no HERDR_PANE_ID) and confirm the launched agent
    // gets HORCH_BACKEND=hc and no HERDR_PANE_ID.
    let cmdline = sends[0].last().unwrap();
    let mut launch = std::process::Command::new("/bin/sh");
    h.seal(&mut launch);
    launch
        .args(["-c", cmdline])
        .current_dir(&h.project)
        .env("HC_TERMINAL_ID", &root_term)
        .env_remove("HERDR_PANE_ID");
    let lo = launch.output().unwrap();
    assert!(lo.status.success(), "{}{}", out(&lo), err(&lo));

    let claude = h
        .calls_of("claude")
        .into_iter()
        .last()
        .expect("pane-launch ran fake-claude");
    let env_keys: Vec<String> = serde_json::from_value(claude["env_keys"].clone()).unwrap();
    assert!(
        env_keys.iter().any(|k| k == "HORCH_BACKEND"),
        "expected HORCH_BACKEND in {env_keys:?}"
    );
    assert!(
        !env_keys.iter().any(|k| k == "HERDR_PANE_ID"),
        "HERDR_PANE_ID must not be added on hc: {env_keys:?}"
    );
    let mailbox = Mailbox::under(&h.state, &sp_id);
    assert_eq!(
        mailbox.pane_for("orchestrator").as_deref(),
        Some(root_term.as_str())
    );
}

#[test]
fn hc_spawn_splits_right_of_the_orchestrator() {
    let mut h = harness("hc-spawn-right");
    let o = h.run(&[
        "--backend",
        "hc",
        "fleet",
        "opus",
        "--no-update",
        "--cwd",
        &cwd(&h),
    ]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));

    let space = fleet_space(&h);
    let sp_id = space["id"].as_str().unwrap().to_string();
    let root_term = space["tabs"][0]["terminals"][0]["terminal"]
        .as_str()
        .unwrap()
        .to_string();
    let root_pane = space["tabs"][0]["terminals"][0]["pane"]
        .as_str()
        .unwrap()
        .to_string();

    let mailbox = Mailbox::under(&h.state, &sp_id);
    std::fs::create_dir_all(mailbox.dir()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(mailbox.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    mailbox.register_role("orchestrator", &root_term).unwrap();

    h.set("HC_TERMINAL_ID", &root_term);
    let so = h.run(&["spawn", "sonnet", "--force"]);
    assert!(so.status.success(), "{}{}", out(&so), err(&so));
    let worker_term = out(&so).lines().last().unwrap().trim().to_string();
    assert_ne!(worker_term, root_term);

    // Confirm herdr-app split right from the orchestrator's app pane, and no
    // herdr pane/layout calls happened.
    let herdr_non_list: Vec<Value> = h
        .calls_of("herdr")
        .into_iter()
        .filter(|c| {
            let argv: Vec<String> = serde_json::from_value(c["argv"].clone()).unwrap_or_default();
            argv.as_slice() != ["workspace", "list"]
        })
        .collect();
    assert!(herdr_non_list.is_empty(), "{herdr_non_list:?}");
    let splits: Vec<Value> = h
        .hc_calls()
        .into_iter()
        .filter(|c| c["program"].as_str() == Some("herdr-app"))
        .filter_map(|c| {
            let argv: Vec<String> = serde_json::from_value(c["argv"].clone()).ok()?;
            (argv.first().map(String::as_str) == Some("do")
                && argv.get(1).map(String::as_str) == Some("pane.split"))
            .then(|| serde_json::from_str::<Value>(argv.get(2)?).ok())?
        })
        .collect();
    assert_eq!(splits.len(), 1, "{splits:?}");
    assert_eq!(splits[0]["direction"].as_str(), Some("right"));
    assert_eq!(splits[0]["pane"].as_str(), Some(root_pane.as_str()));

    let records = SessionLedger::for_project(&h.state, &cwd(&h))
        .read()
        .unwrap();
    let worker = records
        .iter()
        .find(|r| r.role == "sonnet-1")
        .expect("worker record recorded");
    assert_eq!(worker.backend, Backend::Hc);
    assert_eq!(worker.pane_id.as_deref(), Some(worker_term.as_str()));

    let brief = mailbox.read_brief("sonnet-1").unwrap();
    assert_eq!(brief.backend, Backend::Hc);

    // Run `horch worker sonnet-1` in the worker terminal (HC_TERMINAL_ID only)
    // to exercise Lifecycle::pane identity, registration, and agent launch env.
    let mut wcmd = h.horch(&["worker", "sonnet-1"]);
    wcmd.env("HC_TERMINAL_ID", &worker_term)
        .env("HORCH_BACKEND", "hc")
        .env_remove("HERDR_PANE_ID")
        .env_remove("HORCH_WORKSPACE_ID");
    let wo = wcmd.output().unwrap();
    assert!(wo.status.success(), "{}{}", out(&wo), err(&wo));
    assert_eq!(
        mailbox.pane_for("sonnet-1").as_deref(),
        Some(worker_term.as_str())
    );
    let claude = h
        .calls_of("claude")
        .into_iter()
        .last()
        .expect("worker launched fake-claude");
    let env_keys: Vec<String> = serde_json::from_value(claude["env_keys"].clone()).unwrap();
    assert!(env_keys.iter().any(|k| k == "HORCH_BACKEND"));
    assert!(!env_keys.iter().any(|k| k == "HERDR_PANE_ID"));
}

#[test]
fn hc_spawn_fifth_worker_gets_a_new_tab() {
    let mut h = harness("hc-spawn-tab2");
    let o = h.run(&[
        "--backend",
        "hc",
        "fleet",
        "opus",
        "--no-update",
        "--cwd",
        &cwd(&h),
    ]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));

    let space = fleet_space(&h);
    let sp_id = space["id"].as_str().unwrap().to_string();
    let root_term = space["tabs"][0]["terminals"][0]["terminal"]
        .as_str()
        .unwrap()
        .to_string();

    let mailbox = Mailbox::under(&h.state, &sp_id);
    std::fs::create_dir_all(mailbox.dir()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(mailbox.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    mailbox.register_role("orchestrator", &root_term).unwrap();
    h.set("HC_TERMINAL_ID", &root_term);

    let mut spawned = Vec::new();
    for i in 1..=5 {
        let so = h.run(&["spawn", "sonnet", "--force"]);
        assert!(
            so.status.success(),
            "spawn {i} failed: {}{}",
            out(&so),
            err(&so)
        );
        let term = out(&so).lines().last().unwrap().trim().to_string();
        spawned.push(term);
    }

    let updated = fleet_space(&h);
    let tabs = updated["tabs"].as_array().unwrap();
    assert_eq!(tabs.len(), 2, "expected 2 tabs after 5th worker: {tabs:#?}");
    assert_eq!(tabs[0]["terminals"].as_array().unwrap().len(), 5);
    assert_eq!(tabs[1]["name"].as_str(), Some("workers 2"));
    assert_eq!(tabs[1]["terminals"].as_array().unwrap().len(), 1);
    assert_eq!(
        tabs[1]["terminals"][0]["terminal"].as_str(),
        Some(spawned[4].as_str())
    );
}

#[test]
fn hc_spawn_with_the_window_closed_uses_a_labelled_run() {
    let mut h = harness("hc-spawn-closed");
    let o = h.run(&[
        "--backend",
        "hc",
        "fleet",
        "opus",
        "--no-update",
        "--cwd",
        &cwd(&h),
    ]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));

    let space = fleet_space(&h);
    let sp_id = space["id"].as_str().unwrap().to_string();
    let tb_id = space["tabs"][0]["id"].as_str().unwrap().to_string();
    let root_term = space["tabs"][0]["terminals"][0]["terminal"]
        .as_str()
        .unwrap()
        .to_string();

    let mailbox = Mailbox::under(&h.state, &sp_id);
    std::fs::create_dir_all(mailbox.dir()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(mailbox.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    mailbox.register_role("orchestrator", &root_term).unwrap();

    h.close_hc_window();
    h.set("HC_TERMINAL_ID", &root_term);
    let so = h.run(&["spawn", "sonnet", "--force"]);
    assert!(so.status.success(), "{}{}", out(&so), err(&so));
    let worker_term = out(&so).lines().last().unwrap().trim().to_string();

    let runs: Vec<Vec<String>> = h
        .hc_calls()
        .into_iter()
        .filter(|c| c["program"].as_str() == Some("hc"))
        .filter_map(|c| serde_json::from_value::<Vec<String>>(c["argv"].clone()).ok())
        .filter(|argv| argv.first().map(String::as_str) == Some("run"))
        .collect();
    assert_eq!(runs.len(), 1, "expected 1 labelled hc run: {runs:?}");
    let space_label = format!("herdr.space={sp_id}");
    let tab_label = format!("herdr.tab={tb_id}");
    assert!(runs[0].contains(&space_label), "{:?}", runs[0]);
    assert!(runs[0].contains(&tab_label), "{:?}", runs[0]);

    // Reopening the window adopts the labelled terminal into the space and tab.
    h.open_hc_window();
    let ls = h.horch(&["--backend", "hc", "sessions"]).output().unwrap();
    assert!(ls.status.success(), "{}{}", out(&ls), err(&ls));
    let state = h.hc_state();
    let found = state["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["address"].as_str() == Some(worker_term.as_str()));
    assert!(found, "worker terminal {worker_term} missing in {state:#?}");
}

#[test]
fn hc_fleet_start_with_the_window_closed_fails_clearly() {
    let h = harness("hc-fleet-closed");
    h.close_hc_window();
    let o = h.run(&[
        "--backend",
        "hc",
        "fleet",
        "opus",
        "--no-update",
        "--cwd",
        &cwd(&h),
    ]);
    assert!(!o.status.success(), "{}{}", out(&o), err(&o));
    assert!(
        err(&o).contains("the Herdr app window is not open"),
        "expected clear window-closed message, got: {}",
        err(&o)
    );
    let state = h.hc_state();
    assert!(
        state["spaces"].as_array().is_none_or(Vec::is_empty),
        "no space should exist when window is closed: {state:#?}"
    );
}

#[test]
fn hcb_12_e2e_spawn_refuses_opencode_on_hc_before_any_pane_and_channel_refuses_without_record() {
    let mut h = harness("hc-spawn-gate");
    let o = h.run(&[
        "--backend",
        "hc",
        "fleet",
        "opus",
        "--no-update",
        "--cwd",
        &cwd(&h),
    ]);
    assert!(o.status.success(), "{}{}", out(&o), err(&o));

    let space = fleet_space(&h);
    let sp_id = space["id"].as_str().unwrap().to_string();
    let root_term = space["tabs"][0]["terminals"][0]["terminal"]
        .as_str()
        .unwrap()
        .to_string();
    let terms_before = h.hc_state()["terminals"].as_array().unwrap().len();

    let mailbox = Mailbox::under(&h.state, &sp_id);
    std::fs::create_dir_all(mailbox.dir()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(mailbox.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    mailbox.register_role("orchestrator", &root_term).unwrap();

    h.set("HC_TERMINAL_ID", &root_term);
    let refused = h.run(&["spawn", "opencode-ultra", "--force"]);
    assert!(
        !refused.status.success(),
        "{}{}",
        out(&refused),
        err(&refused)
    );
    assert!(
        err(&refused).contains(
            "opencode does not report its status under hc, so horch cannot deliver to it there; run this fleet in the herdr TUI"
        ),
        "unexpected refusal message: {}",
        err(&refused)
    );
    let terms_after = h.hc_state()["terminals"].as_array().unwrap().len();
    assert_eq!(
        terms_after, terms_before,
        "no terminal may be created on a refused harness"
    );
}
