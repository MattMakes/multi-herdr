//! `horch worktree triage apply`: each decision on a real git repository in
//! a temp dir, with a git that records every call (#308, sections 3 and 5).

mod i308_support;

use std::path::{Path, PathBuf};

use chrono::{Duration, Utc};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::triage::apply::{self, Applied, Env};
use horch_core::taskledger::triage::{self, refs, reply, Measure, Row};
use i308_support::{commit, git, write, World};

/// A git that writes each call to `<top>/git.log`, then runs the host git.
/// With `fail_trash`, a write of a trash ref fails.
fn recording_git(top: &Path, fail_trash: bool) -> (Git, PathBuf) {
    let log = top.join("git.log");
    let bin = top.join("fake-git");
    let fail = if fail_trash {
        "case \"$*\" in *\"update-ref refs/horch/trash/\"*) exit 1;; esac\n"
    } else {
        ""
    };
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\n{fail}exec git \"$@\"\n",
            log.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    // macOS scans a new executable at its first exec: warm it here.
    assert!(std::process::Command::new(&bin)
        .arg("--version")
        .status()
        .unwrap()
        .success());
    std::fs::write(&log, "").unwrap();
    (Git::new(&bin), log)
}

fn rows(w: &World, git: &Git) -> Vec<Row> {
    triage::collect(w.store.conn(), git, &w.root, Measure::Ids).unwrap()
}

/// Run `decision` (with `direction`) on the row named `name`.
fn decide(w: &mut World, git: &Git, name: &str, decision: &str, direction: &str) -> Applied {
    decide_with(w, git, name, decision, direction, None)
}

fn decide_with(
    w: &mut World,
    git: &Git,
    name: &str,
    decision: &str,
    direction: &str,
    gh: Option<(&Path, &str)>,
) -> Applied {
    let rows = rows(w, git);
    let row = rows.iter().find(|r| r.name == name).unwrap();
    let text = format!(
        "| ID | Worktree | Suggested | Decision | Direction |\n|---|---|---|---|---|\n| {} | {} | | {decision} | {direction} |\n",
        row.id, row.name
    );
    let decided = reply::parse(&text, &rows, gh.is_some()).unwrap();
    let evidence = w.top.join("evidence");
    let env = Env {
        git,
        root: &w.root,
        evidence_root: &evidence,
        gh: gh.map(|g| g.0),
        remote: gh.map(|g| g.1),
        primary: "opus",
    };
    let mut out = apply::apply(&mut w.store, &env, &rows, &decided).unwrap();
    out.remove(0)
}

fn column(w: &World, path: &Path, col: &str) -> Option<String> {
    w.store
        .conn()
        .query_row(
            &format!("SELECT {col} FROM worktrees WHERE path = ?1"),
            [path.to_string_lossy()],
            |r| r.get(0),
        )
        .unwrap()
}

/// A worktree `feat` with 1 commit of `f.txt`.
fn feat(w: &World) -> PathBuf {
    let base = w.rev("main");
    let path = w.worktree("feat", "feat", &base);
    commit(&path, "f.txt", "work\n");
    path
}

#[test]
fn land_on_main_makes_land_branch_at_the_rebased_tip_and_main_does_not_move() {
    let mut w = World::new();
    let path = feat(&w);
    write(&path, "wip.txt", "uncommitted\n");
    commit(&w.root, "b.txt", "main moved\n");
    let main_before = w.rev("main");
    let g = w.git.clone();
    let a = decide(&mut w, &g, "feat", "LAND", "");
    assert!(a.ok, "{}", a.line());
    assert_eq!(w.rev("main"), main_before);
    let landed = w.rev("land/feat");
    // Rebased onto main, with the commit and the uncommitted file.
    assert!(git_ok(
        &w.root,
        &["merge-base", "--is-ancestor", &main_before, &landed]
    ));
    assert_eq!(
        git(&w.root, &["show", &format!("{landed}:wip.txt")]),
        "uncommitted"
    );
    assert_eq!(git(&w.root, &["show", &format!("{landed}:f.txt")]), "work");
    assert!(!path.exists());
    assert_eq!(column(&w, &path, "decision").as_deref(), Some("LAND"));
    assert_eq!(column(&w, &path, "status").as_deref(), Some("cleaned"));
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap()
        .success()
}

