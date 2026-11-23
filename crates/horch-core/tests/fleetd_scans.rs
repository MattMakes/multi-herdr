use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::cursors_store;
use horch_core::fleetd::fleet_runtime::adapters::scans::{
    self, FolderItem, Listing, Liveness, Presence, ProcessItem, Scan, ScanClock,
};
use horch_core::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline, PassBudget, ScanKind};
use horch_core::taskledger::schema;
use rusqlite::Connection;

fn ledger(path: &Path) -> Connection {
    let mut conn = Connection::open(path).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_1_VERSION).unwrap();
    conn
}

fn outboxes(root: &Path, n: usize) -> Vec<FolderItem> {
    (0..n)
        .map(|i| {
            let task = format!("T-{i:05}");
            let path = scans::outbox_dir(root, &task, "A1");
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("progress.jsonl"), "").unwrap();
            FolderItem {
                key: format!("{task}/A1"),
                path,
            }
        })
        .collect()
}

fn ticking(base: Instant) -> impl FnMut() -> Instant {
    let t = Cell::new(base);
    move || {
        let now = t.get() + Duration::from_millis(1);
        t.set(now);
        now
    }
}

fn scan_deadline(base: Instant, budget_ms: u64) -> Deadline {
    Deadline::pass(base, 20).scan(base, budget_ms)
}

fn pass<F>(
    db: &Path,
    kind: ScanKind,
    stamp: &str,
    run: impl FnOnce(&mut Cursor) -> Scan<F>,
) -> Scan<F> {
    let mut conn = ledger(db);
    let mut cursor = cursors_store::load(&conn, kind.as_str(), stamp).unwrap();
    let scan = run(&mut cursor);
    let tx = conn.transaction().unwrap();
    cursors_store::save(&tx, kind.as_str(), &cursor).unwrap();
    tx.commit().unwrap();
    scan
}

fn keys<F>(facts: &[F], key: impl Fn(&F) -> &str) -> Vec<String> {
    facts.iter().map(|f| key(f).to_string()).collect()
}

fn assert_continues(all: &[String], first: &[String], second: &[String]) {
    assert!(!first.is_empty() && !second.is_empty());
    assert_eq!(first, &all[..first.len()]);
    assert_eq!(second, &all[first.len()..first.len() + second.len()]);
}

fn assert_two_passes_continue(
    db: &Path,
    kind: ScanKind,
    all: &[String],
    mut run: impl FnMut(&mut Cursor, &mut ScanClock<'_>) -> Vec<String>,
) {
    let mut seen = Vec::new();
    for stamp in ["t1", "t2"] {
        let base = Instant::now();
        let mut now = ticking(base);
        let read = pass(db, kind, stamp, |cursor| {
            let mut clock = ScanClock {
                deadline: scan_deadline(base, 50),
                now: &mut now,
                stamp,
            };
            let facts = run(cursor, &mut clock);
            Scan {
                facts,
                stopped_at_budget: true,
            }
        });
        seen.push(read.facts);
    }
    assert_continues(all, &seen[0], &seen[1]);
}

#[test]
fn each_scan_continues_at_its_cursor_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("fleet");
    let db = dir.path().join("ledger.sqlite3");

    let items = outboxes(&root, 10_000);
    let all: Vec<String> = items.iter().map(|i| i.key.clone()).collect();
    let listed = Listing::Names(vec!["progress.jsonl".into()]);
    assert_two_passes_continue(&db, ScanKind::Outboxes, &all, |cursor, clock| {
        let scan = scans::scan_outboxes(&items, &[], cursor, clock);
        assert!(scan.stopped_at_budget);
        assert!(scan.facts.iter().all(|f| f.listing == listed));
        keys(&scan.facts, |f| &f.key)
    });
    assert_two_passes_continue(&db, ScanKind::WorktreeFolders, &all, |cursor, clock| {
        let scan = scans::scan_worktree_folders(&items, &[], cursor, clock);
        assert!(scan.facts.iter().all(|f| f.presence == Presence::Present));
        keys(&scan.facts, |f| &f.key)
    });

    let me = std::process::id();
    let start = horch_platform::process::start_time(me);
    let procs: Vec<ProcessItem> = (0..200)
        .map(|i| ProcessItem {
            key: format!("p{i:03}"),
            pid: me,
            pid_start: start,
        })
        .collect();
    let all: Vec<String> = procs.iter().map(|p| p.key.clone()).collect();
    assert_two_passes_continue(&db, ScanKind::Processes, &all, |cursor, clock| {
        let scan = scans::scan_processes(&procs, cursor, clock);
        assert!(scan.facts.iter().all(|f| f.liveness == Liveness::Alive));
        keys(&scan.facts, |f| &f.key)
    });

    let requests = scans::requests_dir(&root);
    std::fs::create_dir_all(&requests).unwrap();
    let all: Vec<String> = (0..200).map(|i| format!("r{i:03}.json")).collect();
    for name in &all {
        std::fs::write(requests.join(name), "{}").unwrap();
    }
    assert_two_passes_continue(&db, ScanKind::Requests, &all, |cursor, clock| {
        scans::scan_requests(&root, cursor, clock).unwrap().facts
    });

    let conn = ledger(&db);
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM pass_cursors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 4);
}

