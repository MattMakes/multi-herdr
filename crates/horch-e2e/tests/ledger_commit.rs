//! `horch ledger commit` at the CLI seam (T-10, decision 01b): the 1 trusted
//! way a sandboxed worker commits. The real `horch` binary, a temporary
//! ledger file and git repository per test.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_e2e::harness::Harness;
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn ok_as(h: &Harness, worker: &str, args: &[&str]) -> String {
    let o = as_worker(h, worker, &h.project, args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

/// `horch <args>` as `worker`, from `dir`.
fn as_worker(h: &Harness, worker: &str, dir: &Path, args: &[&str]) -> Output {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", worker).current_dir(dir);
    cmd.output().unwrap()
}

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    out(&o).trim().to_string()
}

/// A run with 1 claimed task and its worktree: the harness, the worker id
/// and the worktree path. The project's git config runs a `pre-commit` hook
/// from the worktree that writes `hook-ran` in the harness root.
fn claimed(name: &str) -> (Harness, String, PathBuf) {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    let sweep = h.root.join("code-sweep");
    std::fs::create_dir_all(sweep.join("reviewers")).unwrap();
    for r in ["bug-hunter", "clarity-checker"] {
        std::fs::write(
            sweep.join("reviewers").join(format!("{r}.md")),
            format!("You are {r}.\n"),
        )
        .unwrap();
    }
    h.set("HORCH_CODE_SWEEP_DIR", sweep.to_string_lossy());
    ok(&h, &["ledger", "run", "new", "alpha"]);
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = dir.join("01-a.md");
    std::fs::write(&ticket, "# 01: Feature\n").unwrap();
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &ticket.to_string_lossy(),
            "--primary",
            "opus",
            "--check",
            "true",
        ],
    );
    let w = ok(&h, &["ledger", "worker", "new", "--persona", "opus"])
        .trim()
        .to_string();
    ok_as(&h, &w, &["ledger", "claim"]);
    let wt = ok_as(&h, &w, &["ledger", "worktree", "new", "feature"]);
    let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());

    let project = h.project.clone();
    git_in(&h, &project, &["config", "core.hooksPath", "hooks"]);
    std::fs::create_dir_all(path.join("hooks")).unwrap();
    let hook = path.join("hooks/pre-commit");
    let marker = h.root.join("hook-ran");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    (h, w, path)
}

fn head(h: &Harness, dir: &Path) -> String {
    git_in(h, dir, &["rev-parse", "HEAD"])
}

#[test]
fn a_commit_lands_on_the_worktree_branch_without_running_hooks() {
    let (h, w, wt) = claimed("lc-commit");
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    std::fs::write(wt.join("stale.txt"), "stale\n").unwrap();
    git_in(&h, &wt, &["add", "stale.txt"]);
    // A leading dash is a path after `--`, never an option.
    std::fs::write(wt.join("-n.txt"), "dash\n").unwrap();

    let o = as_worker(
        &h,
        &w,
        &wt,
        &["ledger", "commit", "-m", "feature", "--", "a.txt", "-n.txt"],
    );
    assert!(o.status.success(), "{}", text(&o));
    let id = out(&o).trim().to_string();
    assert_eq!(id, head(&h, &wt));
    assert_eq!(
        git_in(&h, &wt, &["branch", "--show-current"]),
        "fleet/alpha-T1-feature"
    );
    let files = git_in(&h, &wt, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files, "-n.txt\na.txt", "only the named files");
    assert_eq!(
        git_in(&h, &wt, &["diff", "--cached", "--name-only"]),
        "stale.txt",
        "a path staged earlier stays out of the commit"
    );
    assert!(
        !h.root.join("hook-ran").exists(),
        "the repository's pre-commit hook must not run"
    );
    let events = ok(&h, &["ledger", "events"]);
    assert!(
        events.contains(&format!("commit {}", &id[..12])),
        "{events}"
    );
}

