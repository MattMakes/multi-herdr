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
                // A torn last line (power loss) is skipped, as the store does.
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
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
            // B3: a passed preflight starts the round. This test is about
            // preflight only, so it stops there (exit 86).
            ("HORCH_FAULT", "abort-after-preflight"),
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        matches!(out.status.code(), Some(4) | Some(86)),
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

// ── B3: whole rounds through the coordinator ─────────────────────────────

/// Every teammate in the repo roster except `keep`: the `exclude` list that
/// leaves the planner only `keep` to pick from.
fn exclude_all_but(keep: &[&str]) -> Vec<String> {
    let dir = horch_e2e::harness::repo_root().join("teammates");
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".md").map(str::to_string)
        })
        .filter(|n| n != "README" && !n.starts_with('_') && !keep.contains(&n.as_str()))
        .collect();
    out.sort();
    out
}

/// A round harness: git repo with a committed `.multi-herdr/dataset.yaml`,
/// fake-herdr in `exec` (pane commands really run), every quota pool ok, a
/// machine with room, and the candidates' behavior keyed by label.
fn round_harness(name: &str, keep: &[&str], yaml: &str, candidates: Value) -> Option<Harness> {
    let mut h = harness(name)?;
    let machine = machine_fixture(&h, 500_000_000_000);
    h.set("HORCH_MACHINE_FILE", machine.to_string_lossy());
    h.set("HORCH_FAKE_SCENARIO", "exec");
    h.set(
        "HORCH_QUOTA_FILE",
        horch_e2e::harness::fixtures()
            .join("quota/all-ok.json")
            .to_string_lossy(),
    );
    let dir = h.project.join(".multi-herdr");
    std::fs::create_dir_all(&dir).unwrap();
    let exclude: Vec<Value> = exclude_all_but(keep).into_iter().map(Value::from).collect();
    // A stuck candidate fails the test in a minute, not in an hour.
    let caps = if yaml.contains("caps:") {
        ""
    } else {
        "caps:\n  candidate_deadline_s: 60\n"
    };
    std::fs::write(
        dir.join("dataset.yaml"),
        format!("exclude: {}\n{caps}{yaml}", Value::from(exclude)),
    )
    .unwrap();
    for args in [
        &["add", "-A"][..],
        &["commit", "--quiet", "--no-gpg-sign", "-m", "dataset config"],
    ] {
        let out = h.git_cmd(args).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let mut file = h.log.clone().into_os_string();
    file.push(".candidates.json");
    std::fs::write(file, candidates.to_string()).unwrap();
    Some(h)
}

fn dataset(h: &Harness, args: &[&str], extra: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(dataset_bin());
    cmd.args(args);
    h.seal(&mut cmd);
    cmd.envs(extra.iter().copied());
    cmd.output().expect("running multi-herdr-dataset")
}

fn run_round(h: &Harness, n: u32, extra: &[(&str, &str)]) -> Output {
    let n = n.to_string();
    dataset(
        h,
        &[
            "run",
            "add a greeting",
            "--candidates",
            &n,
            "--budget-usd",
            "100",
        ],
        extra,
    )
}

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// The project's execution records.
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

fn candidate_records(h: &Harness) -> Vec<Value> {
    records(h)
        .into_iter()
        .filter(|r| r["experiment_id"].is_string())
        .collect()
}

/// The events of one kind.
fn of_kind<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events.iter().filter(|e| e["kind"] == kind).collect()
}

/// The payload of `candidate.failed` for `label`.
fn failure_of(events: &[Value], label: &str) -> Value {
    of_kind(events, "candidate.failed")
        .into_iter()
        .find(|e| e["payload"]["label"] == label)
        .map(|e| e["payload"]["failure"].clone())
        .unwrap_or(Value::Null)
}

/// The final state of the only round, from `status`.
fn round_state(h: &Harness) -> String {
    let out = dataset(h, &["status"], &[]);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    stdout
        .lines()
        .find(|l| l.trim_start().starts_with("round "))
        .and_then(|l| l.split_whitespace().nth(3))
        .unwrap_or_default()
        .to_string()
}

