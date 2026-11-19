//! Drop, cleanup, dismiss and the run gate through the custody jobs
//! (fleetd-p3 ticket 12): the real `horch` binary, its own git repository,
//! ledger and state folders in a temp dir. Each path keeps each refusal of
//! today (RR-13, rows W6, W11 and L9 of
//! `docs/specs/fleetd/preservation-04-behavior.md`), and each folder that
//! it removes is `landed`, or first on a verified rescue ref, and then
//! removed by 1 `worktree_remove` job (RR-9, FM-27). No fleetd runs: each
//! command runs its jobs itself.
//!
//! The old paths that these tests replace: the `git worktree remove` calls
//! of `worktree_ops::drop_worktree`, `finish::land::delete_worktree` (the
//! cleanup step and `dismiss --cleanup`) and `runs::remove_gate_worktree`,
//! and the `git worktree add --detach` call of `runs::checkout_tip`.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::taskledger::store::Store;
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

fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set("HORCH_LEDGER_DB", ledger(&h).to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.serve_fleetd();
    h
}

fn ledger(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn as_worker(h: &Harness, w: &str, args: &[&str]) -> Output {
    h.horch(args).env("HORCH_WORKER_ID", w).output().unwrap()
}

fn ok_as(h: &Harness, w: &str, args: &[&str]) -> String {
    let o = as_worker(h, w, args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    out(&o).trim().to_string()
}

fn git_ok(h: &Harness, dir: &Path, args: &[&str]) -> bool {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    c.output().unwrap().status.success()
}

fn commit_file(h: &Harness, dir: &Path, file: &str) -> String {
    std::fs::write(dir.join(file), format!("{file}\n")).unwrap();
    git_in(h, dir, &["add", file]);
    git_in(h, dir, &["commit", "--quiet", "--no-gpg-sign", "-m", file]);
    git_in(h, dir, &["rev-parse", "HEAD"])
}

fn tree_path(said: &str) -> PathBuf {
    PathBuf::from(
        said.lines()
            .find_map(|l| l.strip_prefix("path: "))
            .unwrap_or_else(|| panic!("no path line: {said}")),
    )
}

/// A run `alpha` with the finish steps `steps`, 1 task with no check, a
/// worker that claimed it, and its worktree `feat`. Gives the worker, the
/// task id and the path of the tree.
fn claimed(h: &Harness, steps: &str) -> (String, String, PathBuf) {
    ok(h, &["ledger", "run", "new", "alpha", "--steps", steps]);
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = dir.join("01-a.md");
    std::fs::write(&ticket, "# 01: Feature\n\n**Check:** none\n").unwrap();
    ok(
        h,
        &[
            "ledger",
            "add",
            "--file",
            &ticket.to_string_lossy(),
            "--primary",
            "opus",
        ],
    );
    let w = ok(h, &["ledger", "worker", "new", "--persona", "opus"])
        .trim()
        .to_string();
    let claim: Value = serde_json::from_str(&ok_as(h, &w, &["ledger", "claim", "--json"])).unwrap();
    let task = claim["task"]["task_id"].as_str().unwrap().to_string();
    let path = tree_path(&ok_as(h, &w, &["ledger", "worktree", "new", "feat"]));
    (w, task, path)
}

/// The values of the first column of `sql`.
fn query(h: &Harness, sql: &str) -> Vec<String> {
    let store = Store::open(&ledger(h)).unwrap();
    let mut stmt = store.conn().prepare(sql).unwrap();
    let rows = stmt
        .query_map([], |r| r.get::<_, Option<String>>(0))
        .unwrap()
        .map(|v| v.unwrap().unwrap_or_default())
        .collect();
    rows
}

/// `path` as the ledger writes it: with each link of the project folder
/// resolved (`/tmp` is `/private/tmp` on macOS). A path that a command
/// printed is resolved already.
fn stored(h: &Harness, path: &Path) -> String {
    let project = std::fs::canonicalize(&h.project).unwrap();
    match path.strip_prefix(&h.project) {
        Ok(rel) => project.join(rel).display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

/// 1 column of the 1 row of the folder `path`.
fn column(h: &Harness, path: &Path, name: &str) -> String {
    let rows = query(
        h,
        &format!(
            "SELECT {name} FROM worktrees WHERE path = '{}'",
            stored(h, path)
        ),
    );
    assert_eq!(rows.len(), 1, "the rows of {}: {rows:?}", path.display());
    rows[0].clone()
}

/// The state moves of the row of `path` that the door recorded,
/// `<from>><to>` each, oldest first.
fn moves(h: &Harness, path: &Path) -> Vec<String> {
    let id = column(h, path, "worktree_id");
    query(
        h,
        &format!(
            "SELECT from_state || '>' || to_state FROM events
             WHERE entity = 'worktree' AND entity_id = '{id}' AND to_state IS NOT NULL
             ORDER BY rowid"
        ),
    )
}

/// The runs of `kind`: `<state>` each, oldest first.
fn jobs(h: &Harness, kind: &str) -> Vec<String> {
    query(
        h,
        &format!("SELECT state FROM jobs WHERE kind = '{kind}' ORDER BY id"),
    )
}

/// The refs under `prefix`, in name order.
fn refs(h: &Harness, prefix: &str) -> Vec<String> {
    git_in(
        h,
        &h.project,
        &["for-each-ref", "--format=%(refname)", prefix],
    )
    .lines()
    .map(str::to_string)
    .collect()
}

/// A `git` that runs `hook` (a shell snippet) once, the first time its
/// arguments contain `when` while the file `mark` does not exist, and then
/// runs the real git. The hook makes `mark`, or the script makes it. It
/// replaces the `HORCH_GIT_BIN` of `h`. A job runs git in a sandbox that
/// writes only its tree, so the `mark` of a hook in a job is in the tree.
fn git_with_hook(h: &mut Harness, when: &str, mark: &Path, hook: &str) {
    let real = h.git_bin().unwrap().to_path_buf();
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in\n  *\" {when} \"*)\n    if [ ! -e '{mark}' ]; then\n      {hook}\n      [ -e '{mark}' ] || : > '{mark}'\n    fi\n    ;;\nesac\nexec '{real}' \"$@\"\n",
        mark = mark.display(),
        real = real.display(),
    );
    let bin = h.write_bin("git-hooked", script.as_bytes());
    h.set("HORCH_GIT_BIN", bin.to_string_lossy());
}

fn drop_tree(h: &Harness, w: &str, name: &str, keep_branch: bool) -> Output {
    let mut args = vec!["ledger", "worktree", "drop", name, "--reason", "stop"];
    if keep_branch {
        args.push("--keep-branch");
    }
    as_worker(h, w, &args)
}

/// A drop that is refused with `words`: the tree stays, its row is not
/// moved, and no custody job ran.
fn assert_refused(h: &Harness, o: &Output, path: &Path, words: &str) {
    assert!(!o.status.success(), "{}", text(o));
    assert!(
        String::from_utf8_lossy(&o.stderr).contains(words),
        "{}",
        text(o)
    );
    assert!(path.is_dir(), "the refused drop removed {}", path.display());
    assert_eq!(column(h, path, "status"), "open");
    assert_eq!(column(h, path, "state"), "active");
    assert!(jobs(h, "rescue").is_empty(), "{:?}", jobs(h, "rescue"));
    assert!(jobs(h, "worktree_remove").is_empty());
}

// ─── RR-13: the refusals of the drop ────────────────────────────────────

#[test]
fn rr13_drop_of_a_dirty_tree_is_refused_and_the_tree_stays() {
    let h = world("p3-ref-dirty");
    let (w, _, path) = claimed(&h, "land");
    std::fs::write(path.join("README.md"), "changed\n").unwrap();
    std::fs::write(path.join("new.txt"), "untracked\n").unwrap();
    for keep in [false, true] {
        let o = drop_tree(&h, &w, "T1-feat", keep);
        assert_refused(
            &h,
            &o,
            &path,
            "worktree has uncommitted or untracked changes; nothing removed",
        );
    }
    assert_eq!(
        std::fs::read_to_string(path.join("README.md")).unwrap(),
        "changed\n"
    );
    assert_eq!(
        std::fs::read_to_string(path.join("new.txt")).unwrap(),
        "untracked\n"
    );
}

#[test]
fn rr13_drop_of_a_branch_with_a_stash_entry_is_refused_and_the_tree_stays() {
    let h = world("p3-ref-stash");
    let (w, _, path) = claimed(&h, "land");
    std::fs::write(path.join("README.md"), "stashed\n").unwrap();
    git_in(&h, &path, &["stash", "push", "--quiet", "-m", "wip"]);
    let stash = git_in(&h, &h.project, &["stash", "list"]);
    assert!(stash.contains("wip"), "{stash}");
    for keep in [false, true] {
        let o = drop_tree(&h, &w, "T1-feat", keep);
        assert_refused(
            &h,
            &o,
            &path,
            "worktree branch has a stash; nothing removed",
        );
    }
    assert_eq!(git_in(&h, &h.project, &["stash", "list"]), stash);
    assert_eq!(
        git_in(&h, &h.project, &["show", "stash@{0}:README.md"]),
        "stashed"
    );
}

#[test]
fn rr13_drop_of_unpublished_unique_commits_is_refused_and_the_tree_stays() {
    let h = world("p3-ref-unique");
    let (w, _, path) = claimed(&h, "land");
    let head = commit_file(&h, &path, "unique.txt");
    let branch = git_in(&h, &path, &["branch", "--show-current"]);
    let o = drop_tree(&h, &w, "T1-feat", false);
    assert_refused(
        &h,
        &o,
        &path,
        "worktree has unpublished commits that exist nowhere else; use --keep-branch; nothing removed",
    );
    assert_eq!(git_in(&h, &h.project, &["rev-parse", &branch]), head);
    assert_eq!(git_in(&h, &path, &["rev-parse", "HEAD"]), head);
    assert!(path.join("unique.txt").is_file());
}

#[test]
fn rr13_drop_with_a_head_that_moved_during_the_checks_is_refused_and_the_tree_stays() {
    let mut h = world("p3-ref-moved");
    let (w, _, path) = claimed(&h, "land");
    let first = commit_file(&h, &path, "first.txt");
    // The drop reads the HEAD, then lists the unique commits with
    // `rev-list`, then reads the HEAD again. A commit between the 2 reads
    // moves the HEAD.
    let real = h.git_bin().unwrap().to_path_buf();
    let hook = format!(
        "'{}' -C '{}' commit --quiet --allow-empty --no-gpg-sign -m moved",
        real.display(),
        path.display()
    );
    let mark = h.root.join("git-hook-ran");
    git_with_hook(&mut h, "rev-list", &mark, &hook);
    let o = drop_tree(&h, &w, "T1-feat", true);
    assert_refused(
        &h,
        &o,
        &path,
        "worktree HEAD changed during the drop checks; nothing removed",
    );
    let moved = git_in(&h, &path, &["rev-parse", "HEAD"]);
    assert_ne!(moved, first, "the hook did not move the HEAD");
    assert_eq!(git_in(&h, &path, &["rev-parse", "HEAD~1"]), first);
    assert!(path.join("first.txt").is_file());
}

// ─── the drop through the rescue and remove jobs ────────────────────────

#[test]
fn drop_of_a_clean_tree_writes_a_verified_rescue_ref_before_the_removal() {
    let h = world("p3-ref-drop");
    let (w, _, path) = claimed(&h, "land");
    let head = commit_file(&h, &path, "unique.txt");
    let branch = git_in(&h, &path, &["branch", "--show-current"]);
    let id = column(&h, &path, "worktree_id");
    ok_as(
        &h,
        &w,
        &[
            "ledger",
            "worktree",
            "drop",
            "T1-feat",
            "--reason",
            "stop",
            "--keep-branch",
        ],
    );
    assert!(!path.exists());
    // The rescue ref holds the tree, and its run ended before the removal
    // started.
    let rescue = refs(&h, &format!("refs/horch/rescue/{id}/"));
    assert_eq!(rescue, [format!("refs/horch/rescue/{id}/1")]);
    assert_eq!(
        git_in(
            &h,
            &h.project,
            &["show", &format!("{}:unique.txt", rescue[0])]
        ),
        "unique.txt"
    );
    assert_eq!(jobs(&h, "rescue"), ["passed"]);
    assert_eq!(jobs(&h, "worktree_remove"), ["passed"]);
    let order = query(
        &h,
        "SELECT CAST((SELECT ended_at FROM jobs WHERE kind = 'rescue')
             <= (SELECT started_at FROM jobs WHERE kind = 'worktree_remove') AS TEXT)",
    );
    assert_eq!(order, ["1"]);
    let m = moves(&h, &path);
    assert!(
        m.ends_with(&[
            "active>orphaned".into(),
            "orphaned>rescued".into(),
            "rescued>removed".into()
        ]),
        "{m:?}"
    );
    // Today's record of a drop stays: `abandoned`, with the reason.
    assert_eq!(column(&h, &path, "status"), "abandoned");
    assert_eq!(column(&h, &path, "reason"), "stop");
    // `--keep-branch` keeps the branch at its HEAD.
    assert_eq!(git_in(&h, &h.project, &["rev-parse", &branch]), head);
}

#[test]
fn drop_of_a_merged_branch_deletes_it_and_keep_branch_keeps_it() {
    let h = world("p3-ref-keep");
    let (w, _, _) = claimed(&h, "land");
    let mut branches = Vec::new();
    for (name, keep) in [("gone", false), ("kept", true)] {
        let path = tree_path(&ok_as(&h, &w, &["ledger", "worktree", "scratch", name]));
        let branch = git_in(&h, &path, &["branch", "--show-current"]);
        let o = drop_tree(&h, &w, name, keep);
        assert!(o.status.success(), "{}", text(&o));
        assert!(!path.exists());
        let id = column(&h, &path, "worktree_id");
        let result = query(
            &h,
            &format!(
                "SELECT result_json FROM jobs WHERE kind = 'worktree_remove' AND subject = '{id}'"
            ),
        );
        assert_eq!(result.len(), 1, "{result:?}");
        let r: Value = serde_json::from_str(&result[0]).unwrap();
        let data = &r["data"];
        assert_eq!(data["keep_branch"], keep, "{r}");
        assert_eq!(data["branch_deleted"], !keep, "{r}");
        branches.push((branch, keep));
    }
    for (branch, keep) in branches {
        let exists = git_ok(
            &h,
            &h.project,
            &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
        );
        assert_eq!(exists, keep, "{branch}");
    }
}

// ─── the cleanup step ───────────────────────────────────────────────────

fn finish_json(h: &Harness, w: &str) -> Value {
    let o = as_worker(h, w, &["ledger", "finish", "--json"]);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

/// The (status, verdict, note) of step `name` in a finish report.
fn step(r: &Value, name: &str) -> (String, String, String) {
    let s = r["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no step {name}: {r}"));
    let field = |k: &str| s[k].as_str().unwrap_or("").to_string();
    (field("status"), field("verdict"), field("note"))
}

#[test]
fn cleanup_after_land_removes_a_landed_tree_through_the_remove_job() {
    let h = world("p3-ref-cleanup");
    let (w, _, path) = claimed(&h, "land,cleanup");
    let head = commit_file(&h, &path, "feature.txt");
    let branch = git_in(&h, &path, &["branch", "--show-current"]);
    let r = finish_json(&h, &w);
    assert_eq!(step(&r, "land").0, "passed", "{r}");
    assert_eq!(step(&r, "cleanup").0, "passed", "{r}");
    assert!(!path.exists());
    assert!(git_ok(
        &h,
        &h.project,
        &["merge-base", "--is-ancestor", &head, "fleet/alpha"]
    ));
    let m = moves(&h, &path);
    assert!(
        m.ends_with(&[
            "active>landing".into(),
            "landing>landed".into(),
            "landed>removed".into()
        ]),
        "{m:?}"
    );
    // A landed tree needs no rescue.
    assert!(jobs(&h, "rescue").is_empty());
    assert_eq!(jobs(&h, "worktree_remove"), ["passed"]);
    assert_eq!(column(&h, &path, "status"), "cleaned");
    assert_eq!(
        column(&h, &path, "landed_commit"),
        git_in(&h, &h.project, &["rev-parse", "fleet/alpha"])
    );
    assert!(!git_ok(
        &h,
        &h.project,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")]
    ));
}

#[test]
fn cleanup_of_a_landed_tree_with_an_untracked_file_is_refused_and_marked_cleanup_failed() {
    let mut h = world("p3-ref-cleanup-refused");
    let (w, _, path) = claimed(&h, "land,cleanup");
    commit_file(&h, &path, "feature.txt");
    // The untracked file comes after today's checks of the cleanup step
    // passed, as the remove job reads the HEAD of the tree: the guard of
    // the job refuses it.
    let late = path.join("late.txt");
    let hook = format!("echo late > '{}'", late.display());
    git_with_hook(&mut h, "--show-toplevel --verify HEAD", &late, &hook);
    let r = finish_json(&h, &w);
    assert_eq!(step(&r, "land").0, "passed", "{r}");
    let (status, verdict, note) = step(&r, "cleanup");
    assert_eq!(
        (status.as_str(), verdict.as_str()),
        ("failed", "cleanup failed"),
        "{r}"
    );
    assert!(note.ends_with("; the watchdog retries it"), "{note}");
    assert!(path.join("feature.txt").is_file());
    assert_eq!(
        std::fs::read_to_string(&late).ok().as_deref(),
        Some("late\n"),
        "{note}"
    );
    assert_eq!(jobs(&h, "worktree_remove"), ["failed"]);
    assert!(jobs(&h, "rescue").is_empty());
    assert_eq!(column(&h, &path, "status"), "cleanup_failed");
    assert_eq!(column(&h, &path, "state"), "landed");
    let reason = column(&h, &path, "reason");
    assert!(
        reason.contains("late.txt") || reason.contains("untracked"),
        "{reason}"
    );
}

// ─── dismiss --cleanup ──────────────────────────────────────────────────

#[test]
fn dismiss_cleanup_rescues_then_removes_only_the_recorded_path() {
    let h = world("p3-ref-dismiss");
    let (_, task, path) = claimed(&h, "land");
    // A folder beside it that no row records.
    let other = h.project.join(".worktrees").join("other");
    git_in(
        &h,
        &h.project,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "other",
            &other.to_string_lossy(),
        ],
    );
    ok(&h, &["ledger", "cancel", &task, "not needed"]);
    let list: Value = serde_json::from_str(&ok(&h, &["ledger", "unfinished", "--json"])).unwrap();
    let finding = list
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "task_cancelled_open_worktree")
        .unwrap_or_else(|| panic!("no finding of the cancelled tree: {list}"));
    let id = column(&h, &path, "worktree_id");
    ok(
        &h,
        &[
            "ledger",
            "dismiss",
            &finding["finding_id"].to_string(),
            "not needed",
            "--cleanup",
        ],
    );
    assert!(!path.exists());
    assert!(
        other.is_dir(),
        "dismiss removed a folder that it does not record"
    );
    assert_eq!(
        refs(&h, &format!("refs/horch/rescue/{id}/")),
        [format!("refs/horch/rescue/{id}/1")]
    );
    assert_eq!(jobs(&h, "rescue"), ["passed"]);
    assert_eq!(jobs(&h, "worktree_remove"), ["passed"]);
    assert_eq!(column(&h, &path, "state"), "removed");
    assert_eq!(column(&h, &path, "status"), "cleaned");
}

