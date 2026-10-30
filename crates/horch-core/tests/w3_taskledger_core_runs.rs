//! `runs::resolve`: which run a `horch ledger` command acts on (#78).

use std::path::Path;

use horch_core::taskledger::model::RunStatus;
use horch_core::taskledger::runs::{self, NewRun, Run};
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
    assert!(err.ends_with("Create one: horch ledger run new <slug>"), "{err}");
}

#[test]
fn no_key_and_two_active_runs_is_an_error_that_lists_them() {
    let (_d, mut s) = store();
    new_run(&mut s, "/repo", "a", None);
    new_run(&mut s, "/repo", "b", Some("ws1"));
    let err = resolve(&s, None, None).unwrap_err().to_string();
    assert!(err.starts_with("2 active runs in "), "{err}");
    assert!(err.contains(": b (workspace ws1), a. Name one with --run <slug>"), "{err}");
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