#[test]
fn land_on_a_feature_branch_fast_forwards_it_and_records_the_land() {
    let mut w = World::new();
    let base = w.rev("main");
    let path = w.worktree("work", "work", &base);
    commit(&path, "f.txt", "work\n");
    git(&w.root, &["switch", "--quiet", "-c", "feat/x"]);
    commit(&w.root, "x.txt", "x\n");
    let main_before = w.rev("main");
    let g = w.git.clone();
    let a = decide(&mut w, &g, "work", "LAND", "");
    assert!(a.ok, "{}", a.line());
    assert_eq!(w.rev("main"), main_before);
    let x = w.rev("feat/x");
    assert_eq!(git(&w.root, &["show", &format!("{x}:f.txt")]), "work");
    assert_eq!(git(&w.root, &["show", &format!("{x}:x.txt")]), "x");
    assert_eq!(
        std::fs::read_to_string(w.root.join("f.txt")).unwrap(),
        "work\n"
    );
    assert!(!path.exists());
    assert_eq!(
        column(&w, &path, "landed_commit").as_deref(),
        Some(x.as_str())
    );
    assert!(a.text.starts_with("landed on feat/x at "), "{}", a.text);
}

#[test]
fn land_with_a_conflict_changes_no_branch_and_no_folder() {
    let mut w = World::new();
    let base = w.rev("main");
    let path = w.worktree("clash", "clash", &base);
    commit(&path, "a.txt", "clash\n");
    write(&path, "wip.txt", "uncommitted\n");
    commit(&w.root, "a.txt", "main\n");
    let (main_before, clash_before) = (w.rev("main"), w.rev("clash"));
    let g = w.git.clone();
    let a = decide(&mut w, &g, "clash", "LAND", "");
    assert!(!a.ok);
    assert_eq!(a.text, "conflicts: choose FINISH");
    assert_eq!((w.rev("main"), w.rev("clash")), (main_before, clash_before));
    assert!(!w.has_ref("refs/heads/land/clash"));
    assert_eq!(
        std::fs::read_to_string(path.join("wip.txt")).unwrap(),
        "uncommitted\n"
    );
    assert_eq!(git(&path, &["status", "--porcelain"]), "?? wip.txt");
    assert_eq!(
        column(&w, &path, "status").as_deref(),
        Some("cleanup_failed")
    );
    assert_eq!(column(&w, &path, "decision"), None);
}

#[test]
fn archive_keeps_the_branch_and_an_untracked_file_and_restore_brings_it_back() {
    let mut w = World::new();
    let path = feat(&w);
    write(&path, "notes.txt", "keep me\n");
    write(&path, "target/build.bin", "build output\n");
    let g = w.git.clone();
    let a = decide(&mut w, &g, "feat", "ARCHIVE", "");
    assert!(a.ok, "{}", a.line());
    assert!(!path.exists());
    assert!(w.has_ref("refs/heads/feat"));
    let archived = w.rev("refs/horch/archive/feat");
    assert_eq!(archived, w.rev("feat"));
    assert_eq!(
        git(&w.root, &["show", &format!("{archived}:notes.txt")]),
        "keep me"
    );
    // Ignored build output is never committed.
    assert!(!git_ok(
        &w.root,
        &["cat-file", "-e", &format!("{archived}:target/build.bin")]
    ));
    assert_eq!(column(&w, &path, "status").as_deref(), Some("archived"));
    assert_eq!(
        column(&w, &path, "archive_ref").as_deref(),
        Some("refs/horch/archive/feat")
    );
    // Archived rows are settled: the next triage does not list them.
    assert!(rows(&w, &g).is_empty());

    let said = apply::restore(&w.store, &g, &w.root, "feat").unwrap();
    assert!(said.starts_with("restored "), "{said}");
    assert_eq!(
        std::fs::read_to_string(path.join("notes.txt")).unwrap(),
        "keep me\n"
    );
    assert_eq!(git(&path, &["branch", "--show-current"]), "feat");
    assert_eq!(column(&w, &path, "status").as_deref(), Some("open"));
    assert_eq!(column(&w, &path, "decision"), None);
}

