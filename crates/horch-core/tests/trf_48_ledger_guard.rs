//! TRF-48: a development build of horch never migrates the operator's
//! shared ledger, every migration of an existing file keeps a backup, and
//! the "newer schema" error names that backup.

use std::path::{Path, PathBuf};

use horch_core::taskledger::schema;
use horch_core::taskledger::store::{self, migrate_refusal, Store};
use rusqlite::Connection;

/// Undo what `schema::v7()` adds: the incident's downgrade, so the test has
/// a real version 6 file.
const DOWNGRADE_V7_TO_V6: &str = "
DROP INDEX session_records_by_slug;
DROP TABLE session_records;
DROP INDEX outbox_undelivered;
DROP TABLE outbox;
DROP INDEX run_gates_by_tip;
DROP TABLE run_gates;
ALTER TABLE tasks DROP COLUMN work_kind;
ALTER TABLE tasks DROP COLUMN review_mode;
ALTER TABLE tasks DROP COLUMN ticket_snapshot;
ALTER TABLE tasks DROP COLUMN ticket_sha;
ALTER TABLE tasks DROP COLUMN check_none;
ALTER TABLE runs DROP COLUMN default_check;
ALTER TABLE runs DROP COLUMN pr_declined_reason;
ALTER TABLE runs DROP COLUMN orchestrator_stopped_at;
ALTER TABLE runs DROP COLUMN orchestrator_respawns;
ALTER TABLE runs DROP COLUMN last_respawn_at;
ALTER TABLE workers DROP COLUMN handoff_path;
ALTER TABLE workers DROP COLUMN target_task_id;
PRAGMA user_version = 6;";

/// A version 6 ledger at `path` with 1 project row.
fn version_6_file(path: &Path) {
    assert_eq!(
        schema::VERSION,
        7,
        "update DOWNGRADE_V7_TO_V6 for the new version"
    );
    drop(Store::open(path).unwrap());
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(DOWNGRADE_V7_TO_V6).unwrap();
    conn.execute("INSERT INTO projects VALUES ('p', '/r', 'now')", [])
        .unwrap();
}

fn user_version(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

/// The backup files in `dir`, sorted by name.
fn backups(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("ledger.v") && n.contains("-backup-"))
        .collect();
    names.sort();
    names
}

#[test]
fn trf_48_dev_build_refuses_to_migrate_the_shared_ledger() {
    let home = Path::new("/Users/op");
    let shared = store::default_path(home);
    for exe in [
        "/Users/op/src/horch/target/debug/horch",
        "/Users/op/src/horch/target/release/horch",
        "/Users/op/src/horch/target/debug/deps/trf_48_ledger_guard-1a2b3c",
        "/Users/op/src/horch/target/release/deps/horch-9f",
        "/Users/op/src/horch/target/aarch64-apple-darwin/debug/horch",
        "/w/.worktrees/T35/target/ci/deps/horch-9f",
    ] {
        let exe = Path::new(exe);
        assert!(store::is_dev_build(exe), "{}", exe.display());
        let text = migrate_refusal(exe, &shared, Some(home), None, 6)
            .unwrap_or_else(|| panic!("{} must be refused", exe.display()));
        assert_eq!(text.lines().count(), 2, "{text}");
        for part in [
            exe.to_str().unwrap(),
            shared.to_str().unwrap(),
            "HORCH_LEDGER_DB=<scratch file>",
            "HORCH_ALLOW_LEDGER_MIGRATE=1",
        ] {
            assert!(text.contains(part), "missing {part:?} in {text}");
        }
    }
    for exe in [
        "/Users/op/.local/bin/horch",
        "/opt/homebrew/bin/horch",
        "/Users/op/target/horch",
        "/Users/op/debug/horch",
    ] {
        assert!(!store::is_dev_build(Path::new(exe)), "{exe}");
    }
    assert!(store::is_shared_ledger(&shared, home));
    assert!(!store::is_shared_ledger(
        Path::new("/tmp/x/ledger.sqlite3"),
        home
    ));
}

