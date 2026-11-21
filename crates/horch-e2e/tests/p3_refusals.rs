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

use horch_core::taskledger::runs::Run;
use horch_core::taskledger::store::Store;
use horch_e2e::harness::{wait_for, wait_for_or, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
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

fn pass(h: &Harness) {
    let o = h.without_fleetd(|| {
        h.run(&[
            "ledger", "watch", "--once", "--force", "--json", "--run", "alpha",
        ])
    });
    assert!(o.status.success(), "horch ledger watch: {}", text(&o));
}

fn tree_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}

fn seed_alpha(h: &Harness, steps: &str) -> Run {
    let run = seed_run(h, "alpha", None, "w1:p1");
    let words: Vec<String> = steps.split(',').map(|s| format!("{s:?}")).collect();
    Store::open(&ledger(h))
        .unwrap()
        .conn()
        .execute(
            "UPDATE runs SET finish_steps = ?1 WHERE run_id = ?2",
            (format!("[{}]", words.join(",")), &run.run_id),
        )
        .unwrap();
    run
}

/// A run `alpha` with the finish steps `steps`, 1 task with no check, a
/// worker bound to it, and its worktree `feat`. Gives the worker, the
/// task id and the path of the tree.
fn claimed(h: &Harness, steps: &str) -> (String, String, PathBuf) {
    claimed_as(h, steps, None)
}