#[test]
fn delete_writes_a_trash_ref_removes_folder_and_branch_and_restore_works() {
    let mut w = World::new();
    let path = feat(&w);
    let tip = w.rev("feat");
    let g = w.git.clone();
    let a = decide(&mut w, &g, "feat", "DELETE", "");
    assert!(a.ok, "{}", a.line());
    assert!(!path.exists());
    assert!(!w.has_ref("refs/heads/feat"));
    let trash = column(&w, &path, "archive_ref").unwrap();
    assert!(trash.starts_with("refs/horch/trash/feat/"), "{trash}");
    assert_eq!(w.rev(&trash), tip);
    assert_eq!(column(&w, &path, "status").as_deref(), Some("trashed"));
    apply::restore(&w.store, &g, &w.root, "feat").unwrap();
    assert_eq!(git(&path, &["rev-parse", "HEAD"]), tip);
}

#[test]
fn gc_keeps_a_trash_ref_at_29_days_and_deletes_it_at_31() {
    let w = World::new();
    let tip = w.rev("main");
    let now = Utc::now();
    let young = refs::trash_ref("young", now - Duration::days(29));
    let old = refs::trash_ref("old", now - Duration::days(31));
    for r in [&young, &old] {
        git(&w.root, &["update-ref", r, &tip]);
    }
    git(&w.root, &["update-ref", "refs/horch/archive/kept", &tip]);
    let deleted = refs::gc(&w.git, &w.root, now).unwrap();
    assert_eq!(deleted, vec![old.clone()]);
    assert!(w.has_ref(&young));
    assert!(!w.has_ref(&old));
    assert!(w.has_ref("refs/horch/archive/kept"));
}

#[test]
fn no_decision_forces_git_or_force_deletes_a_branch() {
    let mut w = World::new();
    let (g, log) = recording_git(&w.top, false);
    let base = w.rev("main");
    for name in ["a1", "d1", "l1", "k1"] {
        let p = w.worktree(name, name, &base);
        commit(&p, &format!("{name}.txt"), "w\n");
        write(&p, "wip.txt", "uncommitted\n");
    }
    for (name, d) in [
        ("a1", "ARCHIVE"),
        ("d1", "DELETE"),
        ("l1", "LAND"),
        ("k1", "KEEP"),
    ] {
        let a = decide(&mut w, &g, name, d, "");
        assert!(a.ok, "{}", a.line());
    }
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(calls.contains("worktree remove"), "{calls}");
    for line in calls.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        assert!(
            !words
                .iter()
                .any(|a| matches!(*a, "--force" | "-f" | "-D" | "--delete")),
            "a forced git call: {line}"
        );
    }
}

#[test]
fn delete_deletes_no_branch_and_no_folder_when_the_trash_ref_write_fails() {
    let mut w = World::new();
    let (g, log) = recording_git(&w.top, true);
    let path = feat(&w);
    let a = decide(&mut w, &g, "feat", "DELETE", "");
    assert!(!a.ok, "{}", a.line());
    assert!(path.exists());
    assert!(w.has_ref("refs/heads/feat"));
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains("update-ref refs/horch/trash/feat/"),
        "{calls}"
    );
    assert!(!calls.contains("update-ref -d"), "{calls}");
    assert!(!calls.contains("worktree remove"), "{calls}");
    assert_eq!(
        column(&w, &path, "status").as_deref(),
        Some("cleanup_failed")
    );
}

#[test]
fn keep_records_the_decision_and_the_row_comes_back() {
    let mut w = World::new();
    let path = feat(&w);
    let g = w.git.clone();
    let a = decide(&mut w, &g, "feat", "KEEP", "");
    assert!(a.ok);
    assert!(path.exists());
    assert_eq!(column(&w, &path, "decision").as_deref(), Some("KEEP"));
    let again = rows(&w, &g);
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].decision.as_deref(), Some("KEEP"));
}

