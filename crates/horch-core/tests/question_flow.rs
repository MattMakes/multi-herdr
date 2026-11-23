use std::collections::BTreeMap;
use std::path::PathBuf;

use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::budget::PassBudget;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::adapters::questions_store;
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::ideas::application::question_flow::{
    self, needs_you_key, owner_of, Flow, Flowed, Ring,
};
use horch_core::fleetd::ideas::application::questions::{self, Ask};
use horch_core::fleetd::ideas::application::sub_orchestrators::{
    keep_sub_orchestrators, Keep, Kept,
};
use horch_core::fleetd::ideas::domain::questions::{Question, State};
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::task_execution::application::commands;
use horch_core::fleetd::task_execution::domain::task::{Facts, InPane, Recorded, Seen};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::Store;
use rusqlite::{params, Connection};

const FEATURE: &str = "F-0001";
const OWNER_1: &str = "I-0001/supervising/1";
const OWNER_2: &str = "I-0001/supervising/2";
const PANE: &str = "w1:p1";
const P1: Recorded = Recorded {
    pid: 4101,
    pid_start: Some(11),
};
const P2: Recorded = Recorded {
    pid: 4102,
    pid_start: Some(12),
};
const LIMIT_S: u64 = 900;
const ASKED_AT: i64 = 1000;

struct World {
    dir: tempfile::TempDir,
    store: Store,
    task: String,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Operator,
        text,
    }
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, 1, 1, 'then', ?1, 'then')",
            [ASKED_AT],
        )
        .unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = store
        .immediate(|tx| {
            let run = runs::create(tx, dir.path(), &new_run)?;
            horch_core::taskledger::schema::run_idea(
                tx,
                &run.run_id,
                &horch_core::taskledger::util::now(),
                true,
            )?;
            Ok(run)
        })
        .unwrap();
    let task = store
        .immediate(|tx| {
            let t = NewTask::work("a", "tickets/a.md", "dev");
            let id = tasks::add(tx, &run, &t, Actor::Orchestrator)?.task_id;
            for to in ["running", "blocked_on_user"] {
                transition(
                    tx,
                    None,
                    Entity::Task,
                    &id,
                    to,
                    cause("test"),
                    &NeedsYouList,
                )?;
            }
            Ok(id)
        })
        .unwrap();
    World { dir, store, task }
}

fn facts(alive: &[Recorded], dead: &[Recorded]) -> Facts {
    let mut processes = BTreeMap::new();
    let mut panes = BTreeMap::new();
    for p in alive {
        processes.insert(*p, Seen::Alive);
        let runs = InPane::Runs {
            pid: p.pid,
            pid_start: p.pid_start.unwrap(),
        };
        panes.insert(PANE.to_string(), runs);
    }
    for p in dead {
        processes.insert(*p, Seen::Dead);
        panes.entry(PANE.to_string()).or_insert(InPane::Gone(p.pid));
    }
    Facts {
        processes,
        panes: Some(panes),
        stopped: Vec::new(),
    }
}

impl World {
    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    fn ideas(&self) -> PathBuf {
        self.dir.path().join("ideas")
    }

    fn ask(&self, mail: &str, clock: i64) -> Question {
        let a = Ask {
            task_id: &self.task,
            attempt_id: &format!("{}/A1", self.task),
            mail_id: Some(mail),
            text: "QUESTION: which store?",
            asked_at: "2026-11-18T00:00:00Z",
        };
        questions::ask(self.conn(), &a, FleetSeconds(clock), LIMIT_S).unwrap()
    }

    fn flow(&mut self, clock: i64, elapsed_s: i64) -> Flowed {
        let ctx = Flow {
            guard: None,
            clock: FleetSeconds(clock),
            elapsed_s,
            stamp: "2026-11-18T00:00:00Z",
            budget: PassBudget::new(100),
        };
        self.store
            .immediate(|tx| question_flow::run(tx, &ctx))
            .unwrap()
    }

    fn keep(&mut self, facts: &Facts) -> Kept {
        let ideas = self.ideas();
        let ctx = Keep {
            guard: None,
            ideas: &ideas,
            facts,
            stamp: "2026-11-18T00:00:00Z",
            budget: PassBudget::new(100),
        };
        self.store
            .immediate(|tx| keep_sub_orchestrators(tx, &ctx))
            .unwrap()
    }

    fn launched(&mut self, id: &str, p: Recorded) {
        self.conn()
            .execute(
                "UPDATE attempts SET mux_handle = ?2 WHERE id = ?1",
                params![id, PANE],
            )
            .unwrap();
        self.store
            .immediate(|tx| commands::attempt_live(tx, None, id, Some(p)))
            .unwrap();
    }