#[cfg(unix)]
#[test]
fn an_unreadable_worktree_folder_is_not_missing() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let parent = dir.path().join("locked");
    let path = parent.join("wt");
    std::fs::create_dir_all(&path).unwrap();
    let items = [
        FolderItem {
            key: "a".into(),
            path: path.clone(),
        },
        FolderItem {
            key: "b".into(),
            path: dir.path().join("gone"),
        },
    ];
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o000)).unwrap();
    let base = Instant::now();
    let mut now = ticking(base);
    let mut cursor = Cursor::new("t1");
    let mut clock = ScanClock {
        deadline: scan_deadline(base, 100),
        now: &mut now,
        stamp: "t1",
    };
    let scan = scans::scan_worktree_folders(&items, &[], &mut cursor, &mut clock);
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(scan.facts[0].presence, Presence::Unreadable(_)),
        "{scan:?}"
    );
    assert_eq!(scan.facts[1].presence, Presence::Missing);
}

#[test]
fn a_scan_stops_at_its_time_budget() {
    let dir = tempfile::tempdir().unwrap();
    let items = outboxes(dir.path(), 10_000);

    let base = Instant::now();
    let mut now = ticking(base);
    let mut cursor = Cursor::new("t1");
    let mut clock = ScanClock {
        deadline: scan_deadline(base, 10),
        now: &mut now,
        stamp: "t1",
    };
    let scan = scans::scan_outboxes(&items, &[], &mut cursor, &mut clock);
    assert!(scan.stopped_at_budget);
    assert_eq!(scan.facts.len(), 9);
    assert_eq!(cursor.after_key.as_deref(), Some("T-00008/A1"));

    let budget = Duration::from_millis(20);
    let mut cursor = Cursor::new("t1");
    let mut read: Vec<String> = Vec::new();
    let mut passes = 0;
    while read.len() < items.len() {
        passes += 1;
        assert!(passes <= 10_000, "the scans make no progress");
        let start = Instant::now();
        let mut now = Instant::now;
        let mut clock = ScanClock {
            deadline: Deadline::pass(start, 20).scan(start, budget.as_millis() as u64),
            now: &mut now,
            stamp: "t1",
        };
        let scan = scans::scan_outboxes(&items, &[], &mut cursor, &mut clock);
        let took = start.elapsed();
        assert!(
            took < budget + Duration::from_millis(200),
            "pass {passes} took {took:?}"
        );
        read.extend(scan.facts.into_iter().map(|f| f.key));
    }
    let all: Vec<String> = items.iter().map(|i| i.key.clone()).collect();
    assert_eq!(read[..all.len()], all[..]);
}

