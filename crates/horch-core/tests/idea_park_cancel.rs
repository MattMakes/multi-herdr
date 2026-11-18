//! `horch idea park` and `horch idea cancel` (docs/specs/fleetd/design.md,
//! "The idea state machine", rows `parked` and `cancelled`; "The feature
//! state machine", row `abandoned`; "A cancel"; FM-39, A34;
//! docs/tickets/fleetd-p6/20-park-cancel.md).
//!
//! Each test makes its own temporary ledger. No test reads the clock, the
//! load or the limits of the machine: the fleet clock and the facts of a
//! pass are given. The only process is a child `sleep`, the agent of 1
//! attempt.

use std::process::{Child, Command};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::ideas::application::park_cancel::{self, IdeaCancel};
use horch_core::fleetd::task_execution::application::cancel;
use horch_core::fleetd::task_execution::application::dispatch::{choices, CheckSandboxes};
use horch_core::fleetd::task_execution::domain::dispatch::{Choice, Hold, Holds};
use horch_core::fleetd::task_execution::domain::task::{Facts, Recorded, Seen};
use horch_core::fleetd::worktree_custody::adapters::rescue_git::rescue_key;
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, NewWorktree};
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, ClaimOutcome, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;
use rusqlite::{params, Connection};

/// The idea and the feature of the first run of a ledger.
const IDEA: &str = "I-0001";
const FEATURE: &str = "F-0001";

/// A child `sleep`: the agent process of an attempt. It is killed and
/// reaped when the test ends.
struct Sleep(Child);

impl Sleep {
    fn start() -> Sleep {
        Sleep(Command::new("sleep").arg("60").spawn().unwrap())
    }

    fn recorded(&self) -> Recorded {
        let pid = self.0.id();
        Recorded {
            pid,
            pid_start: horch_core::procid::start_time(pid),
        }
    }

    /// The stop: kill and reap it.
    fn stop(&mut self) {
        self.0.kill().unwrap();
        self.0.wait().unwrap();
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Each persona has a check sandbox.
struct AllSandboxes;

impl CheckSandboxes for AllSandboxes {
    fn supported(&self, _persona: &str) -> bool {
        true
    }
}

struct World {
    dir: tempfile::TempDir,
    store: Store,
    config: FleetConfig,
    run: Run,
}

/// A temp ledger with 1 run. Its first task gives the run the idea
/// [`IDEA`] of the feature [`FEATURE`], in `in_progress`.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = store
        .immediate(|tx| runs::create(tx, dir.path(), &new_run))
        .unwrap();
    World {
        dir,
        store,
        config: FleetConfig::default(),
        run,
    }
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Operator,
        text,
    }
}

fn state_of(conn: &Connection, table: &str, key: &str, id: &str) -> String {
    let column = if table == "tasks" { "status" } else { "state" };
    conn.query_row(
        &format!("SELECT {column} FROM {table} WHERE {key} = ?1"),
        [id],
        |r| r.get(0),
    )
    .unwrap()
}

impl World {
    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    /// A new ready task of `dev` with `title`. Its id.
    fn task(&mut self, title: &str) -> String {
        let new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
        let run = self.run.clone();
        self.store
            .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id
    }

    fn set_task_status(&self, task: &str, word: &str) {
        self.conn()
            .execute(
                "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
                params![task, word],
            )
            .unwrap();
    }

    fn task_status(&self, task: &str) -> String {
        state_of(self.conn(), "tasks", "task_id", task)
    }

    fn idea_state(&self, idea: &str) -> String {
        state_of(self.conn(), "ideas", "id", idea)
    }

    fn tree_state(&self, id: &str) -> String {
        state_of(self.conn(), "worktrees", "worktree_id", id)
    }

    /// A worker of `dev` whose agent is `agent` claims the next ready task:
    /// 1 live attempt. The id of the worker.
    fn claimed(&mut self, agent: Recorded) -> String {
        let new = NewWorker {
            run_id: self.run.run_id.clone(),
            persona: "dev".into(),
            pane_id: Some("w0:p1".into()),
            ..Default::default()
        };
        let w = self
            .store
            .immediate(|tx| {
                let id = workers::insert(tx, &new)?;
                workers::agent_started(tx, &id, agent.pid, agent.pid_start)?;
                Ok(id)
            })
            .unwrap();
        let out = self.store.immediate(|tx| tasks::claim(tx, &w)).unwrap();
        assert!(matches!(out, ClaimOutcome::Claimed(_)));
        w
    }