fn claimed_as(h: &Harness, steps: &str, agent: Option<u32>) -> (String, String, PathBuf) {
    std::fs::write(
        h.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    git_in(h, &h.project, &["add", "fleet.toml"]);
    git_in(
        h,
        &h.project,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
    );
    let run = seed_alpha(h, steps);
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = dir.join("01-a.md");
    std::fs::write(&ticket, "# 01: Feature\n\n**Check:** none\n").unwrap();
    let bound = h.without_fleetd(|| {
        let task = seed_task(h, &run, &ticket, "opus");
        Store::open(&ledger(h))
            .unwrap()
            .conn()
            .execute(
                "UPDATE tasks SET check_none = 1 WHERE task_id = ?1",
                [&task.task_id],
            )
            .unwrap();
        horch_e2e::bind::bind(h, "opus", agent)
    });
    let (w, task) = (bound.worker_id, bound.task_id);
    let path = wait_for("the active tree of the task", || {
        pass(h);
        query(
            h,
            &format!("SELECT path FROM worktrees WHERE task_id = '{task}' AND state = 'active'"),
        )
        .pop()
        .map(PathBuf::from)
    });
    (w, task, path)
}

/// The run gives each task 1 attempt: when the attempt ends, the task
/// fails and no retry attempt takes its tree again during the test.
fn no_retry(h: &Harness) {
    Store::open(&ledger(h))
        .unwrap()
        .conn()
        .execute("UPDATE runs SET max_attempts = 1", [])
        .unwrap();
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
    h.without_fleetd(|| ());
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
        let o = drop_tree(&h, &w, &tree_name(&path), keep);
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
        let o = drop_tree(&h, &w, &tree_name(&path), keep);
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
    let o = drop_tree(&h, &w, &tree_name(&path), false);
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
    let real = h.git_bin().unwrap().to_path_buf();
    let hook = format!(
        "'{}' -C '{}' commit --quiet --allow-empty --no-gpg-sign -m moved",
        real.display(),
        path.display()
    );
    let mark = h.root.join("git-hook-ran");
    git_with_hook(&mut h, "rev-list", &mark, &hook);
    let o = drop_tree(&h, &w, &tree_name(&path), true);
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

#[test]
fn rr13_a_commit_after_an_accepted_drop_is_kept_in_the_rescue_ref() {
    let h = world("p3-ref-moved");
    let (w, _, path) = claimed(&h, "land");
    let first = commit_file(&h, &path, "first.txt");
    let o = drop_tree(&h, &w, &tree_name(&path), true);
    assert!(o.status.success(), "{}", text(&o));
    git_in(
        &h,
        &path,
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "--no-gpg-sign",
            "-m",
            "moved",
        ],
    );
    let moved = git_in(&h, &path, &["rev-parse", "HEAD"]);
    assert_ne!(moved, first, "the commit did not move the HEAD");
    assert_eq!(git_in(&h, &path, &["rev-parse", "HEAD~1"]), first);
    wait_for_or(
        "the rescue of the dropped tree",
        || {
            h.advance_fleet_clock(30);
            pass(&h);
            (column(&h, &path, "state") == "rescued").then_some(())
        },
        || dump(&h),
    );
    let id = column(&h, &path, "worktree_id");
    let rescue = refs(&h, &format!("refs/horch/rescue/{id}/"));
    let saved = rescue.last().expect("a rescue ref");
    assert!(
        git_ok(
            &h,
            &h.project,
            &["merge-base", "--is-ancestor", &moved, saved]
        ),
        "the rescue ref {saved} does not hold the commit {moved} that came after the drop"
    );
    assert!(jobs(&h, "worktree_remove").is_empty());
    assert_eq!(git_in(&h, &path, &["rev-parse", "HEAD"]), moved);
    assert!(path.join("first.txt").is_file());
}

// ─── the drop through the rescue and remove jobs ────────────────────────

#[test]
fn drop_of_a_clean_tree_writes_a_verified_rescue_ref_before_the_removal() {
    let h = world("p3-ref-drop");
    let (w, task, path) = claimed(&h, "land");
    no_retry(&h);
    let head = commit_file(&h, &path, "unique.txt");
    let branch = git_in(&h, &path, &["branch", "--show-current"]);
    let id = column(&h, &path, "worktree_id");
    let said = ok_as(
        &h,
        &w,
        &[
            "ledger",
            "worktree",
            "drop",
            &tree_name(&path),
            "--reason",
            "stop",
            "--keep-branch",
        ],
    );
    wait_for_or(
        "the rescue of the dropped tree",
        || {
            h.advance_fleet_clock(30);
            pass(&h);
            (column(&h, &path, "state") == "rescued").then_some(())
        },
        || format!("{said}\n{}", dump(&h)),
    );
    assert!(path.exists());
    merge_idea_of(&h, &task);
    wait_for_or(
        "the removal of the dropped tree",
        || {
            h.advance_fleet_clock(30);
            pass(&h);
            (column(&h, &path, "state") == "removed").then_some(())
        },
        || format!("{said}\n{}", dump(&h)),
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

fn dropped_scratch_trees(h: &Harness, w: &str) -> Vec<(PathBuf, String, bool)> {
    let mut trees = Vec::new();
    for (name, keep) in [("gone", false), ("kept", true)] {
        let said = ok_as(h, w, &["ledger", "worktree", "scratch", name]);
        let path = tree_path(&said);
        wait_for_or(
            "the scratch tree",
            || {
                h.advance_fleet_clock(30);
                pass(h);
                (path.is_dir() && column(h, &path, "state") != "reserved").then_some(())
            },
            || format!("{said}\n{}", dump(h)),
        );
        let branch = git_in(h, &path, &["branch", "--show-current"]);
        let o = drop_tree(h, w, name, keep);
        assert!(o.status.success(), "{}", text(&o));
        trees.push((path, branch, keep));
    }
    for (path, _, _) in &trees {
        // The removal follows the verified rescue of a dropped tree, so a
        // later pass can show it removed already.
        wait_for_or(
            "the rescue of the dropped scratch tree",
            || {
                h.advance_fleet_clock(30);
                pass(h);
                ["rescued", "removed"]
                    .contains(&column(h, path, "state").as_str())
                    .then_some(())
            },
            || dump(h),
        );
    }
    trees
}

#[test]
fn drop_of_a_scratch_tree_rescues_it_before_the_removal() {
    let h = world("p3-ref-scratch");
    let (w, _, _) = claimed(&h, "land");
    for (path, branch, keep) in dropped_scratch_trees(&h, &w) {
        let id = column(&h, &path, "worktree_id");
        let rescue = refs(&h, &format!("refs/horch/rescue/{id}/"));
        assert_eq!(rescue.first(), Some(&format!("refs/horch/rescue/{id}/1")));
        let m = moves(&h, &path);
        assert!(m.contains(&"orphaned>rescued".to_string()), "{m:?}");
        assert!(
            !m.iter().any(|s| s.ends_with(">removed")) || m.ends_with(&["rescued>removed".into()]),
            "{m:?}"
        );
        if keep {
            assert!(git_ok(
                &h,
                &h.project,
                &["rev-parse", "--verify", &format!("refs/heads/{branch}")]
            ));
        }
        assert_eq!(column(&h, &path, "reason"), "stop");
    }
}

#[test]
fn drop_of_a_merged_branch_deletes_it_and_keep_branch_keeps_it() {
    let h = world("p3-ref-keep");
    let (w, task, _) = claimed(&h, "land");
    let trees = dropped_scratch_trees(&h, &w);
    merge_idea_of(&h, &task);
    let mut branches = Vec::new();
    for (path, branch, keep) in trees {
        wait_for_or(
            "the removal of the dropped scratch tree",
            || {
                h.advance_fleet_clock(30);
                pass(&h);
                (column(&h, &path, "state") == "removed").then_some(())
            },
            || dump(&h),
        );
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

fn send_finish(h: &Harness, w: &str) {
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", "read go; exec \"$@\"", "sh"])
        .arg(Harness::horch_bin())
        .args(["ledger", "finish"])
        .env("HORCH_WORKER_ID", w)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    h.seal(&mut cmd);
    cmd.env("HORCH_WORKER_ID", w);
    let mut child = cmd.spawn().unwrap();
    let wrapper = child.id();
    Store::open(&ledger(h))
        .unwrap()
        .conn()
        .execute(
            "UPDATE attempts SET pid = ?1, pid_start = ?2 WHERE worker_id = ?3",
            (wrapper, horch_core::procid::start_time(wrapper).unwrap(), w),
        )
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), b"go\n").unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "horch ledger finish: {}", text(&o));
}

