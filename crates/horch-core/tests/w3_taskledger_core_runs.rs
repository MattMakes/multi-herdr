//! `runs::resolve`: which run a `horch ledger` command acts on (#78); and the
//! checks of `runs::create` and `runs::close_run`.

use std::path::Path;

use horch_core::taskledger::model::{Actor, RunStatus};
use horch_core::taskledger::runs::{self, NewRun, Run, Thresholds};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::Store;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    (dir, store)
}

fn new_run(store: &mut Store, root: &str, slug: &str, workspace: Option<&str>) -> Run {
    store
        .immediate(|tx| {
            runs::create(
                tx,
                Path::new(root),
                &NewRun {
                    slug: slug.into(),
                    workspace_id: workspace.map(str::to_string),
                    ..Default::default()
                },
            )
        })
        .unwrap()
}

fn resolve(
    store: &Store,
    key: Option<&str>,
    workspace: Option<&str>,
) -> anyhow::Result<(Run, Option<String>)> {
    runs::resolve(store.conn(), Path::new("/repo"), key, workspace)
}

#[test]
fn a_run_id_picks_that_run() {
    let (_d, mut s) = store();
    let a = new_run(&mut s, "/repo", "a", None);
    let _b = new_run(&mut s, "/repo", "b", None);
    let (run, note) = resolve(&s, Some(&a.run_id), None).unwrap();
    assert_eq!(run.run_id, a.run_id);
    assert_eq!(note, None);
}

#[test]
fn a_slug_picks_the_active_run_over_a_newer_closed_one() {
    let (_d, mut s) = store();
    let old = new_run(&mut s, "/repo", "dup", None);
    runs::close(s.conn(), &old.run_id, RunStatus::Done).unwrap();
    let live = new_run(&mut s, "/repo", "dup", None);
    // Make the closed run the newest: the active one must still win.
    s.conn()
        .execute(
            "UPDATE runs SET started_at = '2999-01-01T00:00:00Z' WHERE run_id = ?1",
            [&old.run_id],
        )
        .unwrap();
    let (run, _) = resolve(&s, Some("dup"), None).unwrap();
    assert_eq!(run.run_id, live.run_id);
}

#[test]
fn a_slug_without_an_active_run_picks_the_newest() {
    let (_d, mut s) = store();
    let first = new_run(&mut s, "/repo", "dup", None);
    runs::close(s.conn(), &first.run_id, RunStatus::Done).unwrap();
    let second = new_run(&mut s, "/repo", "dup", None);
    runs::close(s.conn(), &second.run_id, RunStatus::Done).unwrap();
    let (run, _) = resolve(&s, Some("dup"), None).unwrap();
    assert_eq!(run.run_id, second.run_id);
}

#[test]
fn a_run_id_from_another_project_still_resolves() {
    let (_d, mut s) = store();
    let _mine = new_run(&mut s, "/repo", "mine", None);
    let other = new_run(&mut s, "/elsewhere", "theirs", None);
    let (run, _) = resolve(&s, Some(&other.run_id), None).unwrap();
    assert_eq!(run.run_id, other.run_id);
}

#[test]
fn an_unknown_key_is_an_error_that_names_it() {
    let (_d, mut s) = store();
    new_run(&mut s, "/repo", "a", None);
    let err = resolve(&s, Some("nope"), None).unwrap_err();
    assert!(
        format!("{err:#}").starts_with("no run 'nope' in "),
        "{err:#}"
    );
}

#[test]
fn an_empty_key_means_no_key() {
    let (_d, mut s) = store();
    let a = new_run(&mut s, "/repo", "a", None);
    let (run, _) = resolve(&s, Some(""), None).unwrap();
    assert_eq!(run.run_id, a.run_id);
}

#[test]
fn no_key_and_no_active_run_is_an_error() {
    let (_d, s) = store();
    let err = resolve(&s, None, None).unwrap_err().to_string();
    assert!(err.starts_with("no active run in "), "{err}");
    assert!(
        err.ends_with("Create one: horch ledger run new <slug>"),
        "{err}"
    );
}

#[test]
fn no_key_and_two_active_runs_is_an_error_that_lists_them() {
    let (_d, mut s) = store();
    new_run(&mut s, "/repo", "a", None);
    new_run(&mut s, "/repo", "b", Some("ws1"));
    let err = resolve(&s, None, None).unwrap_err().to_string();
    assert!(err.starts_with("2 active runs in "), "{err}");
    assert!(
        err.contains(": b (workspace ws1), a. Name one with --run <slug>"),
        "{err}"
    );
}

#[test]
fn the_callers_workspace_picks_among_two_active_runs() {
    let (_d, mut s) = store();
    new_run(&mut s, "/repo", "a", Some("ws1"));
    let b = new_run(&mut s, "/repo", "b", Some("ws2"));
    let (run, note) = resolve(&s, None, Some("ws2")).unwrap();
    assert_eq!(run.run_id, b.run_id);
    assert_eq!(note, None);
}

