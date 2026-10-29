//! The agent CLI update step (ARC-32 to ARC-35): `horch fleet` and
//! `horch orchestration` run each CLI's own update command once before the
//! first pane starts, and `horch update` does the same on demand. Each
//! updater runs only once its CLI's newest release is past the release
//! cooldown (ARC-36), whose dates come from a local fake npm registry.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use horch_e2e::harness::Harness;
use serde_json::Value;

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// Each agent fake with an npm package as its log names it, its program
/// name, and the update command horch must run, in the order of the result
/// lines. The cooldown can pass for these, so their updaters run.
const UPDATERS: [(&str, &str, &[&str]); 4] = [
    ("claude", "claude", &["update"]),
    ("codex", "codex", &["update"]),
    ("opencode", "opencode", &["upgrade"]),
    ("pi", "pi", &["update", "--self"]),
];

/// The agent fakes whose release dates horch cannot read: the cooldown
/// holds them back, and their updaters never run while it is on.
const UNDATED: [(&str, &str, &[&str]); 2] = [
    ("antigravity", "agy", &["update"]),
    ("prime", "prime-agent", &["update"]),
];

/// The npm package of each fake in [`UPDATERS`].
const PACKAGES: [&str; 4] = [
    "@anthropic-ai/claude-code",
    "@openai/codex",
    "opencode-ai",
    "@earendil-works/pi-coding-agent",
];

/// A publish time far past any cooldown.
const LONG_AGO: &str = "2020-01-01T00:00:00.000Z";

/// The variables that carry the cooldown to the package managers.
const COOLDOWN_ENV: [&str; 5] = [
    "npm_config_before",
    "PNPM_CONFIG_MINIMUM_RELEASE_AGE",
    "YARN_NPM_MINIMAL_AGE_GATE",
    "UV_EXCLUDE_NEWER",
    "PIP_UPLOADED_PRIOR_TO",
];

/// The result lines of a run where every dated fake updates and nothing
/// changes, and the undated ones are held back.
const NO_CHANGE: [&str; 6] = [
    "update: claude: 2.1.284 (Claude Code) (no change)",
    "update: codex: codex-cli 0.158.0 (no change)",
    "update: opencode: 1.18.33 (no change)",
    "update: antigravity: no release dates to check against the 7-day cooldown (skipped)",
    "update: pi: 0.85.1 (no change)",
    "update: prime: no release dates to check against the 7-day cooldown (skipped)",
];

const HEADER: &str = "Updating agent CLIs (horch fleet --no-update skips this)...";

/// A harness that lets the fleet update, with every fake warmed: macOS scans
/// each new executable at its first exec, and that takes 14 to 35 s. Every
/// npm package's newest release is long past the cooldown.
fn harness(name: &str) -> Harness {
    let releases: Vec<(&str, &str, &str)> =
        PACKAGES.iter().map(|p| (*p, "9.9.9", LONG_AGO)).collect();
    harness_with(name, &releases)
}

/// [`harness`], with a fake registry that offers `releases`: (package,
/// newest version, its publish time).
fn harness_with(name: &str, releases: &[(&str, &str, &str)]) -> Harness {
    let mut h = Harness::new(name);
    h.allow_updates();
    h.set("HORCH_NPM_REGISTRY", serve_registry(releases));
    warm(&h);
    h
}

fn warm(h: &Harness) {
    for (_, program, _) in UPDATERS.iter().chain(&UNDATED) {
        run_fake(h, program, &["--version"]);
    }
    run_fake(h, "herdr", &["--version"]);
}