fn dump(h: &Harness) -> String {
    let tasks = query(
        h,
        "SELECT status || ' ' || coalesce(held_reason, '') FROM tasks",
    );
    let trees = query(
        h,
        "SELECT state || ' ' || coalesce(status, '') || ' ' || path FROM worktrees",
    );
    let branches = query(
        h,
        "SELECT name || ' ' || coalesce(landing_state, '') FROM branches",
    );
    let ideas = query(h, "SELECT id || ' ' || state FROM ideas");
    let jobs = query(
        h,
        "SELECT kind || ' ' || state || ' ' || substr(coalesce(result_json, ''), 1, 200) FROM jobs
         WHERE kind NOT IN ('docs_sync', 'conformance', 'fetch', 'worktree_list')",
    );
    format!("{tasks:#?}\n{trees:#?}\n{branches:#?}\n{ideas:#?}\n{jobs:#?}")
}

fn finish_and_land(h: &Harness, w: &str, path: &Path) {
    send_finish(h, w);
    wait_for_or(
        "the landing of the branch and of its tree",
        || {
            h.advance_fleet_clock(30);
            pass(h);
            let landed = !query(
                h,
                "SELECT name FROM branches WHERE task_id IS NOT NULL AND landing_state = 'landed'",
            )
            .is_empty();
            let state = column(h, path, "state");
            (landed && ["landed", "removed"].contains(&state.as_str())).then_some(())
        },
        || dump(h),
    );
}

fn merge_idea_of(h: &Harness, task: &str) {
    set_idea_of(h, task, "merged");
}

fn set_idea_of(h: &Harness, task: &str, state: &str) {
    Store::open(&ledger(h))
        .unwrap()
        .conn()
        .execute(
            "UPDATE ideas SET state = ?2
             WHERE id = (SELECT idea_id FROM tasks WHERE task_id = ?1)",
            [task, state],
        )
        .unwrap();
}

#[test]
fn cleanup_after_land_removes_a_landed_tree_through_the_remove_job() {
    let h = world("p3-ref-cleanup");
    let (w, _task, path) = claimed(&h, "land,cleanup");
    let head = commit_file(&h, &path, "feature.txt");
    let branch = git_in(&h, &path, &["branch", "--show-current"]);
    finish_and_land(&h, &w, &path);
    wait_for_or(
        "the removal of the landed tree",
        || {
            h.advance_fleet_clock(30);
            pass(&h);
            (column(&h, &path, "state") == "removed").then_some(())
        },
        || dump(&h),
    );
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
    assert!(jobs(&h, "rescue").is_empty(), "{:?}", jobs(&h, "rescue"));
    assert_eq!(jobs(&h, "worktree_remove"), ["passed"]);
    assert_eq!(column(&h, &path, "status"), "cleaned");
    let id = column(&h, &path, "worktree_id");
    assert!(refs(&h, &format!("refs/horch/rescue/{id}/")).is_empty());
    assert!(!git_ok(
        &h,
        &h.project,
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")]
    ));
}

