//! End-to-end: the real `horch` binary, a sealed environment, fake harnesses.
//! Every expected number comes from the fixture oracle,
//! `crates/horch-core/tests/fixtures/telemetry/EXPECTED.md`.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use horch_e2e::harness::{copy_tree, fixtures, Harness};
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

/// A harness with the fixture corpus installed and the clock pinned.
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
        // The SQL goes in on stdin: it starts with `--`, which sqlite3
        // reads as an option when it is an argument.
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
    h.set("HORCH_PROBE_TIMEOUT_MS", "1500");
    h
}

fn quota_fixture(name: &str) -> String {
    fixtures()
        .join("quota")
        .join(format!("{name}.json"))
        .to_string_lossy()
        .into_owned()
}

fn ledger_path(h: &Harness, slug: &str) -> PathBuf {
    h.state.join(format!("{slug}.json"))
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

/// Stop the collector a test started, by the pid in `collector.json`, and
/// only while that pid still has the recorded start time: a collector that
/// has ended leaves its file, and its pid can name another program.
/// Never panics, so it is safe to call from `Drop` while a test unwinds.
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
fn stop_collector(h: &Harness) {
    stop_collector_in(&h.state);
}

/// Stops the collector when dropped, so a failing assert cannot leak it.
#[cfg(unix)]
struct CollectorGuard(PathBuf);

#[cfg(unix)]
impl Drop for CollectorGuard {
    fn drop(&mut self) {
        stop_collector_in(&self.0);
    }
}

/// Hold the collector lock as if a live collector (this test process) ran.
fn hold_lock(h: &Harness) {
    let dir = h.state.join("telemetry/collector.lock");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        h.state.join("telemetry/collector.json"),
        format!(
            r#"{{"pid":{},"started_at":"{NOW}","host":"test"}}"#,
            std::process::id()
        ),
    )
    .unwrap();
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

// ─── NFR-01 ─────────────────────────────────────────────────────────────────

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
    // Every binary horch is told to use is a fake.
    let mut cmd = std::process::Command::new("/bin/sh");
    // By absolute path: the sealed PATH holds only the fakes.
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

// ─── TEL ────────────────────────────────────────────────────────────────────

#[test]
fn tel_10_usage_equals_cost() {
    let mut h = corpus("tel10");
    for project in ["/work/alpha", "/work/beta"] {
        h.set("HORCH_PROJECT_DIR", project);
        let usage = json(&h.run(&["usage", "--json", "--project", project]));
        let cost = json(&h.run(&["cost", "--json"]));
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

#[cfg(unix)]
#[test]
fn tel_02_fleet_writes_orchestrator_record() {
    let mut h = Harness::new("tel02");
    let _collector = CollectorGuard(h.state.clone());
    h.set("HORCH_FAKE_SCENARIO", "exec");
    let project = h.project.to_string_lossy().into_owned();
    let out = h.run(&["fleet", "--cwd", &project]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    // The pane runs `horch pane-launch`, which starts (fake) claude.
    wait_for("the orchestrator's claude", || {
        h.calls_of("claude").iter().any(|c| {
            c["argv"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x == "--session-id"))
        })
    });
    let claude = h
        .calls_of("claude")
        .into_iter()
        .find(|c| {
            c["argv"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x == "--session-id"))
        })
        .unwrap();
    let argv: Vec<String> = serde_json::from_value(claude["argv"].clone()).unwrap();
    let sid = &argv[argv.iter().position(|a| a == "--session-id").unwrap() + 1];
    let slug: String = project
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect();
    // The ledger is replaced atomically, so a read can see no file or a
    // partial one. Retry the whole read and parse on every poll.
    let mut orch = Value::Null;
    wait_for("the orchestrator record in the ledger", || {
        let found = std::fs::read_to_string(ledger_path(&h, &slug))
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
            .and_then(|rs| rs.into_iter().find(|r| r["kind"] == "orchestrator"));
        found.map(|r| orch = r).is_some()
    });
    assert_eq!(orch["session_id"].as_str(), Some(sid.as_str()));
    assert_eq!(orch["role"], "orchestrator");
    assert_eq!(orch["task"], "(orchestrating)");
}

/// `horch fleet` records the orchestrator's plugin skills as the worker
/// spawn does: `<plugin>:<skill>` refs next to the catalog skills, so the
/// launch's skill check (SKL-04) accepts the record and the orchestrator
/// starts. The orchestrator here is the built-in one plus a plugin `code`
/// with skills `review` and `lint`, of which it names `review`.
#[cfg(unix)]
#[test]
fn tel_02_fleet_records_the_orchestrators_plugin_skills() {
    let mut h = Harness::new("tel02p");
    let _collector = CollectorGuard(h.state.clone());
    h.set("HORCH_FAKE_SCENARIO", "exec");
    let root = h.home.join("plugins/code");
    std::fs::create_dir_all(root.join(".claude-plugin")).unwrap();
    std::fs::write(
        root.join(".claude-plugin/plugin.json"),
        r#"{"name":"code","version":"1.2.0"}"#,
    )
    .unwrap();
    for skill in ["review", "lint"] {
        let dir = root.join("skills").join(skill);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {skill}\ndescription: Plugin {skill}.\n---\nbody\n"),
        )
        .unwrap();
    }
    // `$HORCH_TEAMMATES_DIR` (the repo's folder in the harness) wins over
    // `~/.config`, so the test points it at an edited copy.
    let teammates = h.root.join("teammates");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates"),
        &teammates,
    );
    let builtin = std::fs::read_to_string(teammates.join("orchestrator.md")).unwrap();
    assert!(builtin.contains("\nagent: claude\n"));
    let overlay = builtin.replacen(
        "\nagent: claude\n",
        "\nagent: claude\nplugin_dirs: [~/plugins/code]\nplugin_skills:\n  code: [review]\n",
        1,
    );
    std::fs::write(teammates.join("orchestrator.md"), overlay).unwrap();
    h.set("HORCH_TEAMMATES_DIR", teammates.to_string_lossy());

    let project = h.project.to_string_lossy().into_owned();
    let out = h.run(&["fleet", "--cwd", &project]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    // The launch accepted the record: the (fake) claude started.
    wait_for("the orchestrator's claude", || {
        h.calls_of("claude").iter().any(|c| {
            c["argv"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x == "--session-id"))
        })
    });
    let slug: String = project
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect();
    let mut orch = Value::Null;
    wait_for("the orchestrator record in the ledger", || {
        let found = std::fs::read_to_string(ledger_path(&h, &slug))
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
            .and_then(|rs| rs.into_iter().find(|r| r["kind"] == "orchestrator"));
        found.map(|r| orch = r).is_some()
    });
    let skills = orch["skills"].as_array().unwrap();
    let ids: Vec<&str> = skills.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"code:review"), "{ids:?}");
    assert!(ids.contains(&"orchestrate"), "{ids:?}");
    assert!(!ids.contains(&"code:lint"), "{ids:?}");
    let review = skills.iter().find(|s| s["id"] == "code:review").unwrap();
    assert_eq!(review["source"], "plugin:code@inline", "{review}");
    assert!(
        review["version"].as_str().unwrap().starts_with("1.2.0+"),
        "{review}"
    );
}

