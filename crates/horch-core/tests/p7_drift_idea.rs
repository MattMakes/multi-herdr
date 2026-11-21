//! A drift becomes an idea: a failed conformance run adds 1 idea "Update
//! the <tool> adapter for <version>" with CHANGES.md and the failed checks,
//! and you see it in `needs-you/` (docs/specs/fleetd/design.md, "Docs sync
//! and adapter conformance", step 4; FM-19; FR-8;
//! docs/tickets/fleetd-p7/27-drift-idea.md).
//!
//! Each test makes its own temporary ledger, git repository, fleet root and
//! needs-you folder. No test touches this repository or the live ledger,
//! and no test starts a process other than git.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::domain::budget::{Cursor, PassBudget};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::adapters::needs_you_folder;
use horch_core::fleetd::ideas::application::intake::{Added, Target};
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::integration::application::new_features::Project;
use horch_core::fleetd::worker_profiles::application::drift_idea::{add_drift_idea, KIND};
use horch_core::fleetd::worker_profiles::domain::conformance::{
    Check, CheckResult, ConformanceReport,
};
use horch_core::taskledger::Store;
use horch_marketplace::git::scrub_repo_env;
use rusqlite::{Connection, TransactionBehavior};

const CHANGES: &str = "# claude 2.1.0\n\n- removed: --mcp-config takes a list\n";

struct World {
    _dir: tempfile::TempDir,
    repo: PathBuf,
    fleet: PathBuf,
    conn: Connection,
    config: FleetConfig,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(dir.path()).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let fleet = root.join("fleet");
    let docs = fleet.join("docs").join("claude");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("CHANGES.md"), CHANGES).unwrap();
    let db = root.join("ledger.sqlite3");
    drop(Store::open(&db).unwrap());
    let conn = Connection::open(&db).unwrap();
    World {
        _dir: dir,
        repo,
        fleet,
        conn,
        config: FleetConfig::default(),
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    scrub_repo_env(&mut cmd);
    let out = cmd
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// A report of claude 2.1.0 with the adapter hash `hash`: `Shared write`
/// fails when `failed`, and each other check passes.
fn report(hash: &str, failed: bool) -> ConformanceReport {
    let shared = if failed {
        CheckResult::fail(Check::SharedWrite, "result.json was not written")
    } else {
        CheckResult::pass(Check::SharedWrite, "result.json written")
    };
    ConformanceReport {
        cli: "claude".into(),
        version: "2.1.0".into(),
        adapter_hash: hash.into(),
        checks: vec![
            CheckResult::pass(Check::LaunchHeadless, "first event in 2 s"),
            shared,
            CheckResult::pass(Check::StopGate, "stop gate held"),
        ],
    }
}

impl World {
    /// `add_drift_idea` for `report` in 1 committed write transaction.
    fn drift(&self, report: &ConformanceReport) -> Option<Added> {
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let target = Target::New(Project {
            root: &self.repo,
            primary_branch: "main",
        });
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
            .unwrap();
        let added = add_drift_idea(&tx, &ctx, report, &self.fleet, target).unwrap();
        tx.commit().unwrap();
        added
    }

    fn idea_count(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM ideas", [], |r| r.get(0))
            .unwrap()
    }
}

#[test]
fn failed_conformance_adds_1_idea_with_the_diff_and_the_checks() {
    let w = world();
    let added = w
        .drift(&report("h1", true))
        .expect("a failed run adds an idea");
    assert_eq!(w.idea_count(), 1);
    let text = &added.file.text;
    assert!(
        text.starts_with("Update the claude adapter for 2.1.0\n"),
        "{text}"
    );
    let changes_path = w.fleet.join("docs/claude/CHANGES.md");
    assert!(text.contains(&changes_path.display().to_string()), "{text}");
    assert!(text.contains("--mcp-config takes a list"), "{text}");
    assert!(
        text.contains("Shared write: result.json was not written"),
        "{text}"
    );
    assert!(!text.contains("Launch headless"), "{text}");
    let changes_at = text.find("--mcp-config").unwrap();
    let checks_at = text.find("Shared write").unwrap();
    assert!(changes_at < checks_at, "CHANGES.md comes first: {text}");
}

#[test]
fn idempotent_adapter_conformance_failed_adds_no_second_idea() {
    let w = world();
    assert!(w.drift(&report("h1", true)).is_some());
    assert!(w.drift(&report("h1", true)).is_none());
    assert_eq!(w.idea_count(), 1);
    // The same version with another adapter is another request.
    assert!(w.drift(&report("h2", true)).is_some());
    assert_eq!(w.idea_count(), 2);
}

#[test]
fn drift_idea_is_in_needs_you() {
    let w = world();
    let added = w.drift(&report("h1", true)).unwrap();
    let items = needs_you::open_items(&w.conn).unwrap();
    let ours: Vec<_> = items.iter().filter(|i| i.kind == KIND).collect();
    assert_eq!(ours.len(), 1, "{items:?}");
    assert_eq!(ours[0].entity_id, added.file.id);
    assert!(w.drift(&report("h1", true)).is_none());
    assert_eq!(needs_you::open_items(&w.conn).unwrap().len(), items.len());

    let folder = w.fleet.join("needs-you");
    let mut cursor = Cursor::new("t0");
    needs_you_folder::mirror(&folder, &items, &mut cursor, PassBudget::new(200), "t1").unwrap();
    let file = folder.join(needs_you_folder::file_name(ours[0]));
    let body = std::fs::read_to_string(&file).unwrap();
    assert!(
        body.contains("Update the claude adapter for 2.1.0"),
        "{body}"
    );
    assert!(body.contains(&added.file.id), "{body}");
}

#[test]
fn passed_conformance_adds_no_idea() {
    let w = world();
    assert!(w.drift(&report("h1", false)).is_none());
    assert_eq!(w.idea_count(), 0);
    assert!(needs_you::open_items(&w.conn).unwrap().is_empty());
}
