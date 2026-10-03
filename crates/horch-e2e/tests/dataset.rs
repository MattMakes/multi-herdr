//! End to end: `multi-herdr-dataset run` refuses before any worktree or
//! model call (PRE-06, PRE-07, PRE-12).
//!
//! Each test runs the real binary in a sealed harness with a real git repo
//! (`Harness::with_git`), the fakes for every harness and herdr, and a
//! machine fixture. The fakes log every call, so a test sees each argv.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_e2e::harness::Harness;
use horch_e2e::{bin_dir, violations};
use serde_json::Value;

/// The harness fakes that stand for agent CLIs. Any call to one of them
/// other than `--version` would be a model launch.
const AGENTS: [&str; 5] = ["claude", "codex", "opencode", "pi", "prime"];

fn dataset_bin() -> PathBuf {
    let path = bin_dir().join(format!(
        "multi-herdr-dataset{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        path.is_file(),
        "{} is not built; run `cargo build --workspace --bins` first",
        path.display()
    );
    path
}

/// A harness with git, or `None` when git is missing (and not required).
fn harness(name: &str) -> Option<Harness> {
    let h = Harness::new(name).with_git();
    h.git_bin()?;
    Some(h)
}

fn machine_fixture(h: &Harness, disk_free: u64) -> PathBuf {
    let file = h.root.join("machine.json");
    let snapshot = serde_json::json!({
        "os": "macos",
        "arch": "arm64",
        "cpus": 18,
        "mem_total_bytes": 137_438_953_472u64,
        "mem_available_bytes": 85_899_345_920u64,
        "disk_free_bytes": disk_free,
        "disk_total_bytes": 994_662_584_320u64,
        "gpu": "apple_silicon",
        "max_open_files": 10240,
        "max_processes": 8000
    });
    std::fs::write(&file, snapshot.to_string()).unwrap();
    file
}

fn run(h: &Harness, extra: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(dataset_bin());
    cmd.args([
        "run",
        "add a greeting",
        "--candidates",
        "2",
        "--budget-usd",
        "100",
    ]);
    h.seal(&mut cmd);
    cmd.envs(extra.iter().copied());
    cmd.output().expect("running multi-herdr-dataset")
}

/// Every event in the dataset dir, in file and line order.
fn events(h: &Harness) -> Vec<Value> {
    let mut files = files_under(&h.state.join("multi-herdr"));
    files.retain(|f| f.parent().is_some_and(|p| p.ends_with("events")));
    files.sort();
    files
        .iter()
        .flat_map(|f| {
            std::fs::read_to_string(f)
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str::<Value>(l).unwrap())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

fn kinds(events: &[Value]) -> Vec<&str> {
    events.iter().filter_map(|e| e["kind"].as_str()).collect()
}

/// The checks `experiment.aborted` names.
fn failed_checks(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .find(|e| e["kind"] == "experiment.aborted")
        .and_then(|e| e["payload"]["failed_checks"].as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The repo still has its one worktree, and no candidate branch exists.
fn assert_no_worktree(h: &Harness) {
    let out = h
        .git_cmd(&["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let list = String::from_utf8_lossy(&out.stdout);
    assert_eq!(list.matches("worktree ").count(), 1, "{list}");
    let out = h
        .git_cmd(&["for-each-ref", "--format=%(refname)", "refs/heads/mh/"])
        .output()
        .unwrap();
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Agent fakes were asked for `--version` only: no model turn.
fn assert_no_agent_launch(h: &Harness) {
    for agent in AGENTS {
        for call in h.calls_of(agent) {
            let argv: Vec<&str> = call["argv"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(Value::as_str)
                .collect();
            assert_eq!(argv, ["--version"], "{agent} was launched: {call}");
        }
    }
}

#[test]
fn pre_06_harness_resolution_before_worktree() {
    let Some(h) = harness("pre06") else { return };
    let machine = machine_fixture(&h, 500_000_000_000);
    let missing = h.bin.join("no-such-claude");
    let out = run(
        &h,
        &[
            ("HORCH_MACHINE_FILE", machine.to_str().unwrap()),
            ("HORCH_CLAUDE_BIN", missing.to_str().unwrap()),
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(4),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let events = events(&h);
    assert_eq!(
        kinds(&events),
        [
            "experiment.created",
            "preflight.completed",
            "experiment.aborted"
        ]
    );
    let failed = failed_checks(&events);
    assert!(failed.contains(&"PRE-06".to_string()), "{failed:?}");
    let report = &events[1]["payload"]["report"];
    assert_eq!(report["passed"], false);
    assert!(stdout.contains("REFUSED"), "{stdout}");

    assert_no_worktree(&h);
    assert_no_agent_launch(&h);
    let pre06 = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "PRE-06")
        .unwrap();
    assert_eq!(pre06["status"], "fail", "{pre06}");
    // The missing claude was never started; no fake claude ran either.
    assert!(h.calls_of("claude").is_empty());
    assert!(violations(&h.log).is_empty(), "{:?}", violations(&h.log));
}

#[test]
fn pre_12_e2e_refuses_before_worktree_or_model() {
    let Some(h) = harness("pre12") else { return };
    // A machine with no free disk: PRE-02 fails.
    let machine = machine_fixture(&h, 0);
    let out = run(&h, &[("HORCH_MACHINE_FILE", machine.to_str().unwrap())]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(4),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let events = events(&h);
    assert_eq!(
        kinds(&events),
        [
            "experiment.created",
            "preflight.completed",
            "experiment.aborted"
        ]
    );
    assert!(failed_checks(&events).contains(&"PRE-02".to_string()));
    let exp = events[0]["experiment_id"].as_str().unwrap().to_string();

    // The report is persisted in the event and in the manifest, with the
    // environment digest.
    let report = &events[1]["payload"]["report"];
    assert_eq!(report["passed"], false);
    let manifest = files_under(&h.state.join("multi-herdr"))
        .into_iter()
        .find(|f| f.ends_with(Path::new(&exp).join("manifest.json")))
        .expect("the manifest exists");
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(manifest).unwrap()).unwrap();
    assert_eq!(&manifest["preflight"], report);
    assert_eq!(manifest["environment_digest"], report["environment_digest"]);
    assert_eq!(
        events[0]["payload"]["environment_digest"],
        report["environment_digest"]
    );

    // No worktree, no worktree directory, no agent launch.
    assert_no_worktree(&h);
    let roots: Vec<PathBuf> = files_under(&h.state.join("multi-herdr"))
        .into_iter()
        .filter(|f| f.components().any(|c| c.as_os_str() == "worktrees"))
        .collect();
    assert!(roots.is_empty(), "{roots:?}");
    assert_no_agent_launch(&h);
    assert!(violations(&h.log).is_empty(), "{:?}", violations(&h.log));
}

#[cfg(unix)]
#[test]
fn pre_07_probe_no_secret_persisted() {
    use std::os::unix::fs::PermissionsExt;

    let Some(h) = harness("pre07") else { return };
    const TOKEN: &str = "sk-ant-api03-PRE07leakedTOKENabcdefghij";
    // A claude whose `--version` prints a token. It uses only `echo`, a
    // shell builtin, because the sealed PATH has no system programs.
    let fake = h.bin.join("claude-leaky");
    std::fs::write(
        &fake,
        format!("#!/bin/sh\necho \"9.9.9 (Claude Code) {TOKEN}\"\n"),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let machine = machine_fixture(&h, 500_000_000_000);

    let out = run(
        &h,
        &[
            ("HORCH_MACHINE_FILE", machine.to_str().unwrap()),
            ("HORCH_CLAUDE_BIN", fake.to_str().unwrap()),
            // A dummy value, never a real key: it proves nothing persists it.
            ("ANTHROPIC_API_KEY", "SENTINEL"),
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        matches!(out.status.code(), Some(0) | Some(4)),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let files = files_under(&h.state);
    assert!(
        files.iter().any(|f| f.ends_with("manifest.json")),
        "{files:?}"
    );
    for file in &files {
        let bytes = std::fs::read(file).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("SENTINEL"),
            "{} holds the key",
            file.display()
        );
        assert!(!text.contains(TOKEN), "{} holds the token", file.display());
        assert!(
            !text.contains("PRE07leaked"),
            "{} holds part of the token",
            file.display()
        );
    }
    // The version was resolved, and recorded redacted.
    let manifest = files.iter().find(|f| f.ends_with("manifest.json")).unwrap();
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(manifest).unwrap()).unwrap();
    let claude = manifest["harness_versions"]["claude"].as_str().unwrap();
    assert!(claude.contains("[REDACTED]"), "{claude}");
    assert_no_worktree(&h);
}