#[test]
fn trf_48_opt_in_and_scratch_ledgers_migrate() {
    let home = Path::new("/Users/op");
    let shared = store::default_path(home);
    let dev = Path::new("/Users/op/src/horch/target/debug/horch");
    let installed = Path::new("/Users/op/.local/bin/horch");
    // The opt-in.
    assert_eq!(
        migrate_refusal(dev, &shared, Some(home), Some("1"), 6),
        None
    );
    assert!(migrate_refusal(dev, &shared, Some(home), Some("0"), 6).is_some());
    assert!(migrate_refusal(dev, &shared, Some(home), Some(""), 6).is_some());
    // An installed horch.
    assert_eq!(
        migrate_refusal(installed, &shared, Some(home), None, 6),
        None
    );
    // A scratch ledger (HORCH_LEDGER_DB, a temp dir, a test HOME).
    for other in [
        "/tmp/scratch/ledger.sqlite3",
        "/Users/op/.horch/scratch.sqlite3",
        "/var/folders/x/T/.tmpAb/.horch/ledger.sqlite3",
    ] {
        assert_eq!(
            migrate_refusal(dev, Path::new(other), Some(home), None, 6),
            None,
            "{other}"
        );
    }
    // No passwd home: nothing is shared.
    assert_eq!(migrate_refusal(dev, &shared, None, None, 6), None);

    // A real open by this test binary (a dev build in target/*/deps/) of a
    // version 6 file outside the passwd home migrates it.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".horch").join("ledger.sqlite3");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    version_6_file(&path);
    assert!(store::is_dev_build(&std::env::current_exe().unwrap()));
    Store::open(&path).unwrap();
    assert_eq!(user_version(&path), schema::VERSION);
}

#[test]
fn trf_48_migration_writes_a_backup_and_keeps_3() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    version_6_file(&path);
    // 3 older backups: the migration adds 1 and removes the oldest.
    let older = [
        "ledger.v4-backup-20240101T000000Z.sqlite3",
        "ledger.v5-backup-20250101T000000Z.sqlite3",
        "ledger.v6-backup-20260101T000000Z.sqlite3",
    ];
    for name in older {
        std::fs::write(dir.path().join(name), b"old").unwrap();
    }
    // A file that is not a backup stays.
    std::fs::write(dir.path().join("ledger.v6-notes.txt"), b"x").unwrap();

    Store::open(&path).unwrap();

    let names = backups(dir.path());
    assert_eq!(names.len(), 3, "{names:?}");
    assert_eq!(&names[..2], &older[1..]);
    let new = &names[2];
    assert!(new.starts_with("ledger.v6-backup-2"), "{new}");
    assert!(new.ends_with("Z.sqlite3"), "{new}");
    // yyyymmddTHHMMSSZ
    let stamp = &new["ledger.v6-backup-".len()..new.len() - ".sqlite3".len()];
    assert_eq!(stamp.len(), 16, "{stamp}");
    assert_eq!(&stamp[8..9], "T", "{stamp}");
    // The backup is the version 6 file with its rows.
    let copy = dir.path().join(new);
    assert_eq!(user_version(&copy), 6);
    let rows: i64 = Connection::open(&copy)
        .unwrap()
        .query_row("SELECT count(*) FROM projects", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 1);
    assert!(dir.path().join("ledger.v6-notes.txt").exists());

    // A fresh file and a no-op open write no backup.
    let fresh_dir = tempfile::tempdir().unwrap();
    let fresh = fresh_dir.path().join("ledger.sqlite3");
    Store::open(&fresh).unwrap();
    Store::open(&fresh).unwrap();
    assert_eq!(backups(fresh_dir.path()), Vec::<String>::new());
}

#[test]
fn trf_48_newer_schema_error_names_the_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.sqlite3");
    drop(Store::open(&path).unwrap());
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", schema::VERSION + 1)
        .unwrap();
    drop(conn);

    // No backup: self-update is 1 option of 2.
    let text = format!("{:#}", Store::open(&path).err().unwrap());
    assert!(text.contains("newer or development horch"), "{text}");
    assert!(text.contains("horch self-update"), "{text}");
    assert!(!text.contains("backup"), "{text}");
    assert_eq!(text.lines().count(), 2, "{text}");

    // The newest backup by name.
    for name in [
        "ledger.v6-backup-20261106T000200Z.sqlite3",
        "ledger.v5-backup-20251106T000200Z.sqlite3",
    ] {
        std::fs::write(dir.path().join(name), b"x").unwrap();
    }
    let text = format!("{:#}", Store::open(&path).err().unwrap());
    let newest: PathBuf = dir.path().join("ledger.v6-backup-20261106T000200Z.sqlite3");
    assert!(text.contains(newest.to_str().unwrap()), "{text}");
    assert!(!text.contains("20251106"), "{text}");
    assert!(text.contains("horch self-update"), "{text}");
    assert!(text.contains("newer or development horch"), "{text}");
    assert_eq!(text.lines().count(), 2, "{text}");
}