    fn owner_runs(&mut self, id: &str, p: Recorded) {
        let k = self.keep(&facts(&[], &[]));
        let started: Vec<&str> = k.started.iter().map(|s| s.attempt.as_str()).collect();
        assert_eq!(started, vec![id], "fleetd starts the owner of the question");
        self.launched(id, p);
    }

    fn question(&self, id: &str) -> Question {
        questions_store::get(self.conn(), id).unwrap().unwrap()
    }

    fn task_status(&self) -> String {
        tasks::get(self.conn(), &self.task).unwrap().status
    }

    fn doorbells(&self, attempt: &str) -> i64 {
        self.conn()
            .query_row(
                "SELECT count(*) FROM events WHERE kind = 'doorbell' AND entity = 'attempt'
                   AND entity_id = ?1",
                [attempt],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn items(&self, id: &str) -> Vec<(String, String)> {
        let mut stmt = self
            .conn()
            .prepare("SELECT state, text FROM needs_you WHERE key = ?1")
            .unwrap();
        stmt.query_map([needs_you_key(id)], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }
}

fn rung(f: &Flowed) -> Vec<(&str, &str, Vec<&str>)> {
    f.rings
        .iter()
        .map(|r: &Ring| {
            let qs = r.questions.iter().map(String::as_str).collect();
            (r.attempt.as_str(), r.pane.as_str(), qs)
        })
        .collect()
}

#[test]
fn owner_is_the_sub_orchestrator_of_the_feature() {
    let mut w = world();
    let q = w.ask("m1", ASKED_AT);
    assert_eq!(
        q.feature_id, FEATURE,
        "the feature of the task, not its run"
    );
    assert_eq!(q.owner, "idea-lead:F-0001");
    assert_eq!(q.owner, owner_of(FEATURE));
    assert_eq!(q.due_at, ASKED_AT + 900);

    let second = w
        .store
        .immediate(|tx| {
            let id = runs::create_feature_run(tx, std::path::Path::new("/tmp"), "main")?;
            let new = NewFeature {
                id: &id,
                name: None,
                branch: None,
                primary_branch: "main",
            };
            rows::insert_feature(tx, None, &new, cause("feature opened"))?;
            tx.execute(
                "UPDATE ideas SET feature_id = ?1 WHERE id = 'I-0001'",
                [&id],
            )?;
            Ok(id)
        })
        .unwrap();
    let f = w.flow(ASKED_AT + 5, 5);
    assert_eq!(f.moved, vec![q.id.clone()]);
    let moved = w.question(&q.id);
    assert_eq!(moved.feature_id, second);
    assert_eq!(moved.owner, owner_of(&second));
    assert_eq!(moved.state, State::Open);
    let causes: Vec<String> = {
        let mut stmt = w
            .conn()
            .prepare(
                "SELECT cause FROM events WHERE kind = 'question' AND entity = 'task'
                   AND entity_id = ?1 AND cause LIKE '%owner%'",
            )
            .unwrap();
        stmt.query_map([&w.task], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    assert_eq!(causes.len(), 1, "1 event records the change of owner");
    assert!(
        causes[0].contains("idea-lead:F-0001") && causes[0].contains(&owner_of(&second)),
        "the cause names both owners: {}",
        causes[0]
    );
    assert_eq!(w.flow(ASKED_AT + 10, 5).moved, Vec::<String>::new());
}

#[test]
fn question_whose_owner_is_not_running_is_answered_after_fleetd_starts_it() {
    let mut w = world();
    let q = w.ask("m1", ASKED_AT);
    let f = w.flow(ASKED_AT + 5, 5);
    assert_eq!(f.rings, vec![]);
    assert_eq!(f.escalated, Vec::<String>::new());

    w.owner_runs(OWNER_1, P1);
    let f = w.flow(ASKED_AT + 10, 5);
    assert_eq!(rung(&f), vec![(OWNER_1, PANE, vec![q.id.as_str()])]);

    questions::answered(w.conn(), &q, Some("use SQLite")).unwrap();
    assert_eq!(w.question(&q.id).state, State::Answered);
    assert_eq!(w.task_status(), "ready");
    let f = w.flow(ASKED_AT + 15, 5);
    assert_eq!(f, Flowed::default(), "an answered question needs nothing");
}

#[test]
fn new_question_rings_the_owner_once() {
    let mut w = world();
    let q1 = w.ask("m1", ASKED_AT);
    w.owner_runs(OWNER_1, P1);
    let f = w.flow(ASKED_AT + 5, 5);
    assert_eq!(rung(&f), vec![(OWNER_1, PANE, vec![q1.id.as_str()])]);
    assert!(f.rings[0].event > 0, "the ring is its event row");
    assert_eq!(w.doorbells(OWNER_1), 1);

    assert_eq!(w.flow(ASKED_AT + 10, 5).rings, vec![]);
    assert_eq!(w.flow(ASKED_AT + 15, 5).rings, vec![]);
    assert_eq!(w.doorbells(OWNER_1), 1);

    let q2 = w.ask("m2", ASKED_AT + 15);
    let f = w.flow(ASKED_AT + 20, 5);
    assert_eq!(rung(&f), vec![(OWNER_1, PANE, vec![q2.id.as_str()])]);
    assert_eq!(w.flow(ASKED_AT + 25, 5).rings, vec![]);

    let k = w.keep(&facts(&[], &[P1]));
    let started: Vec<&str> = k.started.iter().map(|s| s.attempt.as_str()).collect();
    assert_eq!(started, vec![OWNER_2]);
    w.launched(OWNER_2, P2);
    let f = w.flow(ASKED_AT + 30, 5);
    assert_eq!(
        rung(&f),
        vec![(OWNER_2, PANE, vec![q1.id.as_str(), q2.id.as_str()])]
    );
    assert_eq!(w.flow(ASKED_AT + 35, 5).rings, vec![]);
    assert_eq!(w.doorbells(OWNER_2), 1);
}

#[test]
fn question_escalates_after_15_min_and_operator_wait_has_no_limit() {
    let mut w = world();
    let q = w.ask("m1", ASKED_AT);
    w.owner_runs(OWNER_1, P1);
    let f = w.flow(ASKED_AT + 899, 899);
    assert_eq!(
        f.escalated,
        Vec::<String>::new(),
        "899 s is inside the limit"
    );
    assert_eq!(w.question(&q.id).state, State::Open);

    let f = w.flow(ASKED_AT + 900, 1);
    assert_eq!(f.escalated, vec![q.id.clone()]);
    assert_eq!(w.question(&q.id).state, State::Escalated);
    let items = w.items(&q.id);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].0, "open");
    assert!(
        items[0].1.contains(&q.id) && items[0].1.contains("which store?"),
        "the item names the question and holds its text: {}",
        items[0].1
    );
    assert_eq!(w.task_status(), "blocked_on_user");

    for clock in [ASKED_AT + 1_000, ASKED_AT + 86_400, ASKED_AT + 3_000_000] {
        let f = w.flow(clock, 10);
        assert_eq!(f, Flowed::default(), "at {clock}");
    }
    assert_eq!(w.question(&q.id).state, State::Escalated);
    assert_eq!(w.task_status(), "blocked_on_user");
    assert_eq!(w.items(&q.id).len(), 1);
    assert_eq!(needs_you::open_keys(w.conn(), "question").unwrap().len(), 1);

    let id = questions::answer_mail(w.conn(), "m1", "use SQLite").unwrap();
    assert_eq!(id.as_deref(), Some(q.id.as_str()));
    assert_eq!(w.question(&q.id).state, State::Answered);
    assert_eq!(w.task_status(), "ready");
    assert_eq!(w.items(&q.id)[0].0, "resolved");
}

#[test]
fn limit_counts_only_while_the_owner_runs() {
    let mut w = world();
    let q = w.ask("m1", ASKED_AT);
    for clock in [ASKED_AT + 500, ASKED_AT + 1_000] {
        let f = w.flow(clock, 500);
        assert_eq!(f.escalated, Vec::<String>::new(), "at {clock}");
    }
    assert_eq!(w.question(&q.id).due_at, ASKED_AT + 1_000 + 900);

    w.owner_runs(OWNER_1, P1);
    let f = w.flow(ASKED_AT + 1_899, 899);
    assert_eq!(f.escalated, Vec::<String>::new());
    assert_eq!(w.question(&q.id).state, State::Open);

    w.keep(&facts(&[], &[P1]));
    let f = w.flow(ASKED_AT + 2_199, 300);
    assert_eq!(f.escalated, Vec::<String>::new());
    assert_eq!(w.question(&q.id).due_at, ASKED_AT + 2_200);

    w.launched(OWNER_2, P2);
    let f = w.flow(ASKED_AT + 2_200, 1);
    assert_eq!(f.escalated, vec![q.id.clone()]);
    assert_eq!(w.question(&q.id).state, State::Escalated);
}
