//! End to end: opt-in promotion from the dataset binary (B5, PRO-08,
//! PRO-02, dataset design §5.2).
//!
//! `run` without `--promote-to` only collects. `run --promote-to <branch>`
//! and a later `promote <round> --to <branch>` move the branch once, with a
//! receipt. `rollback` and `cleanup` are the operator's other commands.
//!
//! Each test runs whole rounds of the real binary in a sealed harness with a
//! real git repo and the fakes, as `tests/dataset.rs` does. The helpers
//! below are a copy of that file's round helpers: test files do not share
//! code.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_e2e::bin_dir;
use horch_e2e::harness::Harness;
use serde_json::{json, Value};

/// The branch the tests promote onto. No worktree has it checked out, so
/// the engine publishes with a compare-and-swap `update-ref`.
const TARGET: &str = "release";

// ── round helpers (a copy of tests/dataset.rs) ───────────────────────────

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

fn machine_fixture(h: &Harness) -> PathBuf {
    let file = h.root.join("machine.json");
    let snapshot = json!({
        "os": "macos",
        "arch": "arm64",
        "cpus": 18,
        "mem_total_bytes": 137_438_953_472u64,
        "mem_available_bytes": 85_899_345_920u64,
        "disk_free_bytes": 500_000_000_000u64,
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

/// A 2-candidate round harness (A is codex-sol, B is sonnet), with the
/// branch [`TARGET`] at the initial commit and `sh` on the sealed PATH for
/// the gates. `None` when git is missing (and not required).
fn round_harness(name: &str, yaml: &str, candidates: Value) -> Option<Harness> {
    let mut h = Harness::new(name).with_git();
    h.git_bin()?;
    let machine = machine_fixture(&h);
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
            "exclude: {}\ncaps:\n  candidate_deadline_s: 60\n{yaml}",
            Value::from(exclude)
        ),
    )
    .unwrap();
    git(&h, &["add", "-A"]);
    git(
        &h,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "dataset config"],
    );
    git(&h, &["branch", TARGET]);
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

fn tip(h: &Harness, branch: &str) -> String {
    git(h, &["rev-parse", &format!("refs/heads/{branch}")])
}

fn done(file: &str) -> Value {
    json!({"write": {file: "hello\n"}, "commit": true, "exit": "done"})
}

fn dataset(h: &Harness, args: &[&str], extra: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(dataset_bin());
    cmd.args(args);
    h.seal(&mut cmd);
    cmd.envs(extra.iter().copied());
    cmd.output().expect("running multi-herdr-dataset")
}

fn run_round(h: &Harness, more: &[&str], extra: &[(&str, &str)]) -> Output {
    let mut args = vec![
        "run",
        "add a greeting",
        "--candidates",
        "2",
        "--budget-usd",
        "100",
    ];
    args.extend_from_slice(more);
    dataset(h, &args, extra)
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

/// The rounds in creation order.
fn rounds(h: &Harness) -> Vec<String> {
    of_kind(&events(h), "round.created")
        .iter()
        .map(|e| e["round_id"].as_str().unwrap().to_string())
        .collect()
}

/// The state `status` prints for `round`.
fn round_state(h: &Harness, round: &str) -> String {
    let out = dataset(h, &["status"], &[]);
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.trim_start().starts_with("round ") && l.contains(round))
        .and_then(|l| l.split_whitespace().nth(3))
        .unwrap_or_default()
        .to_string()
}

/// The receipts under `promotions/`.
fn receipts(h: &Harness) -> Vec<Value> {
    files_under(&h.state.join("multi-herdr"))
        .into_iter()
        .filter(|f| f.parent().is_some_and(|p| p.ends_with("promotions")))
        .map(|f| serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap())
        .collect()
}

/// The frozen head of `label` in `round`.
fn frozen_head(events: &[Value], round: &str, label: &str) -> String {
    of_kind(events, "candidate.frozen")
        .into_iter()
        .find(|e| e["round_id"] == round && e["payload"]["label"] == label)
        .map(|e| e["payload"]["head_sha"].as_str().unwrap().to_string())
        .unwrap_or_else(|| panic!("no freeze of {label} in {round}"))
}

fn winner(events: &[Value], round: &str) -> Value {
    of_kind(events, "winner.selected")
        .into_iter()
        .find(|e| e["round_id"] == round)
        .map(|e| e["payload"].clone())
        .unwrap_or(Value::Null)
}

/// The candidate branches that exist.
fn candidate_branches(h: &Harness) -> Vec<String> {
    git(
        h,
        &["for-each-ref", "--format=%(refname)", "refs/heads/mh/exp/"],
    )
    .lines()
    .map(str::to_string)
    .collect()
}

/// The worktrees git lists, the main checkout included.
fn worktree_count(h: &Harness) -> usize {
    git(h, &["worktree", "list", "--porcelain"])
        .matches("worktree ")
        .count()
}

/// `rebuild <exp>` exits 0 for every experiment.
fn assert_rebuild_equal(h: &Harness) {
    let events = events(h);
    let mut exps: Vec<&str> = events
        .iter()
        .filter_map(|e| e["experiment_id"].as_str())
        .collect();
    exps.dedup();
    for exp in exps {
        let out = dataset(h, &["rebuild", exp], &[]);
        assert!(out.status.success(), "{}", text(&out));
    }
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

/// The round promoted `label` onto [`TARGET`] once: one start, one
/// completion, one receipt for the round, and the branch at the receipt's
/// `dest_after`.
fn assert_promoted_once(h: &Harness, round: &str) -> Value {
    let events = events(h);
    let in_round = |kind: &str| {
        of_kind(&events, kind)
            .into_iter()
            .filter(|e| e["round_id"] == round)
            .count()
    };
    assert_eq!(in_round("promotion.started"), 1, "{round}");
    assert_eq!(in_round("promotion.completed"), 1, "{round}");
    let receipt = receipts(h)
        .into_iter()
        .find(|r| r["round_id"] == round)
        .unwrap_or_else(|| panic!("no receipt for {round}"));
    assert_eq!(receipt["dest_ref"], format!("refs/heads/{TARGET}"));
    assert_eq!(tip(h, TARGET), receipt["dest_after"].as_str().unwrap());
    assert_eq!(round_state(h, round), "COMPLETE");
    let completed = of_kind(&events, "round.completed")
        .into_iter()
        .rfind(|e| e["round_id"] == round)
        .unwrap();
    assert_eq!(completed["payload"]["final_outcome"], "promoted");
    receipt
}

// ── PRO-08 ───────────────────────────────────────────────────────────────

#[test]
fn pro_08_default_collects_only() {
    let Some(h) = round_harness(
        "pro08default",
        "",
        json!({"A": done("a.txt"), "B": done("b.txt")}),
    ) else {
        return;
    };
    let before = tip(&h, TARGET);
    let main_before = tip(&h, "main");
    let out = run_round(&h, &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let round = &rounds(&h)[0];
    let events = events(&h);
    assert_eq!(winner(&events, round)["promotion"], "not_requested");
    assert_eq!(round_state(&h, round), "COMPLETE");
    // Nothing moved, nothing was published.
    assert_eq!(tip(&h, TARGET), before);
    assert_eq!(tip(&h, "main"), main_before);
    assert!(of_kind(&events, "promotion.started").is_empty());
    assert!(receipts(&h).is_empty());
    // The worktrees are gone; the branches stay.
    assert_eq!(worktree_count(&h), 1);
    assert_eq!(candidate_branches(&h).len(), 2);
    assert_rebuild_equal(&h);
    assert_clean(&h);
}

#[test]
fn pro_08_promote_to_and_promote_cmd() {
    let Some(h) = round_harness(
        "pro08promote",
        "",
        json!({"A": done("a.txt"), "B": done("b.txt")}),
    ) else {
        return;
    };
    // 1. `run --promote-to`: the target fast-forwards to the winner.
    let before = tip(&h, TARGET);
    let out = run_round(&h, &["--promote-to", TARGET], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(text(&out).contains("PROMOTED"), "{}", text(&out));
    let first = rounds(&h)[0].clone();
    let events1 = events(&h);
    let w = winner(&events1, &first);
    assert_eq!(w["promotion"]["requested"]["target"], TARGET, "{w}");
    let receipt = assert_promoted_once(&h, &first);
    assert_eq!(receipt["dest_before"], before);
    assert_eq!(receipt["strategy"], "fast_forward");
    assert_eq!(receipt["publish"], "update_ref_cas");
    let label = w["label"].as_str().unwrap();
    assert_eq!(tip(&h, TARGET), frozen_head(&events1, &first, label));
    assert_eq!(worktree_count(&h), 1, "cleanup ran after the receipt");

    // 2. A round collected first (other files, so its commits pick
    //    cleanly onto the moved target), then `promote <round> --to`.
    set_candidates(&h, json!({"A": done("c.txt"), "B": done("d.txt")}));
    let out = run_round(&h, &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let second = rounds(&h)[1].clone();
    assert_eq!(round_state(&h, &second), "COMPLETE");
    let moved = tip(&h, TARGET);
    let out = dataset(&h, &["promote", &second, "--to", TARGET], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let events2 = events(&h);
    let operator: Vec<&Value> = of_kind(&events2, "operator.promote")
        .into_iter()
        .filter(|e| e["round_id"] == second.as_str())
        .collect();
    assert_eq!(operator.len(), 1);
    assert_eq!(operator[0]["payload"]["target"], TARGET);
    assert_eq!(operator[0]["actor"], "operator");
    let receipt = assert_promoted_once(&h, &second);
    assert_eq!(receipt["dest_before"], moved);
    // The target had moved past the round's base: cherry-picked.
    assert_eq!(receipt["strategy"], "cherry_pick");
    assert_eq!(receipts(&h).len(), 2);
    // The cherry-picked commit carries the winner's file on top of round 1.
    let label = winner(&events2, &second)["label"]
        .as_str()
        .unwrap()
        .to_string();
    let file = if label == "A" { "c.txt" } else { "d.txt" };
    git(&h, &["cat-file", "-e", &format!("{TARGET}:{file}")]);

    // A third promotion of the same round is refused, and moves nothing.
    let now = tip(&h, TARGET);
    let out = dataset(&h, &["promote", &second, "--to", TARGET], &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("already promoted"), "{}", text(&out));
    assert_eq!(tip(&h, TARGET), now);
    assert_rebuild_equal(&h);
    assert_clean(&h);
}

// ── PRO-02 ───────────────────────────────────────────────────────────────

/// A gate that fails when the tree holds `bad.txt`.
const GATE: &str =
    "gates:\n  - name: no-bad\n    command: \"test ! -f bad.txt\"\n    timeout_s: 30\n";

#[test]
fn pro_02_e2e_only_valid_candidate_promotable() {
    // A writes `bad.txt`: its gate fails, so it is not eligible. B passes.
    let Some(h) = round_harness(
        "pro02e2e",
        GATE,
        json!({"A": done("bad.txt"), "B": done("b.txt")}),
    ) else {
        return;
    };
    let before = tip(&h, TARGET);
    let out = run_round(&h, &["--promote-to", TARGET], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let round = rounds(&h)[0].clone();
    let ev = events(&h);
    let eligible = |label: &str| {
        of_kind(&ev, "validation.completed")
            .into_iter()
            .find(|e| e["payload"]["label"] == label)
            .map(|e| e["payload"]["report"]["eligible"].clone())
            .unwrap()
    };
    assert_eq!(eligible("A"), false);
    assert_eq!(eligible("B"), true);
    assert_eq!(winner(&ev, &round)["label"], "B");
    // The integrated commit was validated again before the publish.
    let started = &of_kind(&ev, "promotion.started")[0]["payload"];
    assert_eq!(started["validation_ids"].as_array().unwrap().len(), 1);
    let receipt = assert_promoted_once(&h, &round);
    assert_eq!(receipt["label"], "B");
    assert_eq!(receipt["dest_before"], before);
    assert_eq!(tip(&h, TARGET), frozen_head(&ev, &round, "B"));
    // Only B's work is on the target.
    git(&h, &["cat-file", "-e", &format!("{TARGET}:b.txt")]);
    assert!(!h
        .git_cmd(&["cat-file", "-e", &format!("{TARGET}:bad.txt")])
        .output()
        .unwrap()
        .status
        .success());

    // `fail-gate:<name>` reaches the gates from `run`: no candidate is
    // eligible, the judge is skipped, nothing is promoted (exit 6).
    set_candidates(&h, json!({"A": done("c.txt"), "B": done("d.txt")}));
    let promoted = tip(&h, TARGET);
    let out = run_round(
        &h,
        &["--promote-to", TARGET],
        &[("HORCH_FAULT", "fail-gate:no-bad")],
    );
    assert_eq!(out.status.code(), Some(6), "{}", text(&out));
    let second = rounds(&h)[1].clone();
    assert!(winner(&events(&h), &second).is_null());
    assert_eq!(tip(&h, TARGET), promoted);
    assert_rebuild_equal(&h);
    assert_clean(&h);
}

// ── rollback and cleanup ─────────────────────────────────────────────────

#[test]
fn pro_rollback_e2e() {
    let Some(h) = round_harness(
        "prorollback",
        "",
        json!({"A": done("a.txt"), "B": done("b.txt")}),
    ) else {
        return;
    };
    let before = tip(&h, TARGET);
    let out = run_round(&h, &["--promote-to", TARGET], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let round = rounds(&h)[0].clone();
    let receipt = assert_promoted_once(&h, &round);
    let after = receipt["dest_after"].as_str().unwrap().to_string();

    // Someone moved the target since: rollback refuses (exit 5), and
    // touches nothing.
    let tree = git(&h, &["rev-parse", &format!("{after}^{{tree}}")]);
    let other = git(
        &h,
        &["commit-tree", &tree, "-p", &after, "-m", "later work"],
    );
    git(&h, &["update-ref", &format!("refs/heads/{TARGET}"), &other]);
    let out = dataset(&h, &["rollback", &round], &[]);
    assert_eq!(out.status.code(), Some(5), "{}", text(&out));
    assert!(text(&out).contains("moved"), "{}", text(&out));
    assert_eq!(tip(&h, TARGET), other);
    assert!(of_kind(&events(&h), "promotion.rolled_back").is_empty());

    // Back at dest_after: the rollback swaps it to dest_before.
    git(&h, &["update-ref", &format!("refs/heads/{TARGET}"), &after]);
    let out = dataset(&h, &["rollback", &round], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(tip(&h, TARGET), before);
    let events1 = events(&h);
    let rolled = of_kind(&events1, "promotion.rolled_back");
    assert_eq!(rolled.len(), 1);
    assert_eq!(rolled[0]["actor"], "operator");
    assert_eq!(rolled[0]["payload"]["restored"], before);

    // A second rollback changes nothing and records nothing new.
    let out = dataset(&h, &["rollback", &round], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(tip(&h, TARGET), before);
    assert_eq!(of_kind(&events(&h), "promotion.rolled_back").len(), 1);
    assert_eq!(round_state(&h, &round), "COMPLETE");

    // A round that was never promoted has nothing to roll back.
    set_candidates(&h, json!({"A": done("c.txt"), "B": done("d.txt")}));
    let out = run_round(&h, &[], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let second = rounds(&h)[1].clone();
    let out = dataset(&h, &["rollback", &second], &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("never promoted"), "{}", text(&out));
    assert_rebuild_equal(&h);
    assert_clean(&h);
}

#[test]
fn pro_cleanup_needs_force_from_intervention() {
    // The judge fails both attempts: the round needs the operator, and
    // keeps its worktrees.
    let Some(h) = round_harness(
        "procleanup",
        "",
        json!({"A": done("a.txt"), "B": done("b.txt")}),
    ) else {
        return;
    };
    let out = run_round(
        &h,
        &["--promote-to", TARGET],
        &[("HORCH_FAULT", "abort-in-judge-job-before-output")],
    );
    assert_eq!(out.status.code(), Some(5), "{}", text(&out));
    let round = rounds(&h)[0].clone();
    assert_eq!(round_state(&h, &round), "NEEDS_INTERVENTION");
    assert_eq!(worktree_count(&h), 3);

    // `promote` needs a winner; this round has none.
    let out = dataset(&h, &["promote", &round, "--to", TARGET], &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(text(&out).contains("without a winner"), "{}", text(&out));

    // `cleanup` without `--force` refuses (exit 5) and removes nothing.
    let out = dataset(&h, &["cleanup", &round], &[]);
    assert_eq!(out.status.code(), Some(5), "{}", text(&out));
    assert!(text(&out).contains("--force"), "{}", text(&out));
    assert_eq!(worktree_count(&h), 3);
    assert_eq!(round_state(&h, &round), "NEEDS_INTERVENTION");

    // `--force` removes the worktrees and keeps the branches.
    let out = dataset(&h, &["cleanup", &round, "--force"], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(worktree_count(&h), 1);
    assert_eq!(candidate_branches(&h).len(), 2);
    assert_eq!(round_state(&h, &round), "COMPLETE");
    let events = events(&h);
    let completed = of_kind(&events, "round.completed");
    assert_eq!(completed.len(), 1);
    assert_eq!(
        completed[0]["payload"]["final_outcome"],
        "needs_intervention"
    );

    // Again: already complete, nothing to do.
    let out = dataset(&h, &["cleanup", &round, "--force"], &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(text(&out).contains("already COMPLETE"), "{}", text(&out));
    assert_rebuild_equal(&h);
    assert_clean(&h);
}
