//! SPC-03: the collector start handshake. `horch telemetry ensure` does not
//! wait for the collector it starts, so a slow start is no failure and loses
//! no event, and a start that never runs is reported and started again by
//! the next `ensure`.
//!
//! A stand-in herdr wraps the fake one and puts a shell prefix before every
//! `pane run` command: `/bin/sleep 8;` for a slow start (the sealed `PATH` has no `sleep`), `exit 3;` for a start
//! that never runs. The old `ensure` waited a fixed 5 s, so it failed both.
#![cfg(unix)]

use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant, SystemTime};

use horch_e2e::harness::{self, Harness};
use serde_json::Value;

const SESSION: &str = "33333333-3333-4333-8333-333333333333";

fn text(o: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A herdr that runs every `pane run` command after the shell `prefix`.
/// Every other call goes to the fake herdr unchanged.
fn prefixed_herdr(h: &mut Harness, prefix: &str) {
    let real = h.bin.join("herdr");
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = pane ] && [ \"$2\" = run ]; then\n\
         \x20 exec '{}' pane run \"$3\" \"{prefix} $4\"\n\
         fi\n\
         exec '{}' \"$@\"\n",
        real.display(),
        real.display()
    );
    let path = h.write_bin("prefixed-herdr", script.as_bytes());
    h.set("HORCH_HERDR_BIN", path.to_string_lossy());
}

/// One Claude record and its transcript with 1 response, all stamped now,
/// so retention keeps them whatever the date.
fn one_session(h: &Harness) {
    let now = horch_core::clock::stamp(horch_core::clock::now());
    let record = serde_json::json!([{
        "record_id": "rec-slow", "session_id": SESSION, "agent": "claude",
        "tier": "worker", "model": "claude-sonnet-5", "phase": "implementation",
        "role": "sonnet-1", "status": "working", "task": "slow start",
        "history": [{"at": now, "event": "spawned", "text": "slow start"}],
        "created_at": now, "updated_at": now, "kind": "worker",
        "project": "/work/slow",
    }]);
    std::fs::write(h.state.join("-work-slow.json"), record.to_string()).unwrap();
    let dir = h.home.join(".claude/projects/-work-slow");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{SESSION}.jsonl")), response("msg_early")).unwrap();
}

/// 1 assistant line of the session, with message id `id`.
fn response(id: &str) -> String {
    let now = horch_core::clock::stamp(horch_core::clock::now());
    let line = serde_json::json!({
        "type": "assistant", "sessionId": SESSION, "cwd": "/work/slow",
        "uuid": format!("u-{id}"), "timestamp": now,
        "message": {"id": id, "type": "message", "role": "assistant",
            "model": "claude-sonnet-5", "content": [],
            "usage": {"input_tokens": 10, "output_tokens": 20}},
    });
    format!("{line}\n")
}

/// The message ids in the event store.
fn stored(h: &Harness) -> Vec<String> {
    horch_core::telemetry::store::read_all(&h.state.join("telemetry"))
        .into_iter()
        .map(|e| e.event_id)
        .collect()
}

/// Stops the collector when dropped, after an outstanding start ends.
struct CollectorGuard(std::path::PathBuf);

impl Drop for CollectorGuard {
    fn drop(&mut self) {
        let until = Instant::now() + harness::DEADLINE;
        while self.0.join("telemetry/start.json").exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
        }
        // The kill syscall, not a `kill` program: a fork can fail on a
        // loaded machine, and then nothing is signalled.
        if let Some((pid, start)) = collector(&self.0) {
            horch_core::procid::signal_same(pid, start, libc::SIGTERM);
        }
    }
}

/// The recorded collector's pid and start time.
fn collector(state: &Path) -> Option<(u32, u64)> {
    let text = std::fs::read_to_string(state.join("telemetry/collector.json")).ok()?;
    let record: Value = serde_json::from_str(&text).ok()?;
    let pid = u32::try_from(record["pid"].as_u64()?).ok()?;
    Some((pid, record["pid_start"].as_u64()?))
}

/// Wait until a live collector holds the lock and has ended its start.
fn wait_started(h: &Harness) {
    let start = h.state.join("telemetry/start.json");
    harness::wait_for_or(
        "the started collector to hold the lock",
        || {
            let (pid, since) = collector(&h.state)?;
            let live = horch_core::procid::alive(pid, Some(since));
            (live && !start.exists()).then_some(())
        },
        || format!("start.json: {:?}", std::fs::read_to_string(&start).ok()),
    );
}