#[test]
fn paths_outside_the_worktree_or_in_a_nested_repository_are_refused() {
    let (h, w, wt) = claimed("lc-refuse");
    let before = head(&h, &wt);
    std::fs::write(h.root.join("outside.txt"), "x\n").unwrap();
    let outside = h.root.join("outside.txt");
    let nested = wt.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    git_in(&h, &nested, &["init", "--quiet"]);
    std::fs::write(nested.join("f.txt"), "f\n").unwrap();

    for (args, why) in [
        (vec!["--", "../outside.txt"], "is outside the worktree"),
        (
            vec!["--", outside.to_str().unwrap()],
            "is outside the worktree",
        ),
        (vec!["--", "nested/f.txt"], "nested repository"),
        (vec!["--", "nested"], "nested repository"),
    ] {
        let mut all = vec!["ledger", "commit", "-m", "x"];
        all.extend(args.iter().copied());
        let o = as_worker(&h, &w, &wt, &all);
        assert!(!o.status.success(), "{}", text(&o));
        assert!(text(&o).contains(why), "{}", text(&o));
    }
    // From inside the nested repository, too.
    let o = as_worker(
        &h,
        &w,
        &nested,
        &["ledger", "commit", "-m", "x", "--", "f.txt"],
    );
    assert!(text(&o).contains("nested repository"), "{}", text(&o));
    // From outside the worktree.
    let o = as_worker(&h, &w, &h.project, &["ledger", "commit", "-m", "x"]);
    assert!(
        !o.status.success() && text(&o).contains("inside the worktree"),
        "{}",
        text(&o)
    );
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

#[test]
fn a_worker_without_a_task_or_with_a_review_task_cannot_commit() {
    let (h, w, wt) = claimed("lc-review");
    let idle = ok(&h, &["ledger", "worker", "new", "--persona", "opus"])
        .trim()
        .to_string();
    let o = as_worker(&h, &idle, &wt, &["ledger", "commit", "-m", "x"]);
    assert!(
        !o.status.success() && text(&o).contains("has no claimed task"),
        "{}",
        text(&o)
    );

    // A real review task: finish queues reviewer tasks after a commit.
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    // No paths: every change, the hook file too, so the worktree is clean.
    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "feature"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(git_in(&h, &wt, &["status", "--porcelain"]).is_empty());
    let o = as_worker(&h, &w, &wt, &["ledger", "finish", "--json"]);
    let r: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
    assert_eq!(r["stopped_at"], "code-sweep", "{r}");

    let rw = ok(
        &h,
        &["ledger", "worker", "new", "--persona", "codex-reviewer"],
    )
    .trim()
    .to_string();
    let c: Value = serde_json::from_str(&ok_as(&h, &rw, &["ledger", "claim", "--json"])).unwrap();
    assert_eq!(c["task"]["kind"], "review", "{c}");
    let before = head(&h, &wt);
    std::fs::write(wt.join("b.txt"), "b\n").unwrap();
    let o = as_worker(&h, &rw, &wt, &["ledger", "commit", "-m", "x"]);
    assert!(
        !o.status.success() && text(&o).contains("is a review task"),
        "{}",
        text(&o)
    );
    assert_eq!(head(&h, &wt), before, "nothing is committed");
}

#[test]
fn a_rewritten_worktree_git_file_is_refused_and_its_config_never_runs() {
    let (h, w, wt) = claimed("lc-gitfile");
    let before = head(&h, &wt);
    // The sandbox can write `<worktree>/.git`: point it at a git dir whose
    // config names a clean filter, as a worker could.
    let evil = wt.join("evil");
    git_in(&h, &wt, &["init", "--quiet", "--bare", "evil"]);
    let marker = h.root.join("filter-ran");
    git_in(
        &h,
        &evil,
        &[
            "config",
            "filter.x.clean",
            &format!("touch '{}'; cat", marker.display()),
        ],
    );
    std::fs::write(wt.join(".gitattributes"), "* filter=x\n").unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", evil.display())).unwrap();

    let o = as_worker(&h, &w, &wt, &["ledger", "commit", "-m", "x", "--", "a.txt"]);
    assert!(
        !o.status.success() && text(&o).contains("was changed"),
        "{}",
        text(&o)
    );
    assert!(!marker.exists(), "the planted filter must not run");
    let main_head = git_in(&h, &h.project, &["rev-parse", "fleet/alpha-T1-feature"]);
    assert_eq!(main_head, before, "nothing is committed");
}

#[test]
fn an_attribute_filter_never_runs_and_the_commit_still_lands() {
    let (h, w, wt) = claimed("lc-filter");
    let marker = h.root.join("filter-ran");
    let project = h.project.clone();
    git_in(
        &h,
        &project,
        &[
            "config",
            "filter.evil.clean",
            &format!("touch '{}'; cat", marker.display()),
        ],
    );
    std::fs::write(wt.join(".gitattributes"), "* filter=evil\n").unwrap();
    std::fs::write(wt.join("a.txt"), "a\n").unwrap();

    let o = as_worker(
        &h,
        &w,
        &wt,
        &[
            "ledger",
            "commit",
            "-m",
            "x",
            "--",
            ".gitattributes",
            "a.txt",
        ],
    );
    assert!(o.status.success(), "{}", text(&o));
    assert!(!marker.exists(), "the clean filter must not run");
    let files = git_in(&h, &wt, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files, ".gitattributes\na.txt");
}