#[test]
fn tel_02_resume_refuses_orchestrator() {
    let mut h = corpus("tel02r");
    h.set("HORCH_PROJECT_DIR", "/work/alpha");
    h.set("HORCH_WORKSPACE_ID", "w1");
    let out = h.run(&["spawn", "--resume", "rec-o1", "continue"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("record rec-o1 is an orchestrator; restart it with horch fleet"),
        "{}",
        stderr(&out)
    );
}

// ─── QUO ────────────────────────────────────────────────────────────────────

#[test]
fn quo_01_claude_probe_protocol() {
    let mut h = corpus("quo01");
    h.set("HORCH_FAKE_SCENARIO", "usage_exhausted");
    h.set("ANTHROPIC_API_KEY", "dummy");
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
fn quo_03_fallback_on_probe_failure() {
    for scenario in ["hang", "error"] {
        let mut h = corpus(&format!("quo03-{scenario}"));
        h.set("HORCH_FAKE_SCENARIO", scenario);
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
        // The text form shows the fallback's age.
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

/// The quota probe records a harness whose `--version` crashes, by harness,
/// with its error line; the others stay healthy.
#[test]
fn quota_probe_records_a_crashing_harness() {
    let mut h = corpus("quota-harness-broken");
    h.set("HORCH_FAKE_SCENARIO", "crash");
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

// ─── SPC ────────────────────────────────────────────────────────────────────

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
    std::thread::sleep(Duration::from_millis(1500));
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
    let out = child.wait_with_output().unwrap();
    assert!(
        stderr(&out).contains("read-only viewer"),
        "{}",
        stderr(&out)
    );
    // And a second one-shot collector is refused.
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
    let out = h.run(&["telemetry", "ensure"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    let calls: Vec<Vec<String>> = h
        .calls_of("herdr")
        .into_iter()
        .map(|c| serde_json::from_value(c["argv"].clone()).unwrap())
        .collect();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(
        calls[0][..4],
        ["workspace", "create", "--label", "horch telemetry"]
    );
    assert!(calls[0].contains(&"--no-focus".to_string()), "{calls:?}");
    assert_eq!(calls[1][..2], ["pane", "run"]);
    assert!(h.violations().is_empty(), "{:?}", h.violations());
    stop_collector(&h);
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

// ─── BAL ────────────────────────────────────────────────────────────────────

fn gated(name: &str, fixture: &str) -> Harness {
    let mut h = corpus(name);
    h.set("HORCH_QUOTA_FILE", quota_fixture(fixture));
    h.set("HORCH_PROJECT_DIR", "/work/alpha");
    h.set("HORCH_WORKSPACE_ID", "w1");
    h
}

#[test]
fn bal_04_gate_before_side_effects() {
    let h = gated("bal04", "all-exhausted");
    let ledger = ledger_path(&h, "-work-alpha");
    let before = std::fs::read(&ledger).unwrap();
    let out = h.run(&["spawn", "opus", "x", "--from-pane", "w1:p1"]);
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    assert!(
        stdout(&out).starts_with("REFUSED: opus cannot start."),
        "{}",
        stdout(&out)
    );
    assert_eq!(
        std::fs::read(&ledger).unwrap(),
        before,
        "ledger byte-identical"
    );
    assert!(
        !h.calls_of("herdr").iter().any(|c| c["argv"][1] == "split"),
        "no pane split"
    );
    assert!(!h
        .tmp
        .join("herdr-orchestration-w1")
        .join("opus-1.brief.json")
        .exists());
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
    let out = h.run(&[
        "spawn",
        "opus",
        "x",
        "--from-pane",
        "w1:p1",
        "--force",
        "--no-tile",
    ]);
    assert!(out.status.success(), "--force spawns: {}", stderr(&out));
    assert!(
        stdout(&out).starts_with("NOTE: claude pool exhausted"),
        "{}",
        stdout(&out)
    );
}

fn records(h: &Harness) -> Vec<Value> {
    serde_json::from_str(&std::fs::read_to_string(ledger_path(h, "-work-alpha")).unwrap()).unwrap()
}

#[test]
fn bal_05_substituted_record_fields() {
    let h = gated("bal05", "claude-exhausted-codex-ok");
    let out = h.run(&[
        "spawn",
        "researcher",
        "look into ai_docs/plans/p.md",
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    let first = stdout(&out).lines().next().unwrap().to_string();
    assert_eq!(
        first,
        "SUBSTITUTED: researcher runs on codex-sol. Reason: claude 7d 100%, resets 2026-10-02T14:00Z."
    );
    let r = records(&h)
        .into_iter()
        .find(|r| r["tier"] == "researcher")
        .unwrap();
    assert_eq!(r["agent"], "codex");
    assert_eq!(r["model"], "gpt-5.6-sol");
    assert_eq!(r["via"], "codex-sol");
    assert_eq!(
        r["substitution_reason"],
        "claude 7d 100%, resets 2026-10-02T14:00Z"
    );
    assert_eq!(
        r["role"], "researcher-1",
        "the role keeps the original's name"
    );
    assert_eq!(r["plan"], "p");
    let brief: Value = serde_json::from_str(
        &std::fs::read_to_string(h.tmp.join("herdr-orchestration-w1/researcher-1.brief.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(brief["resolved"]["agent"], "codex");
    assert_eq!(
        brief["resolved"]["phase"], "research",
        "the persona's phase stays"
    );
}

#[test]
fn bal_05_resume_uses_via() {
    let mut h = gated("bal05r", "claude-exhausted-codex-ok");
    assert!(h
        .run(&[
            "spawn",
            "researcher",
            "x",
            "--from-pane",
            "w1:p1",
            "--no-tile"
        ])
        .status
        .success());
    let r = records(&h)
        .into_iter()
        .find(|r| r["tier"] == "researcher")
        .unwrap();
    let id = r["record_id"].as_str().unwrap().to_string();
    // The session finishes, and gets a session id as a harvest would.
    assert!(h
        .run(&[
            "ledger",
            "set-session",
            &id,
            "01d0e61c-d73e-74a3-837c-b5aade8b1c38"
        ])
        .status
        .success());
    assert!(h.run(&["ledger", "done", &id, "finished"]).status.success());
    // Now claude is fine again: a resume still runs on codex, ungated.
    h.set("HORCH_QUOTA_FILE", quota_fixture("all-ok"));
    let out = h.run(&[
        "spawn",
        "--resume",
        &id,
        "more",
        "--from-pane",
        "w1:p1",
        "--no-tile",
        "--role",
        "researcher-9",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        !stdout(&out).contains("SUBSTITUTED"),
        "a resume is not gated"
    );
    let brief: Value = serde_json::from_str(
        &std::fs::read_to_string(h.tmp.join("herdr-orchestration-w1/researcher-9.brief.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(brief["resolved"]["agent"], "codex");
    assert_eq!(brief["resume"], true);
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
            let spawn = h.run(&["spawn", teammate, "x", "--from-pane", "w1:p1", "--no-tile"]);
            let gate_line = stdout(&spawn)
                .lines()
                .next()
                .filter(|l| {
                    l.starts_with("NOTE:")
                        || l.starts_with("SUBSTITUTED:")
                        || l.starts_with("REFUSED:")
                })
                .map(str::to_owned);
            assert_eq!(
                r["line"].as_str().map(str::to_owned),
                gate_line,
                "{fixture} {teammate}"
            );
            assert_eq!(
                route.status.code() == Some(3),
                spawn.status.code() == Some(3),
                "{fixture} {teammate}"
            );
        }
    }
}

#[test]
fn bal_08_fleet_auto_choice() {
    for (fixture, flavor, kind) in [
        ("all-ok", "opus", "fleet-orchestrator"),
        (
            "claude-exhausted-codex-ok",
            "sol",
            "fleet-codex-orchestrator",
        ),
        ("claude-unknown-codex-ok", "sol", "fleet-codex-orchestrator"),
        ("all-exhausted", "opus", "fleet-orchestrator"),
    ] {
        let mut h = Harness::new(&format!("bal08-{fixture}"));
        h.set("HORCH_QUOTA_FILE", quota_fixture(fixture));
        h.set("HORCH_NOW", NOW);
        let project = h.project.to_string_lossy().into_owned();
        let out = h.run(&["fleet", "auto", "--cwd", &project]);
        assert!(out.status.success(), "{}", stderr(&out));
        let line = stdout(&out)
            .lines()
            .find(|l| l.starts_with("fleet: auto chose"))
            .unwrap()
            .to_string();
        assert!(
            line.starts_with(&format!("fleet: auto chose {flavor} - claude ")),
            "{fixture}: {line}"
        );
        assert!(line.contains(", codex "), "{line}");
        let ran = h
            .calls_of("herdr")
            .into_iter()
            .find(|c| c["argv"][1] == "run")
            .unwrap();
        assert!(
            ran["ran"].as_str().unwrap().contains(kind),
            "{fixture}: {ran}"
        );
    }
}
