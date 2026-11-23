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

fn stop_fleetd_and_jobs(h: &Harness) {
    use horch_core::procid::{signal_same, start_time};
    h.stop_fleetd();
    let marks: Vec<String> = [Some(h.root.clone()), std::fs::canonicalize(&h.root).ok()]
        .into_iter()
        .flatten()
        .map(|root| format!("{}/", root.display()))
        .collect();
    let jobs = || -> Vec<(u32, u64)> {
        horch_e2e::process::commands_with_environment()
            .into_iter()
            .filter(|(_, l)| {
                l.contains(" job run ") && marks.iter().any(|m| l.contains(m.as_str()))
            })
            .filter_map(|(pid, _)| Some((pid, start_time(pid)?)))
            .collect()
    };
    let first = std::time::Instant::now();
    horch_e2e::harness::wait_for("the jobs of fleetd to end", || {
        let running = jobs();
        let signal = if first.elapsed() < std::time::Duration::from_secs(10) {
            15
        } else {
            9
        };
        for (pid, started) in &running {
            signal_same(*pid, *started, signal);
        }
        running.is_empty().then_some(())
    });
}

#[cfg(unix)]
#[test]
fn worker_done_reports_without_the_default_tell_grace() {
    let mut h = Harness::new("tell-grace");
    h.set(
        "HORCH_LEDGER_DB",
        h.state.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_TELL_GRACE_MS", "");
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", "exec,shell");
    let out = herdr(&h, &["workspace", "create", "--label", "tell-grace"]);
    assert!(out.status.success(), "{}", text(&out));

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
    stop_fleetd_and_jobs(&h);
}
