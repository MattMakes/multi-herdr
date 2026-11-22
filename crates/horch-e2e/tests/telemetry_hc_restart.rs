#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use horch_core::runtime::Backend;
use horch_core::telemetry::start;
use horch_e2e::harness::{self, Harness};
use serde_json::{json, Value};

const LABEL: &str = "horch telemetry";

fn text(o: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn harness(name: &str) -> Harness {
    let mut h = Harness::new(name);
    h.set("HORCH_FAKE_SCENARIO", "exec");
    h
}

fn ensure_on_hc(h: &Harness) -> Output {
    h.run(&["--backend", "hc", "telemetry", "ensure"])
}

fn seed_hc_space(h: &Harness) -> (String, String) {
    let mut app = Command::new(h.bin.join("herdr-app"));
    h.seal(&mut app);
    let payload = json!({"name": LABEL, "focus": false}).to_string();
    let out = app.args(["do", "space.new", &payload]).output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let made: Value = serde_json::from_slice(&out.stdout).unwrap();
    (
        made["space"].as_str().unwrap().to_string(),
        made["terminal"].as_str().unwrap().to_string(),
    )
}

fn finish_terminal(h: &Harness, address: &str) {
    let mut hc = Command::new(h.bin.join("hc"));
    h.seal(&mut hc);
    let out = hc.args(["kill", "--json", address]).output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
}

fn record_start(h: &Harness, space: &str, pane: &str, backend: Backend) {
    let request = start::Request {
        id: "q1".into(),
        requested_at: "2026-10-30T05:00:00Z".into(),
        workspace_id: space.into(),
        pane_id: pane.into(),
        herdr: h.bin.join("herdr"),
        backend,
    };
    start::record(&h.state, &request).unwrap();
}

fn sends_to(h: &Harness, pane: &str) -> usize {
    h.hc_calls()
        .iter()
        .filter(|c| c["argv"][0] == "send")
        .filter(|c| {
            c["argv"]
                .as_array()
                .is_some_and(|a| a.iter().any(|w| w == pane))
        })
        .count()
}

fn hc_telemetry_spaces(h: &Harness) -> Vec<String> {
    h.hc_state()["spaces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["name"] == LABEL)
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect()
}

fn collector_info(h: &Harness) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(h.state.join("telemetry/collector.json")).unwrap(),
    )
    .unwrap()
}

struct CollectorGuard(PathBuf);

impl Drop for CollectorGuard {
    fn drop(&mut self) {
        let until = Instant::now() + harness::DEADLINE;
        while self.0.join("telemetry/start.json").exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Some((pid, since)) = live_holder(&self.0) {
            horch_core::procid::signal_same(pid, since, libc::SIGTERM);
        }
    }
}

fn live_holder(state: &std::path::Path) -> Option<(u32, u64)> {
    let text = std::fs::read_to_string(state.join("telemetry/collector.json")).ok()?;
    let record: Value = serde_json::from_str(&text).ok()?;
    let pid = u32::try_from(record["pid"].as_u64()?).ok()?;
    Some((pid, record["pid_start"].as_u64()?))
}

fn wait_started(h: &Harness, old_pid: u32) {
    let request = h.state.join("telemetry/start.json");
    harness::wait_for_or(
        "the new collector to hold the lock",
        || {
            let (pid, since) = live_holder(&h.state)?;
            let live = pid != old_pid && horch_core::procid::alive(pid, Some(since));
            (live && !request.exists()).then_some(())
        },
        || format!("start.json: {:?}", std::fs::read_to_string(&request).ok()),
    );
}

struct OldCollector {
    pid: u32,
    stop: Arc<AtomicBool>,
    reaper: Option<JoinHandle<()>>,
}

