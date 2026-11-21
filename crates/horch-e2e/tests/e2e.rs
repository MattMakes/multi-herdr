use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use horch_e2e::harness::{self, copy_tree, fixtures, Harness};
use serde_json::Value;

const NOW: &str = "2026-09-28T18:00:00Z";

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| {
        panic!(
            "not JSON ({e}):\nstdout: {}\nstderr: {}",
            stdout(o),
            stderr(o)
        )
    })
}

fn corpus(name: &str) -> Harness {
    let mut h = Harness::new(name);
    let f = fixtures();
    copy_tree(&f.join("claude"), &h.home.join(".claude/projects"));
    copy_tree(&f.join("codex/sessions"), &h.home.join(".codex/sessions"));
    copy_tree(&f.join("pi/sessions"), &h.home.join(".pi/agent/sessions"));
    copy_tree(&f.join("prime"), &h.state.join("prime"));
    for ledger in ["-work-alpha.json", "-work-beta.json"] {
        let text = std::fs::read_to_string(f.join("ledgers").join(ledger)).unwrap();
        let text = text.replace("{STATE}", &h.state.to_string_lossy());
        std::fs::write(h.state.join(ledger), text).unwrap();
    }
    if h.has_sqlite3() {
        let db = h.home.join(".local/share/opencode/opencode.db");
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        let sql = std::fs::read_to_string(f.join("opencode/opencode.sql")).unwrap();
        let mut child = std::process::Command::new(h.bin.join("sqlite3"))
            .arg(&db)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(sql.as_bytes())
            .unwrap();
        let ok = child.wait().unwrap();
        assert!(ok.success());
    }
    h.set("HORCH_NOW", NOW);
    h.set(
        "HORCH_PROBE_TIMEOUT_MS",
        horch_e2e::harness::DEADLINE.as_millis().to_string(),
    );
    h
}

fn warm_quota_fakes(h: &Harness) {
    for fake in ["claude", "codex", "pi", "ollama"] {
        assert!(std::process::Command::new(h.bin.join(fake))
            .arg("warm")
            .status()
            .unwrap()
            .success());
    }
}

fn quota_fixture(name: &str) -> String {
    fixtures()
        .join("quota")
        .join(format!("{name}.json"))
        .to_string_lossy()
        .into_owned()
}

#[cfg(unix)]
fn stop_collector_in(state: &Path) {
    let info = state.join("telemetry/collector.json");
    let record = std::fs::read_to_string(&info)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    let pid = record["pid"].as_u64().and_then(|p| u32::try_from(p).ok());
    let start = record["pid_start"].as_u64();
    if let (Some(pid), Some(start)) = (pid, start) {
        if !horch_core::procid::is_same(pid, start) {
            return;
        }
        let _ = std::process::Command::new("/bin/kill")
            .arg(pid.to_string())
            .status();
        let until = Instant::now() + Duration::from_secs(15);
        while info.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(unix)]
struct CollectorGuard(PathBuf);

#[cfg(unix)]
impl Drop for CollectorGuard {
    fn drop(&mut self) {
        let until = Instant::now() + harness::DEADLINE;
        while self.0.join("telemetry/start.json").exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
        }
        stop_collector_in(&self.0);
    }
}

#[cfg(unix)]
fn wait_started(h: &Harness) {
    let start = h.state.join("telemetry/start.json");
    let info = h.state.join("telemetry/collector.json");
    harness::wait_for_or(
        "the started collector to hold the lock",
        || {
            let record: Value = serde_json::from_str(&std::fs::read_to_string(&info).ok()?).ok()?;
            let pid = u32::try_from(record["pid"].as_u64()?).ok()?;
            let live = horch_core::procid::alive(pid, record["pid_start"].as_u64());
            (live && !start.exists()).then_some(())
        },
        || {
            format!(
                "start.json: {:?}\nstarting.json: {:?}",
                std::fs::read_to_string(&start).ok(),
                std::fs::read_to_string(h.state.join("telemetry/starting.json")).ok()
            )
        },
    );
}