    /// A worktree row `id` of `task` in `state`, through the door, with
    /// `head` as the HEAD that git gave.
    fn tree(&mut self, id: &str, task: &str, state: &str, head: &str) {
        let root = self.dir.path().display().to_string();
        let new = NewWorktree {
            worktree_id: id.to_string(),
            task_id: Some(task.to_string()),
            worker_id: None,
            repo_root: root.clone(),
            path: format!("{root}/.worktrees/{id}"),
            branch: format!("fleet/r1/{id}"),
            base: "main".into(),
            base_commit: None,
            status: WorktreeStatus::Open,
            mode: None,
        };
        let path: &[&str] = match state {
            "created" => &["created"],
            "active" => &["created", "active"],
            "orphaned" => &[],
            other => panic!("no path to {other}"),
        };
        self.store
            .immediate(|tx| {
                if state == "orphaned" {
                    trees::insert_orphan(tx, None, &new, cause("a test tree"))?;
                } else {
                    trees::insert_reserved(tx, None, &new, cause("a test tree"))?;
                }
                for to in path {
                    transition(
                        tx,
                        None,
                        Entity::Worktree,
                        id,
                        to,
                        cause("a test tree"),
                        &NeedsYouList,
                    )?;
                }
                trees::set_facts(tx, id, head, false, None, "2026-01-01T00:00:00Z")?;
                Ok(())
            })
            .unwrap();
    }

    /// A new idea `id` of [`FEATURE`], moved through the door along `path`.
    fn idea(&mut self, id: &str, path: &[&str]) {
        self.store
            .immediate(|tx| {
                let new = NewIdea {
                    id,
                    feature_id: FEATURE,
                    priority: 0,
                };
                ideas_store::insert_idea(tx, None, &new, cause("a test idea"))?;
                for to in path {
                    transition(
                        tx,
                        None,
                        Entity::Idea,
                        id,
                        to,
                        cause("a test idea"),
                        &NeedsYouList,
                    )?;
                }
                Ok(())
            })
            .unwrap();
    }

    fn cancel(&mut self, idea: &str, reason: &str) -> IdeaCancel {
        let config = self.config.clone();
        self.store
            .immediate(|tx| park_cancel::cancel(&ctx(&config), tx, idea, reason))
            .unwrap()
    }

    fn park(&mut self, idea: &str, reason: &str) -> anyhow::Result<Vec<String>> {
        let config = self.config.clone();
        self.store
            .immediate(|tx| park_cancel::park(&ctx(&config), tx, idea, reason, FleetSeconds(100)))
    }

    fn complete_idea_cancels(&mut self) -> Vec<(String, IdeaCancel)> {
        let config = self.config.clone();
        self.store
            .immediate(|tx| park_cancel::complete_cancels(&ctx(&config), tx))
            .unwrap()
    }

    fn feature_abandoned(&mut self, feature: &str) -> Vec<(String, IdeaCancel)> {
        let config = self.config.clone();
        self.store
            .immediate(|tx| park_cancel::on_feature_abandoned(&ctx(&config), tx, feature))
            .unwrap()
    }

    /// The `rescue` runs of the tree `id`.
    fn rescues(&self, id: &str) -> Vec<(State, String)> {
        let mut out = Vec::new();
        if let Some(job) = jobs_store::active(self.conn(), Kind::Rescue, id).unwrap() {
            out.push((job.state, job.input_key));
        }
        out
    }
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

/// What a pass saw of `p`.
fn facts(p: Recorded, seen: Seen) -> Facts {
    let mut f = Facts::default();
    f.processes.insert(p, seen);
    f
}

#[test]
fn cancel_of_an_idea_cancels_each_task_that_is_not_terminal() {
    let mut w = world();
    let done = w.task("done");
    let ready = w.task("ready");
    let escalated = w.task("escalated");
    let failed = w.task("failed");
    w.set_task_status(&done, "done");
    w.set_task_status(&escalated, "escalated");
    w.set_task_status(&failed, "failed");
    assert_eq!(w.idea_state(IDEA), "in_progress");

    let out = w.cancel(IDEA, "the operator drops it");

    assert_eq!(
        out,
        IdeaCancel {
            cancelled: true,
            stop: vec![]
        }
    );
    assert_eq!(w.task_status(&done), "done", "a terminal task stays");
    for t in [&ready, &escalated, &failed] {
        assert_eq!(w.task_status(t), "cancelled", "{t}");
    }
    assert_eq!(w.idea_state(IDEA), "cancelled");

    // A second cancel of a cancelled idea changes nothing.
    assert_eq!(w.cancel(IDEA, "again"), out);
    assert_eq!(w.idea_state(IDEA), "cancelled");
}

#[test]
fn cancel_stops_a_live_attempt_before_the_task_is_cancelled() {
    let mut w = world();
    let running = w.task("running");
    let ready = w.task("ready");
    let mut agent = Sleep::start();
    let p = agent.recorded();
    let worker = w.claimed(p);
    assert_eq!(w.task_status(&running), "running");

    let out = w.cancel(IDEA, "the operator drops it");

    assert_eq!(
        out,
        IdeaCancel {
            cancelled: false,
            stop: vec![worker.clone()]
        },
        "the caller stops the worker of the live attempt"
    );
    assert_eq!(w.task_status(&ready), "cancelled");
    assert_eq!(
        w.task_status(&running),
        "running",
        "no move before the stop"
    );
    assert_eq!(w.idea_state(IDEA), "in_progress", "a task is not terminal");

    // The process still runs: nothing moves.
    let alive = facts(p, Seen::Alive);
    let moved = w
        .store
        .immediate(|tx| cancel::complete_cancels(tx, &alive))
        .unwrap();
    assert!(moved.is_empty());
    let pending = w.complete_idea_cancels();
    assert_eq!(
        pending,
        vec![(
            IDEA.to_string(),
            IdeaCancel {
                cancelled: false,
                stop: vec![worker]
            }
        )]
    );
    assert_eq!(w.idea_state(IDEA), "in_progress");

    // The stop, then the task moves, then the idea.
    agent.stop();
    let dead = facts(p, Seen::Dead);
    let moved = w
        .store
        .immediate(|tx| cancel::complete_cancels(tx, &dead))
        .unwrap();
    assert_eq!(moved, vec![running.clone()]);
    assert_eq!(w.task_status(&running), "cancelled");
    let done = w.complete_idea_cancels();
    assert_eq!(
        done,
        vec![(
            IDEA.to_string(),
            IdeaCancel {
                cancelled: true,
                stop: vec![]
            }
        )]
    );
    assert_eq!(w.idea_state(IDEA), "cancelled");
    assert!(w.complete_idea_cancels().is_empty(), "no cancel waits");
}

#[test]
fn abandoned_feature_cancels_each_idea_that_is_not_merged() {
    let mut w = world();
    let task = w.task("a");
    w.idea("I-0100", &[]);
    w.idea(
        "I-0101",
        &["ticketing", "in_progress", "integrating", "merged"],
    );
    w.idea("I-0102", &["parked"]);
    w.store
        .immediate(|tx| {
            transition(
                tx,
                None,
                Entity::Feature,
                FEATURE,
                "abandoned",
                cause("horch feature abandon"),
                &NeedsYouList,
            )?;
            Ok(())
        })
        .unwrap();

    let out = w.feature_abandoned(FEATURE);

    let ids: Vec<&str> = out.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["I-0001", "I-0100", "I-0102"]);
    assert!(out.iter().all(|(_, c)| c.cancelled));
    assert_eq!(w.idea_state(IDEA), "cancelled");
    assert_eq!(w.task_status(&task), "cancelled");
    assert_eq!(w.idea_state("I-0100"), "cancelled");
    assert_eq!(w.idea_state("I-0102"), "cancelled");
    assert_eq!(w.idea_state("I-0101"), "merged", "a merged idea stays");

