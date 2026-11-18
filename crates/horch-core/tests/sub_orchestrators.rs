//! The sub-orchestrators: 1 for each feature that is not merged or
//! abandoned, a fresh process for each phase, started when missing and
//! stopped when extra (docs/specs/fleetd/design.md, "Ideas: the dispatcher
//! and sub-orchestrators": How many sub-orchestrators, A fresh process for
//! every phase; I7, I13, FM-16, FM-31; docs/tickets/fleetd-p6/28-sub-orch.md).
//!
//! Each test makes its own temporary ledger. No test reads the clock, the
//! load or the limits of the machine, and no test starts a process: the
//! facts of a pass (the processes and the herdr panes) are given.

use std::collections::BTreeMap;
use std::path::PathBuf;

use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::budget::PassBudget;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::adapters::questions_store;
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::ideas::application::sub_orchestrators::{
    check_i13, check_i7, keep_sub_orchestrators, Breach, Ended, Keep, Kept, IDLE_STOP, PERSONA,
};
use horch_core::fleetd::ideas::domain::questions::{Question, State};
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::task_execution::application::commands;
use horch_core::fleetd::task_execution::domain::task::{Facts, InPane, Recorded, Seen, Target};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::Store;
use rusqlite::{params, Connection};

/// The feature and the idea that the first task of the run gets.
const FEATURE: &str = "F-0001";
const FIRST: &str = "I-0001";
/// A second idea of [`FEATURE`].
const SECOND: &str = "I-0002";
/// The pane of the sub-orchestrator of [`FEATURE`], as herdr names it.
const PANE: &str = "w1:p1";
/// The process that the launch of the first attempt recorded.
const P1: Recorded = Recorded {
    pid: 4101,
    pid_start: Some(11),
};
/// The process of a second attempt.
const P2: Recorded = Recorded {
    pid: 4102,
    pid_start: Some(12),
};

struct World {
    dir: tempfile::TempDir,
    store: Store,
    /// The 1 task of the run, of the idea [`FIRST`].
    task: String,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Operator,
        text,
    }
}

/// A temp ledger with 1 run and its 1 task: the feature [`FEATURE`] with
/// the idea [`FIRST`] in `in_progress`.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, 1, 1, 'then', 1000, 'then')",
            [],
        )
        .unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = store
        .immediate(|tx| runs::create(tx, dir.path(), &new_run))
        .unwrap();
    let task = store
        .immediate(|tx| {
            let t = NewTask::work("a", "tickets/a.md", "dev");
            Ok(tasks::add(tx, &run, &t, Actor::Orchestrator)?.task_id)
        })
        .unwrap();
    World { dir, store, task }
}

/// The facts of a pass: each process of `alive` alive, each of `dead`
/// dead, and the herdr panes `panes`.
fn facts(alive: &[Recorded], dead: &[Recorded], panes: &[(&str, InPane)]) -> Facts {
    let mut processes = BTreeMap::new();
    for p in alive {
        processes.insert(*p, Seen::Alive);
    }
    for p in dead {
        processes.insert(*p, Seen::Dead);
    }
    Facts {
        processes,
        panes: Some(panes.iter().map(|(k, v)| (k.to_string(), *v)).collect()),
        stopped: Vec::new(),
    }
}

/// `p` runs in a pane.
fn runs_in(p: Recorded) -> InPane {
    InPane::Runs {
        pid: p.pid,
        pid_start: p.pid_start.unwrap(),
    }
}

impl World {
    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    fn ideas(&self) -> PathBuf {
        self.dir.path().join("ideas")
    }

    /// The pass of the use case with the facts `facts`.
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

    /// A new idea `id` of `feature`, moved through `path` from `captured`.
    fn idea(&mut self, id: &str, feature: &str, path: &[&str]) {
        let new = NewIdea {
            id,
            feature_id: feature,
            priority: 0,
        };
        self.store
            .immediate(|tx| ideas_store::insert_idea(tx, None, &new, cause("idea added")))
            .unwrap();
        for to in path {
            self.move_idea(id, to);
        }
    }

    fn move_idea(&mut self, id: &str, to: &str) {
        self.store
            .immediate(|tx| {
                transition(tx, None, Entity::Idea, id, to, cause("test"), &NeedsYouList)?;
                Ok(())
            })
            .unwrap();
    }

    fn move_feature(&mut self, id: &str, to: &str) {
        self.store
            .immediate(|tx| {
                transition(
                    tx,
                    None,
                    Entity::Feature,
                    id,
                    to,
                    cause("test"),
                    &NeedsYouList,
                )?;
                Ok(())
            })
            .unwrap();
    }