fn hold_lock(h: &Harness) {
    let identity = horch_core::exeid::exe_identity(&Harness::horch_bin())
        .expect("the horch binary under test has an identity");
    let mut record = serde_json::json!({
        "pid": std::process::id(),
        "started_at": NOW,
        "host": "test",
        "exe": {
            "path": identity.path,
            "len": identity.len,
            "mtime_ns": identity.mtime_ns,
        },
    });
    if let Some(start) = horch_core::procid::start_time(std::process::id()) {
        record["pid_start"] = start.into();
    }
    write_lock(h, &record.to_string());
}

fn hold_old_lock(h: &Harness) {
    write_lock(
        h,
        &format!(
            r#"{{"pid":{},"started_at":"{NOW}","host":"test"}}"#,
            std::process::id()
        ),
    );
}

fn write_lock(h: &Harness, record: &str) {
    std::fs::create_dir_all(h.state.join("telemetry/collector.lock")).unwrap();
    std::fs::write(h.state.join("telemetry/collector.json"), record).unwrap();
}

fn probes_of(h: &Harness, fake: &str, marker: &str) -> Vec<Value> {
    h.calls_of(fake)
        .into_iter()
        .filter(|c| {
            c["argv"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x == marker))
        })
        .collect()
}

#[cfg(unix)]
#[test]
fn nfr_01_no_real_binaries() {
    let h = Harness::new("nfr01");
    for name in ["claude", "codex", "herdr", "pi", "ollama"] {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.arg("-c").arg(format!("command -v {name}"));
        h.seal(&mut cmd);
        let out = cmd.output().unwrap();
        let found = stdout(&out).trim().to_string();
        assert!(
            Path::new(&found).starts_with(&h.bin),
            "{name} resolves to {found:?}, outside the fakes dir"
        );
    }
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.arg("-c").arg("/usr/bin/env");
    h.seal(&mut cmd);
    let env = stdout(&cmd.output().unwrap());
    for key in [
        "HORCH_CLAUDE_BIN",
        "HORCH_CODEX_BIN",
        "HORCH_HERDR_BIN",
        "HORCH_PI_BIN",
        "HORCH_OLLAMA_BIN",
    ] {
        let line = env
            .lines()
            .find(|l| l.starts_with(&format!("{key}=")))
            .unwrap();
        assert!(line.contains(&*h.bin.to_string_lossy()), "{line}");
    }
    assert!(!env.lines().any(|l| l.starts_with("ANTHROPIC_API_KEY=")));
}

#[test]
fn tel_10_usage_equals_cost() {
    let mut h = corpus("tel10");
    for project in ["/work/alpha", "/work/beta"] {
        h.set("HORCH_PROJECT_DIR", project);
        let usage = json(&h.run(&["usage", "--json", "--project", project]));
        let cost = json(&h.run(&["cost", "--json", "--since", "2000-01-01"]));
        let rows = cost["rows"].as_array().unwrap();
        assert!(!rows.is_empty(), "{project}");
        for row in rows {
            let id = row["record_id"].as_str().unwrap();
            let rec = usage["records"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["record_id"] == id)
                .unwrap_or_else(|| panic!("{id} missing from horch usage"));
            for class in [
                "input",
                "cache_write_5m",
                "cache_write_1h",
                "cache_read",
                "output",
            ] {
                assert_eq!(rec["tokens"][class], row["tokens"][class], "{id} {class}");
            }
            let (a, b) = (
                rec["cost_usd"].as_f64().unwrap(),
                row["cost"].as_f64().unwrap(),
            );
            assert!((a - b).abs() < 1e-6, "{id}: usage {a} vs cost {b}");
        }
    }
}

#[test]
fn tel_11_no_content_or_identity_persisted() {
    let mut h = corpus("tel11");
    h.set("HORCH_FAKE_SCENARIO", "usage_exhausted");
    assert!(h.run(&["usage", "--json"]).status.success());
    assert!(h.run(&["quota", "--refresh", "--json"]).status.success());
    assert!(h.run(&["telemetry", "collect", "--once"]).status.success());
    let files: Vec<PathBuf> = h
        .state_files()
        .into_iter()
        .filter(|p| p.starts_with(h.state.join("telemetry")))
        .collect();
    assert!(files.iter().any(|p| p.ends_with("quota.json")));
    for p in files {
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        for s in [
            "SENTINEL-CONTENT",
            "sentinel@example.invalid",
            "acct_",
            "user.email",
            "You've hit",
        ] {
            assert!(!text.contains(s), "{s} in {}", p.display());
        }
    }
}

