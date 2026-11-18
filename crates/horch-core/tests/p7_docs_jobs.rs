//! fleetd queues the `docs_sync` and `conformance` jobs (fleetd-p7 ticket
//! 28): each day, on a new horch binary, on a new CLI binary, and a
//! `conformance` run after each `docs_sync` run.
//!
//! Each test gives the fleet clock, the horch binary id and the CLI binary
//! ids. No test reads the real clock or starts a CLI. A run starts and ends
//! through `transition()`, as the pass and the job process move it.

use std::path::Path;

use chrono::{DateTime, Utc};
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{JobCtx, Queued};
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{self as job_kinds, Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worker_profiles::application::docs_jobs::{
    self, queue_docs_jobs, CliBinary, QueuedDocsJob,
};
use horch_core::fleetd::worker_profiles::domain::docs_jobs::{Why, DAILY_S};
use horch_core::harness::cooldown::{self, Packument, Verdict};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::schema;
use rusqlite::{Connection, TransactionBehavior};

const HORCH: &str = "h1";

struct World {
    _dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    World {
        _dir: dir,
        conn,
        config: FleetConfig::default(),
    }
}

fn cli(name: &str, binary_id: &str) -> CliBinary {
    CliBinary {
        cli: name.into(),
        binary_id: binary_id.into(),
    }
}

/// The kind, subject and rule of each run that a pass queued as new.
fn new_runs(out: &[QueuedDocsJob]) -> Vec<(Kind, String, Why)> {
    out.iter()
        .filter(|q| matches!(q.queued, Queued::New(_)))
        .map(|q| (q.kind, q.subject.clone(), q.why))
        .collect()
}

fn new_id(q: &QueuedDocsJob) -> i64 {
    match q.queued {
        Queued::New(id) => id,
        ref other => panic!("not a new run: {other:?}"),
    }
}

impl World {
    /// 1 pass of the docs jobs at `clock`, in its own transaction.
    fn pass(&mut self, clock: i64, horch: &str, clis: &[CliBinary]) -> Vec<QueuedDocsJob> {
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let out = queue_docs_jobs(&ctx, &tx, FleetSeconds(clock), horch, clis).unwrap();
        tx.commit().unwrap();
        out
    }

    fn row(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    fn moved(&mut self, id: i64, to: &str) {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let cause = Cause {
            actor: Actor::Horch,
            text: "test",
        };
        transition(
            &tx,
            None,
            Entity::Job,
            &id.to_string(),
            to,
            cause,
            &NeedsYouList,
        )
        .unwrap();
        tx.commit().unwrap();
    }

    /// Start the run `id` at `clock` on the fleet clock, as part 3 of a
    /// pass records it.
    fn start(&mut self, id: i64, clock: i64) {
        let limit = self.row(id).time_limit_s;
        let deadline = job_kinds::deadline(FleetSeconds(clock), limit);
        jobs_store::set_started(&self.conn, id, 4242, None, deadline).unwrap();
        self.moved(id, "running");
    }

    /// Start the run `id` at `clock` and end it `passed`.
    fn run_to_end(&mut self, id: i64, clock: i64) {
        self.start(id, clock);
        self.moved(id, "passed");
    }

    /// Run each new run of `out` to its end at `clock`.
    fn end_each(&mut self, out: &[QueuedDocsJob], clock: i64) {
        for q in out {
            if let Queued::New(id) = q.queued {
                self.run_to_end(id, clock);
            }
        }
    }
}

#[test]
fn daily_run_starts_a_new_run() {
    let mut w = world();
    let clis = [cli("claude", "b1")];
    let first = w.pass(1_000, HORCH, &clis);
    let docs = new_id(&first[0]);
    w.end_each(&first, 1_000);
    // The first docs_sync run ended, and the conformance run of the new
    // binary is later, so rule 4 asks for nothing.
    assert!(new_runs(&w.pass(1_000, HORCH, &clis)).is_empty());

    assert!(
        new_runs(&w.pass(1_000 + DAILY_S - 1, HORCH, &clis)).is_empty(),
        "no daily run before 86400 s"
    );
    let daily = w.pass(1_000 + DAILY_S, HORCH, &clis);
    assert_eq!(
        new_runs(&daily),
        [(Kind::DocsSync, "claude b1".to_string(), Why::Daily)]
    );
    let next = w.row(new_id(&daily[0]));
    let last = w.row(docs);
    assert_eq!(next.state, State::Queued);
    assert_ne!(next.id, last.id, "a daily run is a new row");
    assert_eq!(
        (next.subject.as_str(), next.input_key.as_str()),
        (last.subject.as_str(), last.input_key.as_str())
    );
    assert_eq!(next.run, last.run + 1, "it has the next run number");
}

#[test]
fn docs_sync_and_conformance_with_no_repository_are_valid_rows() {
    let mut w = world();
    let out = w.pass(0, HORCH, &[cli("codex", "c1")]);
    assert_eq!(
        new_runs(&out),
        [
            (Kind::DocsSync, "codex c1".to_string(), Why::NewCli),
            (Kind::Conformance, "codex c1".to_string(), Why::NewCli),
        ]
    );
    for q in &out {
        let row = w.row(new_id(q));
        assert_eq!(row.repo, None);
        assert_eq!(row.state, State::Queued);
        assert_eq!(row.input_key, "codex c1 horch h1");
        assert_eq!(row.run, 1);
    }
}

#[test]
fn new_horch_binary_queues_docs_sync() {
    let mut w = world();
    let clis = [cli("claude", "b1"), cli("codex", "c1")];
    let first = w.pass(0, HORCH, &clis);
    w.end_each(&first, 0);
    assert!(new_runs(&w.pass(10, HORCH, &clis)).is_empty());

    let out = w.pass(20, "h2", &clis);
    assert_eq!(
        new_runs(&out),
        [
            (Kind::DocsSync, "claude b1".to_string(), Why::NewHorch),
            (Kind::DocsSync, "codex c1".to_string(), Why::NewHorch),
        ]
    );
    assert_eq!(w.row(new_id(&out[0])).input_key, "claude b1 horch h2");
}

#[test]
fn new_cli_binary_queues_docs_sync_and_conformance() {
    let mut w = world();
    let first = w.pass(0, HORCH, &[cli("claude", "b1")]);
    w.end_each(&first, 0);

    let out = w.pass(10, HORCH, &[cli("claude", "b2")]);
    assert_eq!(
        new_runs(&out),
        [
            (Kind::DocsSync, "claude b2".to_string(), Why::NewCli),
            (Kind::Conformance, "claude b2".to_string(), Why::NewCli),
        ]
    );
    w.end_each(&out, 10);

    // A binary that goes back to an earlier id is new again.
    let back = w.pass(20, HORCH, &[cli("claude", "b1")]);
    assert_eq!(
        new_runs(&back),
        [
            (Kind::DocsSync, "claude b1".to_string(), Why::NewCli),
            (Kind::Conformance, "claude b1".to_string(), Why::NewCli),
        ]
    );
    assert_eq!(w.row(new_id(&back[0])).run, 2);
}

#[test]
fn conformance_follows_each_docs_sync_run() {
    let mut w = world();
    let clis = [cli("claude", "b1")];
    let first = w.pass(0, HORCH, &clis);
    w.end_each(&first, 0);

    let daily = w.pass(DAILY_S, HORCH, &clis);
    let docs = new_id(&daily[0]);
    w.start(docs, DAILY_S);
    assert!(
        new_runs(&w.pass(DAILY_S + 1, HORCH, &clis)).is_empty(),
        "no conformance run while the docs_sync run runs"
    );
    w.moved(docs, "failed");
    let after = w.pass(DAILY_S + 2, HORCH, &clis);
    assert_eq!(
        new_runs(&after),
        [(
            Kind::Conformance,
            "claude b1".to_string(),
            Why::AfterDocsSync
        )]
    );
    w.end_each(&after, DAILY_S + 2);
    assert!(
        new_runs(&w.pass(DAILY_S + 3, HORCH, &clis)).is_empty(),
        "1 conformance run for each docs_sync run"
    );
}

#[test]
fn queued_run_is_not_queued_twice() {
    let mut w = world();
    let clis = [cli("claude", "b1")];
    let first = w.pass(0, HORCH, &clis);
    assert_eq!(new_runs(&first).len(), 2);

    // The same pass again, a new horch binary, a later day: each run of the
    // CLI is still queued, so no second row.
    for (clock, horch) in [(0, HORCH), (5, "h2"), (DAILY_S * 2, HORCH)] {
        assert!(new_runs(&w.pass(clock, horch, &clis)).is_empty());
    }
    // A new CLI binary while the runs of the old one are queued.
    assert!(new_runs(&w.pass(10, HORCH, &[cli("claude", "b2")])).is_empty());
    let count: i64 = w
        .conn
        .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
}

/// Preservation row 06 S7 (Slice 7): docs sync starts no CLI update. A
/// release younger than the cooldown is still held, and a new CLI binary
/// queues only `docs_sync` and `conformance`, which read the installed
/// binary and never change it.
#[test]
fn pres_06_s7_cli_release_younger_than_cooldown_is_held() {
    let doc: Packument = serde_json::from_str(
        r#"{"dist-tags": {"latest": "2.0.0"},
            "time": {"2.0.0": "2026-11-17T00:00:00.000Z"}}"#,
    )
    .unwrap();
    let now: DateTime<Utc> = "2026-11-18T00:00:00Z".parse().unwrap();
    assert!(matches!(
        cooldown::judge(&doc, now, cooldown::DEFAULT_DAYS),
        Verdict::TooNew { .. }
    ));

    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("fakecli");
    std::fs::write(&bin, "#!/bin/sh\necho 1.0.0\n").unwrap();
    let before = std::fs::read(&bin).unwrap();
    let scanned = docs_jobs::scan_cli("claude", &bin).expect("a readable binary has an id");
    assert_eq!(docs_jobs::scan_cli("claude", &bin), Some(scanned.clone()));
    assert_eq!(
        docs_jobs::scan_cli("claude", Path::new("/no/such/cli")),
        None
    );

    let mut w = world();
    let out = w.pass(0, HORCH, &[scanned]);
    let kinds: Vec<Kind> = out.iter().map(|q| q.kind).collect();
    assert_eq!(kinds, [Kind::DocsSync, Kind::Conformance]);
    assert_eq!(
        std::fs::read(&bin).unwrap(),
        before,
        "the binary is unchanged"
    );
}
