#![cfg(unix)]

use std::process::Output;

use horch_core::messaging::mailbox::Mailbox;
use horch_e2e::harness::{fixtures, Harness};
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn json_of(h: &Harness, args: &[&str]) -> Value {
    let o = h.run(args);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
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
    let teammates = h.root.join("teammates");
    std::fs::create_dir_all(&teammates).unwrap();
    let ctl = "---\nname: ctl\nbrief_description: Test-only ctl.\nhidden: true\n\
               base: fleet-worker\nagent: none\nmodel: none\nspawn_only: [\"intake-*\"]\n\
               ---\nTest-only controller.\n";
    std::fs::write(teammates.join("ctl.md"), ctl).unwrap();
    let role = "---\nname: intake-smoke\nbrief_description: Test-only intake-smoke.\n\
                hidden: true\nbase: report-worker\nagent: none\nmodel: none\n\
                horch_access: report\nsandbox: {}\n---\nTest-only report role.\n";
    std::fs::write(teammates.join("intake-smoke.md"), role).unwrap();
    h.set("HORCH_TEAMMATES_DIR", teammates.to_string_lossy());
    h
}

fn ledger_workers(h: &Harness) -> Vec<Value> {
    let st = json_of(h, &["ledger", "status", "--json"]);
    st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["worker"].clone())
        .filter(|w| w["kind"] == "worker")
        .collect()
}

fn record_agent(h: &Harness, worker_id: &str, pid: u32) {
    let store = horch_core::taskledger::Store::open(&h.root.join("ledger.sqlite3")).unwrap();
    horch_core::taskledger::workers::agent_started(
        store.conn(),
        worker_id,
        pid,
        horch_core::procid::start_time(pid),
    )
    .unwrap();
}

#[test]
fn hc_a_controller_in_a_run_spawns_a_report_role_with_a_brief() {
    let mut h = harness("hc-ctl-brief");
    let project = h.project.to_string_lossy().into_owned();
    let o = h.run(&[
        "--backend",
        "hc",
        "fleet",
        "opus",
        "--no-update",
        "--cwd",
        &project,
    ]);
    assert!(o.status.success(), "{}", text(&o));
    let state = h.hc_state();
    let space = state["spaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"].as_str() != Some("horch telemetry"))
        .cloned()
        .expect("a fleet space");
    let sp_id = space["id"].as_str().unwrap().to_string();
    let root_term = space["tabs"][0]["terminals"][0]["terminal"]
        .as_str()
        .unwrap()
        .to_string();
    let mailbox = Mailbox::under(&h.state, &sp_id);
    std::fs::create_dir_all(mailbox.dir()).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(mailbox.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    mailbox.register_role("orchestrator", &root_term).unwrap();

    h.set("HC_TERMINAL_ID", &root_term);
    h.set("HORCH_BACKEND", "hc");
    h.set("HORCH_WORKSPACE_ID", &sp_id);
    let o = h.run(&["ledger", "run", "new", "hc-intake-run"]);
    assert!(o.status.success(), "{}", text(&o));
    let ticket = h.project.join("ctl-ticket.md");
    std::fs::write(&ticket, "# ctl\n**Check:** `true`\n").unwrap();
    let ticket = ticket.to_string_lossy().into_owned();
    let o = h.run(&["ledger", "add", "--file", &ticket, "--primary", "ctl"]);
    assert!(o.status.success(), "{}", text(&o));

    let o = h.run(&["spawn", "ctl", "--exact"]);
    assert!(o.status.success(), "{}", text(&o));
    let ctl_term = String::from_utf8_lossy(&o.stdout)
        .lines()
        .last()
        .unwrap()
        .trim()
        .to_string();
    assert_ne!(ctl_term, root_term);
    mailbox.register_role("ctl-1", &ctl_term).unwrap();
    let ctl_row = ledger_workers(&h)
        .into_iter()
        .find(|w| w["persona"] == "ctl")
        .expect("the controller has a worker row");
    record_agent(
        &h,
        ctl_row["worker_id"].as_str().unwrap(),
        std::process::id(),
    );

    let brief = h.project.join("brief.md");
    std::fs::write(&brief, "# brief\n").unwrap();
    let task = format!("Read and follow {} exactly.", brief.to_string_lossy());
    let mut cmd = h.horch(&["spawn", "intake-smoke", "--exact", &task]);
    cmd.env("HC_TERMINAL_ID", &ctl_term)
        .env_remove("HERDR_PANE_ID");
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "{}", text(&o));

    let row = ledger_workers(&h)
        .into_iter()
        .find(|w| w["persona"] == "intake-smoke")
        .unwrap_or_else(|| panic!("the role has no worker row: {}", text(&o)));
    assert_eq!(row["run_id"], ctl_row["run_id"], "{row}");
    let role_brief = mailbox.read_brief("intake-smoke-1").unwrap();
    assert_eq!(role_brief.controller.as_deref(), Some("ctl-1"));
    assert_eq!(role_brief.task, task);
}