#[test]
fn one_active_run_of_another_workspace_comes_with_a_note() {
    let (_d, mut s) = store();
    let a = new_run(&mut s, "/repo", "a", Some("ws1"));
    let (run, note) = resolve(&s, None, Some("ws2")).unwrap();
    assert_eq!(run.run_id, a.run_id);
    assert_eq!(
        note.as_deref(),
        Some("NOTE: this command uses run a of workspace ws1, not of this workspace ws2. Name a run with --run <slug>.")
    );
}

#[test]
fn two_runs_of_the_same_workspace_are_ambiguous() {
    let (_d, mut s) = store();
    new_run(&mut s, "/repo", "a", Some("ws1"));
    new_run(&mut s, "/repo", "b", Some("ws1"));
    new_run(&mut s, "/repo", "c", Some("ws2"));
    let err = resolve(&s, None, Some("ws1")).unwrap_err().to_string();
    assert!(err.starts_with("2 active runs in "), "{err}");
    assert!(!err.contains("c (workspace"), "{err}");
}

#[test]
fn a_run_needs_a_slug_and_at_least_one_attempt() {
    let (_d, mut s) = store();
    let refused = |s: &mut Store, edit: &dyn Fn(&mut NewRun)| {
        let mut new = NewRun {
            slug: "a".into(),
            ..Default::default()
        };
        edit(&mut new);
        s.immediate(|tx| runs::create(tx, Path::new("/repo"), &new))
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        refused(&mut s, &|n| n.slug = "  ".into()),
        "a run needs a slug"
    );
    for bad in [0, -3] {
        assert_eq!(
            refused(&mut s, &|n| n.thresholds.max_attempts = Some(bad)),
            "--max-attempts must be 1 or more",
            "{bad}"
        );
    }
    let one = NewRun {
        slug: "a".into(),
        thresholds: Thresholds {
            max_attempts: Some(1),
            ..Default::default()
        },
        ..Default::default()
    };
    let run = s
        .immediate(|tx| runs::create(tx, Path::new("/repo"), &one))
        .unwrap();
    assert_eq!(run.max_attempts, 1, "the least that is accepted");
}

/// `close_run` refuses open work and says how to abandon it, `--abandon`
/// cancels the work and closes the run as abandoned, and a closed run does
/// not close again.
#[test]
fn a_run_with_open_work_closes_only_with_abandon_and_closes_once() {
    let (_d, mut s) = store();
    let run = new_run(&mut s, "/repo", "a", None);
    s.immediate(|tx| {
        tasks::add(
            tx,
            &run,
            &NewTask::work("t", "t.md", "dev"),
            Actor::Orchestrator,
        )
    })
    .unwrap();

    let err = runs::close_run(s.conn(), &run, None)
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("run a still has open work: "), "{err}");
    assert!(
        err.ends_with(
            "Finish or cancel it, or run horch ledger run close --run a --abandon \"<reason>\""
        ),
        "{err}"
    );

    let closed = s
        .immediate(|tx| runs::close_run(tx, &run, Some("not needed")))
        .unwrap();
    assert_eq!(closed.status, "abandoned");
    assert_eq!(closed.cancelled.len(), 1);
    assert!(closed.parked.is_empty());

    let (after, _) = resolve(&s, Some(&run.run_id), None).unwrap();
    assert_eq!(after.status, "abandoned");
    let err = runs::close_run(s.conn(), &after, None)
        .unwrap_err()
        .to_string();
    assert_eq!(err, "run a is already abandoned");
}

/// A parked worktree with no triage decision keeps a run open: the close is
/// refused with the way out, and with `--abandon` the run stays active.
#[test]
fn a_parked_worktree_without_a_decision_keeps_the_run_open() {
    let (dir, mut s) = store();
    let run = new_run(&mut s, "/repo", "a", None);
    // The folder must be on disk: a parked worktree that is gone blocks nothing.
    let path = dir.path().join("wt1");
    std::fs::create_dir_all(&path).unwrap();
    s.conn()
        .execute(
            "INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, status, created_at)
             VALUES ('wt1', '/repo', ?1, 'wt1', 'main', 'unlanded', '2000-01-01T00:00:00Z')",
            [path.to_string_lossy()],
        )
        .unwrap();
    s.conn()
        .execute(
            "INSERT INTO worktree_modes (worktree_id, run_id, name) VALUES ('wt1', ?1, 'wt1')",
            [&run.run_id],
        )
        .unwrap();

    let err = runs::close_run(s.conn(), &run, None)
        .unwrap_err()
        .to_string();
    assert!(
        err.starts_with("run a has 1 parked worktrees with no decision: "),
        "{err}"
    );
    assert!(err.contains("wt1"), "{err}");
    assert!(
        err.ends_with(
            ". Run horch worktree triage, fill in its reply table, then run horch worktree triage apply <reply>"
        ),
        "{err}"
    );

    let closed = s
        .immediate(|tx| runs::close_run(tx, &run, Some("done with it")))
        .unwrap();
    assert_eq!(closed.status, "active", "the run stays open");
    assert_eq!(closed.parked.len(), 1);
    let (after, _) = resolve(&s, Some(&run.run_id), None).unwrap();
    assert_eq!(after.status, "active");
}