#[test]
fn finish_queues_a_task_that_inherits_the_worktree() {
    let mut w = World::new();
    let path = feat(&w);
    let root = w.root.clone();
    let run = w
        .store
        .immediate(|tx| {
            runs::create(
                tx,
                &root,
                &NewRun {
                    slug: "r1".into(),
                    base_branch: Some("main".into()),
                    run_branch: Some("main".into()),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let g = w.git.clone();
    let a = decide(
        &mut w,
        &g,
        "feat",
        "FINISH",
        "Fix the flaky test, then land",
    );
    assert!(a.ok, "{}", a.line());
    let q = a.queued.unwrap();
    assert_eq!(
        (q.run_id.as_str(), q.task.as_str()),
        (run.run_id.as_str(), "T-1")
    );
    assert_eq!(column(&w, &path, "task_id"), Some(q.task_id.clone()));
    assert_eq!(column(&w, &path, "status").as_deref(), Some("open"));
    let (file, status): (String, String) = w
        .store
        .conn()
        .query_row(
            "SELECT task_file, status FROM tasks WHERE task_id = ?1",
            [&q.task_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (file.as_str(), status.as_str()),
        ("ai_docs/triage/feat.md", "queued")
    );
    let brief = std::fs::read_to_string(w.root.join(&file)).unwrap();
    assert!(brief.contains("Fix the flaky test, then land"), "{brief}");
    // The queued task's worktree is not parked: a worker is coming.
    assert!(rows(&w, &g).is_empty());
}

#[test]
fn finish_with_no_active_run_fails_and_keeps_everything() {
    let mut w = World::new();
    let path = feat(&w);
    let g = w.git.clone();
    let a = decide(&mut w, &g, "feat", "FINISH", "go on");
    assert!(!a.ok);
    assert!(
        a.text.starts_with("FINISH needs an active run"),
        "{}",
        a.text
    );
    assert!(path.exists());
}

#[test]
fn pr_pushes_opens_a_pull_request_and_archives_the_folder() {
    let mut w = World::new();
    let bare = w.top.join("remote.git");
    git(
        &w.top,
        &["init", "--quiet", "--bare", &bare.to_string_lossy()],
    );
    git(
        &w.root,
        &["remote", "add", "origin", &bare.to_string_lossy()],
    );
    let gh = w.top.join("fake-gh");
    let gh_log = w.top.join("gh.log");
    std::fs::write(
        &gh,
        format!(
            "#!/bin/sh\n[ \"$1\" = --version ] && exit 0\necho \"$*\" >> '{}'\necho https://example.com/pr/7\n",
            gh_log.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        apply::pr_remote(&w.git, &w.root, &gh).as_deref(),
        Some("origin")
    );
    let path = feat(&w);
    let tip = w.rev("feat");
    let g = w.git.clone();
    let a = decide_with(
        &mut w,
        &g,
        "feat",
        "PR",
        "Cache the facts",
        Some((&gh, "origin")),
    );
    assert!(a.ok, "{}", a.line());
    assert!(a.text.contains("https://example.com/pr/7"), "{}", a.text);
    assert_eq!(git(&bare, &["rev-parse", "feat"]), tip);
    let calls = std::fs::read_to_string(&gh_log).unwrap();
    assert!(
        calls.starts_with("pr create --head feat --title Cache the facts"),
        "{calls}"
    );
    assert!(!path.exists());
    assert_eq!(column(&w, &path, "decision").as_deref(), Some("PR"));
}

#[test]
fn pr_is_not_offered_without_gh_or_without_a_remote() {
    let w = World::new();
    let gh = w.top.join("fake-gh");
    std::fs::write(&gh, "#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A gh, but no remote.
    assert_eq!(apply::pr_remote(&w.git, &w.root, &gh), None);
    // A remote, but no gh.
    git(&w.root, &["remote", "add", "origin", "/nowhere.git"]);
    assert_eq!(
        apply::pr_remote(&w.git, &w.root, &w.top.join("no-gh")),
        None
    );
    assert_eq!(
        apply::pr_remote(&w.git, &w.root, &gh).as_deref(),
        Some("origin")
    );
}

#[test]
fn a_decision_closes_the_open_finding_of_its_worktree() {
    let mut w = World::new();
    let path = feat(&w);
    w.store
        .conn()
        .execute(
            "INSERT INTO import_findings (kind, detail, status, created_at) VALUES ('worktree_orphan', ?1, 'open', 'now')",
            [format!("{} (branch feat): on disk", path.display())],
        )
        .unwrap();
    let g = w.git.clone();
    assert_eq!(rows(&w, &g)[0].findings, vec![1]);
    assert!(decide(&mut w, &g, "feat", "KEEP", "").ok);
    let status: String = w
        .store
        .conn()
        .query_row(
            "SELECT status FROM import_findings WHERE finding_id = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "dismissed");
}