    // The event again: the same effect as 1 time.
    assert!(w.feature_abandoned(FEATURE).is_empty());
    assert_eq!(w.idea_state("I-0101"), "merged");
}

#[test]
fn park_keeps_running_tasks_and_stops_dispatch() {
    let mut w = world();
    let running = w.task("running");
    let agent = Sleep::start();
    let worker = w.claimed(agent.recorded());
    let ready = w.task("ready");
    assert_eq!(w.task_status(&running), "running");

    w.park(IDEA, "the operator waits for a decision").unwrap();

    assert_eq!(w.idea_state(IDEA), "parked");
    assert_eq!(w.task_status(&running), "running", "a running task goes on");
    let live = workers::get(w.conn(), &worker).unwrap();
    assert_eq!(live.status, "working");
    assert_eq!(w.task_status(&ready), "ready");
    let held = choices(w.conn(), &Holds::default(), &AllSandboxes, None).unwrap();
    assert_eq!(
        held,
        vec![Choice::Held {
            task_id: ready.clone(),
            hold: Hold::IdeaParked {
                idea: IDEA.to_string()
            },
        }],
        "no new task of a parked idea is dispatched"
    );

    // A park of a parked idea is refused; the idea stays parked.
    let err = w.park(IDEA, "again").unwrap_err();
    assert!(
        format!("{err:#}").contains("horch idea approve"),
        "the refusal names the resume: {err:#}"
    );
    assert_eq!(w.idea_state(IDEA), "parked");
}

#[test]
fn park_rescues_each_worktree_that_is_not_active() {
    let mut w = world();
    let a = w.task("a");
    let b = w.task("b");
    let c = w.task("c");
    w.tree("wt-active", &a, "active", &"a".repeat(40));
    w.tree("wt-created", &b, "created", &"b".repeat(40));
    w.tree("wt-orphan", &c, "orphaned", &"c".repeat(40));

    let rescued = w.park(IDEA, "the operator waits").unwrap();

    assert_eq!(rescued, ["wt-created", "wt-orphan"]);
    assert_eq!(w.idea_state(IDEA), "parked");
    assert_eq!(
        w.tree_state("wt-active"),
        "active",
        "an active tree works on"
    );
    assert!(w.rescues("wt-active").is_empty());
    assert_eq!(
        w.tree_state("wt-created"),
        "orphaned",
        "its rescue comes first"
    );
    for (id, head) in [("wt-created", "b"), ("wt-orphan", "c")] {
        let path = format!("{}/.worktrees/{id}", w.dir.path().display());
        assert_eq!(
            w.rescues(id),
            [(State::Queued, rescue_key(&path, &head.repeat(40)))],
            "{id}"
        );
    }
}
