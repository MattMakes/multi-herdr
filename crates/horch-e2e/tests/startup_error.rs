//! End-to-end: a worker that dies before it registers still shows its error
//! (W13 C4).
//!
//! A pane line starts with `exec`, so a real pane closes when `horch worker`
//! exits and takes the worker's stderr with it. The worker writes its startup
//! error to a file, and `horch spawn`, which waits for the worker to register,
//! reads that file and fails with the text.
//!
//! fake-herdr runs in the `exec` scenario, so the pane really runs
//! `horch worker`. For the missing brief, a herdr wrapper gives that pane
//! another `HORCH_WORKSPACE_ID`, so the worker looks for its brief in a
//! mailbox that does not have it. For the late worker, a wrapper starts the
//! pane line 2 s late.

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

/// A harness with workspace `w1`, whose pane `w1:p1` runs its commands
/// (fake-herdr `exec`), and the spawn wait `wait_ms`.
fn workspace(name: &str, wait_ms: &str) -> Harness {
    let mut h = Harness::new(name);
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    h.set("HORCH_SPAWN_WAIT_MS", wait_ms);
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let out = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    h
}

fn spawn_smoke(h: &Harness) -> Output {
    h.run(&[
        "spawn",
        "smoke",
        "start",
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ])
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

/// A worker that registers: the spawn waits for it, and stdout is still the
/// pane id alone, which callers chain.
#[cfg(unix)]
#[test]
fn spawn_waits_for_a_worker_that_registers_and_prints_only_the_pane() {
    let h = workspace("startup-ok", "20000");
    let out = spawn_smoke(&h);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "w1:p2\n");
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("has not registered"),
        "{}",
        text(&out)
    );
}

/// A worker that registers after the wait: the spawn succeeds with a
/// warning on stderr, and the worker still runs to its end.
#[cfg(unix)]
#[test]
fn a_worker_that_registers_after_the_wait_still_works() {
    let mut h = workspace("startup-late", "300");
    // The pane starts its line 2 s late, after the wait ends.
    let fake = h.bin.join("herdr");
    let wrapper = h.write_bin(
        "herdr-slow",
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = pane ] && [ \"$2\" = run ]; then\n\
             \x20 exec '{fake}' pane run \"$3\" \"/bin/sleep 2; $4\"\n\
             fi\n\
             exec '{fake}' \"$@\"\n",
            fake = fake.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_HERDR_BIN", wrapper.to_string_lossy());
    let out = spawn_smoke(&h);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "w1:p2\n");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("smoke-1 (pane w1:p2) has not registered"),
        "{}",
        text(&out)
    );
    let finished = || {
        records(&h)
            .iter()
            .any(|r| r["role"] == "smoke-1" && r["state"]["state"] == "done")
    };
    assert!(
        within(Duration::from_secs(30), finished),
        "the late worker never finished: {:?}",
        records(&h)
    );
}

#[cfg(unix)]
#[test]
fn spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing() {
    let mut h = workspace("startup-error", "20000");
    let fake = h.bin.join("herdr");
    let wrapper = h.write_bin(
        "herdr-elsewhere",
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = pane ] && [ \"$2\" = run ]; then\n\
             \x20 export HORCH_WORKSPACE_ID=elsewhere\n\
             fi\n\
             exec '{}' \"$@\"\n",
            fake.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_HERDR_BIN", wrapper.to_string_lossy());

    let started = Instant::now();
    let out = h.run(&[
        "spawn",
        "smoke",
        "never starts",
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{}", text(&out));
    assert!(
        stderr.contains("smoke-1") && stderr.contains("no brief at "),
        "spawn's stderr must hold the worker's error: {}",
        text(&out)
    );
    assert!(
        stderr.contains("herdr-orchestration-elsewhere"),
        "{}",
        text(&out)
    );
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "spawn reads the error as soon as it appears, not at the timeout"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).trim().is_empty(),
        "no pane id on stdout for a worker that never started: {}",
        text(&out)
    );

    // Spawn deletes the error file after it reads it.
    let errors = h.home.join(".local/share/horch/startup-errors");
    assert!(
        errors.is_dir(),
        "the worker wrote under {}",
        errors.display()
    );
    let left: Vec<_> = std::fs::read_dir(&errors)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "error files left: {left:?}");

    // The record ends as a failed launch, with the worker's error.
    let r = records(&h)
        .into_iter()
        .find(|r| r["role"] == "smoke-1")
        .expect("a record for smoke-1");
    assert_eq!(r["status"], "done", "{r}");
    assert_eq!(r["state"]["state"], "launch_failed", "{r}");
    assert_eq!(r["state"]["stage"], "run", "{r}");
    let reason = r["state"]["reason"].as_str().unwrap_or_default();
    assert!(reason.starts_with("no brief at "), "{r}");
}
