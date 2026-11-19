//! Worktree triage through the custody jobs (fleetd-p3 ticket 13): the
//! real `horch` binary, its own git repository, ledger and state folders in
//! a temp dir. Triage keeps each of its safeties (RR-16, RR-17, rows W7 to
//! W9 of `docs/specs/fleetd/preservation-04-behavior.md`), and each folder
//! that it removes is first on a verified rescue ref, then removed by a
//! `worktree_remove` job (RR-9). A restore makes its tree through a
//! `worktree_add` job.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::taskledger::store::Store;
use horch_e2e::harness::{wait_for, Harness};

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

/// A worktree `.worktrees/<name>` on the branch `<name>` that no ledger
/// row names, with 1 commit of the file `<name>.txt`.
fn folder(h: &Harness, name: &str) -> PathBuf {
    let path = h.project.join(".worktrees").join(name);
    git_in(
        h,
        &h.project,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            name,
            &path.to_string_lossy(),
        ],
    );
    std::fs::write(path.join(format!("{name}.txt")), "work\n").unwrap();
    commit_all(h, &path, name);
    path
}

fn commit_all(h: &Harness, dir: &Path, message: &str) {
    git_in(h, dir, &["add", "-A"]);
    git_in(
        h,
        dir,
        &["commit", "--quiet", "--no-gpg-sign", "-m", message],
    );
}

