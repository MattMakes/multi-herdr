//! Smoke tests for the fakes themselves: each fake answers the calls horch
//! makes, in the shape horch parses. These are not requirement tests.

use std::process::{Command, Output};

use horch_e2e::harness::Harness;
use serde_json::Value;

/// Run one fake by the name of the program it stands in for, in the sealed
/// environment of `h`.
fn fake(h: &Harness, name: &str, args: &[&str]) -> Output {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let mut cmd = Command::new(h.bin.join(exe));
    cmd.args(args);
    h.seal(&mut cmd);
    cmd.output().expect("running a fake")
}

fn json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&o.stdout)))
}

fn pane_ids(h: &Harness) -> Vec<String> {
    let listed = json(&fake(h, "herdr", &["pane", "list"]));
    listed["result"]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["pane_id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn fake_herdr_split_adds_pane_and_close_removes_it() {
    let h = Harness::new("fakes-split");
    let created = json(&fake(&h, "herdr", &["workspace", "create", "--label", "t"]));
    let root = created["result"]["root_pane"]["pane_id"].as_str().unwrap();
    assert_eq!(pane_ids(&h), [root]);

    let split = json(&fake(
        &h,
        "herdr",
        &["pane", "split", root, "--direction", "right", "--no-focus"],
    ));
    let new = split["result"]["pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(new, root);
    assert_eq!(
        split["result"]["pane"]["workspace_id"],
        created["result"]["root_pane"]["workspace_id"]
    );
    assert_eq!(
        split["result"]["pane"]["tab_id"],
        created["result"]["root_pane"]["tab_id"]
    );
    assert_eq!(pane_ids(&h), [root, new.as_str()]);
    assert!(fake(&h, "herdr", &["pane", "get", &new]).status.success());

    let closed = fake(&h, "herdr", &["pane", "close", &new]);
    assert!(closed.status.success());
    assert_eq!(pane_ids(&h), [root]);
    assert!(!fake(&h, "herdr", &["pane", "get", &new]).status.success());
    assert!(!fake(&h, "herdr", &["pane", "close", &new]).status.success());

    // Both calls stay violations, so existing assertions keep their meaning.
    let violations = h.violations();
    assert!(
        violations.iter().any(|v| v.contains("pane split")),
        "{violations:?}"
    );
    assert!(
        violations.iter().any(|v| v.contains("pane close")),
        "{violations:?}"
    );
}

#[test]
fn fake_herdr_fail_split_and_fail_run() {
    let mut h = Harness::new("fakes-fail");
    let created = json(&fake(&h, "herdr", &["workspace", "create", "--label", "t"]));
    let root = created["result"]["root_pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string();

    // A list: `fail_split` combines with other scenarios.
    h.set("HORCH_FAKE_SCENARIO", "exec,fail_split");
    let split = fake(
        &h,
        "herdr",
        &["pane", "split", &root, "--direction", "right"],
    );
    assert_eq!(split.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&split.stderr).contains("fail_split"));
    assert_eq!(pane_ids(&h), [root.as_str()], "state unchanged");
    // `pane run` is not affected by `fail_split`.
    assert!(fake(&h, "herdr", &["pane", "run", &root, "true"])
        .status
        .success());

    h.set("HORCH_FAKE_SCENARIO", "fail_run");
    let marker = h.tmp.join("ran");
    let command = format!("touch {}", marker.display());
    let run = fake(&h, "herdr", &["pane", "run", &root, &command]);
    assert_eq!(run.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&run.stderr).contains("fail_run"));
    assert_eq!(pane_ids(&h), [root.as_str()], "state unchanged");
    // And `fail_run` does not stop a split.
    assert!(fake(&h, "herdr", &["pane", "split", &root])
        .status
        .success());
    assert!(!marker.exists(), "a failed run starts no command");
}

#[cfg(unix)]
#[test]
fn fake_herdr_close_kills_the_exec_process() {
    let mut h = Harness::new("fakes-kill");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    let created = json(&fake(&h, "herdr", &["workspace", "create", "--label", "t"]));
    let root = created["result"]["root_pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    let pidfile = h.tmp.join("pid");
    let command = format!("echo $$ > {}; /bin/sleep 60", pidfile.display());
    assert!(fake(&h, "herdr", &["pane", "run", &root, &command])
        .status
        .success());
    let started = std::time::Instant::now();
    while std::fs::read_to_string(&pidfile)
        .unwrap_or_default()
        .trim()
        .is_empty()
        && started.elapsed().as_secs() < 10
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let pid = std::fs::read_to_string(&pidfile).expect("the pane command started");
    let alive = |pid: &str| {
        Command::new("/bin/sh")
            .args(["-c", &format!("kill -0 {}", pid.trim())])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    assert!(alive(&pid));
    assert!(fake(&h, "herdr", &["pane", "close", &root])
        .status
        .success());
    let started = std::time::Instant::now();
    while alive(&pid) && started.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!alive(&pid), "pane close kills the pane process");
}

#[test]
fn fake_opencode_session_list_matches_cwd() {
    let mut h = Harness::new("fakes-opencode");
    assert_eq!(
        String::from_utf8_lossy(&fake(&h, "opencode", &["--version"]).stdout).trim(),
        "1.18.33"
    );

    // The same call horch makes, through the library that parses it.
    let listed = fake(&h, "opencode", &["session", "list", "--format", "json"]);
    assert!(listed.status.success());
    let text = String::from_utf8_lossy(&listed.stdout).into_owned();
    let records: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(records.len(), 1);
    let id = horch_e2e::opencode_session_id(&h.project);
    assert!(id.starts_with("ses_") && id.len() == 4 + 16, "{id}");
    assert_eq!(records[0]["id"], id.as_str());
    let dir = records[0]["directory"].as_str().unwrap();
    assert_eq!(
        std::fs::canonicalize(dir).unwrap(),
        std::fs::canonicalize(&h.project).unwrap()
    );
    // The fields `horch_core::opencode::parse_sessions` reads: `created` is
    // milliseconds since the epoch and is recent, so the "after launch" filter
    // keeps it.
    let created = records[0]["created"].as_u64().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(
        created <= now && now - created < 60_000,
        "{created} vs {now}"
    );

    // A launch is recorded and exits 0 at once; a resume names its session.
    let launch = fake(
        &h,
        "opencode",
        &[
            "--model",
            "opencode/m",
            "--session",
            "ses_old",
            "--prompt",
            "go",
        ],
    );
    assert!(launch.status.success());
    let calls = h.calls_of("opencode");
    let last = calls.last().unwrap();
    assert_eq!(last["model"], "opencode/m");
    assert_eq!(last["resumed"], "ses_old");
    assert_eq!(last["session_id"], id.as_str());

    // `stay` keeps a launch alive until it is killed.
    h.set("HORCH_FAKE_SCENARIO", "stay");
    let mut cmd = Command::new(h.bin.join("opencode"));
    cmd.args(["--model", "opencode/m", "--prompt", "go"]);
    h.seal(&mut cmd);
    let mut child = cmd.spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(child.try_wait().unwrap().is_none(), "stay keeps running");
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn fake_opencode_writes_transcript_rows_when_asked() {
    let mut h = Harness::new("fakes-opencode-db");
    let works = h.has_sqlite3()
        && Command::new(h.bin.join("sqlite3"))
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success());
    if !works {
        assert!(
            std::env::var_os("HORCH_REQUIRE_SQLITE").is_none(),
            "HORCH_REQUIRE_SQLITE is set but the harness sqlite3 does not run"
        );
        return;
    }
    h.set("HORCH_FAKE_TRANSCRIPTS", "1");
    let launch = fake(&h, "opencode", &["--model", "opencode/m", "--prompt", "go"]);
    assert!(launch.status.success());
    let db = h.home.join(".local/share/opencode/opencode.db");
    assert!(db.is_file(), "no database at {}", db.display());
    let id = horch_e2e::opencode_session_id(&h.project);
    let mut cmd = Command::new(h.bin.join("sqlite3"));
    cmd.arg("-readonly").arg("-json").arg(&db).arg(format!(
        "SELECT count(*) AS n FROM message WHERE session_id = '{id}';"
    ));
    let out = cmd.output().unwrap();
    let rows: Vec<Value> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rows[0]["n"], 1);
}
