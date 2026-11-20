//! End-to-end: `horch tell`'s grace wait with its default value (U-70).
//!
//! The harness sets `HORCH_TELL_GRACE_MS=0`, because a fake pane never shows
//! an agent. This file sets it empty, which is the default grace, so the
//! worker's `horch done` reports to an orchestrator that registered seconds
//! ago and is a plain shell (fake-herdr `shell`): the case where `tell`
//! waits up to `TELL_GRACE` (30 s). The worker gives its agent
//! `HORCH_TELL_GRACE_MS=0`, so the pane closes at once (commit `2792508`,
//! after `horch smoke fleet` waited 34 s).

use std::process::Output;
use std::time::{Duration, Instant};

use horch_core::execution::legacy::Record;
use horch_core::execution::records::SessionLedger;
use horch_core::messaging::brief::Brief;
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

fn herdr(h: &Harness, args: &[&str]) -> Output {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    cmd.args(args).output().unwrap()
}

/// The session records of the project, as the session ledger holds them.
fn records(h: &Harness) -> Vec<Value> {
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .read()
        .map(|all| {
            all.into_iter()
                .map(|r| serde_json::to_value(r).unwrap())
                .collect()
        })
        .unwrap_or_default()
}

/// The pane of a `smoke-1` worker that `horch worker smoke-1` runs in, as
/// `horch spawn` left it: its brief and its session record. `horch spawn`
/// no longer starts a worker outside a run, and fleetd cannot launch the
/// `smoke` teammate (`agent: none`). The test fleetd of the harness writes
/// the records that the worker changes.
fn smoke_pane(h: &Harness) -> String {
    let split = herdr(h, &["pane", "split", "w1:p1", "--direction", "right"]);
    assert!(split.status.success(), "{}", text(&split));
    let reply: Value = serde_json::from_slice(&split.stdout).unwrap();
    let pane = reply["result"]["pane"]["pane_id"]
        .as_str()
        .expect("the reply names the new pane")
        .to_string();
    let brief: Brief = serde_json::from_value(serde_json::json!({
        "schema": 2,
        "role": "smoke-1",
        "teammate": "smoke",
        "agent": "none",
        "model": "none",
        "record_id": "r-smoke",
        "session_id": "",
        "resume": false,
        "project_dir": h.project,
    }))
    .unwrap();
    let mailbox = Mailbox::under(&h.state, "w1");
    std::fs::create_dir_all(mailbox.dir()).unwrap();
    mailbox.write_brief(&brief).unwrap();
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .insert(Record {
            record_id: "r-smoke".into(),
            agent: "none".into(),
            tier: "smoke".into(),
            model: "none".into(),
            role: "smoke-1".into(),
            task: "smoke".into(),
            ..Record::default()
        })
        .unwrap();
    pane
}

/// Poll `f` every 50 ms until it is true; false after `limit`.
fn within(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + limit;
    while Instant::now() < until {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    f()
}

/// The `pane get` calls on `pane` that the process which typed the first
/// line holding `marker` into `pane` made just before it: the polls of its
/// grace wait. Each fake-herdr call logs its `caller` pid, so the polls of
/// other processes on the same pane (the mail courier) do not count.
fn polls_before_line(h: &Harness, pane: &str, marker: &str) -> Option<usize> {
    let calls: Vec<Value> = h
        .calls()
        .into_iter()
        .filter(|c| c["fake"] == "herdr")
        .collect();
    let argv = |c: &Value| -> Vec<String> {
        c["argv"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a.as_str().map(str::to_string))
            .collect()
    };
    let typed = calls.iter().position(|c| {
        let a = argv(c);
        a.len() > 3 && a[1] != "get" && a[2] == pane && a[3..].join(" ").contains(marker)
    })?;
    let caller = &calls[typed]["caller"];
    let polls = calls[..typed]
        .iter()
        .rev()
        .filter(|c| &c["caller"] == caller)
        .take_while(|c| argv(c) == ["pane", "get", pane])
        .count();
    Some(polls)
}

/// U-70: with the default grace, the smoke worker's `horch done` reports to
/// the orchestrator without the grace wait: it looks at the orchestrator's
/// pane once and types the report. A grace wait polls that pane until an
/// agent shows or the grace ends, at least 3 looks. The count does not
/// depend on how fast the machine is, unlike the 5 s limit this test had.
#[cfg(unix)]
#[test]
fn worker_done_reports_without_the_default_tell_grace() {
    let mut h = Harness::new("tell-grace");
    // The session records and the task ledger share 1 file below the state
    // folder, as in production.
    h.set(
        "HORCH_LEDGER_DB",
        h.state.join("ledger.sqlite3").to_string_lossy(),
    );
    // `seal` sets the variable to 0; an empty value is the default grace.
    h.set("HORCH_TELL_GRACE_MS", "");
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", "exec,shell");
    let out = herdr(&h, &["workspace", "create", "--label", "tell-grace"]);
    assert!(out.status.success(), "{}", text(&out));

    // The orchestrator: a plain shell in the root pane, registered now, so
    // no agent shows in it and its role is fresh.
    let mut register = h.horch(&["register", "orchestrator"]);
    register.env("HERDR_PANE_ID", "w1:p1");
    let out = register.output().unwrap();
    assert!(out.status.success(), "{}", text(&out));

    h.serve_fleetd();
    let pane = smoke_pane(&h);
    let line = format!("{} worker smoke-1", Harness::horch_bin().display());
    let out = herdr(&h, &["pane", "run", &pane, &line]);
    assert!(out.status.success(), "{}", text(&out));

    let is_done = |h: &Harness| {
        records(h)
            .iter()
            .any(|r| r["role"] == "smoke-1" && r["status"] == "done")
    };
    assert!(
        within(Duration::from_secs(30), || is_done(&h)),
        "the smoke session never reached done: {:?}",
        records(&h)
    );
    assert!(
        within(Duration::from_secs(40), || !herdr(
            &h,
            &["pane", "get", &pane]
        )
        .status
        .success()),
        "the worker pane {pane} never closed"
    );
    // FR-7: done types the mail line, and its report waits in the mail.
    let polls = polls_before_line(&h, "w1:p1", horch_core::messaging::delivery::MAIL_LINE)
        .expect("done typed the mail line");
    assert!(horch_core::messaging::mail::Mail::under(&h.state, "w1")
        .pending()
        .unwrap()
        .iter()
        .any(|m| m.text.contains("DONE:")));
    assert_eq!(
        polls, 1,
        "done looked at the orchestrator pane {polls} times before its report: \
         tell waited for its grace"
    );
}