#[test]
fn orphaned_tree_is_rescued_and_removed_when_its_idea_is_merged() {
    let h = world("p3-ref-orphan-merged");
    let mut agent = std::process::Command::new("/bin/sleep")
        .arg("600")
        .spawn()
        .unwrap();
    let (_, task, path) = claimed_as(&h, "land,cleanup", Some(agent.id()));
    no_retry(&h);
    commit_file(&h, &path, "feature.txt");
    set_idea_of(&h, &task, "parked");
    agent.kill().unwrap();
    agent.wait().unwrap();
    wait_for_or(
        "the rescue of the orphaned tree",
        || {
            h.advance_fleet_clock(30);
            pass(&h);
            (column(&h, &path, "state") == "rescued").then_some(())
        },
        || dump(&h),
    );
    assert!(path.exists());
    // After the removal a new attempt of the task can reserve the same
    // path: read the row by its id.
    let id = column(&h, &path, "worktree_id");
    let state = || {
        query(
            &h,
            &format!("SELECT state FROM worktrees WHERE worktree_id = '{id}'"),
        )
    };
    merge_idea_of(&h, &task);
    wait_for_or(
        "the removal of the rescued tree of the merged idea",
        || {
            h.advance_fleet_clock(30);
            pass(&h);
            (state() == ["removed"]).then_some(())
        },
        || dump(&h),
    );
    let m = query(
        &h,
        &format!(
            "SELECT from_state || '>' || to_state FROM events
             WHERE entity = 'worktree' AND entity_id = '{id}' AND to_state IS NOT NULL
             ORDER BY rowid"
        ),
    );
    assert!(
        m.ends_with(&[
            "active>orphaned".into(),
            "orphaned>rescued".into(),
            "rescued>removed".into()
        ]),
        "{m:?}"
    );
    for kind in ["rescue", "worktree_remove"] {
        let runs = query(
            &h,
            &format!("SELECT state FROM jobs WHERE kind = '{kind}' AND subject = '{id}'"),
        );
        assert_eq!(runs, ["passed"], "{kind}");
    }
    assert_eq!(
        refs(&h, &format!("refs/horch/rescue/{id}/")),
        [format!("refs/horch/rescue/{id}/1")]
    );
}

#[test]
fn cleanup_of_a_landed_tree_with_an_untracked_file_is_refused_and_marked_cleanup_failed() {
    let mut h = world("p3-ref-cleanup-refused");
    let (w, _task, path) = claimed(&h, "land,cleanup");
    commit_file(&h, &path, "feature.txt");
    // The untracked file comes after today's checks of the cleanup step
    // passed, as the remove job reads the HEAD of the tree: the guard of
    // the job refuses it (RR-13).
    let late = path.join("late.txt");
    let hook = format!("echo late > '{}'", late.display());
    git_with_hook(&mut h, "--show-toplevel --verify HEAD", &late, &hook);
    finish_and_land(&h, &w, &path);
    wait_for_or(
        "the refusal of the removal of the tree",
        || {
            h.advance_fleet_clock(30);
            pass(&h);
            (jobs(&h, "worktree_remove").contains(&"failed".to_string())).then_some(())
        },
        || dump(&h),
    );
    assert!(path.join("feature.txt").is_file());
    assert_eq!(
        std::fs::read_to_string(&late).ok().as_deref(),
        Some("late\n")
    );
    assert_eq!(jobs(&h, "worktree_remove"), ["failed"]);
    assert_eq!(column(&h, &path, "state"), "landed");
    let id = column(&h, &path, "worktree_id");
    let items = query(
        &h,
        &format!(
            "SELECT text FROM needs_you WHERE kind = 'remove_failed' AND entity_id = '{id}'
             AND state = 'open'"
        ),
    );
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(
        items[0].contains("untracked") || items[0].contains("late.txt"),
        "{}",
        items[0]
    );
}

// ─── dismiss --cleanup ──────────────────────────────────────────────────