    /// An open worker question on the task of the run.
    fn question(&self) {
        let q = Question {
            id: format!("{}/A1/Q1", self.task),
            task_id: self.task.clone(),
            feature_id: FEATURE.into(),
            owner: format!("idea-lead:{FEATURE}"),
            state: State::Open,
            due_at: 1900,
            asked_at: "then".into(),
            answered_at: None,
        };
        questions_store::insert(self.conn(), &q).unwrap();
    }

    /// The launch of the attempt `id` is seen: its process `p` in `pane`.
    fn launched(&mut self, id: &str, p: Recorded, pane: &str) {
        self.conn()
            .execute(
                "UPDATE attempts SET mux_handle = ?2 WHERE id = ?1",
                params![id, pane],
            )
            .unwrap();
        self.store
            .immediate(|tx| commands::attempt_live(tx, None, id, Some(p)))
            .unwrap();
    }

    /// A second live attempt `id` of `idea`, outside the use case, with
    /// its own worker, process `p` and pane `pane`.
    fn extra_attempt(&self, id: &str, idea: &str, p: Recorded, pane: &str) {
        let run: String = self
            .conn()
            .query_row(
                "SELECT run_id FROM tasks WHERE task_id = ?1",
                [&self.task],
                |r| r.get(0),
            )
            .unwrap();
        let worker = format!("W-{id}");
        self.conn()
            .execute(
                "INSERT INTO workers (worker_id, run_id, persona, status, spawned_at, host)
                 VALUES (?1, ?2, 'idea-lead', 'working', 'then', ?3)",
                params![worker, run, horch_core::taskledger::util::host_name()],
            )
            .unwrap();
        self.conn()
            .execute(
                "INSERT INTO attempts (id, worker_id, idea_id, mux_handle, pid, pid_start, state, started_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'live', 'then')",
                params![id, worker, idea, pane, p.pid, p.pid_start.map(|s| s as i64)],
            )
            .unwrap();
    }

    /// The sub-orchestrator attempts that are not ended: (id, idea).
    fn live(&self) -> Vec<(String, String)> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT id, idea_id FROM attempts
                 WHERE task_id IS NULL AND idea_id IS NOT NULL AND state <> 'ended' ORDER BY rowid",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn attempt_state(&self, id: &str) -> (String, Option<String>) {
        self.conn()
            .query_row(
                "SELECT state, outcome FROM attempts WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    fn i7(&self) -> Vec<Breach> {
        check_i7(self.conn()).unwrap()
    }

    fn i13(&self) -> Vec<Breach> {
        check_i13(self.conn()).unwrap()
    }
}

fn started_ids(k: &Kept) -> Vec<&str> {
    k.started.iter().map(|s| s.attempt.as_str()).collect()
}

fn stopped_pids(k: &Kept) -> Vec<u32> {
    k.stops
        .iter()
        .filter_map(|s| match s.target {
            Target::Process { pid, .. } => Some(pid),
            Target::Pane { .. } => None,
        })
        .collect()
}

fn closed_panes(k: &Kept) -> Vec<&str> {
    k.stops
        .iter()
        .filter_map(|s| match &s.target {
            Target::Pane { pane, .. } => Some(pane.as_str()),
            Target::Process { .. } => None,
        })
        .collect()
}

fn ended(attempt: &str, outcome: &str) -> Ended {
    Ended {
        attempt: attempt.into(),
        outcome: outcome.into(),
    }
}

#[test]
fn i7_missing_sub_orchestrator_is_started_extra_is_stopped() {
    let mut w = world();
    w.idea(SECOND, FEATURE, &["clarifying"]);
    assert_eq!(
        w.i7(),
        vec![Breach::Missing {
            feature: FEATURE.into(),
            idea: SECOND.into(),
            phase: "clarifying".into(),
        }]
    );

    let k = w.keep(&Facts::default());
    assert_eq!(started_ids(&k), vec!["I-0002/clarifying/1"]);
    let s = &k.started[0];
    assert_eq!(
        (s.feature.as_str(), s.idea.as_str(), s.phase.as_str()),
        (FEATURE, SECOND, "clarifying")
    );
    // The first instruction is the launch argument: the idea, the phase and
    // the folder (FR-7). The pane of the feature is new.
    assert_eq!(s.folder, w.ideas().join(SECOND));
    assert_eq!(s.pane, None);
    assert_eq!(s.pane_name, "idea-0001");
    let (task, idea, state, persona): (Option<String>, String, String, String) = w
        .conn()
        .query_row(
            "SELECT a.task_id, a.idea_id, a.state, w.persona FROM attempts a
             JOIN workers w ON w.worker_id = a.worker_id WHERE a.id = 'I-0002/clarifying/1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        (task, idea.as_str(), state.as_str(), persona.as_str()),
        (None, SECOND, "launching", PERSONA)
    );
    assert_eq!(w.i7(), vec![]);

    w.launched("I-0002/clarifying/1", P1, PANE);
    w.extra_attempt("I-0002/clarifying/9", SECOND, P2, "w1:p2");
    assert_eq!(
        w.i7(),
        vec![Breach::Extra {
            feature: FEATURE.into(),
            attempt: "I-0002/clarifying/9".into(),
        }]
    );

    let f = facts(
        &[P1, P2],
        &[],
        &[(PANE, runs_in(P1)), ("w1:p2", runs_in(P2))],
    );
    let k = w.keep(&f);
    assert!(k.started.is_empty(), "{k:?}");
    assert_eq!(k.ended, vec![ended("I-0002/clarifying/9", "stopped")]);
    assert_eq!(stopped_pids(&k), vec![P2.pid]);
    assert_eq!(closed_panes(&k), vec!["w1:p2"]);
    assert_eq!(
        w.live(),
        vec![("I-0002/clarifying/1".to_string(), SECOND.to_string())]
    );
    assert_eq!(w.i7(), vec![]);
}

