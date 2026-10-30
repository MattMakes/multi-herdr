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

/// U-70: with the default grace, the smoke worker's pane closes in under
/// 5 s after its `horch done` records the session as done.
#[cfg(unix)]
#[test]
fn worker_pane_closes_within_5_s_of_done_with_the_default_tell_grace() {
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
    let done_at = Instant::now();
    assert!(
        within(Duration::from_secs(40), || !herdr(
            &h,
            &["pane", "get", &pane]
        )
        .status
        .success()),
        "the worker pane {pane} never closed"
    );
    let waited = done_at.elapsed();
    assert!(
        waited < Duration::from_secs(5),
        "the pane closed {waited:?} after done; tell waited for its grace"
    );
}