#[test]
fn dismiss_cleanup_rescues_then_removes_only_the_recorded_path() {
    let h = world("p3-ref-dismiss");
    let (w, task, path) = claimed(&h, "land");
    let mut wrapper = std::process::Command::new("/bin/sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let (pid, start) = (
        wrapper.id(),
        horch_core::procid::start_time(wrapper.id()).unwrap(),
    );
    wrapper.kill().unwrap();
    wrapper.wait().unwrap();
    Store::open(&ledger(&h))
        .unwrap()
        .conn()
        .execute(
            "UPDATE attempts SET pid = ?1, pid_start = ?2 WHERE worker_id = ?3",
            (pid, start, &w),
        )
        .unwrap();
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
    ok(
        &h,
        &["ledger", "cancel", "--run", "alpha", &task, "not needed"],
    );
    let finding = wait_for_or(
        "the finding of the tree of the cancelled task",
        || {
            h.advance_fleet_clock(30);
            pass(&h);
            let list: Value =
                serde_json::from_str(&ok(&h, &["ledger", "unfinished", "--json"])).unwrap();
            list.as_array()
                .unwrap()
                .iter()
                .find(|f| f["kind"] == "task_cancelled_open_worktree")
                .cloned()
        },
        || {
            format!(
                "{:?}\n{:?}",
                query(&h, "SELECT status FROM tasks"),
                query(&h, "SELECT state || ' ' || status FROM worktrees")
            )
        },
    );
    wait_for_or(
        "the rescue of the tree of the cancelled task",
        || {
            h.advance_fleet_clock(30);
            pass(&h);
            (column(&h, &path, "state") == "rescued").then_some(())
        },
        || dump(&h),
    );
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
    wait_for_or(
        "the removal of the tree",
        || {
            h.advance_fleet_clock(30);
            pass(&h);
            (column(&h, &path, "state") == "removed").then_some(())
        },
        || dump(&h),
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
    // The passes adopt the folder `other` too and rescue it: count the
    // jobs of the tree only.
    let of_tree = |kind: &str| {
        query(
            &h,
            &format!(
                "SELECT state FROM jobs WHERE kind = '{kind}' AND subject = '{id}' ORDER BY id"
            ),
        )
    };
    assert_eq!(of_tree("rescue"), ["passed"]);
    assert_eq!(of_tree("worktree_remove"), ["passed"]);
    assert_eq!(column(&h, &path, "state"), "removed");
}

#[test]
fn run_gate_folder_is_added_and_removed_through_the_jobs() {
    use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
    use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
    use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
    use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
    use horch_core::fleetd::worktree_custody::application::gate_tree::{
        self, GateTree, GateTreeRequest,
    };

    let h = world("p3-ref-gate");
    let run = seed_alpha(&h, "land");
    let feature = query(&h, "SELECT id FROM features").pop().unwrap();
    let repo = std::fs::canonicalize(&h.project).unwrap();
    let gate = gate_tree::path(&repo, &feature);
    let main = git_in(&h, &h.project, &["rev-parse", "main"]);
    let config = FleetConfig::default();
    let reserved = h.without_fleetd(|| {
        let mut store = Store::open(&ledger(&h)).unwrap();
        store
            .immediate(|tx| {
                let ctx = JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                };
                let req = GateTreeRequest {
                    feature_id: &feature,
                    run_id: &run.run_id,
                    repo: &repo,
                    base: "fleet/alpha",
                    tip: &main,
                };
                gate_tree::reserve(&ctx, tx, &req, FleetSeconds(0))
            })
            .unwrap()
    });
    assert!(
        matches!(reserved, GateTree::Reserved { .. }),
        "{reserved:?}"
    );
    wait_for("the gate tree to be created", || {
        pass(&h);
        (column(&h, &gate, "state") == "created").then_some(())
    });
    assert!(gate.join(".git").exists());
    assert_eq!(git_in(&h, &gate, &["rev-parse", "HEAD"]), main);
    assert_eq!(jobs(&h, "worktree_add"), ["passed"]);
    assert_eq!(
        query(
            &h,
            "SELECT task_id FROM worktrees WHERE path LIKE '%gate-%'"
        ),
        [""]
    );
    let id = column(&h, &gate, "worktree_id");

    let released = h.without_fleetd(|| {
        let mut store = Store::open(&ledger(&h)).unwrap();
        store
            .immediate(|tx| {
                let ctx = JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                };
                gate_tree::release(&ctx, tx, &repo, &feature, "its feature is merged")
            })
            .unwrap()
    });
    assert!(released);
    assert_eq!(column(&h, &gate, "state"), "orphaned");
    wait_for_or(
        "the removal of the gate tree",
        || {
            h.advance_fleet_clock(30);
            pass(&h);
            (column(&h, &gate, "state") == "removed").then_some(())
        },
        || {
            let trees = query(
                &h,
                "SELECT state || ' ' || path || ' head=' || coalesce(head_sha, '-') FROM worktrees",
            );
            let jobs = query(&h, "SELECT id || ' ' || kind || ' ' || state || ' ' || substr(subject, 1, 40) || ' nb=' || coalesce(not_before, '-') FROM jobs");
            format!("{trees:#?}\n{jobs:#?}")
        },
    );
    assert!(!gate.exists());
    assert_eq!(
        refs(&h, &format!("refs/horch/rescue/{id}/")),
        [format!("refs/horch/rescue/{id}/1")]
    );
    assert_eq!(jobs(&h, "rescue"), ["passed"]);
    assert_eq!(jobs(&h, "worktree_remove"), ["passed"]);
    let m = moves(&h, &gate);
    assert!(
        m.ends_with(&["orphaned>rescued".into(), "rescued>removed".into()]),
        "{m:?}"
    );
    assert_eq!(column(&h, &gate, "status"), "cleaned");
}