#[test]
fn the_cursor_wraps_and_each_item_gets_its_turn() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("ledger.sqlite3");
    let items = outboxes(&dir.path().join("fleet"), 25);
    let all: Vec<String> = items.iter().map(|i| i.key.clone()).collect();

    let mut seen = Vec::new();
    for stamp in ["t1", "t2", "t3"] {
        let base = Instant::now();
        let mut now = ticking(base);
        let scan = pass(&db, ScanKind::Outboxes, stamp, |cursor| {
            let mut clock = ScanClock {
                deadline: scan_deadline(base, 11),
                now: &mut now,
                stamp,
            };
            scans::scan_outboxes(&items, &[], cursor, &mut clock)
        });
        assert_eq!(scan.facts.len(), 10, "pass {stamp}");
        seen.extend(keys(&scan.facts, |f| &f.key));
    }
    let want: Vec<String> = all.iter().chain(&all[..5]).cloned().collect();
    assert_eq!(seen, want);

    let conn = ledger(&db);
    let cursor = cursors_store::load(&conn, ScanKind::Outboxes.as_str(), "unused").unwrap();
    assert_eq!(cursor.after_key.as_deref(), Some(all[4].as_str()));
    assert_eq!(cursor.lap_started_at, "t3");
    assert_eq!(cursor.prev_lap_started_at.as_deref(), Some("t1"));
}

#[test]
fn a_watched_change_goes_first() {
    let dir = tempfile::tempdir().unwrap();
    let items = outboxes(dir.path(), 100);
    let mut cursor = Cursor::new("t0");
    cursor.step(&items[10].key, "t0");

    let changed: PathBuf = items[50].path.join("result.json");
    let base = Instant::now();
    let mut now = ticking(base);
    let mut clock = ScanClock {
        deadline: scan_deadline(base, 6),
        now: &mut now,
        stamp: "t1",
    };
    let scan = scans::scan_outboxes(&items, &[changed], &mut cursor, &mut clock);

    let read = keys(&scan.facts, |f| &f.key);
    let want: Vec<String> = [50, 11, 12, 13, 14]
        .iter()
        .map(|&i| items[i].key.clone())
        .collect();
    assert_eq!(read, want);
    assert_eq!(cursor.after_key.as_deref(), Some(items[14].key.as_str()));
}

#[test]
fn rows_past_the_pass_budget_wait_for_the_next_pass() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("ledger.sqlite3");
    let rows: Vec<String> = (0..450).map(|i| format!("T-{i:04}")).collect();
    let budget = PassBudget::new(200);

    let mut taken = Vec::new();
    for stamp in ["t1", "t2", "t3"] {
        let mut conn = ledger(&db);
        let mut cursor = cursors_store::load(&conn, "dispatch", stamp).unwrap();
        let chosen: Vec<String> = cursor
            .take(&rows, budget, stamp)
            .into_iter()
            .map(str::to_string)
            .collect();
        let tx = conn.transaction().unwrap();
        cursors_store::save(&tx, "dispatch", &cursor).unwrap();
        tx.commit().unwrap();
        taken.push(chosen);
    }
    assert_eq!(taken[0], rows[..200]);
    assert_eq!(taken[1], rows[200..400]);
    let wrapped: Vec<String> = rows[400..].iter().chain(&rows[..150]).cloned().collect();
    assert_eq!(taken[2], wrapped);
}

#[test]
fn a_cursor_not_committed_scans_the_same_items_again() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("ledger.sqlite3");
    let items = outboxes(&dir.path().join("fleet"), 30);

    let mut read = Vec::new();
    for commit in [false, true] {
        let mut conn = ledger(&db);
        let mut cursor = cursors_store::load(&conn, ScanKind::Outboxes.as_str(), "t1").unwrap();
        let base = Instant::now();
        let mut now = ticking(base);
        let mut clock = ScanClock {
            deadline: scan_deadline(base, 6),
            now: &mut now,
            stamp: "t1",
        };
        let scan = scans::scan_outboxes(&items, &[], &mut cursor, &mut clock);
        read.push(keys(&scan.facts, |f| &f.key));
        let tx = conn.transaction().unwrap();
        cursors_store::save(&tx, ScanKind::Outboxes.as_str(), &cursor).unwrap();
        if commit {
            tx.commit().unwrap();
        }
    }
    assert_eq!(read[0], read[1]);
}
