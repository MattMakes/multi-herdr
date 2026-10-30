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
    let slug: String = h
        .project
        .to_string_lossy()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect();
    std::fs::read_to_string(h.state.join(format!("{slug}.json")))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
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

    let out = h.run(&[
        "spawn",
        "smoke",
        "verify the tell grace",
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ]);
    assert!(out.status.success(), "{}", text(&out));
    let pane = String::from_utf8_lossy(&out.stdout)
        .lines()
        .last()
        .unwrap()
        .trim()
        .to_string();

    let is_done = |h: &Harness| {
        records(h)
            .iter()
            .any(|r| r["pane_id"] == pane.as_str() && r["status"] == "done")
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
    let polls = polls_before_line(&h, "w1:p1", "DONE:").expect("done typed its report");
    assert_eq!(
        polls, 1,
        "done looked at the orchestrator pane {polls} times before its report: \
         tell waited for its grace"
    );
}
