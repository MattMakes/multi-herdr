//! `horch worktree triage apply` on a detached-HEAD worktree (L14): its
//! ledger row records the branch `(detached)`, and `ARCHIVE` and `DELETE`
//! use the commit id of HEAD, never the text `(detached)`, as a git ref.

mod i308_support;
#[path = "common/test_jobs.rs"]
mod test_jobs;

use std::path::{Path, PathBuf};

use horch_core::taskledger::model::WorktreeStatus;
use horch_core::taskledger::triage::apply::{self, Applied, Env};
use horch_core::taskledger::triage::{self, reply, Measure, Row};
use i308_support::{commit, git, World};
use test_jobs::TestJobs;

/// The branch text that custody records for a detached HEAD.
const DETACHED: &str = "(detached)";

fn rows(w: &World) -> Vec<Row> {
    triage::collect(w.store.conn(), &w.git, &w.root, Measure::Ids).unwrap()
}

/// Run `decision` on the row named `name`.
fn decide(w: &mut World, name: &str, decision: &str) -> Applied {
    let rows = rows(w);
    let row = rows.iter().find(|r| r.name == name).unwrap();
    let text = format!(
        "| ID | Worktree | Suggested | Decision | Direction |\n|---|---|---|---|---|\n| {} | {} | | {decision} | |\n",
        row.id, row.name
    );
    let decided = reply::parse(&text, &rows, false).unwrap();
    let evidence = w.top.join("evidence");
    let git = w.git.clone();
    let root = w.root.clone();
    let env = Env {
        git: &git,
        root: &root,
        evidence_root: &evidence,
        gh: None,
        remote: None,
        primary: "opus",
        jobs: &TestJobs,
    };
    let mut out = apply::apply(&mut w.store, &env, &rows, &decided).unwrap();
    out.remove(0)
}

fn column(w: &World, path: &Path, col: &str) -> Option<String> {
    w.store
        .conn()
        .query_row(
            &format!("SELECT {col} FROM worktrees WHERE path = ?1 ORDER BY rowid DESC LIMIT 1"),
            [path.to_string_lossy()],
            |r| r.get(0),
        )
        .unwrap()
}

/// A worktree `.worktrees/det` with a detached HEAD at the tip of the
/// branch `keep`, which reaches it (RR-17), and a ledger row whose branch
/// is `(detached)`. Returns the folder and its HEAD commit id.
fn detached(w: &mut World) -> (PathBuf, String) {
    git(&w.root, &["branch", "keep"]);
    let path = w.root.join(".worktrees").join("det");
    git(
        &w.root,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            &path.to_string_lossy(),
            "keep",
        ],
    );
    let tip = commit(&path, "d.txt", "detached work\n");
    git(&w.root, &["branch", "-f", "keep", &tip]);
    // KEEP gives the folder its ledger row; custody records `(detached)`.
    let a = decide(w, "det", "KEEP");
    assert!(a.ok, "{}", a.line());
    w.store
        .conn()
        .execute(
            "UPDATE worktrees SET branch = ?2 WHERE path = ?1",
            [path.to_string_lossy().as_ref(), DETACHED],
        )
        .unwrap();
    (path, tip)
}

#[test]
fn triage_archive_detached_worktree_uses_commit_id() {
    let mut w = World::new();
    let (path, tip) = detached(&mut w);
    let a = decide(&mut w, "det", "ARCHIVE");
    assert!(a.ok, "{}", a.line());
    assert!(!a.text.contains(DETACHED), "{}", a.text);
    assert_eq!(w.rev("refs/horch/archive/det"), tip);
    assert!(!path.exists());
    assert_eq!(
        column(&w, &path, "status").as_deref(),
        Some(WorktreeStatus::Archived.as_str())
    );
    assert_eq!(column(&w, &path, "state").as_deref(), Some("removed"));
}

#[test]
fn triage_delete_detached_worktree_uses_commit_id() {
    let mut w = World::new();
    let (path, tip) = detached(&mut w);
    let a = decide(&mut w, "det", "DELETE");
    assert!(a.ok, "{}", a.line());
    assert!(a.text.contains("no branch to delete"), "{}", a.text);
    let trash = column(&w, &path, "archive_ref").unwrap();
    assert!(trash.starts_with("refs/horch/trash/det/"), "{trash}");
    assert_eq!(w.rev(&trash), tip);
    assert!(!path.exists());
    assert!(w.has_ref("refs/heads/keep"));
    assert_eq!(
        column(&w, &path, "status").as_deref(),
        Some(WorktreeStatus::Trashed.as_str())
    );
    assert_eq!(column(&w, &path, "state").as_deref(), Some("removed"));
}
