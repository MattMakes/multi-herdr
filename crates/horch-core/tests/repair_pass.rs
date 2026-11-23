use horch_core::fleetd::fleet_runtime::adapters::sqlite::has_fact;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{record, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::application::dispatch::{bind, Bound, DispatchCtx};
use horch_core::fleetd::task_execution::domain::repair::{
    repair_pass, RepairFacts, RepairPort, Repaired, REPAIR_CAUSE, REPAIR_LINE,
};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, EventKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::Store;

struct World {
    _tmp: tempfile::TempDir,
    store: Store,
    bound: Bound,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
        let mut store = Store::open(&root.join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        ..Default::default()
                    },
                )?;
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        let mut task = NewTask::work("repair", "docs/repair.md", "opus");
        task.worktree_mode = WorktreeMode::Shared;
        let config = FleetConfig::default();
        let git = Git::new("git");
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            },
            clock: FleetSeconds(0),
            git: &git,
        };
        let bound = store
            .immediate(|tx| {
                let id = tasks::add(tx, &run, &task, Actor::Orchestrator)?.task_id;
                bind(tx, &ctx, &id, "opus", &NewWorker::default())
            })
            .unwrap();
        World {
            _tmp: tmp,
            store,
            bound,
        }
    }

    fn attempt(&mut self, facts: RepairFacts) -> Attempt<'_> {
        Attempt {
            store: &mut self.store,
            id: self.bound.attempt_id.clone(),
            facts,
            resumes: Vec::new(),
        }
    }

    fn repair_events(&self) -> i64 {
        self.store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM events
                 WHERE entity = 'attempt' AND entity_id = ?1 AND cause = ?2",
                [&self.bound.attempt_id, REPAIR_CAUSE],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn attempts(&self) -> (i64, i64, String) {
        let conn = self.store.conn();
        let task = tasks::get(conn, &self.bound.task_id).unwrap();
        let rows = conn
            .query_row(
                "SELECT COUNT(*) FROM attempts WHERE task_id = ?1",
                [&self.bound.task_id],
                |r| r.get(0),
            )
            .unwrap();
        let state = conn
            .query_row(
                "SELECT state FROM attempts WHERE id = ?1",
                [&self.bound.attempt_id],
                |r| r.get(0),
            )
            .unwrap();
        (task.attempts, rows, state)
    }
}

struct Attempt<'a> {
    store: &'a mut Store,
    id: String,
    facts: RepairFacts,
    resumes: Vec<String>,
}

impl RepairPort for Attempt<'_> {
    type Error = anyhow::Error;

    fn facts(&mut self) -> anyhow::Result<RepairFacts> {
        let repaired = has_fact(
            self.store.conn(),
            Entity::Attempt,
            &self.id,
            EventKind::Note,
            REPAIR_CAUSE,
        )?;
        Ok(RepairFacts {
            repaired,
            ..self.facts
        })
    }

    fn record(&mut self) -> anyhow::Result<()> {
        let id = self.id.clone();
        self.store.immediate(|tx| {
            let cause = Cause {
                actor: Actor::Supervisor,
                text: REPAIR_CAUSE,
            };
            let data = serde_json::json!({ "line": REPAIR_LINE });
            Ok(record(
                tx,
                None,
                EventKind::Note,
                Entity::Attempt,
                &id,
                cause,
                &data,
            )?)
        })
    }

    fn resume(&mut self, line: &str) -> anyhow::Result<Option<i32>> {
        self.resumes.push(line.to_string());
        Ok(Some(0))
    }
}

fn exited_with_commits() -> RepairFacts {
    RepairFacts {
        exited: true,
        commits_since_base: true,
        ..RepairFacts::default()
    }
}

fn pass(w: &mut World, facts: RepairFacts) -> (Repaired, Vec<String>) {
    let mut attempt = w.attempt(facts);
    let done = repair_pass(&mut attempt).unwrap();
    (done, attempt.resumes)
}

#[test]
fn exit_with_commits_and_no_result_runs_1_repair() {
    let mut w = World::new();
    let (done, resumes) = pass(&mut w, exited_with_commits());
    assert_eq!(done, Repaired::Resumed(Some(0)));
    assert_eq!(resumes, [REPAIR_LINE]);
    assert_eq!(
        REPAIR_LINE,
        "Write the result of your task now: run `horch result`."
    );
    assert_eq!(w.repair_events(), 1);
}

#[test]
fn exit_with_a_result_runs_no_repair() {
    let mut w = World::new();
    let facts = RepairFacts {
        result_json: true,
        ..exited_with_commits()
    };
    assert_eq!(pass(&mut w, facts), (Repaired::No, Vec::new()));
    assert_eq!(w.repair_events(), 0);
}

#[test]
fn exit_with_a_question_file_runs_no_repair() {
    let mut w = World::new();
    let facts = RepairFacts {
        question_file: true,
        ..exited_with_commits()
    };
    assert_eq!(pass(&mut w, facts), (Repaired::No, Vec::new()));
    assert_eq!(w.repair_events(), 0);
}

#[test]
fn exit_with_no_commits_runs_no_repair() {
    let mut w = World::new();
    let facts = RepairFacts {
        commits_since_base: false,
        ..exited_with_commits()
    };
    assert_eq!(pass(&mut w, facts), (Repaired::No, Vec::new()));
    assert_eq!(w.repair_events(), 0);
}

#[test]
fn second_repair_of_1_attempt_never_runs() {
    let mut w = World::new();
    assert_eq!(pass(&mut w, exited_with_commits()).1, [REPAIR_LINE]);
    assert_eq!(
        pass(&mut w, exited_with_commits()),
        (Repaired::No, Vec::new())
    );
    assert_eq!(w.repair_events(), 1);
}

#[test]
fn repair_burns_no_attempt() {
    let mut w = World::new();
    let before = w.attempts();
    assert_eq!(before.0, 1, "the bind made attempt 1");
    let status = tasks::get(w.store.conn(), &w.bound.task_id).unwrap().status;
    pass(&mut w, exited_with_commits());
    assert_eq!(w.attempts(), before, "no attempt is added or ended");
    assert_eq!(
        tasks::get(w.store.conn(), &w.bound.task_id).unwrap().status,
        status,
        "the task does not move"
    );
}