/// Serve 1 registry document per package of `releases` until the test ends,
/// and 404 for any other. Returns the registry URL.
fn serve_registry(releases: &[(&str, &str, &str)]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let docs: Vec<(String, String)> = releases
        .iter()
        .map(|(package, version, published)| {
            let doc = serde_json::json!({
                "name": package,
                "dist-tags": {"latest": version},
                "versions": {*version: {"name": package, "version": version}},
                "time": {"created": LONG_AGO, *version: published},
            });
            (format!("/{}", package.replace('/', "%2f")), doc.to_string())
        })
        .collect();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            let _ = reader.read_line(&mut request);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 2 {
                line.clear();
            }
            let path = request.split_whitespace().nth(1).unwrap_or("");
            let found = docs
                .iter()
                .find(|(p, _)| p.eq_ignore_ascii_case(path))
                .map(|(_, doc)| doc.clone());
            let (status, body) = match found {
                Some(doc) => ("200 OK", doc),
                None => ("404 Not Found", "{}".to_string()),
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
        }
    });
    base
}

fn run_fake(h: &Harness, program: &str, args: &[&str]) -> Output {
    let exe = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    let mut cmd = Command::new(h.bin.join(exe));
    cmd.args(args);
    h.seal(&mut cmd);
    cmd.output().expect("running a fake")
}

fn argv(call: &Value) -> Vec<String> {
    serde_json::from_value(call["argv"].clone()).unwrap_or_default()
}

/// The update calls `fake` recorded, 1 per process.
fn update_calls(h: &Harness, fake: &str, args: &[&str]) -> Vec<Value> {
    h.calls_of(fake)
        .into_iter()
        .filter(|c| argv(c) == args)
        .collect()
}

fn fleet(h: &Harness, extra: &[&str]) -> Output {
    let project = h.project.to_string_lossy().into_owned();
    let mut args = vec!["fleet", "--cwd", project.as_str()];
    args.extend_from_slice(extra);
    h.run(&args)
}

/// The fleet started: herdr created its workspace.
fn workspace_created(h: &Harness) -> bool {
    h.calls_of("herdr")
        .iter()
        .any(|c| argv(c).get(..2) == Some(&["workspace".to_string(), "create".to_string()][..]))
}

fn notes(o: &Output) -> Vec<String> {
    stderr(o)
        .lines()
        .filter(|l| l.starts_with("horch: NOTE: update:"))
        .map(str::to_owned)
        .collect()
}

fn result_lines(o: &Output) -> Vec<String> {
    stdout(o)
        .lines()
        .filter(|l| l.starts_with("update: "))
        .map(str::to_owned)
        .collect()
}

#[test]
fn arc_32_fleet_updates_each_cli_once_before_the_orchestrator_pane() {
    let h = harness("arc32-fleet");
    let out = fleet(&h, &[]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));

    for (fake, _, args) in UPDATERS {
        assert_eq!(update_calls(&h, fake, args).len(), 1, "{fake} {args:?}");
    }
    for (fake, _, args) in UNDATED {
        assert!(update_calls(&h, fake, args).is_empty(), "{fake} updated");
    }
    // Every updater ran before herdr created the workspace of the
    // orchestrator pane.
    let calls = h.calls();
    let created = calls
        .iter()
        .position(|c| {
            c["fake"] == "herdr"
                && argv(c)
                    .get(..2)
                    .is_some_and(|a| a == ["workspace", "create"])
        })
        .expect("the fleet created its workspace");
    for (fake, _, args) in UPDATERS {
        let at = calls
            .iter()
            .position(|c| c["fake"] == fake && argv(c) == args)
            .unwrap();
        assert!(at < created, "{fake} updated after the workspace");
    }

    let text = stdout(&out);
    let header = text.find(HEADER).expect("the header line");
    let first = text.find("update: ").unwrap();
    assert!(header < first, "{text}");
    assert_eq!(result_lines(&out), NO_CHANGE, "{text}");
    assert!(notes(&out).is_empty(), "{}", stderr(&out));
}

