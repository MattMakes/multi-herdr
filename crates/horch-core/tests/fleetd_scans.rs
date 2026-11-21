//! The fact scans of a pass with their cursors and time budget, and the
//! work budget of the rows (docs/specs/fleetd/design.md, "Supervision,
//! liveness and reconciliation"; docs/tickets/fleetd-p1/18-pass-budgets-and-scans.md).

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

/// A ledger at `path` at the Phase 1 version, which adds `pass_cursors`.
fn ledger(path: &Path) -> Connection {
    let mut conn = Connection::open(path).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_1_VERSION).unwrap();
    conn
}

/// `n` outbox folders under `root`, keyed `T-<i>/A1`, each with 1 file.
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

/// A monotonic clock that moves 1 ms at each read, from `base`.
fn ticking(base: Instant) -> impl FnMut() -> Instant {
    let t = Cell::new(base);
    move || {
        let now = t.get() + Duration::from_millis(1);
        t.set(now);
        now
    }
}

/// The deadline of a scan of `budget_ms` that starts at `base`, in a pass
/// with the starting value of `pass_time_budget_s`.
fn scan_deadline(base: Instant, budget_ms: u64) -> Deadline {
    Deadline::pass(base, 20).scan(base, budget_ms)
}

/// 1 pass of `run` with a cursor read from and written to the ledger at
/// `db`, through a new connection, as a new fleetd process would.
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

/// Pass 1 and pass 2 of a scan read disjoint items in order: pass 2 starts
/// at the item after the last item of pass 1.
fn assert_continues(all: &[String], first: &[String], second: &[String]) {
    assert!(!first.is_empty() && !second.is_empty());
    assert_eq!(first, &all[..first.len()]);
    assert_eq!(second, &all[first.len()..first.len() + second.len()]);
}

/// 2 passes of a scan of `kind`, each with a budget of 50 ms on a clock of
/// 1 ms for each read and a cursor through a new connection to `db`. `run`
/// scans and gives the keys read. Pass 2 continues where pass 1 stopped.
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

    // Outboxes, and the same folders as worktrees: 10,000 folders.
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

    // Processes: this test process, named by 200 rows.
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

    // Requests: 200 request files.
    let requests = scans::requests_dir(&root);
    std::fs::create_dir_all(&requests).unwrap();
    let all: Vec<String> = (0..200).map(|i| format!("r{i:03}.json")).collect();
    for name in &all {
        std::fs::write(requests.join(name), "{}").unwrap();
    }
    assert_two_passes_continue(&db, ScanKind::Requests, &all, |cursor, clock| {
        scans::scan_requests(&root, cursor, clock).unwrap().facts
    });

    // Each scan has its own row.
    let conn = ledger(&db);
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM pass_cursors", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 4);
}

/// A worktree folder that cannot be read is not reported missing (review F1).
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

    // A clock of 1 ms for each read and a budget of 10 ms: the 10th read
    // of the clock reaches the deadline, so 9 items are read.
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

    // On the real clock, each pass of 10,000 folders ends within its
    // budget of 20 ms, and the passes read every folder in turns.
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
        // The budget, and the read of 1 folder that started before it ended.
        assert!(
            took < budget + Duration::from_millis(200),
            "pass {passes} took {took:?}"
        );
        read.extend(scan.facts.into_iter().map(|f| f.key));
    }
    // The passes read the 10,000 folders in turn order; the last pass can
    // wrap to the first folders again.
    let all: Vec<String> = items.iter().map(|i| i.key.clone()).collect();
    assert_eq!(read[..all.len()], all[..]);
}

#[test]
fn the_cursor_wraps_and_each_item_gets_its_turn() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("ledger.sqlite3");
    let items = outboxes(&dir.path().join("fleet"), 25);
    let all: Vec<String> = items.iter().map(|i| i.key.clone()).collect();

    // 10 items for each pass: 0-9, 10-19, then 20-24 and the wrap to 0-4.
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
    // The first pass made the cursor at t1; the third pass wrapped at t3.
    assert_eq!(cursor.lap_started_at, "t3");
    assert_eq!(cursor.prev_lap_started_at.as_deref(), Some("t1"));
}

#[test]
fn a_watched_change_goes_first() {
    let dir = tempfile::tempdir().unwrap();
    let items = outboxes(dir.path(), 100);
    let mut cursor = Cursor::new("t0");
    cursor.step(&items[10].key, "t0");

    // File watching reports a new file in the outbox of item 50.
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
    // The watched folder does not move the cursor.
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
        } // else: a crash before the commit drops the transaction.
    }
    assert_eq!(read[0], read[1]);
}