#[test]
fn quo_01_claude_probe_protocol() {
    let mut h = corpus("quo01");
    h.set("HORCH_FAKE_SCENARIO", "usage_exhausted");
    h.set("ANTHROPIC_API_KEY", "dummy");
    warm_quota_fakes(&h);
    let out = h.run(&["quota", "--refresh", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let probes = probes_of(&h, "claude", "stream-json");
    assert_eq!(probes.len(), 1, "one probe process");
    let p = &probes[0];
    let stdin = p["stdin"].as_array().unwrap();
    assert_eq!(stdin.len(), 1, "exactly one stdin line: {stdin:?}");
    let line: Value = serde_json::from_str(stdin[0].as_str().unwrap()).unwrap();
    assert_eq!(line["type"], "control_request");
    assert_eq!(line["request"]["subtype"], "get_usage");
    assert!(
        p["violations"].as_array().unwrap().is_empty(),
        "{:?}",
        p["violations"]
    );
    assert!(!p["env_keys"]
        .as_array()
        .unwrap()
        .iter()
        .any(|k| k == "ANTHROPIC_API_KEY"));
    let argv: Vec<String> = serde_json::from_value(p["argv"].clone()).unwrap();
    assert!(
        argv.windows(2)
            .any(|w| w[0] == "--model" && w[1] == "haiku"),
        "{argv:?}"
    );
    let q = json(&out);
    assert_eq!(q["pools"]["claude"]["state"], "exhausted");
    let seven = q["pools"]["claude"]["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["name"] == "7d" && w["scope_model"].is_null())
        .unwrap();
    assert_eq!(seven["used"].as_f64(), Some(1.0));
    assert_eq!(seven["resets_at"], "2026-10-02T13:59:59Z");
}

#[test]
fn quo_02_codex_probe_protocol() {
    let mut h = corpus("quo02");
    h.set("HORCH_FAKE_SCENARIO", "limits_5h_weekly");
    warm_quota_fakes(&h);
    let out = h.run(&["quota", "--refresh", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let probes = probes_of(&h, "codex", "app-server");
    assert_eq!(probes.len(), 1);
    let methods: Vec<String> = probes[0]["stdin"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            serde_json::from_str::<Value>(l.as_str().unwrap()).unwrap()["method"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(
        methods,
        ["initialize", "initialized", "account/rateLimits/read"]
    );
    assert!(probes[0]["violations"].as_array().unwrap().is_empty());
    let q = json(&out);
    let names: Vec<&str> = q["pools"]["codex"]["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["5h", "7d"]);
}

#[test]
fn quo_google_probe_reads_agy_usage() {
    let mut h = corpus("quogoogle");
    h.set("HORCH_FAKE_SCENARIO", "agy_argon_exhausted");
    h.set("GEMINI_API_KEY", "dummy");
    warm_quota_fakes(&h);
    assert!(std::process::Command::new(h.bin.join("agy"))
        .arg("--version")
        .status()
        .unwrap()
        .success());
    let out = h.run(&["quota", "--refresh", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let probes = probes_of(&h, "antigravity", "/usage");
    assert_eq!(probes.len(), 1, "one probe process");
    let argv: Vec<String> = serde_json::from_value(probes[0]["argv"].clone()).unwrap();
    assert_eq!(argv[..4], ["-p", "/usage", "--output-format", "json"]);
    assert!(
        !argv
            .iter()
            .any(|a| a == "--prompt-interactive" || a == "--model"),
        "{argv:?}"
    );
    assert!(!probes[0]["env_keys"]
        .as_array()
        .unwrap()
        .iter()
        .any(|k| k == "GEMINI_API_KEY"));
    let q = json(&out);
    let argon = q["pools"]["google"]["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["scope_model"] == "argon")
        .unwrap()
        .clone();
    assert_eq!(argon["used"].as_f64(), Some(1.0));
    assert_eq!(q["pools"]["google"]["source"], "usage");

    let route = h.run(&["route", "spec-writer"]);
    let text = format!("{}{}", stdout(&route), stderr(&route));
    assert!(
        text.contains("spec-writer cannot start. google exhausted (7d:argon 100%"),
        "{text}"
    );
    let route = h.run(&["route", "antigravity"]);
    assert!(
        stdout(&route).contains("SPAWN: antigravity runs as itself"),
        "{}",
        stdout(&route)
    );
}

#[test]
fn quo_03_fallback_on_probe_failure() {
    for scenario in ["hang", "error"] {
        let mut h = corpus(&format!("quo03-{scenario}"));
        h.set("HORCH_FAKE_SCENARIO", scenario);
        if scenario == "hang" {
            h.set("HORCH_PROBE_TIMEOUT_MS", "1500");
        }
        let out = h.run(&["quota", "--refresh", "--json"]);
        assert!(out.status.success(), "{}", stderr(&out));
        let q = json(&out);
        let codex = &q["pools"]["codex"];
        assert_eq!(codex["source"], "rollout", "{scenario}: {codex}");
        assert!(
            codex["error"].as_str().is_some_and(|e| !e.is_empty()),
            "{codex}"
        );
        assert!(!codex["windows"].as_array().unwrap().is_empty());
        assert_ne!(
            q["pools"]["claude"]["state"], "ok",
            "a failed probe never reads ok"
        );
        let text = stdout(&h.run(&["quota"]));
        assert!(text.contains("codex: from a rollout written"), "{text}");
        assert!(text.contains("min old"), "{text}");
    }
}

#[test]
fn quo_06_local_broken() {
    for (scenario, why) in [
        ("crash", "pi --version exited 1"),
        ("model_missing", "not in ollama list"),
    ] {
        let mut h = corpus(&format!("quo06-{scenario}"));
        h.set("HORCH_FAKE_SCENARIO", scenario);
        warm_quota_fakes(&h);
        assert!(h.run(&["quota", "--refresh"]).status.success());
        let out = h.run(&["route", "pi", "--json"]);
        let r = json(&out);
        let local = r["pools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["pool"] == "local")
            .unwrap();
        assert_eq!(local["state"], "broken", "{scenario}: {r}");
        assert!(
            local["detail"].as_str().unwrap().contains(why),
            "{scenario}: {r}"
        );
        assert_eq!(
            out.status.code(),
            Some(3),
            "pi has no fallback, so it is refused"
        );
    }
}

#[test]
fn quota_probe_records_a_crashing_harness() {
    let mut h = corpus("quota-harness-broken");
    h.set("HORCH_FAKE_SCENARIO", "crash");
    warm_quota_fakes(&h);
    let q = json(&h.run(&["quota", "--refresh", "--json"]));
    let why = "pi --version exited 1: pi: Node.js 22.19 or newer is required";
    assert_eq!(q["harnesses"]["pi"]["error"], why, "{q}");
    assert_eq!(q["harnesses"]["claude"]["error"], Value::Null, "{q}");
    assert!(q["harnesses"]["claude"]["version"].is_string(), "{q}");
}

#[test]
fn quo_07_cli_does_not_probe_when_collector_live() {
    let h = corpus("quo07a");
    hold_lock(&h);
    assert!(h.run(&["quota", "--refresh"]).status.success());
    assert!(
        probes_of(&h, "claude", "stream-json").is_empty(),
        "no probe with a live collector"
    );
    assert!(probes_of(&h, "codex", "app-server").is_empty());
}

#[test]
fn quo_07_probe_cadence() {
    let mut h = corpus("quo07b");
    let count = |h: &Harness| probes_of(h, "claude", "stream-json").len();
    h.set("HORCH_NOW", "2026-09-28T18:00:00Z");
    warm_quota_fakes(&h);
    h.run(&["quota", "--refresh"]);
    assert_eq!(count(&h), 1);
    h.set("HORCH_NOW", "2026-09-28T18:05:00Z");
    h.run(&["quota", "--refresh"]);
    assert_eq!(count(&h), 1, "5 min later: still fresh");
    h.set("HORCH_NOW", "2026-09-28T18:11:00Z");
    h.run(&["quota", "--refresh"]);
    assert_eq!(count(&h), 2, "older than probe_on_demand_age_min");
    h.run(&["quota"]);
    assert_eq!(count(&h), 2, "plain `horch quota` never probes");
}

#[cfg(unix)]
#[test]
fn spc_02_viewer_when_locked() {
    let h = corpus("spc02");
    hold_lock(&h);
    let mut child = h
        .horch(&["telemetry"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stderr_text = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    {
        let mut pipe = child.stderr.take().unwrap();
        let text = stderr_text.clone();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = [0u8; 4096];
            while let Ok(n @ 1..) = pipe.read(&mut buf) {
                text.lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        });
    }
    harness::wait_for_or(
        "the read-only viewer line",
        || {
            (stderr_text.lock().unwrap().contains("read-only viewer")
                || child.try_wait().unwrap().is_some())
            .then_some(())
        },
        || stderr_text.lock().unwrap().clone(),
    );
    let text = stderr_text.lock().unwrap().clone();
    assert!(text.contains("read-only viewer"), "{text}");
    let info: Value = serde_json::from_str(
        &std::fs::read_to_string(h.state.join("telemetry/collector.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        info["pid"].as_u64(),
        Some(std::process::id() as u64),
        "the viewer took no lock"
    );
    assert!(
        child.try_wait().unwrap().is_none(),
        "the viewer keeps running"
    );
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(
        h.run(&["telemetry", "collect", "--once"]).status.code(),
        Some(2)
    );
}

#[cfg(unix)]
#[test]
fn spc_03_ensure_argv() {
    let mut h = corpus("spc03");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    let _guard = CollectorGuard(h.state.clone());
    let out = h.run(&["telemetry", "ensure"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    wait_started(&h);
    let calls: Vec<Vec<String>> = h
        .calls_of("herdr")
        .into_iter()
        .map(|c| serde_json::from_value::<Vec<String>>(c["argv"].clone()).unwrap())
        .filter(|a| a[..2] != ["workspace", "list"])
        .collect();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(
        calls[0][..4],
        ["workspace", "create", "--label", "horch telemetry"]
    );
    assert!(calls[0].contains(&"--no-focus".to_string()), "{calls:?}");
    assert_eq!(calls[1][..2], ["pane", "run"]);
    assert!(h.violations().is_empty(), "{:?}", h.violations());
}

#[test]
fn spc_03_ensure_noop_when_live() {
    let h = corpus("spc03b");
    hold_lock(&h);
    let out = h.run(&["telemetry", "ensure"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("telemetry collector is live"),
        "{}",
        stdout(&out)
    );
    assert!(h.calls_of("herdr").is_empty(), "no herdr call at all");
}

#[test]
fn spc_03_ensure_warns_and_succeeds_for_a_collector_it_cannot_stop() {
    let h = corpus("spc03c");
    hold_old_lock(&h);
    let out = h.run(&["telemetry", "ensure"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert!(
        stdout(&out).contains("telemetry collector is live"),
        "{}",
        stdout(&out)
    );
    let pid = std::process::id().to_string();
    let err = stderr(&out);
    assert_eq!(err.matches("did not stop it").count(), 1, "{err}");
    assert!(err.contains(&format!("pid {pid}")), "{err}");
    assert!(
        err.contains(&format!("`ps -p {pid} -o command=`")),
        "the hint names the check: {err}"
    );
    assert!(err.contains("runs `horch telemetry collect`"), "{err}");
    assert!(h.calls_of("herdr").is_empty(), "no herdr call at all");
}

fn seed_workspaces(h: &Harness, workspaces: &[(&str, &str, usize)]) {
    let all: Vec<Value> = workspaces
        .iter()
        .map(|(id, label, panes)| {
            let panes: Vec<Value> = (1..=*panes)
                .map(|n| {
                    serde_json::json!({
                        "pane_id": format!("{id}:p{n}"),
                        "workspace_id": id,
                        "tab_id": format!("{id}:t1"),
                    })
                })
                .collect();
            serde_json::json!({
                "workspace_id": id, "label": label, "active_tab_id": format!("{id}:t1"),
                "panes": panes, "next_pane": panes.len() + 1, "focused": false,
            })
        })
        .collect();
    let mut path = h.log.clone().into_os_string();
    path.push(".state.json");
    let state = serde_json::json!({"workspaces": all, "next": workspaces.len() + 1});
    std::fs::write(PathBuf::from(path), state.to_string()).unwrap();
}

fn workspaces_of(h: &Harness) -> Vec<(String, String)> {
    let mut path = h.log.clone().into_os_string();
    path.push(".state.json");
    let state: Value =
        serde_json::from_str(&std::fs::read_to_string(PathBuf::from(path)).unwrap()).unwrap();
    state["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| {
            (
                w["workspace_id"].as_str().unwrap().to_string(),
                w["label"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[cfg(unix)]
fn hold_dead_lock(h: &Harness, pane: &str) {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    write_lock(
        h,
        &serde_json::json!({
            "pid": pid, "started_at": NOW, "host": "test", "pane_id": pane,
            "pid_start": 1u64,
        })
        .to_string(),
    );
}

fn herdr_violations(h: &Harness) -> Vec<String> {
    let mut all = h.violations();
    all.dedup();
    all
}

#[cfg(unix)]
#[test]
fn spc_03_restart_after_a_dead_collector_leaves_1_telemetry_workspace() {
    let mut h = corpus("spc03d");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    seed_workspaces(
        &h,
        &[
            ("w1", "multi-herdr", 1),
            ("w2", "horch telemetry", 1),
            ("w3", "horch telemetry", 1),
        ],
    );
    hold_dead_lock(&h, "w2:p1");
    let _guard = CollectorGuard(h.state.clone());
    let out = h.run(&["telemetry", "ensure"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    wait_started(&h);
    assert_eq!(
        workspaces_of(&h),
        [
            ("w1".to_string(), "multi-herdr".to_string()),
            ("w2".to_string(), "horch telemetry".to_string()),
        ]
    );
    let info: Value = serde_json::from_str(
        &std::fs::read_to_string(h.state.join("telemetry/collector.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(info["pane_id"], "w2:p1", "the old workspace is reused");
    assert_eq!(
        herdr_violations(&h),
        ["herdr: mutating call: herdr workspace close w3"]
    );
}

#[cfg(unix)]
#[test]
fn spc_03_restart_closes_an_old_workspace_it_cannot_reuse() {
    let mut h = corpus("spc03e");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    seed_workspaces(&h, &[("w1", "horch telemetry", 2), ("w2", "telemetry", 1)]);
    let _guard = CollectorGuard(h.state.clone());
    let out = h.run(&["telemetry", "ensure"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    wait_started(&h);
    assert_eq!(
        workspaces_of(&h),
        [
            ("w2".to_string(), "telemetry".to_string()),
            ("w3".to_string(), "horch telemetry".to_string()),
        ]
    );
    assert_eq!(
        herdr_violations(&h),
        ["herdr: mutating call: herdr workspace close w1"]
    );
}

#[test]
fn spc_03_a_live_collector_keeps_every_workspace() {
    let h = corpus("spc03f");
    seed_workspaces(
        &h,
        &[("w1", "horch telemetry", 1), ("w2", "horch telemetry", 1)],
    );
    hold_lock(&h);
    let out = h.run(&["telemetry", "ensure"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(workspaces_of(&h).len(), 2);
    assert!(h.calls_of("herdr").is_empty(), "no herdr call at all");
}

#[test]
fn spc_05_usage_json_schema() {
    let h = corpus("spc05u");
    let u = json(&h.run(&["usage", "--json", "--by", "plan", "--window", "7d"]));
    for key in [
        "generated_at",
        "since",
        "by",
        "rows",
        "records",
        "total",
        "live",
        "insights",
        "unread",
    ] {
        assert!(u.get(key).is_some(), "usage --json lacks {key}");
    }
    let row = &u["rows"][0];
    for key in [
        "key",
        "tokens",
        "cost_usd",
        "records",
        "done",
        "cache_hit",
        "cost_per_done",
    ] {
        assert!(row.get(key).is_some(), "rollup row lacks {key}");
    }
    for class in [
        "input",
        "cache_write_5m",
        "cache_write_1h",
        "cache_read",
        "output",
        "reasoning",
    ] {
        assert!(row["tokens"][class].is_u64(), "{class}");
    }
    assert!(u["live"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["kind"] == "orchestrator"));
    let snap: Value = serde_json::from_str(
        &std::fs::read_to_string(h.state.join("telemetry/snapshot.json")).unwrap(),
    )
    .unwrap();
    for key in [
        "schema",
        "generated_at",
        "collector",
        "pools",
        "live",
        "rollups",
        "insights",
        "unread",
    ] {
        assert!(snap.get(key).is_some(), "snapshot lacks {key}");
    }
    for w in ["5h", "today", "7d"] {
        for g in [
            "by_teammate",
            "by_phase",
            "by_agent",
            "by_project",
            "by_plan",
            "by_kind",
        ] {
            assert!(snap["rollups"][w][g].is_array(), "{w} {g}");
        }
    }
}

#[test]
fn spc_05_quota_json_schema() {
    let mut h = corpus("spc05q");
    h.set("HORCH_QUOTA_FILE", quota_fixture("all-exhausted"));
    let q = json(&h.run(&["quota", "--json"]));
    assert_eq!(q["schema"], 1);
    for pool in ["claude", "codex", "opencode-zen", "local"] {
        assert!(q["pools"][pool]["state"].is_string(), "{pool}");
    }
    let c = &q["pools"]["claude"];
    for key in [
        "state",
        "reason",
        "observed_at",
        "source",
        "harness_version",
        "windows",
        "refusal_seen_at",
        "error",
    ] {
        assert!(c.get(key).is_some(), "claude lacks {key}");
    }
    for key in ["name", "minutes", "scope_model", "used", "resets_at"] {
        assert!(c["windows"][0].get(key).is_some(), "window lacks {key}");
    }
    assert_eq!(q["pools"]["local"]["state"], "broken");
    assert_eq!(q["pools"]["codex"]["ordinary_usage_allowed"], true);
}

#[test]
fn spc_06_fleet_survives_ensure_failure() {
    let mut h = Harness::new("spc06");
    h.set("HORCH_FAKE_FAIL_LABEL", "horch telemetry");
    let project = h.project.to_string_lossy().into_owned();
    let out = h.run(&["fleet", "--cwd", &project]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    let warnings: Vec<&str> = stdout(&out)
        .lines()
        .filter(|l| l.starts_with("warning: telemetry"))
        .map(|_| "w")
        .collect();
    assert_eq!(warnings.len(), 1, "{}", stdout(&out));
}

fn gated(name: &str, fixture: &str) -> Harness {
    let mut h = corpus(name);
    h.set("HORCH_QUOTA_FILE", quota_fixture(fixture));
    h.set("HORCH_PROJECT_DIR", "/work/alpha");
    h.set("HORCH_WORKSPACE_ID", "w1");
    h
}

#[test]
fn bal_04_exit_code_3() {
    let h = gated("bal04b", "all-exhausted");
    assert_eq!(h.run(&["route", "opus"]).status.code(), Some(3));
    let mut forced = gated("bal04c", "all-exhausted");
    forced.set("HORCH_BALANCE", "advise");
    assert_eq!(
        forced.run(&["route", "opus"]).status.code(),
        Some(0),
        "advise never refuses"
    );
}

#[test]
fn bal_06_route_matches_gate() {
    for fixture in [
        "all-ok",
        "all-exhausted",
        "claude-exhausted-codex-ok",
        "claude-tight-codex-ok",
        "claude-tight-codex-close",
        "claude-unknown-codex-ok",
        "claude-refused",
    ] {
        for teammate in ["opus", "sonnet", "codex-sol"] {
            let h = gated(&format!("bal06-{fixture}-{teammate}"), fixture);
            let route = h.run(&["route", teammate, "--json"]);
            let r = json(&route);
            let line = r["line"].as_str();
            if let Some(l) = line {
                assert!(
                    l.starts_with("NOTE:")
                        || l.starts_with("SUBSTITUTED:")
                        || l.starts_with("REFUSED:"),
                    "{fixture} {teammate}: {l}"
                );
            }
            let refused = line.is_some_and(|l| l.starts_with("REFUSED:"));
            assert_eq!(
                route.status.code() == Some(3),
                refused,
                "{fixture} {teammate}: exit code 3 means REFUSED"
            );
            assert_eq!(
                r["decision"] == "refuse",
                refused,
                "{fixture} {teammate}: {r}"
            );
        }
    }
}