#[test]
fn arc_32_orchestration_updates_before_its_panes() {
    let h = harness("arc32-orch");
    let project = h.project.to_string_lossy().into_owned();
    let out = h.run(&["orchestration", "--cwd", &project]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    for (fake, _, args) in UPDATERS {
        assert_eq!(update_calls(&h, fake, args).len(), 1, "{fake} {args:?}");
    }
    for (fake, _, args) in UNDATED {
        assert!(update_calls(&h, fake, args).is_empty(), "{fake} updated");
    }
    assert_eq!(result_lines(&out), NO_CHANGE, "{}", stdout(&out));
    assert!(workspace_created(&h));
}

/// `cpx` launches `claude` and has no `update` command: with `cpx` on PATH,
/// the update runs `claude`.
#[cfg(unix)]
#[test]
fn arc_32_a_cpx_launch_updates_claude_not_cpx() {
    let mut h = harness("arc32-cpx");
    h.without("HORCH_CLAUDE_BIN");
    let cpx_log = h.root.join("cpx.log");
    h.write_bin(
        "cpx",
        format!("#!/bin/sh\necho \"$*\" >> '{}'\n", cpx_log.display()).as_bytes(),
    );
    let out = fleet(&h, &[]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));

    assert_eq!(update_calls(&h, "claude", &["update"]).len(), 1);
    let cpx_calls = std::fs::read_to_string(&cpx_log).unwrap_or_default();
    assert!(
        !cpx_calls.lines().any(|l| l.trim() == "update"),
        "cpx got an update: {cpx_calls:?}"
    );
    assert_eq!(result_lines(&out)[0], NO_CHANGE[0], "{}", stdout(&out));
}

