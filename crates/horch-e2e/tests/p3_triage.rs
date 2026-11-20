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

use horch_core::fleetd::fleet_runtime::adapters::job_result_file;
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
/// result line. fleetd applies `KEEP` in the request, and the command
/// prints its line. fleetd applies `ARCHIVE`, `DELETE`, `LAND` and `PR` in
/// a `triage_apply` job (T-888): the command prints that the job is queued,
/// so this waits until the job ended and gives the line of its result.
fn apply(h: &Harness, name: &str, decision: &str) -> String {
    apply_many(h, &[("W1", name, decision)])
}

fn apply_many(h: &Harness, rows: &[(&str, &str, &str)]) -> String {
    let mut body =
        "| ID | Worktree | Suggested | Decision | Direction |\n|----|----|----|----|----|\n"
            .to_string();
    for (id, name, decision) in rows {
        body.push_str(&format!("| {id} | {name} | | {decision} | |\n"));
    }
    let p = h.root.join("reply.md");
    std::fs::write(&p, body).unwrap();
    let last = "SELECT CAST(COALESCE(MAX(id), 0) AS TEXT) FROM jobs WHERE kind = 'triage_apply'";
    let last = |h: &Harness| query(h, last)[0].clone();
    let before = last(h);
    let o = h.run(&["worktree", "triage", "apply", &p.to_string_lossy()]);
    let said = out(&o);
    assert!(said.starts_with(&format!("{} ", rows[0].0)), "{}", text(&o));
    let job = last(h);
    if job == before {
        return said;
    }
    let result = wait_for("the triage_apply job to end", || {
        query(
            h,
            &format!(
                "SELECT result_json FROM jobs WHERE id = {job} AND state IN ('passed', 'failed')"
            ),
        )
        .into_iter()
        .next()
    });
    let result: serde_json::Value = serde_json::from_str(&result).unwrap();
    format!("{}\n", result["text"].as_str().unwrap_or_default())
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

/// The read-back of each ref that triage writes fails. The `triage_apply`
/// job reads the fault, and it runs under fleetd, so the test fleetd
/// starts again with it (T-888).
fn read_back_fault(h: &mut Harness) {
    h.set("HORCH_FAULT", "triage-ref-read-back");
    h.without_fleetd(|| ());
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
    // Row W7: the folder had no row; triage adopts it as `orphaned`. The
    // triage_apply job rescues it and removes it with the custody adapters,
    // and its ingest moves the row to `rescued`, then `removed` (T-888).
    assert_eq!(states(&h, &path), ["removed"]);
    assert_eq!(
        query(
            &h,
            "SELECT rescue_ref FROM worktrees WHERE state = 'removed'"
        ),
        [rescue[0].clone()]
    );
    assert_eq!(jobs(&h, "triage_apply"), ["passed"]);
    assert!(jobs(&h, "rescue").is_empty());
    assert!(jobs(&h, "worktree_remove").is_empty());
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
    read_back_fault(&mut h2);
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
    read_back_fault(&mut h);
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
    // The triage_apply job removed the first folder with the custody
    // removal adapter, in the job (T-888): no worktree_remove job ran.
    assert_eq!(jobs(&h, "triage_apply"), ["passed"]);
    assert!(jobs(&h, "worktree_remove").is_empty());
}

#[test]
fn multi_item_triage_apply_preserves_completed_first_item_when_second_item_fails() {
    let h = world("p3-triage-multi");
    let path_a = folder(&h, "feat-a");
    std::fs::write(path_a.join("feat-a.txt"), "changed\n").unwrap();
    std::fs::write(path_a.join("new-a.txt"), "untracked\n").unwrap();
    let path_b = folder(&h, "feat-b");
    git_in(&h, &path_b, &["checkout", "--quiet", "--detach"]);
    std::fs::write(path_b.join("only-b.txt"), "unique\n").unwrap();
    commit_all(&h, &path_b, "unique");
    let unique = git_in(&h, &path_b, &["rev-parse", "HEAD"]);
    let path_c = folder(&h, "feat-c");
    git_in(
        &h,
        &h.project,
        &["update-ref", "refs/horch/archive/feat-c", &unique],
    );

    let said = apply_many(
        &h,
        &[
            ("W1", "feat-a", "DELETE"),
            ("W2", "feat-b", "DELETE"),
            ("W3", "feat-c", "ARCHIVE"),
        ],
    );
    let lines: Vec<&str> = said.lines().collect();
    assert_eq!(lines.len(), 3, "{said}");
    assert!(
        lines[0].starts_with("W1 feat-a DELETE: trashed at refs/horch/trash/feat-a/"),
        "{said}"
    );
    assert!(
        lines[1].starts_with(
            "W2 feat-b DELETE FAILED: the worktree HEAD is detached at a unique commit; nothing removed"
        ),
        "{said}"
    );
    assert!(
        lines[2].starts_with(&format!(
            "W3 feat-c ARCHIVE FAILED: refs/horch/archive/feat-c holds other work ({unique}); nothing removed"
        )),
        "{said}"
    );

    assert!(!path_a.exists());
    let trash = refs(&h, "refs/horch/trash/feat-a/");
    assert_eq!(trash.len(), 1, "{trash:?}");
    let rescue = refs(&h, "refs/horch/rescue/");
    assert_eq!(rescue.len(), 1, "{rescue:?}");
    for r in [&trash[0], &rescue[0]] {
        assert_eq!(show(&h, r, "feat-a.txt"), "changed", "{r}");
        assert_eq!(show(&h, r, "new-a.txt"), "untracked", "{r}");
    }
    assert!(!git_ok(
        &h,
        &h.project,
        &["rev-parse", "--verify", "refs/heads/feat-a"]
    ));
    assert_eq!(states(&h, &path_a), ["removed"]);
    let row = |path: &Path, column: &str| {
        query(
            &h,
            &format!(
                "SELECT {column} FROM worktrees WHERE path = '{}'",
                stored(&h, path)
            ),
        )
    };
    assert_eq!(row(&path_a, "status"), ["trashed"]);
    assert_eq!(row(&path_a, "rescue_ref"), [rescue[0].clone()]);

    for (path, file) in [(&path_b, "only-b.txt"), (&path_c, "feat-c.txt")] {
        assert!(path.join(file).exists(), "{}", path.display());
        assert_eq!(
            row(path, "status"),
            ["cleanup_failed"],
            "{}",
            path.display()
        );
        assert_ne!(states(&h, path), ["removed"], "{}", path.display());
    }
    assert_eq!(git_in(&h, &path_b, &["rev-parse", "HEAD"]), unique);
    assert_eq!(rev(&h, "refs/horch/archive/feat-c"), unique);
    assert_eq!(jobs(&h, "triage_apply"), ["passed"]);
}

#[test]
fn multi_item_triage_apply_ingest_keeps_the_first_item_when_the_fact_of_the_second_is_refused() {
    let mut h = world("p3-triage-multi-ingest");
    let path_a = folder(&h, "feat-a");
    std::fs::write(path_a.join("feat-a.txt"), "changed\n").unwrap();
    std::fs::write(path_a.join("new-a.txt"), "untracked\n").unwrap();
    let path_b = folder(&h, "feat-b");
    git_in(&h, &path_b, &["checkout", "--quiet", "--detach"]);
    std::fs::write(path_b.join("only-b.txt"), "unique\n").unwrap();
    commit_all(&h, &path_b, "unique");
    let hold = h.project.join(".git").join("hold-triage-job");
    std::fs::write(&hold, "").unwrap();
    let real = h.git_bin().unwrap().to_path_buf();
    let script = format!(
        "#!/bin/sh\ncase \" $* \" in\n  *\" commit \"*)\n    while [ -e '{hold}' ]; do sleep 0.1; done\n    ;;\nesac\nexec '{real}' \"$@\"\n",
        hold = hold.display(),
        real = real.display(),
    );
    let bin = h.write_bin("git-held", script.as_bytes());
    h.set("HORCH_GIT_BIN", bin.to_string_lossy());
    h.without_fleetd(|| ());

    let rows = [("W1", "feat-a", "DELETE"), ("W2", "feat-b", "DELETE")];
    let mut body =
        "| ID | Worktree | Suggested | Decision | Direction |\n|----|----|----|----|----|\n"
            .to_string();
    for (id, name, decision) in rows {
        body.push_str(&format!("| {id} | {name} | | {decision} | |\n"));
    }
    let reply = h.root.join("reply.md");
    std::fs::write(&reply, body).unwrap();
    let o = h.run(&["worktree", "triage", "apply", &reply.to_string_lossy()]);
    assert!(out(&o).starts_with("W1 "), "{}", text(&o));
    let (job, run) = wait_for("the triage_apply job to run", || {
        query(
            &h,
            "SELECT id || ' ' || run FROM jobs WHERE kind = 'triage_apply' AND state = 'running'",
        )
        .into_iter()
        .next()
    })
    .split_once(' ')
    .map(|(j, r)| (j.parse::<i64>().unwrap(), r.parse::<i64>().unwrap()))
    .unwrap();
    assert!(refs(&h, "refs/horch/rescue/").is_empty());
    assert!(path_a.exists());

    let result = job_result_file::path(&h.fleet_root(), job, run);
    h.without_fleetd(|| {
        std::fs::remove_file(&hold).unwrap();
        wait_for("the job to write its result file", || {
            result.is_file().then_some(())
        });
        let mut file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&result).unwrap()).unwrap();
        assert_eq!(file["data"]["decisions"][0]["ok"], true, "{file}");
        assert_eq!(file["data"]["decisions"][1]["ok"], false, "{file}");
        file["data"]["decisions"][1]["rescue_ref"] = "refs/horch/rescue/forged/1".into();
        std::fs::write(&result, serde_json::to_vec(&file).unwrap()).unwrap();
    });
    wait_for("fleetd to end the triage_apply job", || {
        (jobs(&h, "triage_apply") == ["passed"]).then_some(())
    });

    assert!(!path_a.exists());
    let trash = refs(&h, "refs/horch/trash/feat-a/");
    assert_eq!(trash.len(), 1, "{trash:?}");
    let rescue = refs(&h, "refs/horch/rescue/");
    assert_eq!(rescue.len(), 1, "{rescue:?}");
    for r in [&trash[0], &rescue[0]] {
        assert_eq!(show(&h, r, "feat-a.txt"), "changed", "{r}");
        assert_eq!(show(&h, r, "new-a.txt"), "untracked", "{r}");
    }
    assert!(!git_ok(
        &h,
        &h.project,
        &["rev-parse", "--verify", "refs/heads/feat-a"]
    ));
    assert_eq!(states(&h, &path_a), ["removed"]);
    let row = |path: &Path, column: &str| {
        query(
            &h,
            &format!(
                "SELECT {column} FROM worktrees WHERE path = '{}'",
                stored(&h, path)
            ),
        )
    };
    assert_eq!(row(&path_a, "status"), ["trashed"]);
    assert_eq!(row(&path_a, "rescue_ref"), [rescue[0].clone()]);

    assert!(path_b.join("only-b.txt").exists());
    assert_eq!(row(&path_b, "status"), ["cleanup_failed"]);
    assert_ne!(states(&h, &path_b), ["removed"]);
    let refused = query(
        &h,
        "SELECT text FROM events WHERE kind = 'cleanup_failed' AND text LIKE 'Triage W2 %'",
    );
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].contains("fleetd refused the facts of this decision"),
        "{refused:?}"
    );
}