impl OldCollector {
    fn hold_lock(h: &Harness, space: &str, pane: &str) -> Self {
        let mut child = Command::new("/bin/sleep").arg("120").spawn().unwrap();
        let pid = child.id();
        let since = horch_core::procid::start_time(pid).unwrap();
        std::fs::create_dir_all(h.state.join("telemetry/collector.lock")).unwrap();
        let record = json!({
            "pid": pid, "started_at": "2026-10-30T05:00:00Z", "host": "test",
            "workspace_id": space, "pane_id": pane, "pid_start": since,
            "exe": {"path": "/old/horch", "len": 1, "mtime_ns": 1},
        });
        std::fs::write(h.state.join("telemetry/collector.json"), record.to_string()).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let reaper = std::thread::spawn(move || loop {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            if flag.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        });
        Self {
            pid,
            stop,
            reaper: Some(reaper),
        }
    }
}

impl Drop for OldCollector {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(reaper) = self.reaper.take() {
            let _ = reaper.join();
        }
    }
}

#[test]
fn a_replaced_hc_collector_restarts_in_a_live_pane() {
    let h = harness("hcrestart-replace");
    let (old_space, old_pane) = seed_hc_space(&h);
    let old = OldCollector::hold_lock(&h, &old_space, &old_pane);
    let _guard = CollectorGuard(h.state.clone());

    let out = ensure_on_hc(&h);
    assert!(out.status.success(), "{}", text(&out));
    wait_started(&h, old.pid);

    assert_eq!(sends_to(&h, &old_pane), 0, "{:?}", h.hc_calls());
    let info = collector_info(&h);
    assert_ne!(info["pane_id"].as_str(), Some(old_pane.as_str()));
    let spaces = hc_telemetry_spaces(&h);
    assert_eq!(spaces.len(), 1, "{spaces:?}");
    assert_ne!(spaces[0], old_space);
}

#[test]
fn a_start_in_a_finished_hc_pane_is_retried_at_once() {
    let h = harness("hcrestart-finished");
    let (space, pane) = seed_hc_space(&h);
    finish_terminal(&h, &pane);
    record_start(&h, &space, &pane, Backend::Hc);
    let _guard = CollectorGuard(h.state.clone());

    let out = ensure_on_hc(&h);
    assert!(out.status.success(), "{}", text(&out));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&format!("pane {pane} finished")),
        "{}",
        text(&out)
    );
    assert!(err.contains("starting it again"), "{}", text(&out));
    wait_started(&h, 0);

    assert_eq!(sends_to(&h, &pane), 0, "{:?}", h.hc_calls());
    let info = collector_info(&h);
    assert_ne!(info["pane_id"].as_str(), Some(pane.as_str()));
    assert_eq!(hc_telemetry_spaces(&h).len(), 1);
}

#[test]
fn a_start_in_a_live_hc_pane_keeps_waiting() {
    let h = harness("hcrestart-wait");
    let (space, pane) = seed_hc_space(&h);
    record_start(&h, &space, &pane, Backend::Hc);

    let out = ensure_on_hc(&h);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("telemetry collector is starting"),
        "{}",
        text(&out)
    );
    assert_eq!(sends_to(&h, &pane), 0, "{:?}", h.hc_calls());
    assert!(start::request_path(&h.state).is_file());
    assert_eq!(hc_telemetry_spaces(&h), [space]);
}

#[test]
fn a_replaced_herdr_collector_still_reuses_its_pane() {
    let h = harness("hcrestart-herdr");
    let mut path = h.log.clone().into_os_string();
    path.push(".state.json");
    let state = json!({
        "workspaces": [{
            "workspace_id": "w1", "label": LABEL, "active_tab_id": "w1:t1",
            "panes": [{"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1"}],
            "next_pane": 2, "focused": false,
        }],
        "next": 2,
    });
    std::fs::write(PathBuf::from(path), state.to_string()).unwrap();
    let old = OldCollector::hold_lock(&h, "w1", "w1:p1");
    let _guard = CollectorGuard(h.state.clone());

    let out = h.run(&["telemetry", "ensure"]);
    assert!(out.status.success(), "{}", text(&out));
    wait_started(&h, old.pid);

    assert_eq!(collector_info(&h)["pane_id"], "w1:p1");
}
