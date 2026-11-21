//! End-to-end: a detached courier for a workspace recorded on `hc` routes its
//! pane calls through `hc` and never invokes `herdr` (HCB-06).

use std::process::Command;

use horch_core::execution::legacy::{LedgerRecordV1, KIND_ORCHESTRATOR, STATUS_WORKING};
use horch_core::execution::store::ExecutionStore;
use horch_core::messaging::mail::Mail;
use horch_core::messaging::mailbox::Mailbox;
use horch_core::runtime::{Backend, MapEnv, RuntimeContext};
use horch_e2e::harness::{wait_for_or, Harness};
use serde_json::Value;

#[test]
fn hcb_06_a_detached_courier_for_an_hc_workspace_calls_hc() {
    let mut h = Harness::new("hc-courier");
    h.set("HORCH_TEST_MAIL_POLL_MS", "25");

    let mut run_hc = Command::new(h.bin.join("hc"));
    h.seal(&mut run_hc);
    let out = run_hc
        .args([
            "run",
            "--bg",
            "--json",
            "--keep",
            "--no-parent",
            "--label",
            "herdr.space=sp01",
            "--label",
            "herdr.tab=tb01",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let created: Value = serde_json::from_slice(&out.stdout).unwrap();
    let term = created["address"].as_str().unwrap().to_string();
    h.set_hc_terminal_status(&term, Some(("claude-code", "idle")), Some(1000));

    // Clear the log of our own setup `hc run` so everything logged next comes
    // from the courier alone.
    let _ = std::fs::remove_file(h.state.join("fake-hc.log"));

    let env = MapEnv::new(&h.project)
        .with("HOME", &h.home.to_string_lossy())
        .with("HORCH_STATE_DIR", &h.state.to_string_lossy())
        .with("HORCH_PROJECT_DIR", &h.project.to_string_lossy())
        .with("HORCH_LEDGER_DB", &h.ledger_db().to_string_lossy());
    let ctx = RuntimeContext::from_env(&env).unwrap();
    // The run branch writes session records through fleetd (T-884).
    let _fleetd =
        horch_core::fleetd::fleet_runtime::adapters::test_fleetd::serve_session_records(&ctx.paths)
            .unwrap();
    let store = ExecutionStore::open_in(&ctx).unwrap();
    store
        .insert(LedgerRecordV1 {
            record_id: "rec_hc_orch".into(),
            kind: KIND_ORCHESTRATOR.into(),
            status: STATUS_WORKING.into(),
            role: "orchestrator".into(),
            agent: "claude".into(),
            project: Some(h.project.to_string_lossy().into_owned()),
            workspace_id: Some("sp01".into()),
            pane_id: Some(term.clone()),
            backend: Backend::Hc,
            ..LedgerRecordV1::default()
        })
        .unwrap();

    let mailbox = Mailbox::under(&h.state, "sp01");
    std::fs::create_dir_all(mailbox.dir()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(mailbox.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    mailbox.register_role("orchestrator", &term).unwrap();

    let mail = Mail::under(&h.state, "sp01");
    mail.enqueue("sonnet-1", "DONE: finished slice").unwrap();

    // Start the courier outside any pane or project: no HORCH_BACKEND, no
    // HC_TERMINAL_ID, no HERDR_PANE_ID, and HORCH_PROJECT_DIR pointing at an
    // unrelated directory so cross-project lookup resolves `sp01` -> `hc`.
    let mut courier_cmd = h.horch(&["mail", "courier", "--workspace", "sp01"]);
    courier_cmd
        .current_dir(&h.tmp)
        .env("HORCH_PROJECT_DIR", &h.tmp)
        .env_remove("HORCH_BACKEND")
        .env_remove("HC_TERMINAL_ID")
        .env_remove("HERDR_PANE_ID");
    let mut child = courier_cmd.spawn().expect("spawning courier");

    wait_for_or(
        "courier to deliver via fake-hc",
        || {
            h.hc_calls()
                .iter()
                .any(|c| {
                    c["program"].as_str() == Some("hc")
                        && c["argv"]
                            .as_array()
                            .is_some_and(|a| a.first().and_then(Value::as_str) == Some("send"))
                })
                .then_some(())
        },
        || format!("hc_calls={:?}, herdr_calls={:?}", h.hc_calls(), h.calls()),
    );

    mail.ack_all().unwrap();
    let status = child.wait().expect("waiting for courier");
    assert!(status.success(), "{status}");

    assert!(
        h.calls_of("herdr").is_empty(),
        "courier must never invoke fake-herdr for an hc workspace: {:?}",
        h.calls_of("herdr")
    );
}
