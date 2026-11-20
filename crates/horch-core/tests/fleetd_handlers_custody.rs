//! The fleetd handlers of the custody, job and integration request kinds
//! (docs/tickets/fleetd-p5/20-handlers-custody.md; RR-5, RR-8, RR-13 to
//! RR-18).
//!
//! Each test sends requests through `requests::handle` to a temp ledger at
//! generation 1, with the handlers of `handlers_custody` registered. The
//! git facts of a request come in its payload, as the command reads them.
//! No test runs git or a process.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use horch_core::digest::sha256_bytes;
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, NewRun};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Reply, Request};
use horch_core::fleetd::fleet_runtime::application::handlers_custody::{register, Custody};
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers, NOT_HANDLED};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind as JobKind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::application::commands::RefTips;
use horch_core::fleetd::worktree_custody::adapters::rescue_git::rescue_key;
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, WorktreeState};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun as NewLedgerRun, Run};
use horch_core::taskledger::tasks;
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util, Store};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

const RUN_BRANCH: &str = "fleet/r1";

/// No test of this file runs git: git has no ref.
struct NoRefs;

impl RefTips for NoRefs {
    fn tip(&self, _repo: &Path, _name: &str) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}
const BASE: &str = "b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0";
const HEAD: &str = "c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1c1";

/// A temp ledger at generation 1 with 1 run of the folder `root`, and the
/// handlers of this ticket.
struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    store: Store,
    conn: Connection,
    run: Run,
    handlers: Handlers,
}

fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let top = tmp.path().canonicalize().unwrap();
    let root = top.join("repo");
    std::fs::create_dir_all(&root).unwrap();
    let db = top.join("ledger.sqlite3");
    let mut store = Store::open(&db).unwrap();
    let run = store
        .immediate(|tx| {
            tx.execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
                params![std::process::id(), util::now()],
            )?;
            let run = runs::create(
                tx,
                &root,
                &NewLedgerRun {
                    slug: "r1".into(),
                    base_branch: Some("main".into()),
                    run_branch: Some(RUN_BRANCH.into()),
                    ..Default::default()
                },
            )?;
            schema::run_idea(tx, &run.run_id, &util::now(), true)?;
            Ok(run)
        })
        .unwrap();
    let conn = Connection::open(&db).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let mut handlers = Handlers::new();
    register(
        &mut handlers,
        Arc::new(Custody {
            guard: Generation(1),
            config: FleetConfig::default(),
            refs: Box::new(NoRefs),
        }),
    );
    World {
        _tmp: tmp,
        root,
        store,
        conn,
        run,
        handlers,
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A request of `kind` with a new request_id.
fn request(kind: Kind, payload: Value) -> Request {
    let n = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let id = format!("6f1c2a3e-0b9d-4c55-8e7f-{n:012x}");
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    Request {
        request_id: RequestId::parse(&id).unwrap(),
        kind,
        payload,
        token: Some("t".into()),
    }
}

impl World {
    fn send(&mut self, req: &Request) -> Reply {
        let decision = Decision::Authorized {
            auth: "token; channel".into(),
        };
        handle(
            &mut self.conn,
            Generation(1),
            req,
            Some("me"),
            &decision,
            &self.handlers,
        )
        .unwrap()
    }

    fn applied(&mut self, kind: Kind, payload: Value) -> Value {
        match self.send(&request(kind, payload)).outcome {
            Outcome::Applied(v) => v,
            Outcome::Refused(why) => panic!("refused: {why}"),
        }
    }

    fn refused(&mut self, kind: Kind, payload: Value) -> String {
        match self.send(&request(kind, payload)).outcome {
            Outcome::Refused(why) => why,
            Outcome::Applied(v) => panic!("applied: {v}"),
        }
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// A new worker of the run; gives its id.
    fn worker(&mut self) -> String {
        let run = self.run.clone();
        self.store
            .immediate(|tx| {
                let id = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        persona: "opus".into(),
                        role: Some("opus-1".into()),
                        ..Default::default()
                    },
                )?;
                workers::supervisor_started(tx, &id, 4242, Some(1))?;
                Ok(id)
            })
            .unwrap()
    }

    fn scratch_payload(&self, worker: &str, name: &str) -> Value {
        json!({
            "worker_id": worker,
            "name": name,
            "base": RUN_BRANCH,
            "base_commit": BASE,
            "path_exists": false,
            "branch_exists": false,
        })
    }

    /// A scratch tree `name` of a new worker whose folder exists: `active`,
    /// as after its `worktree_add` job. Gives its worktree id.
    fn active_tree(&mut self, name: &str) -> String {
        let worker = self.worker();
        let p = self.scratch_payload(&worker, name);
        let made = self.applied(Kind::WorktreeScratch, p);
        let id = made["worktree_id"].as_str().unwrap().to_string();
        self.store
            .immediate(|tx| {
                let cause = Cause {
                    actor: Actor::Horch,
                    text: "test: its worktree_add job passed",
                };
                transition(
                    tx,
                    None,
                    Entity::Worktree,
                    &id,
                    "created",
                    cause,
                    &NeedsYouList,
                )?;
                transition(
                    tx,
                    None,
                    Entity::Worktree,
                    &id,
                    "active",
                    cause,
                    &NeedsYouList,
                )?;
                Ok(())
            })
            .unwrap();
        id
    }

    fn drop_payload(&self, name: &str, facts: Value) -> Value {
        json!({
            "run_id": self.run.run_id,
            "name": name,
            "reason": "not needed",
            "keep_branch": false,
            "facts": facts,
        })
    }
}