#[test]
fn i7_two_ideas_of_1_feature_share_1_live_sub_orchestrator() {
    let mut w = world();
    w.question();
    let k = w.keep(&Facts::default());
    assert_eq!(started_ids(&k), vec!["I-0001/supervising/1"]);
    w.launched("I-0001/supervising/1", P1, PANE);

    // A second idea of the feature starts its interview: the 1 live process
    // of the feature is now its process, and it owns the open question.
    w.idea(SECOND, FEATURE, &["clarifying"]);
    w.idea("I-0003", FEATURE, &[]);
    let f = facts(&[P1], &[], &[(PANE, runs_in(P1))]);
    let k = w.keep(&f);
    assert_eq!(k.ended, vec![ended("I-0001/supervising/1", "stopped")]);
    assert_eq!(started_ids(&k), vec!["I-0002/clarifying/1"]);
    assert_eq!(k.started[0].pane.as_deref(), Some(PANE));
    assert_eq!(stopped_pids(&k), vec![P1.pid]);
    assert!(closed_panes(&k).is_empty(), "{k:?}");
    assert_eq!(
        w.live(),
        vec![("I-0002/clarifying/1".to_string(), SECOND.to_string())]
    );
    assert_eq!(w.i7(), vec![]);
    assert_eq!(w.i13(), vec![]);

    // The next pass changes nothing.
    let k = w.keep(&f);
    assert_eq!(k, Kept::default());
}

#[test]
fn i13_open_feature_without_sub_orchestrator_gets_one() {
    let mut w = world();
    // A second feature with only a captured idea has no work.
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
            Ok(id)
        })
        .unwrap();
    w.idea("I-0009", &second, &[]);
    // The idea of the run waits for no one: its process may stop.
    assert_eq!(w.i13(), vec![]);
    assert_eq!(w.keep(&Facts::default()), Kept::default());

    w.question();
    assert_eq!(
        w.i13(),
        vec![Breach::Missing {
            feature: FEATURE.into(),
            idea: FIRST.into(),
            phase: "supervising".into(),
        }]
    );
    let k = w.keep(&Facts::default());
    assert_eq!(started_ids(&k), vec!["I-0001/supervising/1"]);
    assert_eq!(w.i13(), vec![]);
    assert_eq!(w.i7(), vec![]);
}

#[test]
fn i13_a_process_stopped_by_the_idle_rule_is_not_started_again() {
    let mut w = world();
    w.idea(SECOND, FEATURE, &["clarifying"]);
    w.keep(&Facts::default());
    w.conn()
        .execute(
            "UPDATE attempts SET state = 'ended', outcome = ?1 WHERE id = 'I-0002/clarifying/1'",
            [IDLE_STOP],
        )
        .unwrap();
    w.conn()
        .execute(
            "UPDATE ideas SET waiting_on_you_since = 'then' WHERE id = ?1",
            [SECOND],
        )
        .unwrap();
    assert_eq!(w.i7(), vec![]);
    assert_eq!(w.i13(), vec![]);
    assert_eq!(w.keep(&Facts::default()), Kept::default());

    // A worker question starts it again from the folder.
    w.question();
    let k = w.keep(&Facts::default());
    assert_eq!(started_ids(&k), vec!["I-0002/clarifying/2"]);
}