// ─── the run gate folder ────────────────────────────────────────────────

#[test]
fn run_gate_folder_is_added_and_removed_through_the_jobs() {
    let h = world("p3-ref-gate");
    ok(&h, &["ledger", "run", "new", "alpha", "--steps", "land"]);
    let gate = h.project.join(".worktrees").join("_gate-alpha");
    ok(&h, &["ledger", "run", "gate", "--check", "true"]);
    assert!(gate.join(".git").exists());
    let tip = git_in(&h, &h.project, &["rev-parse", "fleet/alpha"]);
    assert_eq!(git_in(&h, &gate, &["rev-parse", "HEAD"]), tip);
    assert_eq!(jobs(&h, "worktree_add"), ["passed"]);
    // The next gate reuses the folder: a checkout, no new job.
    ok(&h, &["ledger", "run", "gate", "--check", "true"]);
    assert_eq!(jobs(&h, "worktree_add"), ["passed"]);
    let id = column(&h, &gate, "worktree_id");
    ok(&h, &["ledger", "run", "close", "--abandon", "test"]);
    assert!(!gate.exists());
    assert_eq!(
        refs(&h, &format!("refs/horch/rescue/{id}/")),
        [format!("refs/horch/rescue/{id}/1")]
    );
    assert_eq!(jobs(&h, "rescue"), ["passed"]);
    assert_eq!(jobs(&h, "worktree_remove"), ["passed"]);
    assert_eq!(column(&h, &gate, "state"), "removed");
    assert_eq!(column(&h, &gate, "status"), "cleaned");
}
