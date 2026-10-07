//! End to end: the competition-mode additions to `multi-herdr-dataset run`
//! (docs/specs/fleet-dataset.md §7.2, FDS-09, FDS-10).
//!
//! `--plan <PATH>` builds the task text and refuses a plan the base commit
//! does not hold. `--detach` runs preflight in the caller, starts the round
//! in a pane of its dataset workspace and returns at once; the pane runs
//! `resume --report-to`, and the round's 1 report line reaches the caller's
//! pane.
//!
//! Each test runs the real binary in a sealed harness with a real git repo
//! and the fakes, as `tests/dataset.rs` and `tests/promotion.rs` do. The
//! helpers below are a copy of their round helpers: test files do not share
//! code.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_e2e::bin_dir;
use horch_e2e::harness::Harness;
use serde_json::{json, Value};

/// The harness fakes that stand for agent CLIs. Any call to one of them
/// other than `--version` would be a model launch.
const AGENTS: [&str; 5] = ["claude", "codex", "opencode", "pi", "prime"];

/// The plan file every test commits (or not).
const PLAN: &str = "docs/plan.md";

// ── round helpers (a copy of tests/promotion.rs) ─────────────────────────

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

fn machine_fixture(h: &Harness, disk_free: u64) -> PathBuf {
    let file = h.root.join("machine.json");
    let snapshot = json!({
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

/// Every teammate in the repo roster except `keep`.
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

/// A 2-candidate round harness (A is codex-sol, B is sonnet) with `sh` on
/// the sealed PATH for the gates, and the fake herdr running pane commands.
/// `None` when git is missing (and not required).
fn round_harness(name: &str, disk_free: u64, candidates: Value) -> Option<Harness> {
    let mut h = Harness::new(name).with_git();
    h.git_bin()?;
    let machine = machine_fixture(&h, disk_free);
    h.set("HORCH_MACHINE_FILE", machine.to_string_lossy());
    h.set("HORCH_FAKE_SCENARIO", "exec");
    h.set(
        "HORCH_QUOTA_FILE",
        horch_e2e::harness::fixtures()
            .join("quota/all-ok.json")
            .to_string_lossy(),
    );
    #[cfg(unix)]
    std::os::unix::fs::symlink("/bin/sh", h.bin.join("sh")).unwrap();
    let dir = h.project.join(".multi-herdr");
    std::fs::create_dir_all(&dir).unwrap();
    let exclude: Vec<Value> = exclude_all_but(&["sonnet", "codex-sol"])
        .into_iter()
        .map(Value::from)
        .collect();
    std::fs::write(
        dir.join("dataset.yaml"),
        format!(
            "exclude: {}\ncaps:\n  candidate_deadline_s: 60\n",
            Value::from(exclude)
        ),
    )
    .unwrap();
    git(&h, &["add", "-A"]);
    git(
        &h,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "dataset config"],
    );
    set_candidates(&h, candidates);
    Some(h)
}

/// What each candidate does in the next round, keyed by label.
fn set_candidates(h: &Harness, candidates: Value) {
    let mut file = h.log.clone().into_os_string();
    file.push(".candidates.json");
    std::fs::write(file, candidates.to_string()).unwrap();
}

fn git(h: &Harness, args: &[&str]) -> String {
    let out = h.git_cmd(args).output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Write the plan file into the project.
fn write_plan(h: &Harness, text: &str) {
    let file = h.project.join(PLAN);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

fn commit_plan(h: &Harness) {
    write_plan(h, "# Plan\n\nAdd a greeting.\n");
    git(h, &["add", "--", PLAN]);
    git(
        h,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "Plan: greeting"],
    );
}

fn dataset_in(h: &Harness, cwd: &Path, args: &[&str], extra: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(dataset_bin());
    cmd.args(args);
    h.seal(&mut cmd);
    cmd.current_dir(cwd);
    cmd.envs(extra.iter().copied());
    cmd.output().expect("running multi-herdr-dataset")
}

fn dataset(h: &Harness, args: &[&str], extra: &[(&str, &str)]) -> Output {
    dataset_in(h, &h.project, args, extra)
}

/// `run --plan <plan> --candidates 2 --budget-usd 100 <more>`.
fn run_plan(h: &Harness, cwd: &Path, plan: &str, more: &[&str], extra: &[(&str, &str)]) -> Output {
    let mut args = vec![
        "run",
        "--plan",
        plan,
        "--candidates",
        "2",
        "--budget-usd",
        "100",
    ];
    args.extend_from_slice(more);
    dataset_in(h, cwd, &args, extra)
}

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
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

fn of_kind<'a>(events: &'a [Value], kind: &str) -> Vec<&'a Value> {
    events.iter().filter(|e| e["kind"] == kind).collect()
}

/// The checks each `experiment.aborted` names, in order.
fn failed_checks(events: &[Value]) -> Vec<Vec<String>> {
    of_kind(events, "experiment.aborted")
        .iter()
        .map(|e| {
            e["payload"]["failed_checks"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .collect()
}

/// The task text each experiment's manifest records, in creation order.
fn manifest_tasks(h: &Harness) -> Vec<String> {
    of_kind(&events(h), "experiment.created")
        .iter()
        .map(|e| {
            let exp = e["experiment_id"].as_str().unwrap();
            let manifest = files_under(&h.state.join("multi-herdr"))
                .into_iter()
                .find(|f| f.ends_with(Path::new(exp).join("manifest.json")))
                .expect("the manifest exists");
            let manifest: Value =
                serde_json::from_str(&std::fs::read_to_string(manifest).unwrap()).unwrap();
            manifest["task"].as_str().unwrap().to_string()
        })
        .collect()
}

/// The PRE-01 check of the last `preflight.completed`.
fn pre_01(events: &[Value]) -> Value {
    let report = &of_kind(events, "preflight.completed").last().unwrap()["payload"]["report"];
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "PRE-01")
        .unwrap()
        .clone()
}

/// The worktrees git lists, the main checkout included.
fn worktree_count(h: &Harness) -> usize {
    git(h, &["worktree", "list", "--porcelain"])
        .matches("worktree ")
        .count()
}

/// Agent fakes were asked for `--version` only: no model turn.
fn assert_no_agent_launch(h: &Harness) {
    for agent in AGENTS {
        for call in h.calls_of(agent) {
            assert_eq!(call["argv"], json!(["--version"]), "{agent}: {call}");
        }
    }
}

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

// ── FDS-09 ───────────────────────────────────────────────────────────────

/// `--plan` without TASK briefs the candidates with "Read and follow
/// <PATH> exactly.", <PATH> relative to the project root from any cwd.
/// A machine with no free disk stops each run at PRE-02, before any
/// worktree; the committed plan passes PRE-01.
#[test]
fn fds_09_plan_task_text() {
    let Some(h) = round_harness("fds09text", 0, json!({})) else {
        return;
    };
    commit_plan(&h);
    let from_root = run_plan(&h, &h.project, PLAN, &[], &[]);
    assert_eq!(from_root.status.code(), Some(4), "{}", text(&from_root));
    let from_docs = run_plan(&h, &h.project.join("docs"), "plan.md", &[], &[]);
    assert_eq!(from_docs.status.code(), Some(4), "{}", text(&from_docs));
    let absolute = h.project.join(PLAN);
    let from_abs = run_plan(&h, &h.project, absolute.to_str().unwrap(), &[], &[]);
    assert_eq!(from_abs.status.code(), Some(4), "{}", text(&from_abs));

    let want = format!("Read and follow {PLAN} exactly.");
    assert_eq!(manifest_tasks(&h), [want.clone(), want.clone(), want]);
    let events = events(&h);
    for failed in failed_checks(&events) {
        assert!(failed.contains(&"PRE-02".to_string()), "{failed:?}");
        assert!(!failed.contains(&"PRE-01".to_string()), "{failed:?}");
    }
    assert_eq!(pre_01(&events)["status"], "pass");

    // A TASK wins over the plan's text.
    let out = dataset(
        &h,
        &[
            "run",
            "add a greeting",
            "--plan",
            PLAN,
            "--candidates",
            "2",
            "--budget-usd",
            "100",
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(4), "{}", text(&out));
    assert_eq!(manifest_tasks(&h).last().unwrap(), "add a greeting");
    assert_eq!(worktree_count(&h), 1);
    assert_no_agent_launch(&h);
    assert_clean(&h);
}

/// A plan the base commit does not hold is refused by PRE-01 (exit 4):
/// untracked, then committed but changed in the work tree. The report is
/// persisted, the reason names the fix, and nothing is created.
#[test]
fn fds_09_plan_untracked_or_dirty_refused() {
    let Some(h) = round_harness("fds09refused", 500_000_000_000, json!({})) else {
        return;
    };
    let fix = format!("commit {PLAN} first");
    write_plan(&h, "# Plan\n");
    let untracked = run_plan(&h, &h.project, PLAN, &["--allow-dirty"], &[]);
    assert_eq!(untracked.status.code(), Some(4), "{}", text(&untracked));
    assert!(text(&untracked).contains("REFUSED"), "{}", text(&untracked));
    assert!(
        text(&untracked).contains("not tracked"),
        "{}",
        text(&untracked)
    );
    assert!(text(&untracked).contains(&fix), "{}", text(&untracked));

    commit_plan(&h);
    write_plan(&h, "# Plan\n\nAdd a greeting, then more.\n");
    let dirty = run_plan(&h, &h.project, PLAN, &["--allow-dirty"], &[]);
    assert_eq!(dirty.status.code(), Some(4), "{}", text(&dirty));
    assert!(text(&dirty).contains("differs"), "{}", text(&dirty));
    assert!(text(&dirty).contains(&fix), "{}", text(&dirty));

    let events = events(&h);
    assert_eq!(failed_checks(&events), [vec!["PRE-01"], vec!["PRE-01"]]);
    let check = pre_01(&events);
    assert_eq!(check["status"], "fail");
    assert!(
        check["measured"]["plan_problem"]
            .as_str()
            .unwrap()
            .contains(&fix),
        "{check}"
    );
    assert!(of_kind(&events, "round.created").is_empty());
    assert!(of_kind(&events, "worktree.created").is_empty());
    assert_eq!(worktree_count(&h), 1);
    assert_no_agent_launch(&h);
    assert_clean(&h);
}
