//! The worktree reconciler: it adopts each tree with no row, marks the tree
//! of a dead owner `orphaned`, observes the worktree jobs, escalates a
//! vanished folder, checks I6, and queues the rescue of each orphan and the
//! removal of each landed tree (docs/specs/fleetd/design.md, "Worktrees:
//! leased, reconciled, never lost", "Ledger" I6, RR-10, FM-25, FM-26,
//! FM-27; docs/tickets/fleetd-p3/09-worktree-reconciler-report-only.md and
//! 18-reconciler-acts.md).
//!
//! The reconciler runs no git and reads no folder: each test gives the
//! facts of part 1 of a pass as values. Each test has its own temporary
//! ledger and project folder, with no git repository.

use std::path::{Path, PathBuf};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store;
use horch_core::fleetd::fleet_runtime::adapters::scans::{
    FolderFact, Liveness, Presence, ProcessFact,
};
use horch_core::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worktree_custody::adapters::git::{
    worktree_add_key, ListResult, TreeFacts,
};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeRow, WorktreeState,
};
use horch_core::fleetd::worktree_custody::application::ports::ListEntry;
use horch_core::fleetd::worktree_custody::application::reconcile::{
    check_i6, reconcile_worktrees, Facts, Listed, Report, WorkDir,
};
use horch_core::fleetd::worktree_custody::domain::worktree::Breach;
use horch_core::taskledger::model::{Actor, WorktreeMode, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::Store;
use rusqlite::{Connection, Transaction};

const RUN_BRANCH: &str = "fleet/r1";
const CLOCK: FleetSeconds = FleetSeconds(1_000);

struct World {
    _tmp: tempfile::TempDir,
    /// The project folder, as its canonical path.
    repo: PathBuf,
    store: Store,
    run: Run,
    config: FleetConfig,
}

fn cause() -> Cause<'static> {
    Cause {
        actor: Actor::Horch,
        text: "test",
    }
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = horch_platform::path::canonicalize(tmp.path()).unwrap();
        let repo = top.join("repo");
        std::fs::create_dir_all(repo.join(".worktrees")).unwrap();
        let mut store = Store::open(&top.join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    &repo,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some(RUN_BRANCH.into()),
                        ..Default::default()
                    },
                )?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        World {
            _tmp: tmp,
            repo,
            store,
            run,
            config: FleetConfig::default(),
        }
    }

    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    fn repo_s(&self) -> String {
        self.repo.display().to_string()
    }

    /// The folder `<repo>/.worktrees/<name>`, made on disk.
    fn folder(&self, name: &str) -> String {
        let path = self.repo.join(".worktrees").join(name);
        std::fs::create_dir_all(&path).unwrap();
        path.display().to_string()
    }

    /// A path under `<repo>/.worktrees/` with no folder.
    fn no_folder(&self, name: &str) -> String {
        self.repo
            .join(".worktrees")
            .join(name)
            .display()
            .to_string()
    }

    /// A new task `T<n>`, claimed by a new worker with a live attempt:
    /// (task id, worker id, attempt id).
    fn claimed(&mut self, n: u32) -> (String, String, String) {
        let run = self.run.clone();
        self.store
            .immediate(|tx| {
                // A shared task: the bind makes no tree, so each test
                // writes the rows it needs.
                let mut t = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), "opus");
                t.worktree_mode = WorktreeMode::Shared;
                let task = tasks::add(tx, &run, &t, Actor::Orchestrator)?;
                let new = NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "opus".into(),
                    role: Some(format!("opus-{n}")),
                    ..Default::default()
                };
                let agent = horch_core::fleetd::task_execution::domain::task::Recorded {
                    pid: 4243,
                    pid_start: Some(2),
                };
                let bound = horch_core::taskledger::testing::bind_live(
                    tx,
                    &task.task_id,
                    &new,
                    Some(agent),
                )?;
                tx.execute(
                    "UPDATE workers SET supervisor_pid = 4242, supervisor_started = 1
                     WHERE worker_id = ?1",
                    [&bound.worker_id],
                )?;
                Ok((task.task_id, bound.worker_id, bound.attempt_id))
            })
            .unwrap()
    }

    /// A row `id` at `path` in `state`, of the task, the worker and the
    /// owner attempt given.
    fn row_in(
        &mut self,
        id: &str,
        path: &str,
        state: &str,
        owner: (Option<&str>, Option<&str>, Option<&str>),
    ) {
        let (task, worker, attempt) = owner;
        let new = NewWorktree {
            worktree_id: id.to_string(),
            task_id: task.map(str::to_string),
            worker_id: worker.map(str::to_string),
            repo_root: self.repo_s(),
            path: path.to_string(),
            branch: format!("{RUN_BRANCH}-{id}"),
            base: "main".to_string(),
            base_commit: Some("b0".to_string()),
            status: WorktreeStatus::Open,
            mode: None,
        };
        self.store
            .immediate(|tx| {
                trees::insert_reserved(tx, None, &new, cause())?;
                tx.execute(
                    "UPDATE worktrees SET state = ?2, owner_attempt = ?3 WHERE worktree_id = ?1",
                    rusqlite::params![id, state, attempt],
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn row(&self, id: &str) -> WorktreeRow {
        trees::get(self.conn(), id).unwrap().unwrap()
    }

    fn sql(&mut self, sql: &str, args: &[&str]) {
        self.store
            .immediate(|tx| {
                tx.execute(sql, rusqlite::params_from_iter(args))?;
                Ok(())
            })
            .unwrap();
    }

    fn reconcile(&mut self, facts: &Facts) -> Report {
        let config = self.config.clone();
        self.store
            .immediate(|tx| reconcile_worktrees(&ctx(&config), tx, facts, CLOCK))
            .unwrap()
    }

    fn i6(&self, facts: &Facts) -> Vec<Breach> {
        check_i6(self.conn(), facts).unwrap()
    }

    /// The `worktree_add` run of the reserved row `id`, ended with the
    /// result `passed` and its text.
    fn ended_add(&mut self, id: &str, passed: bool) -> i64 {
        let config = self.config.clone();
        let row = self.row(id);
        let repo = self.repo_s();
        self.store
            .immediate(|tx| {
                let key = worktree_add_key(&row);
                let job = JobKey {
                    kind: Kind::WorktreeAdd,
                    subject: id,
                    input_key: &key,
                };
                let Queued::New(job_id) = jobs::queue(&ctx(&config), tx, &job, Some(&repo), CLOCK)?
                else {
                    panic!("a new run");
                };
                end_run(tx, &config, job_id, passed)?;
                Ok(job_id)
            })
            .unwrap()
    }

    fn count(&self, sql: &str, args: &[&str]) -> i64 {
        self.conn()
            .query_row(sql, rusqlite::params_from_iter(args), |r| r.get(0))
            .unwrap()
    }

    /// Each row of the tables that the reconciler writes, as text.
    fn snapshot(&self) -> Vec<String> {
        let mut out = Vec::new();
        for sql in [
            "SELECT event_id || ' ' || kind || ' ' || coalesce(entity_id, '') || ' '
                || coalesce(cause, '') FROM events ORDER BY event_id",
            "SELECT worktree_id || ' ' || state || ' ' || path || ' '
                || coalesce(owner_attempt, '') || ' ' || coalesce(head_sha, '') || ' '
                || dirty || ' ' || coalesce(bytes_on_disk, '') || ' ' || coalesce(seen_at, '')
                || ' ' || coalesce(task_id, '') FROM worktrees ORDER BY worktree_id",
            "SELECT key || ' ' || state || ' ' || text FROM needs_you ORDER BY key",
            "SELECT id || ' ' || kind || ' ' || state FROM jobs ORDER BY id",
        ] {
            let mut stmt = self.conn().prepare(sql).unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
            out.extend(rows.map(Result::unwrap));
        }
        out
    }

    /// The list result of the repository: its main tree, then `entries`
    /// as (path, branch, row of the path).
    fn listed(&self, entries: &[(&str, &str, Option<&str>)]) -> Listed {
        let entry = |path: &str, branch: &str| ListEntry {
            path: path.to_string(),
            head: Some("h0".to_string()),
            branch: Some(format!("refs/heads/{branch}")),
            detached: false,
            bare: false,
            locked: None,
            prunable: None,
        };
        let main = self.repo_s();
        let mut result = ListResult {
            entries: vec![entry(&main, "main")],
            trees: vec![tree(&main, None, "h0", false, None)],
        };
        for (path, branch, id) in entries {
            result.entries.push(entry(path, branch));
            result.trees.push(tree(path, *id, "h1", false, Some(10)));
        }
        Listed {
            repo: self.repo_s(),
            job: 0,
            seen_at: "2026-11-17T10:00:00Z".to_string(),
            result,
        }
    }

    /// 1 run of `kind` on `subject`, queued, started and passed through the
    /// door. Gives its id.
    fn passed_run(&mut self, kind: Kind, subject: &str) -> i64 {
        let config = self.config.clone();
        let repo = self.repo_s();
        self.store
            .immediate(|tx| {
                let job = JobKey {
                    kind,
                    subject,
                    input_key: "run",
                };
                let Queued::New(id) = jobs::queue(&ctx(&config), tx, &job, Some(&repo), CLOCK)?
                else {
                    panic!("a new run");
                };
                end_run(tx, &config, id, true)?;
                Ok(id)
            })
            .unwrap()
    }

    /// A `removed` row `id` at `path`, whose removal run passed.
    fn removed(&mut self, id: &str, path: &str) {
        self.row_in(id, path, "removed", (None, None, None));
        self.passed_run(Kind::WorktreeRemove, id);
    }

    fn work_dir(&self, folders: &[&str]) -> WorkDir {
        WorkDir {
            repo: self.repo_s(),
            folders: folders.iter().map(|f| f.to_string()).collect(),
        }
    }
}

fn tree(path: &str, id: Option<&str>, head: &str, dirty: bool, bytes: Option<u64>) -> TreeFacts {
    TreeFacts {
        path: path.to_string(),
        worktree_id: id.map(str::to_string),
        head: Some(head.to_string()),
        dirty: Some(dirty),
        bytes,
        error: None,
    }
}

fn folder_fact(id: &str, presence: Presence) -> FolderFact {
    FolderFact {
        key: id.to_string(),
        presence,
    }
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

/// End the `queued` run `job_id` as its process does: `running`, then its
/// result, then a pass sees its process gone.
fn end_run(
    tx: &Transaction<'_>,
    config: &FleetConfig,
    job_id: i64,
    passed: bool,
) -> anyhow::Result<()> {
    let id = job_id.to_string();
    transition(
        tx,
        None,
        Entity::Job,
        &id,
        "running",
        cause(),
        &NeedsYouList,
    )?;
    jobs_store::set_started(tx, job_id, 4_000_000, None, FleetSeconds(1_000_000))?;
    let result = serde_json::json!({
        "passed": passed,
        "verdict": passed,
        "text": if passed { "made" } else { "git failed" },
    });
    jobs_store::set_result(tx, job_id, &result.to_string())?;
    let gone = ProcessFact {
        key: id,
        liveness: Liveness::Dead,
    };
    let seen = jobs::observe(&ctx(config), tx, &[gone], CLOCK)?;
    let end = if passed { State::Passed } else { State::Failed };
    assert_eq!(seen.ended, vec![(job_id, end)]);
    Ok(())
}

fn exists(path: &str) -> bool {
    Path::new(path).is_dir()
}

// ─── the tests ──────────────────────────────────────────────────────────

/// I6, RR-10, FM-26: a tree that the work directory and `git worktree
/// list` show, with no row, gets an `orphaned` row; nothing is deleted.
#[test]
fn i6_worktree_on_disk_without_row_is_adopted_not_deleted() {
    let mut w = World::new();
    let path = w.folder("hand-made");
    let facts = Facts {
        work_dirs: vec![w.work_dir(&[&path])],
        lists: vec![w.listed(&[(&path, "hand-made", None)])],
        ..Facts::default()
    };
    assert_eq!(
        w.i6(&facts),
        vec![Breach::NoRow {
            repo: w.repo_s(),
            path: path.clone()
        }]
    );

    let report = w.reconcile(&facts);

    assert_eq!(report.adopted.len(), 1, "{report:?}");
    let row = trees::by_path(w.conn(), &path).unwrap().unwrap();
    assert_eq!(report.adopted, vec![row.worktree_id.clone()]);
    assert_eq!(row.state, WorktreeState::Orphaned);
    assert_eq!(row.branch, "hand-made");
    assert_eq!(row.repo_root, w.repo_s());
    assert_eq!(row.task_id, None);
    assert_eq!(
        w.count(
            "SELECT count(*) FROM worktrees WHERE worktree_id = ?1 AND origin = 'native'",
            &[&row.worktree_id]
        ),
        1
    );
    assert!(exists(&path), "the folder stays");
    // The adopted orphan gets its rescue, and nothing removes it.
    assert_eq!(
        w.count(
            "SELECT count(*) FROM jobs WHERE kind = 'rescue' AND subject = ?1",
            &[&row.worktree_id]
        ),
        1
    );
    assert_eq!(w.count("SELECT count(*) FROM jobs", &[]), 1);
    // The facts of the list are on the new row in the same call.
    assert_eq!(row.head_sha.as_deref(), Some("h1"));
    let mut after = facts.clone();
    after.folders = vec![folder_fact(&row.worktree_id, Presence::Present)];
    assert_eq!(w.i6(&after), vec![]);
}

/// I6 with history: a folder at the path of a `removed` row (a tree that
/// triage removed, made again by hand) gets a new `orphaned` row; the
/// removed row stays as it is, and nothing is removed.
#[test]
fn a_folder_at_the_path_of_a_removed_row_is_adopted_as_a_new_row() {
    let mut w = World::new();
    let path = w.folder("hand-made");
    w.row_in("old", &path, "removed", (None, None, None));
    w.sql(
        "UPDATE worktrees SET status = 'trashed' WHERE worktree_id = ?1",
        &["old"],
    );
    let facts = Facts {
        work_dirs: vec![w.work_dir(&[&path])],
        lists: vec![w.listed(&[(&path, "hand-made", None)])],
        ..Facts::default()
    };
    assert_eq!(
        w.i6(&facts),
        vec![Breach::NoRow {
            repo: w.repo_s(),
            path: path.clone()
        }]
    );

    let report = w.reconcile(&facts);

    assert_eq!(report.adopted.len(), 1, "{report:?}");
    let row = trees::by_path(w.conn(), &path).unwrap().unwrap();
    assert_ne!(row.worktree_id, "old");
    assert_eq!(row.state, WorktreeState::Orphaned);
    assert_eq!(w.row("old").state, WorktreeState::Removed);
    assert!(exists(&path), "the folder stays");
    // The adopted orphan gets its rescue, and nothing removes it.
    assert_eq!(
        w.count(
            "SELECT count(*) FROM jobs WHERE kind = 'rescue' AND subject = ?1",
            &[&row.worktree_id]
        ),
        1
    );
    assert_eq!(w.count("SELECT count(*) FROM jobs", &[]), 1);
}

/// A tree that the list names by its path alone refreshes the live row of
/// that path, not a `removed` row of the same path.
#[test]
fn worktree_list_result_by_path_refreshes_the_live_row_and_not_a_removed_row() {
    let mut w = World::new();
    let path = w.folder("again");
    w.row_in("old", &path, "removed", (None, None, None));
    w.sql(
        "UPDATE worktrees SET status = 'trashed' WHERE worktree_id = ?1",
        &["old"],
    );
    w.row_in("new", &path, "created", (None, None, None));
    let mut listed = w.listed(&[(&path, "fleet/r1-new", None)]);
    listed.result.trees[1] = tree(&path, None, "c0ffee", false, Some(4096));
    let facts = Facts {
        lists: vec![listed],
        ..Facts::default()
    };

    let report = w.reconcile(&facts);

    assert_eq!(report.refreshed, vec!["new".to_string()]);
    assert_eq!(w.row("new").head_sha.as_deref(), Some("c0ffee"));
    assert_eq!(w.row("old").head_sha, None);
}

/// FM-25: the attempt that owns an `active` tree ends; the same call
/// marks the tree `orphaned`. A live owner keeps it.
#[test]
fn owner_death_marks_the_tree_orphaned_in_the_same_pass() {
    let mut w = World::new();
    let (task, worker, attempt) = w.claimed(1);
    let path = w.folder("T1-feat");
    w.row_in(
        "wt1",
        &path,
        "active",
        (Some(&task), Some(&worker), Some(&attempt)),
    );
    let facts = Facts {
        folders: vec![folder_fact("wt1", Presence::Present)],
        ..Facts::default()
    };
    w.reconcile(&facts);
    assert_eq!(w.row("wt1").state, WorktreeState::Active, "a live owner");

    w.sql(
        "UPDATE attempts SET state = 'ended', outcome = 'crashed' WHERE id = ?1",
        &[&attempt],
    );
    let report = w.reconcile(&facts);

    assert_eq!(report.orphaned, vec!["wt1".to_string()]);
    let row = w.row("wt1");
    assert_eq!(row.state, WorktreeState::Orphaned);
    assert_eq!(row.owner_attempt.as_deref(), Some(attempt.as_str()));
    assert!(exists(&path));
    assert_eq!(w.count("SELECT count(*) FROM jobs", &[]), 0);
}

/// Q23: an `active` tree with no owner attempt stays while its worker is
/// live, and is `orphaned` when it is not.
#[test]
fn active_tree_with_no_attempt_follows_the_liveness_of_its_worker() {
    let mut w = World::new();
    let (task, worker, _) = w.claimed(1);
    let path = w.folder("T1-feat");
    w.row_in("wt1", &path, "active", (Some(&task), Some(&worker), None));
    let facts = Facts::default();
    w.reconcile(&facts);
    assert_eq!(w.row("wt1").state, WorktreeState::Active);

    w.sql(
        "UPDATE workers SET status = 'stopped' WHERE worker_id = ?1",
        &[&worker],
    );
    w.reconcile(&facts);
    assert_eq!(w.row("wt1").state, WorktreeState::Orphaned);
}

/// A cancelled task orphans its `active` tree.
#[test]
fn cancelled_task_orphans_its_tree() {
    let mut w = World::new();
    let (task, worker, attempt) = w.claimed(1);
    let path = w.folder("T1-feat");
    w.row_in(
        "wt1",
        &path,
        "active",
        (Some(&task), Some(&worker), Some(&attempt)),
    );
    w.sql(
        "UPDATE tasks SET status = 'cancelled' WHERE task_id = ?1",
        &[&task],
    );
    let report = w.reconcile(&Facts::default());
    assert_eq!(report.orphaned, vec!["wt1".to_string()]);
}

/// Q10: the attempt of a task parked by `done --unlanded` ended; its tree
/// is `created` with no owner, so a requeue resumes it.
#[test]
fn parked_attempt_leaves_the_tree_created_with_no_owner() {
    let mut w = World::new();
    let (task, worker, attempt) = w.claimed(1);
    let path = w.folder("T1-feat");
    w.row_in(
        "wt1",
        &path,
        "active",
        (Some(&task), Some(&worker), Some(&attempt)),
    );
    w.sql(
        "UPDATE tasks SET status = 'escalated', held_reason = 'unlanded',
            current_worker_id = NULL WHERE task_id = ?1",
        &[&task],
    );
    w.sql(
        "UPDATE attempts SET state = 'ended', outcome = 'done' WHERE id = ?1",
        &[&attempt],
    );

    let report = w.reconcile(&Facts::default());

    assert_eq!(report.created, vec!["wt1".to_string()]);
    assert!(report.orphaned.is_empty());
    let row = w.row("wt1");
    assert_eq!(row.state, WorktreeState::Created);
    assert_eq!(row.owner_attempt, None);
    assert!(exists(&path));
}

/// FR-11: a command that crashed after its `worktree_add` run ended left
/// the row `reserved`. The next call observes the run: a passed run with
/// its folder makes the tree, a failed run drops the row. A `reserved`
/// row with no run cannot exist; I6 names it.
#[test]
fn reserved_row_left_by_crash_is_created_or_dropped_next_pass() {
    let mut w = World::new();
    let (task, worker, attempt) = w.claimed(1);
    let made = w.folder("T1-made");
    w.row_in(
        "made",
        &made,
        "reserved",
        (Some(&task), Some(&worker), Some(&attempt)),
    );
    let passed = w.ended_add("made", true);
    let failed_path = w.no_folder("T2-failed");
    w.row_in("failed", &failed_path, "reserved", (None, None, None));
    let failed = w.ended_add("failed", false);
    let none_path = w.no_folder("T3-none");
    w.row_in("none", &none_path, "reserved", (None, None, None));

    let report = w.reconcile(&Facts::default());

    assert_eq!(report.observed, vec![passed, failed]);
    let row = w.row("made");
    assert_eq!(
        row.state,
        WorktreeState::Active,
        "created, then its owner takes it"
    );
    assert_eq!(row.owner_attempt.as_deref(), Some(attempt.as_str()));
    assert!(trees::get(w.conn(), "failed").unwrap().is_none(), "dropped");
    assert_eq!(w.row("none").state, WorktreeState::Reserved);
    assert!(w
        .i6(&Facts::default())
        .contains(&Breach::ReservedWithNoJob { id: "none".into() }));
}

/// FM-27: a folder that vanished without a removal opens 1 needs-you
/// item and 1 event, 1 time, and the row stays.
#[test]
fn vanished_folder_opens_1_needs_you_item_and_keeps_the_row() {
    let mut w = World::new();
    let gone = w.no_folder("gone");
    w.row_in("gone", &gone, "created", (None, None, None));
    let listed = w.no_folder("listed");
    w.row_in("listed", &listed, "created", (None, None, None));
    let unread = w.no_folder("unread");
    w.row_in("unread", &unread, "created", (None, None, None));
    let facts = Facts {
        folders: vec![
            folder_fact("gone", Presence::Missing),
            folder_fact("listed", Presence::Missing),
        ],
        // git still shows `listed`; the scan did not reach `unread`.
        lists: vec![w.listed(&[(&listed, "fleet/r1-listed", Some("listed"))])],
        ..Facts::default()
    };

    let report = w.reconcile(&facts);
    w.reconcile(&facts);

    assert_eq!(report.vanished, vec!["gone".to_string()]);
    assert_eq!(w.row("gone").state, WorktreeState::Created, "the row stays");
    assert_eq!(
        w.count(
            "SELECT count(*) FROM needs_you WHERE key = 'worktree_vanished:gone' AND state = 'open'",
            &[]
        ),
        1
    );
    assert_eq!(
        w.count(
            "SELECT count(*) FROM needs_you WHERE kind = 'worktree_vanished'",
            &[]
        ),
        1
    );
    assert_eq!(
        w.count(
            "SELECT count(*) FROM events WHERE entity_id = 'gone' AND cause = 'worktree_vanished'",
            &[]
        ),
        1
    );
    assert!(w
        .i6(&facts)
        .contains(&Breach::NoFolder { id: "gone".into() }));
}

/// Q15: an adopted tree on `<run branch>-T<seq>-<name>` gets the task
/// `T<seq>` of that run; another branch gets no task.
#[test]
fn adopted_tree_is_linked_to_the_task_its_branch_names() {
    let mut w = World::new();
    let (task, _, _) = w.claimed(1);
    let seq: i64 = w.count("SELECT seq FROM tasks WHERE task_id = ?1", &[&task]);
    let linked = w.folder("linked");
    let free = w.folder("free");
    let unknown = w.folder("unknown");
    let missing = w.no_folder("missing");
    let facts = Facts {
        lists: vec![w.listed(&[
            (&linked, &format!("{RUN_BRANCH}-T{seq}-feat"), None),
            (&free, "someone/else", None),
            (&unknown, &format!("{RUN_BRANCH}-T99-x"), None),
            (&missing, &format!("{RUN_BRANCH}-T{seq}-gone"), None),
        ])],
        work_dirs: vec![w.work_dir(&[&linked, &free, &unknown])],
        ..Facts::default()
    };

    let report = w.reconcile(&facts);

    assert_eq!(report.adopted.len(), 3, "{report:?}");
    let task_of = |p: &str| trees::by_path(w.conn(), p).unwrap().unwrap().task_id;
    assert_eq!(task_of(&linked), Some(task));
    assert_eq!(task_of(&free), None);
    assert_eq!(task_of(&unknown), None, "no task T99");
    assert!(
        trees::by_path(w.conn(), &missing).unwrap().is_none(),
        "a listed entry whose folder the scan does not show gets no row"
    );
}

/// A `worktree_list` result that a removal outdated adopts nothing: the
/// list named the tree, then its `worktree_remove` run passed, and the same
/// pass reads that list. No new row, and no rescue run (FM-27, FM-28).
#[test]
fn a_list_that_a_removal_outdated_adopts_no_new_row() {
    let mut w = World::new();
    let gone = w.no_folder("gone");
    let raced = w.folder("raced");
    let list = w.passed_run(Kind::WorktreeList, &w.repo_s());
    w.removed("wt-gone", &gone);
    w.removed("wt-raced", &raced);
    // The scan of this pass shows no folder at `gone`; a folder at `raced`
    // came back after the removal, and only a newer list can name it.
    let mut listed = w.listed(&[(&gone, "b-gone", None), (&raced, "b-raced", None)]);
    listed.job = list;
    let facts = Facts {
        lists: vec![listed],
        work_dirs: vec![w.work_dir(&[&raced])],
        ..Facts::default()
    };
    let rows = w.count("SELECT count(*) FROM worktrees", &[]);

    let report = w.reconcile(&facts);

    assert!(report.adopted.is_empty(), "{report:?}");
    assert!(report.rescues.is_empty(), "{report:?}");
    assert_eq!(w.count("SELECT count(*) FROM worktrees", &[]), rows);
    assert_eq!(
        w.count("SELECT count(*) FROM jobs WHERE kind = 'rescue'", &[]),
        0
    );
    let no_row = w
        .i6(&facts)
        .into_iter()
        .filter(|b| matches!(b, Breach::NoRow { .. }))
        .count();
    assert_eq!(no_row, 0, "{:?}", w.i6(&facts));
}

/// The list run and the removal run end in the same second; the order of
/// their run ids decides. A list run queued before the removal run is
/// stale: the folder at the path gets no row.
#[test]
fn a_list_that_ended_before_a_removal_in_the_same_second_adopts_nothing() {
    let mut w = World::new();
    let path = w.folder("tree");
    let list = w.passed_run(Kind::WorktreeList, &w.repo_s());
    w.removed("wt-old", &path);
    let mut listed = w.listed(&[(&path, "b", None)]);
    listed.job = list;
    listed.seen_at = same_second(&mut w);
    let facts = Facts {
        lists: vec![listed],
        work_dirs: vec![w.work_dir(&[&path])],
        ..Facts::default()
    };

    let report = w.reconcile(&facts);

    assert!(report.adopted.is_empty(), "{report:?}");
}

/// A list run that started after the removal ended, in the same second,
/// is fresh: the folder at the path is a tree, and gets a new row.
#[test]
fn a_list_that_started_after_a_removal_in_the_same_second_adopts_the_tree() {
    let mut w = World::new();
    let path = w.folder("tree");
    w.removed("wt-old", &path);
    let list = w.passed_run(Kind::WorktreeList, &w.repo_s());
    let mut listed = w.listed(&[(&path, "b", None)]);
    listed.job = list;
    listed.seen_at = same_second(&mut w);
    let facts = Facts {
        lists: vec![listed],
        work_dirs: vec![w.work_dir(&[&path])],
        ..Facts::default()
    };

    let report = w.reconcile(&facts);

    assert_eq!(report.adopted.len(), 1, "{report:?}");
    let row = trees::by_path(w.conn(), &path).unwrap().unwrap();
    assert_eq!(row.state, WorktreeState::Orphaned);
}

/// The end second of both runs of a same-second test. The fixture writes it
/// to each ended run, so the test does not depend on the wall clock.
const SAME_SECOND: &str = "2026-11-17T10:00:00Z";

/// Give each ended run the end time [`SAME_SECOND`], and give it.
fn same_second(w: &mut World) -> String {
    w.sql(
        "UPDATE jobs SET ended_at = ?1 WHERE ended_at IS NOT NULL",
        &[SAME_SECOND],
    );
    let ends = w.count(
        "SELECT count(*) FROM jobs WHERE ended_at = ?1",
        &[SAME_SECOND],
    );
    assert_eq!(ends, 2, "the list run and the removal run end in 1 second");
    SAME_SECOND.to_string()
}

/// Q9, the act: the reconciler queues 1 rescue of each orphan and 1
/// removal of each landed tree, once, and nothing else. The ref of an
/// earlier rescue of a reused tree does not save the orphan now. An orphan
/// whose HEAD no `worktree_list` result gave yet waits for it.
#[test]
fn act_queues_1_rescue_of_each_orphan_and_1_removal_of_each_landed_tree() {
    let mut w = World::new();
    let orphan = w.folder("orphan");
    w.row_in("orphan", &orphan, "orphaned", (None, None, None));
    w.sql(
        "UPDATE worktrees SET head_sha = 'h1' WHERE worktree_id = 'orphan'",
        &[],
    );
    let unread = w.folder("unread");
    w.row_in("unread", &unread, "orphaned", (None, None, None));
    let landed = w.folder("landed");
    w.row_in("landed", &landed, "landed", (None, None, None));
    let reused = w.folder("reused");
    w.row_in("reused", &reused, "orphaned", (None, None, None));
    w.sql(
        "UPDATE worktrees SET rescue_ref = 'refs/horch/rescue/reused/1', head_sha = 'h2'
          WHERE worktree_id = 'reused'",
        &[],
    );
    let created = w.folder("created");
    w.row_in("created", &created, "created", (None, None, None));
    let facts = Facts::default();

    let report = w.reconcile(&facts);
    let again = w.reconcile(&facts);

    assert_eq!(report.rescues, ["orphan", "reused"]);
    assert_eq!(report.removals, vec!["landed".to_string()]);
    assert!(again.rescues.is_empty(), "{again:?}");
    assert!(again.removals.is_empty(), "{again:?}");
    let jobs = |kind: &str, id: &str| {
        w.count(
            "SELECT count(*) FROM jobs WHERE kind = ?1 AND subject = ?2 AND state = 'queued'",
            &[kind, id],
        )
    };
    assert_eq!(jobs("rescue", "orphan"), 1);
    assert_eq!(jobs("rescue", "reused"), 1);
    assert_eq!(jobs("worktree_remove", "landed"), 1);
    assert_eq!(w.count("SELECT count(*) FROM jobs", &[]), 3);
    assert_eq!(
        w.count(
            "SELECT count(*) FROM events WHERE cause IN ('would_rescue', 'would_remove')",
            &[]
        ),
        0,
        "no report-only event"
    );
    for id in ["orphan", "unread", "landed", "reused", "created"] {
        assert!(exists(&w.row(id).path));
    }
}

/// The reconciler is level-triggered: a second call with the same facts
/// writes nothing more.
#[test]
fn reconcile_twice_with_the_same_facts_writes_nothing_more() {
    let mut w = World::new();
    let (task, worker, attempt) = w.claimed(1);
    let dead = w.folder("T1-feat");
    w.row_in(
        "dead",
        &dead,
        "active",
        (Some(&task), Some(&worker), Some(&attempt)),
    );
    w.sql(
        "UPDATE attempts SET state = 'ended' WHERE id = ?1",
        &[&attempt],
    );
    let gone = w.no_folder("gone");
    w.row_in("gone", &gone, "created", (None, None, None));
    let landed = w.folder("landed");
    w.row_in("landed", &landed, "landed", (None, None, None));
    let failed_path = w.no_folder("failed");
    w.row_in("failed", &failed_path, "reserved", (None, None, None));
    w.ended_add("failed", false);
    let unknown = w.folder("unknown");
    let facts = Facts {
        folders: vec![
            folder_fact("dead", Presence::Present),
            folder_fact("gone", Presence::Missing),
        ],
        work_dirs: vec![w.work_dir(&[&dead, &landed, &unknown])],
        lists: vec![w.listed(&[
            (&dead, "fleet/r1-T1-feat", Some("dead")),
            (&unknown, "hand", None),
        ])],
    };

    let first = w.reconcile(&facts);
    let after_first = w.snapshot();
    w.reconcile(&facts);

    assert_eq!(first.orphaned, vec!["dead".to_string()]);
    assert_eq!(first.vanished, vec!["gone".to_string()]);
    assert_eq!(first.adopted.len(), 1);
    assert_eq!(w.snapshot(), after_first);
}

/// Q11: the `worktree_list` result gives the HEAD, the dirty flag and the
/// size of each tree of a row, seen at the time of the run.
#[test]
fn worktree_list_result_refreshes_head_dirty_and_size() {
    let mut w = World::new();
    let one = w.folder("one");
    w.row_in("one", &one, "created", (None, None, None));
    let bad = w.folder("bad");
    w.row_in("bad", &bad, "created", (None, None, None));
    let mut listed = w.listed(&[(&one, "fleet/r1-one", Some("one"))]);
    listed.result.trees[1] = tree(&one, Some("one"), "c0ffee", true, Some(4096));
    listed.result.trees.push(TreeFacts {
        error: Some("fatal: not a git repository".into()),
        head: None,
        dirty: None,
        ..tree(&bad, Some("bad"), "x", false, Some(1))
    });
    let facts = Facts {
        lists: vec![listed.clone()],
        ..Facts::default()
    };

    let report = w.reconcile(&facts);

    assert_eq!(report.refreshed, vec!["one".to_string()]);
    let row = w.row("one");
    assert_eq!(row.head_sha.as_deref(), Some("c0ffee"));
    assert!(row.dirty);
    assert_eq!(row.bytes_on_disk, Some(4096));
    assert_eq!(row.seen_at.as_deref(), Some(listed.seen_at.as_str()));
    let bad_row = w.row("bad");
    assert_eq!(bad_row.head_sha, None, "a fact that git could not give");
    assert_eq!(bad_row.seen_at, None);
}

impl World {
    fn task_branch(&mut self, branch: &str, task: &str, state: &str) {
        self.sql(
            "INSERT INTO branches (name, feature_id, task_id, landing_state, owner, due_at)
             SELECT ?1, id, ?2, 'in_bundle', 'fleetd', 0 FROM features LIMIT 1",
            &[branch, task],
        );
        if state != "in_bundle" {
            self.task_branch_to(branch, state);
        }
    }

    fn list_at(&self, id: &str, path: &str, head: &str, seen_at: &str) -> Facts {
        let mut listed = self.listed(&[(path, "b", Some(id))]);
        listed.result.trees[1].head = Some(head.to_string());
        listed.seen_at = seen_at.to_string();
        Facts {
            folders: vec![folder_fact(id, Presence::Present)],
            lists: vec![listed],
            ..Facts::default()
        }
    }

    fn task_branch_to(&mut self, branch: &str, state: &str) {
        self.store
            .immediate(|tx| {
                transition(
                    tx,
                    None,
                    Entity::TaskBranch,
                    branch,
                    state,
                    cause(),
                    &NeedsYouList,
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn jobs_of(&self, kind: &str, id: &str) -> i64 {
        self.count(
            "SELECT count(*) FROM jobs WHERE kind = ?1 AND subject = ?2",
            &[kind, id],
        )
    }
}

#[test]
fn verified_attempt_moves_its_tree_to_landing_with_no_rescue() {
    let mut w = World::new();
    let (task, worker, attempt) = w.claimed(1);
    let path = w.folder("T1-feat");
    w.row_in(
        "wt1",
        &path,
        "active",
        (Some(&task), Some(&worker), Some(&attempt)),
    );
    w.sql(
        "UPDATE attempts SET state = 'ended', outcome = 'done' WHERE id = ?1",
        &[&attempt],
    );
    w.sql(
        "UPDATE tasks SET status = 'verifying' WHERE task_id = ?1",
        &[&task],
    );
    let facts = w.list_at("wt1", &path, "h1", "2026-11-17T10:00:00Z");
    let report = w.reconcile(&facts);
    assert_eq!(w.row("wt1").state, WorktreeState::Active, "{report:?}");

    w.sql(
        "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
        &[&task],
    );
    let report = w.reconcile(&facts);

    assert_eq!(report.landing, vec!["wt1".to_string()]);
    assert!(report.orphaned.is_empty(), "{report:?}");
    assert_eq!(w.row("wt1").state, WorktreeState::Landing);
    assert_eq!(w.jobs_of("rescue", "wt1"), 0);
    let again = w.reconcile(&facts);
    assert!(again.rescues.is_empty(), "{again:?}");
    assert_eq!(w.jobs_of("rescue", "wt1"), 0);
}

#[test]
fn landed_task_branch_moves_its_landing_tree_to_landed_and_queues_1_removal() {
    let mut w = World::new();
    let (task, _, _) = w.claimed(1);
    let path = w.folder("T1-feat");
    w.row_in("wt1", &path, "landing", (Some(&task), None, None));
    let branch = w.row("wt1").branch;
    w.task_branch(&branch, &task, "in_bundle");
    let before = w.list_at("wt1", &path, "h1", "2000-01-01T00:00:00Z");
    let report = w.reconcile(&before);
    assert_eq!(w.row("wt1").state, WorktreeState::Landing, "{report:?}");

    w.task_branch_to(&branch, "landed");
    let report = w.reconcile(&before);
    assert_eq!(w.row("wt1").state, WorktreeState::Landing, "{report:?}");

    let after = w.list_at("wt1", &path, "h2", "2999-01-01T00:00:00Z");
    let report = w.reconcile(&after);

    assert_eq!(report.landed, vec!["wt1".to_string()]);
    assert_eq!(report.removals, vec!["wt1".to_string()]);
    assert_eq!(w.row("wt1").state, WorktreeState::Landed);
    assert_eq!(w.row("wt1").status, WorktreeStatus::Landed);
    let landed: String = w
        .conn()
        .query_row(
            "SELECT landed_commit FROM worktrees WHERE worktree_id = 'wt1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(landed, "h2");
    assert_eq!(w.jobs_of("worktree_remove", "wt1"), 1);
    assert_eq!(w.jobs_of("rescue", "wt1"), 0);
    let again = w.reconcile(&after);
    assert!(again.removals.is_empty(), "{again:?}");
    assert_eq!(w.jobs_of("worktree_remove", "wt1"), 1);
}

#[test]
fn abandoned_task_branch_queues_1_rescue_of_its_landing_tree() {
    let mut w = World::new();
    let (task, _, _) = w.claimed(1);
    let path = w.folder("T1-feat");
    w.row_in("wt1", &path, "landing", (Some(&task), None, None));
    let branch = w.row("wt1").branch;
    w.task_branch(&branch, &task, "abandoned");
    let facts = w.list_at("wt1", &path, "h1", "2999-01-01T00:00:00Z");

    let report = w.reconcile(&facts);
    let again = w.reconcile(&facts);

    assert_eq!(report.rescues, vec!["wt1".to_string()]);
    assert!(again.rescues.is_empty(), "{again:?}");
    assert_eq!(w.jobs_of("rescue", "wt1"), 1);
    assert_eq!(w.jobs_of("worktree_remove", "wt1"), 0);
    assert_eq!(w.row("wt1").state, WorktreeState::Landing);
}

#[test]
fn retried_attempt_moves_its_tree_to_created_with_no_owner() {
    let mut w = World::new();
    let (task, worker, attempt) = w.claimed(1);
    let path = w.folder("T1-feat");
    w.row_in(
        "wt1",
        &path,
        "active",
        (Some(&task), Some(&worker), Some(&attempt)),
    );
    w.sql(
        "UPDATE attempts SET state = 'ended', outcome = 'rejected' WHERE id = ?1",
        &[&attempt],
    );
    w.sql(
        "UPDATE tasks SET status = 'ready', current_worker_id = NULL WHERE task_id = ?1",
        &[&task],
    );

    let report = w.reconcile(&Facts::default());

    assert_eq!(report.created, vec!["wt1".to_string()]);
    assert!(report.orphaned.is_empty(), "{report:?}");
    let row = w.row("wt1");
    assert_eq!(row.state, WorktreeState::Created);
    assert_eq!(row.owner_attempt, None);
    assert_eq!(w.jobs_of("rescue", "wt1"), 0);
}

#[test]
fn crashed_attempt_of_a_retried_task_orphans_its_tree() {
    let mut w = World::new();
    let (task, worker, attempt) = w.claimed(1);
    let path = w.folder("T1-feat");
    w.row_in(
        "wt1",
        &path,
        "active",
        (Some(&task), Some(&worker), Some(&attempt)),
    );
    w.sql(
        "UPDATE attempts SET state = 'ended', outcome = 'crashed' WHERE id = ?1",
        &[&attempt],
    );
    w.sql(
        "UPDATE tasks SET status = 'ready', current_worker_id = NULL WHERE task_id = ?1",
        &[&task],
    );

    let report = w.reconcile(&Facts::default());

    assert_eq!(report.orphaned, vec!["wt1".to_string()]);
    assert!(report.created.is_empty(), "{report:?}");
    assert_eq!(w.row("wt1").state, WorktreeState::Orphaned);
}

/// D244-F2: the owner step reads the attempts, workers and tasks of all
/// `active` rows in set reads on 1 snapshot. A pass over 3 and over 300
/// active trees with live owners runs the same count of statements.
#[test]
fn owner_reads_of_3_and_of_300_active_trees_run_the_same_count_of_statements() {
    let count = |n: u32| {
        let mut w = World::new();
        for i in 1..=n {
            let (task, worker, attempt) = w.claimed(i);
            let path = w.folder(&format!("T{i}-feat"));
            w.row_in(
                &format!("wt{i}"),
                &path,
                "active",
                (Some(&task), Some(&worker), Some(&attempt)),
            );
        }
        let config = w.config.clone();
        let conn = w.store.conn();
        let (report, statements) = horch_core::taskledger::testing::count_statements(conn, || {
            reconcile_worktrees(&ctx(&config), conn, &Facts::default(), CLOCK).unwrap()
        });
        assert!(report.orphaned.is_empty(), "{report:?}");
        assert_eq!(w.row("wt1").state, WorktreeState::Active);
        statements
    };
    assert_eq!(count(3), count(300));
}

/// D244-F2: the edges of the owner facts of the set reads give the result
/// of the reads of 1 row each: an owner attempt with no row orphans the
/// tree, a live attempt holds the tree of a worker with no row, and a task
/// id with no task row fails the pass.
/// The foreign keys of the ledger refuse such a row, so the test turns
/// them off to write it.
#[test]
fn owner_with_no_attempt_row_no_worker_row_or_no_task_row() {
    let mut w = World::new();
    w.conn().execute_batch("PRAGMA foreign_keys = OFF").unwrap();
    let (task, worker, _) = w.claimed(1);
    let path = w.folder("T1-feat");
    w.row_in(
        "wt1",
        &path,
        "active",
        (Some(&task), Some(&worker), Some("no-such-attempt")),
    );
    let (task2, _, attempt2) = w.claimed(2);
    let path2 = w.folder("T2-feat");
    w.row_in(
        "wt2",
        &path2,
        "active",
        (Some(&task2), Some("no-such-worker"), Some(&attempt2)),
    );
    let report = w.reconcile(&Facts::default());
    assert_eq!(report.orphaned, ["wt1"], "{report:?}");
    assert_eq!(w.row("wt2").state, WorktreeState::Active);

    let (_, worker3, attempt3) = w.claimed(3);
    let path3 = w.folder("T3-feat");
    w.row_in(
        "wt3",
        &path3,
        "active",
        (Some("no-such-task"), Some(&worker3), Some(&attempt3)),
    );
    let config = w.config.clone();
    let err = w
        .store
        .immediate(|tx| reconcile_worktrees(&ctx(&config), tx, &Facts::default(), CLOCK))
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("no task no-such-task"),
        "{err:#}"
    );
}