#[test]
fn each_phase_gets_a_fresh_process() {
    let mut w = world();
    w.idea(SECOND, FEATURE, &["clarifying"]);
    w.keep(&Facts::default());
    w.launched("I-0002/clarifying/1", P1, PANE);
    let f1 = facts(&[P1], &[], &[(PANE, runs_in(P1))]);

    // awaiting_approval and a revise are the clarifying phase: the process
    // waits for you.
    w.move_idea(SECOND, "awaiting_approval");
    assert_eq!(w.keep(&f1), Kept::default());
    assert_eq!(w.i7(), vec![]);

    // The approval starts the specifying phase in the same pane.
    w.move_idea(SECOND, "specifying");
    let k = w.keep(&f1);
    assert_eq!(k.ended, vec![ended("I-0002/clarifying/1", "stopped")]);
    assert_eq!(started_ids(&k), vec!["I-0002/specifying/1"]);
    assert_eq!(k.started[0].pane.as_deref(), Some(PANE));
    assert_eq!(stopped_pids(&k), vec![P1.pid]);
    assert!(closed_panes(&k).is_empty(), "{k:?}");

    w.launched("I-0002/specifying/1", P2, PANE);
    w.move_idea(SECOND, "ticketing");
    let k = w.keep(&facts(&[P2], &[P1], &[(PANE, runs_in(P2))]));
    assert_eq!(k.ended, vec![ended("I-0002/specifying/1", "stopped")]);
    assert_eq!(started_ids(&k), vec!["I-0002/ticketing/1"]);
    assert_eq!(k.started[0].pane.as_deref(), Some(PANE));
    assert_eq!(stopped_pids(&k), vec![P2.pid]);
    assert_eq!(
        w.live(),
        vec![("I-0002/ticketing/1".to_string(), SECOND.to_string())]
    );
    assert_eq!(w.i7(), vec![]);
}

#[test]
fn dead_sub_orchestrator_restarts_in_the_same_phase() {
    let mut w = world();
    w.idea(SECOND, FEATURE, &["clarifying"]);
    w.keep(&Facts::default());
    w.launched("I-0002/clarifying/1", P1, PANE);

    // The process died mid-interview; herdr still names its pid in the pane.
    let k = w.keep(&facts(&[], &[P1], &[(PANE, InPane::Gone(P1.pid))]));
    assert_eq!(k.ended, vec![ended("I-0002/clarifying/1", "no_result")]);
    assert_eq!(started_ids(&k), vec!["I-0002/clarifying/2"]);
    assert_eq!(k.started[0].phase, "clarifying");
    assert_eq!(k.started[0].pane.as_deref(), Some(PANE));
    assert!(k.stops.is_empty(), "{k:?}");
    assert_eq!(w.i7(), vec![]);
}

#[test]
fn sub_orchestrator_of_a_merged_feature_is_stopped() {
    let mut w = world();
    w.question();
    w.keep(&Facts::default());
    w.launched("I-0001/supervising/1", P1, PANE);
    w.move_feature(FEATURE, "merged");
    assert_eq!(
        w.i13(),
        vec![Breach::Extra {
            feature: FEATURE.into(),
            attempt: "I-0001/supervising/1".into(),
        }]
    );

    let k = w.keep(&facts(&[P1], &[], &[(PANE, runs_in(P1))]));
    assert_eq!(k.ended, vec![ended("I-0001/supervising/1", "stopped")]);
    assert!(k.started.is_empty(), "{k:?}");
    assert_eq!(stopped_pids(&k), vec![P1.pid]);
    assert_eq!(closed_panes(&k), vec![PANE]);
    assert_eq!(
        w.attempt_state("I-0001/supervising/1"),
        ("ended".to_string(), Some("stopped".to_string()))
    );
    assert_eq!(w.i13(), vec![]);
    assert_eq!(w.i7(), vec![]);
    assert!(needs_you::open_items(w.conn()).unwrap().is_empty());
}

#[test]
fn an_unproven_pane_is_not_closed() {
    let mut w = world();
    w.question();
    w.keep(&Facts::default());
    w.launched("I-0001/supervising/1", P1, PANE);
    w.move_feature(FEATURE, "abandoned");

    // herdr names another process in the pane: no fact proves that the pane
    // is the pane of the attempt.
    let other = Recorded {
        pid: 4999,
        pid_start: Some(99),
    };
    let k = w.keep(&facts(&[P1, other], &[], &[(PANE, runs_in(other))]));
    assert_eq!(k.ended, vec![ended("I-0001/supervising/1", "stopped")]);
    assert_eq!(stopped_pids(&k), vec![P1.pid]);
    assert!(closed_panes(&k).is_empty(), "{k:?}");
    let items = needs_you::open_items(w.conn()).unwrap();
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(items[0].text.contains(PANE), "{items:?}");
}
