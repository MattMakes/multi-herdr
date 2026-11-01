//! `horch ledger status` shows the parked worktrees and their disk from the
//! value `horch worktree triage` stored. It never walks a folder (#308).

mod i308_support;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Instant;

use chrono::Duration;
use horch_core::clock;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::triage::{self, Measure};
use horch_core::taskledger::{status, Store};
use i308_support::{write, World};

const GB: i64 = 1 << 30;

fn ledger() -> (tempfile::TempDir, Store, Run) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let run = store
        .immediate(|tx| {
            runs::create(
                tx,
                Path::new("/repo"),
                &NewRun {
                    slug: "r1".into(),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    (dir, store, run)
}

/// A stamp `ago` before now, as the ledger writes it.
fn ago(ago: Duration) -> String {
    clock::stamp(clock::now() - ago)
}

/// 1 unlanded worktree of `run` at `path`, with the stored measurement.
fn parked(store: &Store, run: &Run, path: &Path, measured: Option<(i64, String)>) {
    let conn = store.conn();
    let id = path.file_name().unwrap().to_string_lossy().into_owned();
    conn.execute(
        "INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, status, created_at, bytes, bytes_measured_at)
         VALUES (?1, '/repo', ?2, ?1, 'main', 'unlanded', '2000-01-01T00:00:00Z', ?3, ?4)",
        rusqlite::params![
            id,
            path.to_string_lossy(),
            measured.as_ref().map(|m| m.0),
            measured.as_ref().map(|m| m.1.clone())
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO worktree_modes (worktree_id, run_id, name) VALUES (?1, ?2, ?1)",
        rusqlite::params![id, run.run_id],
    )
    .unwrap();
}

fn parked_line(store: &Store, run: &Run) -> String {
    let board = status::render(&status::report(store.conn(), run).unwrap());
    board
        .lines()
        .find(|l| l.starts_with("Parked worktrees:"))
        .unwrap_or_else(|| panic!("no parked line in:\n{board}"))
        .to_string()
}

#[test]
fn status_sums_the_stored_bytes_and_walks_no_folder() {
    let (dir, store, run) = ledger();
    // A folder that nobody can read: a walk would find 0 bytes here, and
    // the stored 5 GB must still count.
    let locked = dir.path().join("locked");
    write(&locked, "big.bin", &"x".repeat(1000));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    // A folder with a real tree, much smaller than its stored 1 GB.
    let tree = dir.path().join("tree");
    for i in 0..40 {
        write(&tree, &format!("d{}/f{i}.txt", i % 4), &"y".repeat(4096));
    }
    parked(
        &store,
        &run,
        &locked,
        Some((5 * GB, ago(Duration::hours(2)))),
    );
    parked(&store, &run, &tree, Some((GB, ago(Duration::minutes(30)))));

    let started = Instant::now();
    let line = parked_line(&store, &run);
    let took = started.elapsed();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(
        line,
        "Parked worktrees: 2 (6.0 GB, measured 2h ago). Decide about them with horch worktree triage."
    );
    assert!(took.as_secs() < 1, "status took {took:?}");
}

#[test]
fn status_says_when_no_worktree_was_measured() {
    let (dir, store, run) = ledger();
    let folder = dir.path().join("a");
    write(&folder, "f.txt", "hello");
    parked(&store, &run, &folder, None);
    assert_eq!(
        parked_line(&store, &run),
        "Parked worktrees: 1 (disk not measured; run horch worktree triage). Decide about them with horch worktree triage."
    );
}

#[test]
fn status_counts_the_rows_without_a_measurement() {
    let (dir, store, run) = ledger();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    write(&a, "f.txt", "hello");
    write(&b, "f.txt", "hello");
    parked(&store, &run, &a, Some((GB, ago(Duration::minutes(5)))));
    parked(&store, &run, &b, None);
    assert_eq!(
        parked_line(&store, &run),
        "Parked worktrees: 2 (1.0 GB, measured 5m ago; 1 not measured). Decide about them with horch worktree triage."
    );
}

#[test]
fn a_parked_row_whose_folder_is_gone_does_not_count() {
    let (dir, store, run) = ledger();
    let gone = dir.path().join("gone");
    parked(&store, &run, &gone, Some((GB, ago(Duration::minutes(5)))));
    let board = status::render(&status::report(store.conn(), &run).unwrap());
    assert!(!board.contains("Parked worktrees"), "{board}");
}

#[test]
fn triage_stores_the_bytes_that_status_then_shows() {
    let w = World::new();
    let base = w.rev("main");
    let feat = w.worktree("feat", "feat", &base);
    write(&feat, "notes/big.txt", &"x".repeat(5000));
    let run = {
        let conn = w.store.conn();
        conn.execute_batch(
            "INSERT INTO projects VALUES ('p', 'root', 'now');
             INSERT INTO runs (run_id, project_id, slug, status, started_at)
                VALUES ('r1', 'p', 'r1', 'active', '2026-10-01T00:00:00Z');",
        )
        .unwrap();
        runs::get(conn, "r1").unwrap()
    };
    let conn = w.store.conn();
    conn.execute(
        "INSERT INTO worktrees (worktree_id, repo_root, path, branch, base, status, created_at)
         VALUES ('wt-feat', ?1, ?2, 'feat', 'main', 'unlanded', '2026-10-02T00:00:00Z')",
        rusqlite::params![w.root.to_string_lossy(), feat.to_string_lossy()],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO worktree_modes (worktree_id, run_id, name) VALUES ('wt-feat', 'r1', 'feat')",
        [],
    )
    .unwrap();
    // Before triage: nothing stored.
    assert!(parked_line(&w.store, &run).contains("disk not measured"));

    let rows = triage::collect(conn, &w.git, &w.root, Measure::Full).unwrap();
    let measured = rows[0].disk.as_ref().unwrap().bytes;
    assert!(measured > 5000, "{measured}");
    let stored: (i64, String) = conn
        .query_row(
            "SELECT bytes, bytes_measured_at FROM worktrees WHERE worktree_id = 'wt-feat'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored.0 as u64, measured);
    assert!(clock::parse(&stored.1).is_some(), "{stored:?}");
    // The stored value is what status shows, even after the folder grows.
    write(&feat, "notes/more.txt", &"z".repeat(50_000));
    let line = parked_line(&w.store, &run);
    assert!(
        line.starts_with(&format!(
            "Parked worktrees: 1 ({}, measured ",
            triage::human_bytes(measured)
        )),
        "{line}"
    );

    // `Measure::Ids` measures nothing and stores nothing.
    conn.execute("UPDATE worktrees SET bytes_measured_at = 'marker'", [])
        .unwrap();
    triage::collect(conn, &w.git, &w.root, Measure::Ids).unwrap();
    let again: String = conn
        .query_row("SELECT bytes_measured_at FROM worktrees", [], |r| r.get(0))
        .unwrap();
    assert_eq!(again, "marker");
}