/// SPC-03: a collector that takes 8 s to start is no failure. `ensure`
/// exits 0 at once, a second `ensure` sees the start in progress and starts
/// no other, and an event written before the collector runs is stored.
#[test]
fn spc_03_a_slow_collector_start_loses_no_event() {
    let mut h = Harness::new("tstart-slow");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    one_session(&h);
    prefixed_herdr(&mut h, "/bin/sleep 8;");
    let _guard = CollectorGuard(h.state.clone());

    let out = h.run(&["telemetry", "ensure"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("telemetry collector starting"),
        "{}",
        text(&out)
    );
    // The collector process sleeps: no lock yet, so this event happens
    // before any collector runs.
    let transcript = h
        .home
        .join(format!(".claude/projects/-work-slow/{SESSION}.jsonl"));
    let mut lines = std::fs::read_to_string(&transcript).unwrap();
    lines.push_str(&response("msg_late"));
    std::fs::write(&transcript, lines).unwrap();
    // A second `ensure` starts no other: the start is in progress, or the
    // collector is live by now.
    let again = h.run(&["telemetry", "ensure"]);
    assert!(again.status.success(), "{}", text(&again));
    let says = String::from_utf8_lossy(&again.stdout);
    assert!(
        says.contains("telemetry collector is starting") || says.contains("is live"),
        "{}",
        text(&again)
    );

    wait_started(&h);
    harness::wait_for_or(
        "both events in the store",
        || {
            let ids = stored(&h);
            (ids.contains(&"msg_early".to_string()) && ids.contains(&"msg_late".to_string()))
                .then_some(())
        },
        || format!("{:?}", stored(&h)),
    );
    let creates = h
        .calls_of("herdr")
        .into_iter()
        .filter(|c| c["argv"][1] == "create")
        .count();
    assert_eq!(creates, 1, "1 start, never a second one");
}

/// SPC-03: a start whose process never runs is no failure of that `ensure`.
/// While it is young the next call reports it starting; once it is older
/// than the bound, the next `ensure` reports it failed and starts the
/// collector again, in the same workspace.
#[test]
fn spc_03_a_start_that_never_runs_is_reported_and_retried() {
    let mut h = Harness::new("tstart-dead");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    let real = h.bin.join("herdr");
    prefixed_herdr(&mut h, "exit 3;");
    let _guard = CollectorGuard(h.state.clone());

    let out = h.run(&["telemetry", "ensure"]);
    assert!(out.status.success(), "{}", text(&out));
    let request = h.state.join("telemetry/start.json");
    assert!(request.is_file(), "the start is recorded");
    let again = h.run(&["telemetry", "ensure"]);
    assert!(
        String::from_utf8_lossy(&again.stdout).contains("telemetry collector is starting"),
        "{}",
        text(&again)
    );

    // The start is now older than the bound.
    let file = std::fs::File::options().write(true).open(&request).unwrap();
    let old = horch_core::telemetry::start::UNSEEN_AFTER + Duration::from_secs(60);
    file.set_modified(SystemTime::now() - old).unwrap();
    drop(file);
    h.set("HORCH_HERDR_BIN", real.to_string_lossy());
    let retry = h.run(&["telemetry", "ensure"]);
    assert!(retry.status.success(), "{}", text(&retry));
    let err = String::from_utf8_lossy(&retry.stderr);
    assert!(
        err.contains("failed: no collector process started in pane w1:p1"),
        "{}",
        text(&retry)
    );
    assert!(err.contains("starting it again"), "{}", text(&retry));

    wait_started(&h);
    let info: Value = serde_json::from_str(
        &std::fs::read_to_string(h.state.join("telemetry/collector.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        info["pane_id"], "w1:p1",
        "the failed start's workspace is reused"
    );
}

/// Write a start request for pane `w9:p1`, `age` old, and optionally a
/// marker of the same id with pid `marker_pid`.
fn failed_start(h: &Harness, age: Duration, marker_pid: Option<u32>) {
    use horch_core::telemetry::start;
    let request = start::Request {
        id: "q1".into(),
        requested_at: "2026-10-30T05:00:00Z".into(),
        workspace_id: "w9".into(),
        pane_id: "w9:p1".into(),
        herdr: h.bin.join("herdr"),
    };
    start::record(&h.state, &request).unwrap();
    let file = std::fs::File::options()
        .write(true)
        .open(start::request_path(&h.state))
        .unwrap();
    file.set_modified(SystemTime::now() - age).unwrap();
    if let Some(pid) = marker_pid {
        let marker = start::Marker {
            id: "q1".into(),
            pid,
            pid_start: Some(1),
            started_at: "2026-10-30T05:00:01Z".into(),
        };
        std::fs::write(
            start::marker_path(&h.state),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
    }
}

/// SPC-03: `horch quota` reports a failed collector start on stderr: a
/// request no process marked within the bound, and a marker whose pid
/// ended. A start in progress is not reported, and `--json` stays JSON.
#[test]
fn spc_03_quota_reports_a_failed_collector_start() {
    let h = Harness::new("tstart-quota");
    let bound = horch_core::telemetry::start::UNSEEN_AFTER;

    failed_start(&h, Duration::ZERO, None);
    let young = h.run(&["quota"]);
    assert!(young.status.success(), "{}", text(&young));
    assert!(
        !String::from_utf8_lossy(&young.stderr).contains("failed"),
        "a start in progress is no failure: {}",
        text(&young)
    );

    failed_start(&h, bound + Duration::from_secs(60), None);
    let unseen = h.run(&["quota", "--json"]);
    assert!(unseen.status.success(), "{}", text(&unseen));
    let err = String::from_utf8_lossy(&unseen.stderr);
    assert!(
        err.contains("the telemetry collector start in workspace w9 failed: no collector process started in pane w9:p1"),
        "{}",
        text(&unseen)
    );
    serde_json::from_slice::<Value>(&unseen.stdout).expect("stdout is still JSON");

    let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
    let dead = child.id();
    child.wait().unwrap();
    failed_start(&h, Duration::ZERO, Some(dead));
    let ended = h.run(&["quota"]);
    assert!(ended.status.success(), "{}", text(&ended));
    assert!(
        String::from_utf8_lossy(&ended.stderr).contains(&format!(
            "collector pid {dead} ended before it took the lock"
        )),
        "{}",
        text(&ended)
    );
}