/// The facts of a clean folder on its recorded branch `branch`.
fn clean(branch: &str) -> Value {
    json!({
        "path_exists": true,
        "dirty": false,
        "head_branch": branch,
        "head_is_recorded": true,
        "stash": false,
        "unique_commits": false,
        "head": HEAD,
    })
}

fn with(mut facts: Value, key: &str, value: Value) -> Value {
    facts[key] = value;
    facts
}

/// A triage row of the ledger tree `id` at `path` on `branch`.
fn triage_row(id: &str, path: &str, branch: &str, run_id: &str) -> Value {
    json!({
        "id": "W1",
        "name": "_scratch/t1",
        "origin": "ledger",
        "worktree_id": id,
        "status": "open",
        "decision": null,
        "task": null,
        "task_id": null,
        "run_id": run_id,
        "findings": [],
        "branch": branch,
        "path": path,
        "base": RUN_BRANCH,
        "tip": HEAD,
        "on_disk": true,
        "age": {"created_at": null, "last_commit_at": null, "newest_mtime": null},
        "disk": null,
        "ahead": 1,
        "behind": 0,
        "diff": {"files": 1, "insertions": 1, "deletions": 0, "top_paths": []},
        "uncommitted": {"modified": 0, "staged": 0, "untracked": 0, "largest_untracked": []},
        "superseded": false,
        "conflicts": null,
        "last_check": null,
        "why_parked": null,
    })
}

#[test]
fn job_requeue_sets_abandoned_runs_to_0() {
    let mut w = world();
    let held = jobs_store::insert(
        &w.conn,
        &NewRun {
            kind: JobKind::Push,
            subject: "F-1",
            repo: None,
            input_key: "tipA",
            run: 3,
            abandoned_runs: 3,
            not_before: Some(FleetSeconds(150)),
            time_limit_s: 60,
        },
    )
    .unwrap();
    w.conn
        .execute("UPDATE jobs SET state = 'abandoned' WHERE id = ?1", [held])
        .unwrap();

    let out = w.applied(Kind::JobRequeue, json!({ "job": held }));
    let id = out["job"].as_i64().unwrap();
    let row = jobs_store::get(&w.conn, id).unwrap().unwrap();
    assert_eq!((row.run, row.abandoned_runs, row.not_before), (4, 0, None));
    assert_eq!(row.state, State::Queued);
    assert_eq!(out["run"], 4);
    assert_eq!(out["kind"], "push");
    assert_eq!(out["subject"], "F-1");

    let again = w.refused(Kind::JobRequeue, json!({ "job": held }));
    assert_eq!(
        again,
        format!("job {id} of the same kind and subject is queued: no new run")
    );
    assert_eq!(
        w.refused(Kind::JobRequeue, json!({ "job": 999 })),
        "no job 999"
    );
}