/// `horch worktree triage apply` of 1 decision of the row W1 `name`: its
/// result line. A failed decision exits non-zero.
fn apply(h: &Harness, name: &str, decision: &str) -> String {
    let body = format!(
        "| ID | Worktree | Suggested | Decision | Direction |\n|----|----|----|----|----|\n\
         | W1 | {name} | | {decision} | |\n"
    );
    let p = h.root.join("reply.md");
    std::fs::write(&p, body).unwrap();
    let o = h.run(&["worktree", "triage", "apply", &p.to_string_lossy()]);
    let said = out(&o);
    assert!(said.starts_with("W1 "), "{}", text(&o));
    said
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

fn rev(h: &Harness, name: &str) -> String {
    git_in(h, &h.project, &["rev-parse", name])
}

/// The text of `file` in the commit of `refname`.
fn show(h: &Harness, refname: &str, file: &str) -> String {
    git_in(h, &h.project, &["show", &format!("{refname}:{file}")])
}

/// 1 value of the ledger.
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
/// resolved (`/tmp` is `/private/tmp` on macOS).
fn stored(h: &Harness, path: &Path) -> String {
    let rel = path.strip_prefix(&h.project).unwrap();
    std::fs::canonicalize(&h.project)
        .unwrap()
        .join(rel)
        .display()
        .to_string()
}

/// The state of each row of the folder `path`, oldest first.
fn states(h: &Harness, path: &Path) -> Vec<String> {
    query(
        h,
        &format!(
            "SELECT state FROM worktrees WHERE path = '{}' ORDER BY rowid",
            stored(h, path)
        ),
    )
}

/// The ended runs of `kind`: `<state>` each, oldest first.
fn jobs(h: &Harness, kind: &str) -> Vec<String> {
    query(
        h,
        &format!("SELECT state FROM jobs WHERE kind = '{kind}' ORDER BY id"),
    )
}

#[test]
fn pres_04_w9_triage_commits_untracked_before_delete() {
    let h = world("p3-triage-w9");
    let path = folder(&h, "feat");
    std::fs::write(path.join("feat.txt"), "changed\n").unwrap();
    std::fs::write(path.join("new.txt"), "untracked\n").unwrap();
    let said = apply(&h, "feat", "DELETE");
    assert!(
        said.starts_with("W1 feat DELETE: trashed at refs/horch/trash/feat/"),
        "{said}"
    );
    assert!(!path.exists());
    let trash = refs(&h, "refs/horch/trash/feat/");
    assert_eq!(trash.len(), 1, "{trash:?}");
    let rescue = refs(&h, "refs/horch/rescue/");
    assert_eq!(rescue.len(), 1, "{rescue:?}");
    assert!(rescue[0].ends_with("/1"), "{rescue:?}");
    for r in [&trash[0], &rescue[0]] {
        assert_eq!(show(&h, r, "feat.txt"), "changed", "{r}");
        assert_eq!(show(&h, r, "new.txt"), "untracked", "{r}");
    }
    // Row W7: the folder had no row; triage adopts it as `orphaned`, and
    // the custody jobs move it to `rescued`, then `removed`.
    assert_eq!(states(&h, &path), ["removed"]);
    assert_eq!(jobs(&h, "rescue"), ["passed"]);
    assert_eq!(jobs(&h, "worktree_remove"), ["passed"]);
    assert!(!git_ok(
        &h,
        &h.project,
        &["rev-parse", "--verify", "refs/heads/feat"]
    ));
}

#[test]
fn rr17_triage_refuses_a_unique_detached_head_and_the_tree_stays() {
    let h = world("p3-triage-detached");
    let path = folder(&h, "feat");
    git_in(&h, &path, &["checkout", "--quiet", "--detach"]);
    std::fs::write(path.join("only-here.txt"), "unique\n").unwrap();
    commit_all(&h, &path, "unique");
    let head = git_in(&h, &path, &["rev-parse", "HEAD"]);
    let said = apply(&h, "feat", "DELETE");
    assert!(
        said.starts_with(
            "W1 feat DELETE FAILED: the worktree HEAD is detached at a unique commit; nothing removed"
        ),
        "{said}"
    );
    assert!(path.join("only-here.txt").exists());
    assert_eq!(git_in(&h, &path, &["rev-parse", "HEAD"]), head);
    assert!(refs(&h, "refs/horch/rescue/").is_empty());
    assert!(jobs(&h, "worktree_remove").is_empty());
}

#[test]
fn rr17_triage_deletes_a_branch_only_when_the_trash_ref_holds_its_tip() {
    let h = world("p3-triage-branch");
    let path = folder(&h, "feat");
    std::fs::write(path.join("wip.txt"), "wip\n").unwrap();
    let before = rev(&h, "feat");
    let said = apply(&h, "feat", "DELETE");
    assert!(said.contains("the branch feat is deleted"), "{said}");
    let trash = refs(&h, "refs/horch/trash/feat/");
    let held = rev(&h, &trash[0]);
    // The trash ref holds the tip of the branch: the commit of the
    // uncommitted file, on the old tip.
    assert_eq!(rev(&h, &format!("{held}^")), before);
    assert_eq!(show(&h, &trash[0], "wip.txt"), "wip");

    // A trash ref that does not hold the tip (its read-back fails): the
    // branch and the folder stay.
    let mut h2 = world("p3-triage-branch-kept");
    let kept = folder(&h2, "feat");
    h2.set("HORCH_FAULT", "triage-ref-read-back");
    let said = apply(&h2, "feat", "DELETE");
    assert!(said.contains("DELETE FAILED"), "{said}");
    assert!(kept.exists());
    assert!(git_ok(
        &h2,
        &h2.project,
        &["rev-parse", "--verify", "refs/heads/feat"]
    ));
}

#[test]
fn pres_04_w8_refs_move_forward_and_are_read_back() {
    let h = world("p3-triage-w8");
    let path = folder(&h, "feat");
    // Other work at the archive ref: a commit that is not in the history of
    // the folder.
    let other = folder(&h, "other");
    let elsewhere = git_in(&h, &other, &["rev-parse", "HEAD"]);
    git_in(
        &h,
        &h.project,
        &["update-ref", "refs/horch/archive/feat", &elsewhere],
    );
    let said = apply(&h, "feat", "ARCHIVE");
    assert!(
        said.starts_with(&format!(
            "W1 feat ARCHIVE FAILED: refs/horch/archive/feat holds other work ({elsewhere}); nothing removed"
        )),
        "{said}"
    );
    assert!(path.exists());
    assert_eq!(rev(&h, "refs/horch/archive/feat"), elsewhere);
    assert!(refs(&h, "refs/horch/rescue/").is_empty());

    // An archive ref at an older commit of the folder moves forward.
    let old = git_in(&h, &path, &["rev-parse", "HEAD^"]);
    git_in(
        &h,
        &h.project,
        &["update-ref", "refs/horch/archive/feat", &old, &elsewhere],
    );
    let said = apply(&h, "feat", "ARCHIVE");
    assert!(
        said.starts_with("W1 feat ARCHIVE: archived at refs/horch/archive/feat"),
        "{said}"
    );
    assert_eq!(rev(&h, "refs/horch/archive/feat"), rev(&h, "feat"));
    assert!(!path.exists());
    assert_eq!(refs(&h, "refs/horch/rescue/").len(), 1);
}

#[test]
fn rr16_failed_read_back_keeps_the_folder() {
    let mut h = world("p3-triage-read-back");
    let path = folder(&h, "feat");
    std::fs::write(path.join("new.txt"), "untracked\n").unwrap();
    h.set("HORCH_FAULT", "triage-ref-read-back");
    let said = apply(&h, "feat", "ARCHIVE");
    assert!(
        said.starts_with("W1 feat ARCHIVE FAILED: refs/horch/archive/feat does not hold"),
        "{said}"
    );
    assert!(
        said.ends_with("after the write; nothing removed\n"),
        "{said}"
    );
    assert!(path.join("new.txt").exists());
    assert!(refs(&h, "refs/horch/rescue/").is_empty());
    assert!(jobs(&h, "rescue").is_empty());
    assert!(jobs(&h, "worktree_remove").is_empty());
    assert_eq!(
        query(
            &h,
            &format!(
                "SELECT status FROM worktrees WHERE path = '{}'",
                stored(&h, &path)
            )
        ),
        ["cleanup_failed"]
    );
}

#[test]
fn rr16_second_rescue_through_triage_does_not_overwrite_the_first_ref() {
    let h = world("p3-triage-second-rescue");
    let path = folder(&h, "feat");
    std::fs::write(path.join("first.txt"), "first\n").unwrap();
    // git refuses to move or remove a locked tree: the rescue passes, and
    // the worktree_remove job fails.
    git_in(
        &h,
        &h.project,
        &["worktree", "lock", &path.to_string_lossy()],
    );
    let said = apply(&h, "feat", "DELETE");
    assert!(said.starts_with("W1 feat DELETE FAILED: "), "{said}");
    assert!(path.join("first.txt").exists());
    let first = refs(&h, "refs/horch/rescue/");
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(first[0].ends_with("/1"), "{first:?}");
    let first_commit = rev(&h, &first[0]);
    assert_eq!(states(&h, &path), ["rescued"]);

    git_in(
        &h,
        &h.project,
        &["worktree", "unlock", &path.to_string_lossy()],
    );
    std::fs::write(path.join("second.txt"), "second\n").unwrap();
    let said = apply(&h, "feat", "DELETE");
    assert!(said.starts_with("W1 feat DELETE: trashed at "), "{said}");
    assert!(!path.exists());
    let both = refs(&h, "refs/horch/rescue/");
    assert_eq!(both.len(), 2, "{both:?}");
    assert_eq!(both[0], first[0]);
    assert!(both[1].ends_with("/2"), "{both:?}");
    assert_eq!(rev(&h, &first[0]), first_commit);
    assert_eq!(show(&h, &both[1], "second.txt"), "second");
    assert_eq!(states(&h, &path), ["removed"]);
}

#[test]
fn trash_ref_older_than_30_days_is_collected_and_a_rescue_ref_is_not() {
    let h = world("p3-triage-gc");
    folder(&h, "feat");
    apply(&h, "feat", "DELETE");
    let trash = refs(&h, "refs/horch/trash/feat/");
    let rescue = refs(&h, "refs/horch/rescue/");
    assert_eq!((trash.len(), rescue.len()), (1, 1), "{trash:?} {rescue:?}");
    let commit = rev(&h, &trash[0]);
    let old = "refs/horch/trash/feat/20200101T000000Z";
    git_in(&h, &h.project, &["update-ref", old, &commit, ""]);
    git_in(&h, &h.project, &["update-ref", "-d", &trash[0], &commit]);
    let said = ok(&h, &["ledger", "gc"]);
    assert!(said.contains(old), "{said}");
    assert!(refs(&h, "refs/horch/trash/").is_empty());
    assert_eq!(refs(&h, "refs/horch/rescue/"), rescue);
}

#[test]
fn restore_makes_its_tree_through_a_worktree_add_job() {
    let h = world("p3-triage-restore");
    let path = folder(&h, "feat");
    std::fs::write(path.join("new.txt"), "untracked\n").unwrap();
    apply(&h, "feat", "ARCHIVE");
    assert!(!path.exists());
    assert!(jobs(&h, "worktree_add").is_empty());
    let said = ok(&h, &["worktree", "restore", "feat"]);
    assert!(
        said.starts_with(&format!("restored {} on feat at ", stored(&h, &path))),
        "{said}"
    );
    assert!(
        said.trim_end().ends_with("from refs/horch/archive/feat"),
        "{said}"
    );
    // The worktree_add job runs in the next pass of fleetd (ticket 25 of
    // fleetd-p5, T-474).
    wait_for("the worktree_add job to pass", || {
        (jobs(&h, "worktree_add") == ["passed"]).then_some(())
    });
    assert_eq!(
        std::fs::read_to_string(path.join("new.txt")).unwrap(),
        "untracked\n"
    );
    assert_eq!(git_in(&h, &path, &["branch", "--show-current"]), "feat");
    // The removed row keeps its path; the restored tree has a new row. It
    // is for the operator: `created` with no owner, so the reconciler does
    // not orphan it.
    assert_eq!(states(&h, &path), ["removed", "created"]);
}

#[test]
fn restore_runs_no_hook_of_the_project() {
    let h = world("p3-triage-restore-hook");
    let path = folder(&h, "feat");
    apply(&h, "feat", "ARCHIVE");
    let marker = h.root.join("hook-ran");
    let hooks = h.project.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("post-checkout");
    std::fs::write(
        &hook,
        format!("#!/bin/sh\necho ran > '{}'\n", marker.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    ok(&h, &["worktree", "restore", "feat"]);
    // The worktree_add job runs in the next pass of fleetd (ticket 25 of
    // fleetd-p5, T-474).
    wait_for("the restored tree", || {
        (jobs(&h, "worktree_add") == ["passed"]).then_some(())
    });
    assert!(path.exists());
    assert!(
        !marker.exists(),
        "the post-checkout hook of the project ran"
    );
}

#[test]
fn a_folder_at_the_path_of_a_removed_row_is_a_new_orphaned_row_and_nothing_is_removed() {
    let h = world("p3-triage-same-path");
    let path = folder(&h, "feat");
    apply(&h, "feat", "DELETE");
    assert_eq!(states(&h, &path), ["removed"]);
    // A new folder at the same path, on another branch.
    git_in(
        &h,
        &h.project,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "again",
            &path.to_string_lossy(),
        ],
    );
    std::fs::write(path.join("again.txt"), "again\n").unwrap();
    let said = apply(&h, "feat", "KEEP");
    assert!(said.starts_with("W1 feat KEEP: kept"), "{said}");
    assert_eq!(states(&h, &path), ["removed", "orphaned"]);
    assert!(path.join("again.txt").exists());
    assert_eq!(jobs(&h, "worktree_remove"), ["passed"]);
}