#[test]
fn arc_33_a_failed_updater_gives_one_note_and_the_fleet_starts() {
    let mut h = harness("arc33-fail");
    h.set("HORCH_FAKE_SCENARIO", "update_fail:codex");
    let out = fleet(&h, &[]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(
        notes(&out),
        ["horch: NOTE: update: codex: error: codex update failed: network unreachable"],
        "{}",
        stderr(&out)
    );
    // The other 5 still report.
    let lines = result_lines(&out);
    assert_eq!(lines.len(), 5, "{lines:?}");
    assert!(!lines.iter().any(|l| l.starts_with("update: codex:")));
    assert!(workspace_created(&h));
}

/// #205: an updater that passes the time limit is not killed, so a package
/// manager can finish its own install. The fleet does not wait for it.
#[test]
fn arc_33_a_hung_updater_is_left_running_at_the_timeout() {
    let mut h = harness("arc33-hang");
    h.set("HORCH_FAKE_SCENARIO", "update_hang:opencode");
    h.set("HORCH_UPDATE_TIMEOUT_MS", "5000");
    let started = Instant::now();
    let out = fleet(&h, &[]);
    let took = started.elapsed();
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(
        notes(&out),
        ["horch: NOTE: update: opencode: still running after 5 s, not waited for"],
        "{}",
        stderr(&out)
    );
    // The fake sleeps 60 s; the fleet did not wait for it.
    assert!(took < Duration::from_secs(50), "the fleet took {took:?}");
    assert!(workspace_created(&h));

    // The updater outlived the fleet start: horch did not kill it.
    #[cfg(unix)]
    {
        let call = update_calls(&h, "opencode", &["upgrade"]);
        let pid = call[0]["pid"].as_u64().unwrap().to_string();
        let alive = Command::new("kill")
            .args(["-0", &pid])
            .output()
            .is_ok_and(|o| o.status.success());
        assert!(alive, "updater {pid} was killed at its timeout");
        // The test stops it, so it does not outlive the test.
        let _ = Command::new("kill").args(["-9", &pid]).output();
    }
}

#[test]
fn arc_33_no_updater_gets_anthropic_api_key() {
    let mut h = harness("arc33-key");
    h.set("ANTHROPIC_API_KEY", "SENTINEL");
    let out = fleet(&h, &[]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    for (fake, _, args) in UPDATERS {
        let calls = update_calls(&h, fake, args);
        assert_eq!(calls.len(), 1, "{fake}");
        let keys: Vec<String> = serde_json::from_value(calls[0]["env_keys"].clone()).unwrap();
        assert!(
            !keys.iter().any(|k| k == "ANTHROPIC_API_KEY"),
            "{fake}: {keys:?}"
        );
        assert_eq!(calls[0]["violations"], serde_json::json!([]), "{fake}");
    }
}

/// A CLI whose newest release is younger than the cooldown keeps its build:
/// its updater does not run, its line says why, and the step still passes.
#[test]
fn arc_36_a_release_younger_than_the_cooldown_is_held_back() {
    let mut h = harness_with(
        "arc36-young",
        &[
            (PACKAGES[0], "2.1.300", "2026-01-08T00:00:00.000Z"),
            (PACKAGES[1], "9.9.9", LONG_AGO),
            (PACKAGES[2], "1.19.0", "2026-01-03T00:00:00.000Z"),
            (PACKAGES[3], "9.9.9", LONG_AGO),
        ],
    );
    h.set("HORCH_NOW", "2026-01-10T00:00:00Z");
    let out = h.run(&["update"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));

    // 2 days old: held. Exactly 7 days old: past the cooldown.
    assert!(update_calls(&h, "claude", &["update"]).is_empty());
    assert_eq!(update_calls(&h, "opencode", &["upgrade"]).len(), 1);
    assert_eq!(update_calls(&h, "codex", &["update"]).len(), 1);
    let lines = result_lines(&out);
    assert_eq!(
        lines[0], "update: claude: 2.1.300 is 2 days old, under the 7-day cooldown (skipped)",
        "{lines:?}"
    );
    assert_eq!(lines[1..], NO_CHANGE[1..], "{lines:?}");
    assert!(notes(&out).is_empty(), "{}", stderr(&out));

    // A longer cooldown holds opencode back too.
    h.set("HORCH_UPDATE_COOLDOWN_DAYS", "30");
    let out = h.run(&["update", "opencode"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(
        result_lines(&out),
        ["update: opencode: 1.19.0 is 7 days old, under the 30-day cooldown (skipped)"],
        "{}",
        stdout(&out)
    );
    assert_eq!(update_calls(&h, "opencode", &["upgrade"]).len(), 1);
}

/// Every updater that runs gets the cooldown for the package managers it
/// may call.
#[test]
fn arc_36_each_updater_gets_the_package_manager_cooldown() {
    let h = harness("arc36-env");
    let out = h.run(&["update"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    for (fake, _, args) in UPDATERS {
        let calls = update_calls(&h, fake, args);
        assert_eq!(calls.len(), 1, "{fake}");
        let keys: Vec<String> = serde_json::from_value(calls[0]["env_keys"].clone()).unwrap();
        for key in COOLDOWN_ENV {
            assert!(
                keys.iter().any(|k| k == key),
                "{fake} lacks {key}: {keys:?}"
            );
        }
    }
}

/// A registry horch cannot reach updates nothing: each dated CLI gets 1
/// NOTE, `horch update` exits 1, and a fleet still starts.
#[test]
fn arc_36_an_unreachable_registry_updates_nothing() {
    let mut h = harness("arc36-down");
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    h.set("HORCH_NPM_REGISTRY", url);

    let out = h.run(&["update"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    let notes = notes(&out);
    assert_eq!(notes.len(), 4, "{notes:?}");
    for ((fake, _, _), note) in UPDATERS.iter().zip(&notes) {
        assert!(
            note.starts_with(&format!(
                "horch: NOTE: update: {fake}: release date check: cannot reach"
            )),
            "{note}"
        );
    }
    for (fake, _, args) in UPDATERS.iter().chain(&UNDATED) {
        assert!(update_calls(&h, fake, args).is_empty(), "{fake} updated");
    }

    let out = fleet(&h, &[]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert!(workspace_created(&h));
}

/// `HORCH_UPDATE_COOLDOWN_DAYS=0` turns the cooldown off: no registry
/// lookup, every updater runs, and none gets the cooldown variables.
#[test]
fn arc_36_a_zero_day_cooldown_runs_every_updater() {
    let mut h = harness("arc36-off");
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    h.set("HORCH_NPM_REGISTRY", url);
    h.set("HORCH_UPDATE_COOLDOWN_DAYS", "0");

    let out = h.run(&["update"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert!(notes(&out).is_empty(), "{}", stderr(&out));
    for (fake, _, args) in UPDATERS.iter().chain(&UNDATED) {
        let calls = update_calls(&h, fake, args);
        assert_eq!(calls.len(), 1, "{fake}");
        let keys: Vec<String> = serde_json::from_value(calls[0]["env_keys"].clone()).unwrap();
        assert!(
            !keys.iter().any(|k| COOLDOWN_ENV.contains(&k.as_str())),
            "{fake}: {keys:?}"
        );
    }
    assert_eq!(
        result_lines(&out),
        [
            NO_CHANGE[0],
            NO_CHANGE[1],
            NO_CHANGE[2],
            "update: antigravity: 1.1.3 (no change)",
            NO_CHANGE[4],
            "update: prime: 0.10.0 (no change)",
        ],
        "{}",
        stdout(&out)
    );
}

#[test]
fn arc_34_no_update_and_horch_no_update_run_no_updater() {
    let h = harness("arc34-flag");
    let out = fleet(&h, &["--no-update"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    let project = h.project.to_string_lossy().into_owned();
    let orch = h.run(&["orchestration", "--cwd", &project, "--no-update"]);
    assert!(
        orch.status.success(),
        "{}\n{}",
        stdout(&orch),
        stderr(&orch)
    );

    let mut env = harness("arc34-env");
    env.set("HORCH_NO_UPDATE", "1");
    let env_out = fleet(&env, &[]);
    assert!(
        env_out.status.success(),
        "{}\n{}",
        stdout(&env_out),
        stderr(&env_out)
    );

    for (h, outs) in [(&h, vec![&out, &orch]), (&env, vec![&env_out])] {
        for (fake, _, args) in UPDATERS {
            assert!(update_calls(h, fake, args).is_empty(), "{fake} updated");
        }
        for o in outs {
            assert!(!stdout(o).contains(HEADER), "{}", stdout(o));
        }
        assert!(workspace_created(h));
    }
}

#[test]
fn arc_35_horch_update_runs_herdr_only_on_request() {
    let h = harness("arc35");
    let herdr_updates = |h: &Harness| update_calls(h, "herdr", &["update"]).len();

    let out = h.run(&["update"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(result_lines(&out), NO_CHANGE, "{}", stdout(&out));
    assert_eq!(herdr_updates(&h), 0, "horch update ran herdr update");

    let out = h.run(&["update", "herdr"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(herdr_updates(&h), 1);
    assert!(
        stderr(&out).contains(
            "horch: NOTE: herdr update can stop the herdr server. \
             That closes every pane, a running fleet included."
        ),
        "{}",
        stderr(&out)
    );
    // Only herdr: no agent CLI updated again.
    assert_eq!(update_calls(&h, "claude", &["update"]).len(), 1);
    assert!(result_lines(&out).is_empty(), "{}", stdout(&out));

    let out = h.run(&["update", "pi", "codex"]);
    assert!(out.status.success(), "{}\n{}", stdout(&out), stderr(&out));
    assert_eq!(
        result_lines(&out),
        [NO_CHANGE[1], NO_CHANGE[4]],
        "{}",
        stdout(&out)
    );

    let mut failing = harness("arc35-fail");
    failing.set("HORCH_FAKE_SCENARIO", "update_fail:pi");
    let out = failing.run(&["update"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert_eq!(notes(&out).len(), 1, "{}", stderr(&out));

    let out = h.run(&["update", "vim"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}