/// `rebuild <exp>` exits 0: the fold equals the event-by-event projection.
fn assert_rebuild_equal(h: &Harness) {
    let events = events(h);
    let exp = events[0]["experiment_id"].as_str().unwrap();
    let out = dataset(h, &["rebuild", exp], &[]);
    assert!(out.status.success(), "{}", text(&out));
}

/// No fake recorded a violation. Also makes the judge bundle dirs (0500)
/// writable again, so the harness can delete its temp dir.
fn assert_clean(h: &Harness) {
    unlock(&h.state);
    assert!(h.violations().is_empty(), "{:?}", h.violations());
}

#[cfg(unix)]
fn unlock(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
}

#[cfg(not(unix))]
fn unlock(_dir: &Path) {}

#[test]
fn cmp_05_e2e_candidates_in_dataset_workspace() {
    let Some(h) = round_harness(
        "cmp05",
        &["sonnet", "codex-sol"],
        "",
        serde_json::json!({
            "A": {"write": {"greeting.txt": "hello A\n"}, "commit": true, "exit": "done"},
            "B": {"write": {"greeting.txt": "hello B\n"}, "exit": "done"},
        }),
    ) else {
        return;
    };
    let out = run_round(&h, 2, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events = events(&h);
    assert_eq!(
        of_kind(&events, "candidate.spawned").len(),
        2,
        "{}",
        text(&out)
    );
    assert_eq!(of_kind(&events, "candidate.completed").len(), 2);
    assert_eq!(round_state(&h), "COMPLETE", "{}", text(&out));

    // Each candidate ran in its own worktree.
    let worktrees: Vec<(String, String)> = of_kind(&events, "worktree.created")
        .iter()
        .map(|e| {
            (
                e["payload"]["label"].as_str().unwrap().to_string(),
                e["payload"]["path"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    for (label, path) in &worktrees {
        let launch = h
            .calls()
            .into_iter()
            .find(|c| c["candidate"] == label.as_str())
            .unwrap_or_else(|| panic!("no launch of {label}"));
        // Cleanup removed the worktree, so compare the text (macOS reports
        // `/private/var/...` for `/var/...`).
        let plain = |p: &str| p.trim_start_matches("/private").to_string();
        assert_eq!(
            plain(launch["cwd"].as_str().unwrap()),
            plain(path),
            "{label}"
        );
    }

    // The panes were split in the dataset workspace, from its root pane, and
    // no pane registered as orchestrator.
    let herdr = h.calls_of("herdr");
    let create = herdr
        .iter()
        .find(|c| c["argv"][0] == "workspace" && c["argv"][1] == "create")
        .expect("a workspace was created");
    let argv: Vec<&str> = create["argv"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let label = argv[argv.iter().position(|a| *a == "--label").unwrap() + 1];
    assert!(label.starts_with("multi-herdr-dataset "), "{argv:?}");
    assert!(argv.contains(&"--no-focus"), "{argv:?}");
    let splits: Vec<&Value> = herdr
        .iter()
        .filter(|c| c["argv"][0] == "pane" && c["argv"][1] == "split")
        .collect();
    assert_eq!(splits.len(), 2);
    for s in &splits {
        assert_eq!(s["argv"][2], "w1:p1", "{s}");
    }
    let mailbox = h.tmp.join("horch-mailbox");
    let registered = files_under(&h.tmp)
        .into_iter()
        .any(|f| f.file_name().is_some_and(|n| n == "orchestrator"));
    assert!(!registered, "{}", mailbox.display());
    for r in candidate_records(&h) {
        assert_eq!(r["workspace_id"], "w1", "{r}");
        assert!(r["workdir"].is_string(), "{r}");
    }
    assert_rebuild_equal(&h);
    assert_clean(&h);
}

/// The candidates' teammates in 2-candidate rounds: one Claude, one Codex.
const PAIR: [&str; 2] = ["sonnet", "codex-sol"];

fn done(file: &str) -> Value {
    serde_json::json!({"write": {file: "hello\n"}, "commit": true, "exit": "done"})
}

/// A 2-candidate round where `a` is the candidate under test and B
/// finishes normally.
fn pair(name: &str, yaml: &str, a: Value) -> Option<Harness> {
    round_harness(
        name,
        &PAIR,
        yaml,
        serde_json::json!({"A": a, "B": done("b.txt")}),
    )
}

fn experiment_id(h: &Harness) -> String {
    events(h)[0]["experiment_id"].as_str().unwrap().to_string()
}

/// Every `candidate.spawned` has a terminal event for the same execution,
/// and every candidate record is terminal (MEA-10).
fn assert_every_spawn_ended(h: &Harness) {
    let events = events(h);
    for s in of_kind(&events, "candidate.spawned") {
        let id = &s["execution_id"];
        let ended = events.iter().any(|e| {
            (e["kind"] == "candidate.completed" || e["kind"] == "candidate.failed")
                && &e["execution_id"] == id
        });
        assert!(ended, "no terminal event for {s}");
    }
    for r in candidate_records(h) {
        assert_eq!(r["status"], "done", "{r}");
    }
}

/// The candidate branches that exist, with their tips.
fn candidate_branches(h: &Harness) -> Vec<String> {
    let out = h
        .git_cmd(&["for-each-ref", "--format=%(refname)", "refs/heads/mh/exp/"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn cmp_04_n_worktrees_same_base_modify_same_file() {
    let same = |n: &str| {
        serde_json::json!({"write": {"greeting.txt": format!("hello from {n}\n")},
                           "commit": true, "exit": "done"})
    };
    let Some(h) = round_harness(
        "cmp04",
        &["sonnet", "codex-sol", "opus"],
        "",
        serde_json::json!({"A": same("A"), "B": same("B"), "C": same("C")}),
    ) else {
        return;
    };
    let base = h.head_sha().unwrap();
    let out = run_round(&h, 3, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events = events(&h);
    let worktrees = of_kind(&events, "worktree.created");
    assert_eq!(worktrees.len(), 3);
    for w in &worktrees {
        assert_eq!(w["payload"]["base_sha"], base.as_str());
    }
    let frozen = of_kind(&events, "candidate.frozen");
    assert_eq!(frozen.len(), 3);
    let mut heads: Vec<&str> = frozen
        .iter()
        .map(|f| f["payload"]["head_sha"].as_str().unwrap())
        .collect();
    heads.sort();
    heads.dedup();
    assert_eq!(heads.len(), 3, "each candidate has its own commit");
    for f in &frozen {
        let paths: Vec<&str> = f["payload"]["numstat"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|n| n["path"].as_str())
            .collect();
        assert_eq!(paths, ["greeting.txt"], "{f}");
    }
    // The branches stay after cleanup, each one commit on the base.
    let branches = candidate_branches(&h);
    assert_eq!(branches.len(), 3, "{branches:?}");
    for b in &branches {
        let parent = h
            .git_cmd(&["rev-parse", &format!("{b}~1")])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&parent.stdout).trim(), base);
    }
    assert_eq!(
        h.head_sha().unwrap(),
        base,
        "the main checkout is untouched"
    );
    assert_rebuild_equal(&h);
    assert_clean(&h);
}

#[test]
fn cmp_07_done() {
    let Some(h) = pair(
        "cmp07done",
        "",
        serde_json::json!({"write": {"a.txt": "a\n"}, "exit": "exit0"}),
    ) else {
        return;
    };
    let out = run_round(&h, 2, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events = events(&h);
    // B ran `horch done`; A exited 0 by itself. Both completed.
    let completed = of_kind(&events, "candidate.completed");
    assert_eq!(completed.len(), 2, "{}", text(&out));
    let a = completed
        .iter()
        .find(|e| e["payload"]["label"] == "A")
        .unwrap();
    assert_eq!(a["payload"]["exit_code"], 0);
    let b_record = candidate_records(&h)
        .into_iter()
        .find(|r| r["label"] == "B")
        .unwrap();
    let history = b_record["history"].as_array().unwrap();
    assert!(history.iter().any(|e| e["event"] == "done"), "{b_record}");
    // A candidate reports to nobody: no `horch tell` reached herdr.
    for c in h.calls_of("herdr") {
        let argv = c["argv"].to_string();
        assert!(!argv.contains("DONE:"), "a candidate reported: {argv}");
    }
    assert_every_spawn_ended(&h);
    assert_clean(&h);
}

#[test]
fn cmp_07_pane_vanished() {
    let Some(h) = pair("cmp07pane", "", serde_json::json!({"exit": "vanish"})) else {
        return;
    };
    let out = run_round(&h, 2, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events = events(&h);
    assert_eq!(
        failure_of(&events, "A"),
        serde_json::json!({"kind": "pane_vanished"})
    );
    assert_every_spawn_ended(&h);
    assert_clean(&h);
}

#[test]
fn cmp_07_agent_exit() {
    let Some(h) = pair("cmp07exit", "", serde_json::json!({"exit": "crash"})) else {
        return;
    };
    let out = run_round(&h, 2, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events = events(&h);
    assert_eq!(
        failure_of(&events, "A"),
        serde_json::json!({"kind": "agent_exited", "code": 3})
    );
    assert_every_spawn_ended(&h);
    assert_clean(&h);
}

#[test]
fn cmp_07_timeout() {
    let Some(h) = pair(
        "cmp07timeout",
        "caps:\n  candidate_deadline_s: 6\n",
        serde_json::json!({"exit": "hang"}),
    ) else {
        return;
    };
    let out = run_round(&h, 2, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events = events(&h);
    assert_eq!(
        failure_of(&events, "A"),
        serde_json::json!({"kind": "timed_out"})
    );
    // The pane was closed, which killed the hanging agent.
    let a = candidate_records(&h)
        .into_iter()
        .find(|r| r["label"] == "A")
        .unwrap();
    let pane = a["pane_id"].as_str().unwrap().to_string();
    let mut state = h.log.clone().into_os_string();
    state.push(".state.json");
    let state = std::fs::read_to_string(state).unwrap();
    assert!(!state.contains(&format!("\"{pane}\"")), "{state}");
    assert_every_spawn_ended(&h);
    assert_clean(&h);
}

#[test]
fn cmp_07_candidate_crash_kept_in_round() {
    // A writes work, commits nothing and crashes.
    let Some(h) = pair(
        "cmp07crash",
        "",
        serde_json::json!({"write": {"partial.txt": "half done\n"}, "exit": "crash"}),
    ) else {
        return;
    };
    let out = run_round(&h, 2, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events = events(&h);
    // A stays in the round: frozen with its work, validated, not eligible.
    let frozen = of_kind(&events, "candidate.frozen")
        .into_iter()
        .find(|e| e["payload"]["label"] == "A")
        .expect("A is frozen");
    let paths = frozen["payload"]["numstat"].to_string();
    assert!(paths.contains("partial.txt"), "{paths}");
    let validated = of_kind(&events, "validation.completed")
        .into_iter()
        .find(|e| e["payload"]["label"] == "A")
        .expect("A is validated");
    assert_eq!(validated["payload"]["report"]["eligible"], false);
    // B won; A's branch keeps its frozen work.
    let winner = of_kind(&events, "winner.selected");
    assert_eq!(winner.len(), 1);
    assert_eq!(winner[0]["payload"]["label"], "B");
    let head = frozen["payload"]["head_sha"].as_str().unwrap();
    let branch = candidate_branches(&h)
        .into_iter()
        .find(|b| b.ends_with("/A"))
        .unwrap();
    let tip = h.git_cmd(&["rev-parse", &branch]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&tip.stdout).trim(), head);
    assert_clean(&h);
}

#[test]
fn cmp_10_hard_budget_cancels_and_retains() {
    // One Claude candidate whose transcript shows a spend far over the
    // $100 ceiling; it then waits until it is stopped.
    let Some(h) = round_harness(
        "cmp10",
        &["sonnet"],
        "",
        serde_json::json!({"A": {"write": {"work.txt": "w\n"}, "exit": "hang",
            "usage": {"model": "claude-opus-5-5", "input": 0, "output": 1_000_000_000u64}}}),
    ) else {
        return;
    };
    let out = run_round(&h, 1, &[]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    let events = events(&h);
    assert_eq!(
        failure_of(&events, "A"),
        serde_json::json!({"kind": "cancelled", "reason": "budget"})
    );
    // Its data stays: frozen work on its branch, and the usage record.
    let frozen = of_kind(&events, "candidate.frozen");
    assert!(frozen[0]["payload"]["numstat"]
        .to_string()
        .contains("work.txt"));
    assert_eq!(candidate_branches(&h).len(), 1);
    let usage = files_under(&h.state.join("multi-herdr"))
        .into_iter()
        .find(|f| f.parent().is_some_and(|p| p.ends_with("usage")))
        .expect("a usage record");
    let usage: Value = serde_json::from_str(&std::fs::read_to_string(usage).unwrap()).unwrap();
    assert!(
        usage["cost_microusd"].as_i64().unwrap() > 90_000_000,
        "{usage}"
    );
    assert!(of_kind(&events, "judge.scheduled").is_empty());
    assert_every_spawn_ended(&h);
    assert_clean(&h);
}

#[test]
fn cmp_11_disk_pressure_stops_new_work() {
    // One candidate at a time. A fills the disk (its machine fixture now
    // shows 1 MB free), so B never starts.
    let Some(h) = pair(
        "cmp11",
        "caps:\n  max_parallel: 1\n  candidate_deadline_s: 60\n",
        Value::Null,
    ) else {
        return;
    };
    let machine = h.root.join("machine.json");
    let mut low: Value = serde_json::from_str(&std::fs::read_to_string(&machine).unwrap()).unwrap();
    low["disk_free_bytes"] = Value::from(1_000_000u64);
    let mut file = h.log.clone().into_os_string();
    file.push(".candidates.json");
    std::fs::write(
        &file,
        serde_json::json!({
            "A": {"write": {"a.txt": "a\n", machine.to_str().unwrap(): low.to_string()},
                  "commit": true, "exit": "done"},
            "B": done("b.txt"),
        })
        .to_string(),
    )
    .unwrap();
    let out = run_round(&h, 2, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events = events(&h);
    assert_eq!(of_kind(&events, "candidate.spawned").len(), 1);
    let b = failure_of(&events, "B");
    assert_eq!(b["kind"], "cancelled", "{b}");
    assert!(b["reason"].as_str().unwrap().starts_with("disk"), "{b}");
    assert_eq!(candidate_records(&h).len(), 1, "B was never started");
    assert_clean(&h);
}

/// One fault point: run with it armed, then `resume`. The resume
/// invariants: exactly N candidate executions, at most 1 judgment, the
/// projection equals a rebuild.
fn crash_and_resume(point: &str, first_code: i32, resumed_code: i32) {
    crash_and_resume_in(point, "", first_code, resumed_code);
}

/// [`crash_and_resume`] for a round that promotes onto `release` (B5). The
/// invariants add: at most 1 receipt, and `release` moved at most once.
fn crash_and_resume_promoting(point: &str) {
    let Some(h) = crash_and_resume_in(point, "promote_to: release\n", 86, 0) else {
        return;
    };
    let receipts: Vec<Value> = files_under(&h.state.join("multi-herdr"))
        .into_iter()
        .filter(|f| f.parent().is_some_and(|p| p.ends_with("promotions")))
        .map(|f| serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap())
        .collect();
    assert_eq!(receipts.len(), 1, "{point}");
    let out = h
        .git_cmd(&["reflog", "show", "--format=%H", "refs/heads/release"])
        .output()
        .unwrap();
    let moves: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    // The creation, then 1 move.
    assert_eq!(moves.len(), 2, "{point}: {moves:?}");
    assert_eq!(
        moves[0],
        receipts[0]["dest_after"].as_str().unwrap(),
        "{point}"
    );
    let events = events(&h);
    assert_eq!(of_kind(&events, "promotion.completed").len(), 1, "{point}");
    assert_eq!(round_state(&h), "COMPLETE", "{point}");
}

/// Crash at `point` in a round with `yaml`, then `resume`. The harness is
/// returned for more checks; it has a `release` branch at the base.
fn crash_and_resume_in(
    point: &str,
    yaml: &str,
    first_code: i32,
    resumed_code: i32,
) -> Option<Harness> {
    let name = format!("cmp13-{}", point.replace([':', '-'], ""));
    let h = pair(&name, yaml, done("a.txt"))?;
    let out = h.git_cmd(&["branch", "release"]).output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let out = run_round(&h, 2, &[("HORCH_FAULT", point)]);
    assert_eq!(
        out.status.code(),
        Some(first_code),
        "{point}: {}",
        text(&out)
    );
    let exp = experiment_id(&h);
    let out = dataset(&h, &["resume", &exp], &[]);
    assert_eq!(
        out.status.code(),
        Some(resumed_code),
        "{point}: {}",
        text(&out)
    );
    assert_eq!(candidate_records(&h).len(), 2, "{point}");
    let judgments = files_under(&h.state.join("multi-herdr"))
        .into_iter()
        .filter(|f| f.parent().is_some_and(|p| p.ends_with("judgements")))
        .count();
    assert!(judgments <= 1, "{point}: {judgments} judgments");
    let events = events(&h);
    assert_eq!(of_kind(&events, "round.created").len(), 1, "{point}");
    assert_eq!(of_kind(&events, "candidate.spawned").len(), 2, "{point}");
    assert_rebuild_equal(&h);
    assert_every_spawn_ended(&h);
    assert_clean(&h);
    Some(h)
}

#[test]
fn cmp_13_crash_every_boundary() {
    // The coordinator's points: the process aborts (86), `resume` finishes.
    for point in [
        "abort-after-experiment-created",
        "abort-after-preflight",
        "abort-after-worktree:1",
        "abort-after-candidate-spawned:1",
        "abort-after-freeze:A",
        "abort-after-validation:A",
        "abort-after-judge-scheduled",
        "abort-after-judgment-written",
        "abort-after-winner-selected",
        "abort-during-cleanup:1",
    ] {
        crash_and_resume(point, 86, 0);
    }
    // B5: the promotion's points, in a round with `promote_to`.
    for point in [
        "abort-after-promotion-started",
        "abort-after-update-ref",
        "abort-after-receipt",
    ] {
        crash_and_resume_promoting(point);
    }
    // The judge job's points: the job dies, not the coordinator. After the
    // output the answer counts; before it, both attempts fail and the round
    // needs the operator.
    crash_and_resume("abort-after-judge-output", 0, 0);
    crash_and_resume("abort-in-judge-job-before-output", 5, 5);
}

#[test]
fn cmp_13_rebuild_after_power_loss() {
    let Some(h) = pair("cmp13power", "", done("a.txt")) else {
        return;
    };
    let out = run_round(&h, 2, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    // Power loss: the last event line is half written.
    let mut files: Vec<PathBuf> = files_under(&h.state.join("multi-herdr"))
        .into_iter()
        .filter(|f| f.parent().is_some_and(|p| p.ends_with("events")))
        .collect();
    files.sort();
    let last = files.last().unwrap();
    let text_now = std::fs::read_to_string(last).unwrap();
    let trimmed = text_now.trim_end_matches('\n');
    let cut = trimmed.rfind('\n').map_or(0, |i| i + 1)
        + (trimmed.len() - trimmed.rfind('\n').map_or(0, |i| i + 1)) / 2;
    std::fs::write(last, &text_now[..cut]).unwrap();
    let status = dataset(&h, &["status"], &[]);
    assert!(
        String::from_utf8_lossy(&status.stdout).contains("1 torn lines skipped"),
        "{}",
        text(&status)
    );
    assert_rebuild_equal(&h);
    // `resume` redoes the lost step once; the rebuild still matches.
    let exp = experiment_id(&h);
    let out = dataset(&h, &["resume", &exp], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(round_state(&h), "COMPLETE");
    assert_eq!(candidate_records(&h).len(), 2);
    assert_rebuild_equal(&h);
    assert_clean(&h);
}

#[test]
fn cmp_14_all_candidates_fail_round_rejected() {
    let crash = serde_json::json!({"write": {"x.txt": "x\n"}, "exit": "crash"});
    let Some(h) = round_harness(
        "cmp14",
        &PAIR,
        "",
        serde_json::json!({"A": crash, "B": crash}),
    ) else {
        return;
    };
    let out = run_round(&h, 2, &[]);
    assert_eq!(out.status.code(), Some(6), "{}", text(&out));
    let events = events(&h);
    let rejected = of_kind(&events, "winner.rejected");
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0]["payload"]["reason"], "no_eligible");
    assert!(of_kind(&events, "judge.scheduled").is_empty(), "no judge");
    // Every candidate's data stays: frozen, validated, branch kept.
    assert_eq!(of_kind(&events, "candidate.frozen").len(), 2);
    assert_eq!(of_kind(&events, "validation.completed").len(), 2);
    assert_eq!(candidate_branches(&h).len(), 2);
    assert_eq!(round_state(&h), "COMPLETE");
    assert_rebuild_equal(&h);
    assert_clean(&h);
}

#[test]
fn cmp_15_candidate_task_carries_rules() {
    let Some(h) = pair("cmp15", "", done("a.txt")) else {
        return;
    };
    let out = run_round(&h, 2, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events = events(&h);
    let launch = h
        .calls_of("claude")
        .into_iter()
        .find(|c| c["candidate"].is_string())
        .expect("a candidate ran on claude");
    let label = launch["candidate"].as_str().unwrap();
    let path = of_kind(&events, "worktree.created")
        .into_iter()
        .find(|e| e["payload"]["label"] == label)
        .map(|e| e["payload"]["path"].as_str().unwrap().to_string())
        .unwrap();
    let argv = launch["argv"].to_string();
    for rule in [
        "== Competition rules ==",
        "Work only in this directory",
        "Never push.",
        "Never message an orchestrator.",
        "horch done",
        "add a greeting",
    ] {
        assert!(argv.contains(rule), "{rule}: {argv}");
    }
    assert!(argv.contains(&path), "{path}: {argv}");
    assert_clean(&h);
}

#[test]
fn mea_10_every_spawn_has_terminal_event() {
    let Some(h) = round_harness(
        "mea10",
        &["sonnet", "codex-sol", "opus"],
        "caps:\n  candidate_deadline_s: 6\n",
        serde_json::json!({
            "A": done("a.txt"),
            "B": {"exit": "crash"},
            "C": {"exit": "hang"},
        }),
    ) else {
        return;
    };
    let out = run_round(&h, 3, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events = events(&h);
    assert_eq!(of_kind(&events, "candidate.spawned").len(), 3);
    assert_every_spawn_ended(&h);
    assert_clean(&h);
}

#[test]
fn sec_03_no_transcript_copies_by_default() {
    let usage = serde_json::json!({"model": "claude-opus-5-5", "input": 10, "output": 20});
    for retain in [false, true] {
        let yaml = if retain {
            "retain_transcripts: true\n"
        } else {
            ""
        };
        let Some(h) = round_harness(
            &format!("sec03{retain}"),
            &["sonnet"],
            yaml,
            serde_json::json!({"A": {"write": {"a.txt": "a\n"}, "commit": true,
                                     "exit": "done", "usage": usage}}),
        ) else {
            return;
        };
        let out = run_round(&h, 1, &[]);
        assert_eq!(out.status.code(), Some(0), "{}", text(&out));
        let files = files_under(&h.state.join("multi-herdr"));
        let record = files
            .iter()
            .find(|f| f.parent().is_some_and(|p| p.ends_with("usage")))
            .expect("a usage record");
        let record: Value =
            serde_json::from_str(&std::fs::read_to_string(record).unwrap()).unwrap();
        assert!(record["transcript_ref"].is_string(), "{record}");
        assert!(
            record["transcript_digest"]
                .as_str()
                .is_some_and(|d| d.starts_with("sha256:")),
            "{record}"
        );
        // The transcript's bytes (its message id) are in the dataset only
        // when retention is asked for.
        let copies = files
            .iter()
            .filter(|f| std::fs::read_to_string(f).is_ok_and(|t| t.contains("msg_candidate")))
            .count();
        assert_eq!(copies, usize::from(retain), "retain={retain}");
        assert_clean(&h);
    }
}

#[test]
fn sec_08_e2e_no_api_key_in_any_child() {
    let Some(h) = pair("sec08", "", done("a.txt")) else {
        return;
    };
    // A dummy value, never a real key.
    let out = run_round(&h, 2, &[("ANTHROPIC_API_KEY", "SENTINEL")]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let calls = h.calls();
    assert!(calls.iter().any(|c| c["candidate"].is_string()));
    assert!(calls.iter().any(|c| c["mode"] == "judge"));
    for c in &calls {
        if c["fake"] == "herdr" {
            continue;
        }
        let keys = c["env_keys"].to_string();
        assert!(!keys.contains("ANTHROPIC_API_KEY"), "{c}");
    }
    for f in h.state_files() {
        let bytes = std::fs::read(&f).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains("SENTINEL"),
            "{} holds the key",
            f.display()
        );
    }
    assert_clean(&h);
}

#[test]
fn jdg_e2e_round_decided() {
    let Some(h) = pair("jdge2e", "", done("a.txt")) else {
        return;
    };
    let out = run_round(&h, 2, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events = events(&h);
    let kinds = kinds(&events);
    for k in [
        "judge.scheduled",
        "judge.started",
        "judge.completed",
        "winner.selected",
        "round.cleanup_started",
        "round.completed",
    ] {
        assert!(kinds.contains(&k), "{k}: {kinds:?}");
    }
    let winner = &of_kind(&events, "winner.selected")[0]["payload"];
    assert!(["A", "B"].contains(&winner["label"].as_str().unwrap()));
    assert_eq!(round_state(&h), "COMPLETE");
    let judgment = files_under(&h.state.join("multi-herdr"))
        .into_iter()
        .filter(|f| f.parent().is_some_and(|p| p.ends_with("judgements")))
        .count();
    assert_eq!(judgment, 1);
    // The worktrees are gone; the branches stay.
    assert_eq!(candidate_branches(&h).len(), 2);

    // `horch sessions` hides candidates and the judge; `--all` shows them.
    let judge = records(&h)
        .into_iter()
        .find(|r| r["label"] == "judge:1")
        .expect("the judge record");
    let plain = h.run(&["sessions"]);
    let plain = String::from_utf8_lossy(&plain.stdout).into_owned();
    let all = h.run(&["sessions", "--all"]);
    let all = String::from_utf8_lossy(&all.stdout).into_owned();
    for r in candidate_records(&h).iter().chain([&judge]) {
        let id = r["record_id"].as_str().unwrap();
        assert!(!plain.contains(id), "{id} listed: {plain}");
        assert!(all.contains(id), "{id} missing: {all}");
    }
    // `horch spawn --resume` refuses a candidate.
    let id = candidate_records(&h)[0]["record_id"]
        .as_str()
        .unwrap()
        .to_string();
    let resumed = h
        .horch(&["spawn", "--resume", &id, "go on"])
        .env("HORCH_WORKSPACE_ID", "w1")
        .output()
        .unwrap();
    assert!(!resumed.status.success());
    assert!(
        String::from_utf8_lossy(&resumed.stderr).contains("competition candidate"),
        "{}",
        text(&resumed)
    );
    assert_clean(&h);
}