#[test]
fn worktree_drop_keeps_each_refusal_of_today() {
    let mut w = world();
    let id = w.active_tree("t1");
    let branch = trees::get(&w.conn, &id).unwrap().unwrap().branch;
    let refusals = [
        (
            with(clean(&branch), "dirty", json!(true)),
            "worktree has uncommitted or untracked changes; nothing removed".to_string(),
        ),
        (
            with(clean(&branch), "stash", json!(true)),
            "worktree branch has a stash; nothing removed".to_string(),
        ),
        (
            with(clean(&branch), "head_branch", Value::Null),
            format!("worktree HEAD is detached or unreadable; reattach the recorded branch {branch} before dropping it; nothing removed"),
        ),
        (
            with(clean(&branch), "head_branch", json!("other")),
            format!("worktree is on other, not its recorded branch {branch}; nothing removed (including with --keep-branch)"),
        ),
        (
            with(clean(&branch), "head_is_recorded", json!(false)),
            format!("worktree HEAD differs from its recorded branch {branch}; nothing removed"),
        ),
        (
            with(clean(&branch), "unique_commits", json!(true)),
            "worktree has unpublished commits that exist nowhere else; use --keep-branch; nothing removed".to_string(),
        ),
        (
            with(clean(&branch), "dirty", Value::Null),
            "the fact dirty is unknown; nothing removed".to_string(),
        ),
    ];
    for (facts, words) in refusals {
        let p = w.drop_payload("t1", facts);
        assert_eq!(w.refused(Kind::WorktreeDrop, p), words);
        let row = trees::get(&w.conn, &id).unwrap().unwrap();
        assert_eq!(row.state, WorktreeState::Active, "the tree stays");
        assert_eq!(row.status.as_str(), "open");
    }
    let mut p = w.drop_payload("t1", clean(&branch));
    p["reason"] = json!(" ");
    assert_eq!(w.refused(Kind::WorktreeDrop, p), "give a nonempty --reason");
    assert_eq!(
        w.count("SELECT count(*) FROM jobs WHERE kind = 'rescue'"),
        0
    );

    // --keep-branch keeps a tree with unique commits; the drop orphans the
    // tree and queues its rescue at the HEAD of the facts.
    let mut p = w.drop_payload("t1", with(clean(&branch), "unique_commits", json!(true)));
    p["keep_branch"] = json!(true);
    let out = w.applied(Kind::WorktreeDrop, p);
    let row = trees::get(&w.conn, &id).unwrap().unwrap();
    assert_eq!(row.state, WorktreeState::Orphaned);
    assert_eq!(row.owner_attempt, None);
    assert_eq!(row.status.as_str(), "abandoned");
    let job = jobs_store::get(&w.conn, out["job"].as_i64().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(job.kind, JobKind::Rescue);
    assert_eq!(job.subject, id);
    assert_eq!(job.input_key, rescue_key(&row.path, HEAD));
    assert_eq!(out["path"], row.path.as_str());
    assert_eq!(out["reason"], "not needed");
    let reason: String = w
        .conn
        .query_row(
            "SELECT reason FROM worktrees WHERE worktree_id = ?1",
            [&id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(reason, "not needed");
}

#[test]
fn worktree_restore_queues_the_worktree_add_job() {
    let mut w = world();
    let archive = "refs/horch/archive/t9";
    let facts = |path_exists: bool| {
        json!({
            "path_exists": path_exists,
            "branch_exists": false,
            "branch_commit": null,
            "rescue_commit": null,
            "archive": {"name": archive, "commit": HEAD},
        })
    };
    let root = w.root.to_string_lossy().into_owned();
    let payload =
        |facts: Value| json!({"name": "t9", "run_id": null, "root": root, "facts": facts});

    let path = w.root.join(".worktrees").join("t9");
    assert_eq!(
        w.refused(Kind::WorktreeRestore, payload(facts(true))),
        format!("{} exists already", path.display())
    );
    let mut none = facts(false);
    none["archive"] = Value::Null;
    assert_eq!(
        w.refused(Kind::WorktreeRestore, payload(none)),
        "no refs/horch/archive/t9 and no trash ref of t9"
    );
    assert_eq!(w.count("SELECT count(*) FROM jobs"), 0);

    let out = w.applied(Kind::WorktreeRestore, payload(facts(false)));
    let id = out["worktree_id"].as_str().unwrap();
    let row = trees::get(&w.conn, id).unwrap().unwrap();
    assert_eq!(row.state, WorktreeState::Reserved);
    assert_eq!(Path::new(&row.path), path);
    assert_eq!(row.branch, "restored/t9");
    assert_eq!(row.base_commit.as_deref(), Some(HEAD));
    assert_eq!(row.owner_attempt, None);
    let job = jobs_store::get(&w.conn, out["job"].as_i64().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!((job.kind, job.subject.as_str()), (JobKind::WorktreeAdd, id));
    assert_eq!(job.state, State::Queued);
    let reason: String = w
        .conn
        .query_row(
            "SELECT reason FROM worktrees WHERE worktree_id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(reason, format!("restored from {archive}"));
    assert_eq!(
        out["line"],
        format!(
            "restored {} on restored/t9 at {} from {archive}",
            path.display(),
            &HEAD[..7]
        )
    );
}

#[test]
fn worktree_triage_queues_its_jobs() {
    let mut w = world();
    let id = w.active_tree("t1");
    let row = trees::get(&w.conn, &id).unwrap().unwrap();
    let tr = triage_row(&id, &row.path, &row.branch, &w.run.run_id);
    let ticket = "ai_docs/triage/t1.md";
    std::fs::create_dir_all(w.root.join("ai_docs/triage")).unwrap();
    std::fs::write(w.root.join(ticket), "# Finish t1\n").unwrap();
    let sha = sha256_bytes(b"# Finish t1\n").to_string();
    let root = w.root.to_string_lossy().into_owned();
    let triage = |decision: &str, ticket_sha: &str| {
        json!({
            "root": root,
            "primary": "opus",
            "decisions": [{
                "row": tr,
                "decision": decision,
                "direction": "finish the cache",
                "line": 1,
                "ticket": {"path": ticket, "sha256": ticket_sha},
            }],
        })
    };

    let queued = w.applied(Kind::WorktreeTriage, triage("ARCHIVE", &sha));
    let job = queued["job"]
        .as_i64()
        .expect("the id of the triage_apply job");
    assert_eq!(
        queued,
        json!({
            "applied": [],
            "failed": 0,
            "job": job,
            "queued": [format!(
                "W1 _scratch/t1 ARCHIVE: queued in the triage_apply job {job}; \
                 horch worktree triage shows the result"
            )],
        })
    );
    let row_of_job = jobs_store::get(&w.conn, job).unwrap().unwrap();
    assert_eq!(row_of_job.kind, JobKind::TriageApply);
    assert_eq!(row_of_job.state, State::Queued);
    assert_eq!(row_of_job.subject, root);
    for d in ["DELETE", "LAND", "PR"] {
        assert_eq!(
            w.refused(Kind::WorktreeTriage, triage(d, &sha)),
            format!(
                "the triage_apply job {job} of {root} is queued; \
                 send the reply again after it ends"
            )
        );
    }
    let bad = w.refused(Kind::WorktreeTriage, triage("FINISH", "00"));
    assert!(bad.ends_with("differs from its sha256"), "{bad}");
    let tasks_before = w.count("SELECT count(*) FROM tasks");

    let out = w.applied(Kind::WorktreeTriage, triage("FINISH", &sha));
    assert_eq!(out["failed"], 0, "{out}");
    let a = &out["applied"][0];
    assert_eq!(a["ok"], true);
    let task_id = a["queued"]["task_id"].as_str().unwrap();
    assert_eq!(w.count("SELECT count(*) FROM tasks"), tasks_before + 1);
    let task = tasks::get(&w.conn, task_id).unwrap();
    assert_eq!(task.title, "Finish _scratch/t1: finish the cache");
    assert_eq!(a["queued"]["run_id"], w.run.run_id.as_str());
    let (decision, owner): (String, Option<String>) = w
        .conn
        .query_row(
            "SELECT decision, task_id FROM worktrees WHERE worktree_id = ?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(decision, "FINISH");
    assert_eq!(owner.as_deref(), Some(task_id));
    assert_eq!(
        a["text"],
        format!(
            "{} queued for opus with the brief {ticket}; it inherits {}",
            task.short(),
            row.path
        )
    );
}

/// `rescue_drop` (T-885) checks its payload as untrusted input and records
/// nothing that git does not confirm. The CLI test `p5_cli_writers` drops
/// a real ref.
#[test]
fn rescue_drop_refuses_a_payload_that_git_or_the_ledger_does_not_confirm() {
    let mut w = world();
    let good = |w: &World| {
        json!({
            "repo": w.root,
            "name": "refs/horch/rescue/branch/side/1",
            "tip": HEAD,
            "class": "unique",
            "bytes": 10,
        })
    };
    let events = |w: &World| w.count("SELECT count(*) FROM events");
    let before = events(&w);
    let mut p = good(&w);
    p["repo"] = json!("/not/a/project");
    assert!(w
        .refused(Kind::RescueDrop, p)
        .contains("is not the root of a project"));
    let mut p = good(&w);
    p["name"] = json!("refs/heads/main");
    assert!(w
        .refused(Kind::RescueDrop, p)
        .contains("is not a rescue ref"));
    let mut p = good(&w);
    p["class"] = json!("other");
    assert!(w
        .refused(Kind::RescueDrop, p)
        .contains("is not unique or superseded"));
    let mut p = good(&w);
    p["extra"] = json!(1);
    assert!(w
        .refused(Kind::RescueDrop, p)
        .contains("a bad rescue_drop payload"));
    // git has no such ref: the handler records nothing.
    let why = w.refused(Kind::RescueDrop, good(&w));
    assert!(why.contains("does not exist"), "{why}");
    assert_eq!(events(&w), before);
    assert_ne!(why, NOT_HANDLED);
}

#[test]
fn each_kind_twice_with_1_request_id_has_1_effect() {
    let mut w = world();
    let jobs = |w: &World| w.count("SELECT count(*) FROM jobs");

    let held = jobs_store::insert(
        &w.conn,
        &NewRun {
            kind: JobKind::Fetch,
            subject: "origin",
            repo: None,
            input_key: "k",
            run: 1,
            abandoned_runs: 3,
            not_before: None,
            time_limit_s: 60,
        },
    )
    .unwrap();
    w.conn
        .execute("UPDATE jobs SET state = 'abandoned' WHERE id = ?1", [held])
        .unwrap();
    let worker = w.worker();
    let tree = w.active_tree("t2");
    let branch = trees::get(&w.conn, &tree).unwrap().unwrap().branch;
    let root = w.root.to_string_lossy().into_owned();
    let tr = {
        let row = trees::get(&w.conn, &tree).unwrap().unwrap();
        let mut tr = triage_row(&tree, &row.path, &row.branch, &w.run.run_id);
        tr["name"] = json!("_scratch/t2");
        tr
    };
    let requests = [
        request(Kind::JobRequeue, json!({ "job": held })),
        request(Kind::WorktreeScratch, w.scratch_payload(&worker, "s1")),
        request(Kind::WorktreeDrop, w.drop_payload("t2", clean(&branch))),
        request(
            Kind::WorktreeRestore,
            json!({"name": "t9", "run_id": null, "root": root, "facts": {
                "path_exists": false, "branch_exists": false, "branch_commit": null,
                "rescue_commit": null, "archive": {"name": "refs/horch/archive/t9", "commit": HEAD},
            }}),
        ),
        request(
            Kind::WorktreeTriage,
            json!({"root": root, "primary": "opus", "decisions": [
                {"row": tr, "decision": "KEEP", "direction": null, "line": 1, "ticket": null},
            ]}),
        ),
    ];
    for req in &requests {
        let (jobs_0, events_0) = (jobs(&w), w.count("SELECT count(*) FROM events"));
        let first = w.send(req);
        assert!(
            matches!(first.outcome, Outcome::Applied(_)),
            "{:?}: {:?}",
            req.kind,
            first.outcome
        );
        let (jobs_1, events_1) = (jobs(&w), w.count("SELECT count(*) FROM events"));
        assert!(
            jobs_1 > jobs_0 || events_1 > events_0,
            "{:?} has an effect",
            req.kind
        );
        let second = w.send(req);
        assert_eq!(second, first, "{:?}: the stored reply", req.kind);
        assert_eq!(jobs(&w), jobs_1, "{:?}: no second job", req.kind);
        assert_eq!(
            w.count("SELECT count(*) FROM events"),
            events_1,
            "{:?}: no second event",
            req.kind
        );
    }
}
