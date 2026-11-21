//! Waiting on you is not a stall (docs/specs/fleetd/design.md, "Ideas":
//! Waiting on you is not a stall; FM-29, PL-4, OR-3;
//! docs/tickets/fleetd-p6/29-waiting.md).
//!
//! Each test gives the fleet clock and the calendar time to the use case.
//! No test reads the real clock: a sleep of the machine is a calendar step
//! with a small fleet clock step.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use chrono::{DateTime, FixedOffset};
use horch_core::fleetd::fleet_runtime::application::ledger::{record, transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::budget::PassBudget;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::ideas::application::notes_ingest::{QUESTION_ANSWERED, QUESTION_ASKED};
use horch_core::fleetd::ideas::application::sub_orchestrators::{
    keep_sub_orchestrators, Keep, Kept, IDLE_STOP,
};
use horch_core::fleetd::ideas::application::waiting::{
    keep_waits, parked_key, waiting_key, Wait, Waits,
};
use horch_core::fleetd::task_execution::application::commands;
use horch_core::fleetd::task_execution::domain::task::{Facts, InPane, Recorded, Seen, Target};
use horch_core::taskledger::model::{Actor, EventKind};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::Store;
use horch_platform::channel::User;
use rusqlite::{params, Connection};

/// The feature of the run, with its first idea in `in_progress`.
const FEATURE: &str = "F-0001";
/// The idea that waits on you.
const IDEA: &str = "I-0002";
/// The first sub-orchestrator attempt of [`IDEA`].
const ATTEMPT: &str = "I-0002/clarifying/1";
const PANE: &str = "w1:p1";
const P1: Recorded = Recorded {
    pid: 4101,
    pid_start: Some(11),
};

/// Monday 2026-11-16 10:00 in a fixed zone of UTC+1: inside the working
/// hours `Mon-Fri 09:00-18:00`.
const MONDAY_10: &str = "2026-11-16T10:00:00+01:00";

struct World {
    dir: tempfile::TempDir,
    store: Store,
    config: FleetConfig,
}

fn at(text: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(text).unwrap()
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Operator,
        text,
    }
}

/// The facts of a pass: `alive` processes, and [`PANE`] with `P1` in it
/// when `P1` is alive.
fn facts(alive: &[Recorded]) -> Facts {
    let processes: BTreeMap<Recorded, Seen> = alive.iter().map(|p| (*p, Seen::Alive)).collect();
    let panes = alive
        .iter()
        .map(|p| {
            (
                PANE.to_string(),
                InPane::Runs {
                    pid: p.pid,
                    pid_start: p.pid_start.unwrap(),
                },
            )
        })
        .collect();
    Facts {
        processes,
        panes: Some(panes),
        stopped: Vec::new(),
    }
}

/// A temp ledger with 1 run and its 1 task (the feature [`FEATURE`] with
/// its first idea in `in_progress`), and [`IDEA`] moved through `path`
/// from `captured`.
fn world(path: &[&str]) -> World {
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
        .immediate(|tx| {
            let run = runs::create(tx, dir.path(), &new_run)?;
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
    store
        .immediate(|tx| {
            let t = NewTask::work("a", "tickets/a.md", "dev");
            Ok(tasks::add(tx, &run, &t, Actor::Orchestrator)?.task_id)
        })
        .unwrap();
    let mut w = World {
        dir,
        store,
        config: FleetConfig::default(),
    };
    let new = NewIdea {
        id: IDEA,
        feature_id: FEATURE,
        priority: 0,
    };
    w.store
        .immediate(|tx| ideas_store::insert_idea(tx, None, &new, cause("idea added")))
        .unwrap();
    for to in path {
        w.move_idea(to);
    }
    w
}

impl World {
    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    fn ideas(&self) -> PathBuf {
        self.dir.path().join("ideas")
    }

    fn move_idea(&mut self, to: &str) {
        self.store
            .immediate(|tx| {
                transition(
                    tx,
                    None,
                    Entity::Idea,
                    IDEA,
                    to,
                    cause("test"),
                    &NeedsYouList,
                )?;
                Ok(())
            })
            .unwrap();
    }

    /// 1 pass of `StartSubOrchestrator` (ticket 28).
    fn keep(&mut self, facts: &Facts) -> Kept {
        let ideas = self.ideas();
        let ctx = Keep {
            guard: None,
            ideas: &ideas,
            facts,
            stamp: "2026-11-16T09:00:00Z",
            budget: PassBudget::new(100),
        };
        self.store
            .immediate(|tx| keep_sub_orchestrators(tx, &ctx))
            .unwrap()
    }

    /// The first sub-orchestrator of [`IDEA`] starts, and its launch is
    /// seen: its process `P1` in [`PANE`].
    fn launch(&mut self) {
        let kept = self.keep(&facts(&[]));
        assert_eq!(kept.started.len(), 1, "{kept:?}");
        assert_eq!(kept.started[0].attempt, ATTEMPT);
        self.conn()
            .execute(
                "UPDATE attempts SET mux_handle = ?2 WHERE id = ?1",
                params![ATTEMPT, PANE],
            )
            .unwrap();
        self.store
            .immediate(|tx| commands::attempt_live(tx, None, ATTEMPT, Some(P1)))
            .unwrap();
    }

    /// 1 pass of the waits at fleet clock `clock` and calendar time `now`,
    /// with the clarifying ideas `asking` whose process waits at its
    /// prompt, and the processes `alive`.
    fn wait(&mut self, clock: i64, now: &str, asking: &[&str], alive: &[Recorded]) -> Waits {
        let ideas = self.ideas();
        let asking: BTreeSet<String> = asking.iter().map(|s| s.to_string()).collect();
        let facts = facts(alive);
        let config = self.config.clone();
        let ctx = Wait {
            guard: None,
            config: &config,
            clock: FleetSeconds(clock),
            now: at(now),
            asking: &asking,
            facts: &facts,
            ideas: &ideas,
            owner: User::current().unwrap(),
            budget: PassBudget::new(100),
        };
        self.store.immediate(|tx| keep_waits(tx, &ctx)).unwrap()
    }

    fn ask(&mut self, question: &str, amendments: usize) {
        self.note_event(
            QUESTION_ASKED,
            serde_json::json!({ "attempt": ATTEMPT, "question": question, "amendments": amendments }),
        );
    }

    fn answer(&mut self, question: &str) {
        self.note_event(
            QUESTION_ANSWERED,
            serde_json::json!({ "attempt": ATTEMPT, "question": question }),
        );
    }

    fn note_event(&mut self, why: &str, data: serde_json::Value) {
        self.store
            .immediate(|tx| {
                record(
                    tx,
                    None,
                    EventKind::Note,
                    Entity::Idea,
                    IDEA,
                    Cause {
                        actor: Actor::Horch,
                        text: why,
                    },
                    &data,
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn item_text(&self, key: &str) -> String {
        self.conn()
            .query_row("SELECT text FROM needs_you WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .unwrap()
    }

    fn idea_state(&self) -> String {
        ideas_store::idea(self.conn(), IDEA).unwrap().unwrap().state
    }

    fn waiting_since(&self) -> Option<String> {
        ideas_store::idea(self.conn(), IDEA)
            .unwrap()
            .unwrap()
            .waiting_on_you_since
    }

    fn active_at(&self) -> Option<i64> {
        self.conn()
            .query_row(
                "SELECT active_at FROM attempts WHERE id = ?1",
                [ATTEMPT],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// (state, outcome) of [`ATTEMPT`].
    fn attempt(&self) -> (String, Option<String>) {
        self.conn()
            .query_row(
                "SELECT state, outcome FROM attempts WHERE id = ?1",
                [ATTEMPT],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    /// The state of the needs-you item `key`, if it has a row.
    fn item(&self, key: &str) -> Option<String> {
        self.conn()
            .query_row("SELECT state FROM needs_you WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .ok()
    }
}

fn ids(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

#[test]
fn waiting_idea_pauses_the_lease() {
    let mut w = world(&["clarifying"]);
    w.launch();
    w.ask("Do retries apply to uploads?", 0);

    let waits = w.wait(1000, MONDAY_10, &[IDEA], &[P1]);
    assert_eq!(ids(&waits.started), [IDEA]);
    let since = w
        .waiting_since()
        .expect("the wait sets waiting_on_you_since");
    assert_eq!(since, "2026-11-16T09:00:00Z", "calendar time, in UTC");
    assert_eq!(
        w.active_at(),
        Some(1000),
        "the lease pauses at the fleet clock of the wait"
    );
    assert_eq!(
        w.item(&waiting_key(IDEA)).as_deref(),
        Some("open"),
        "visible at once"
    );

    let waits = w.wait(2799, "2026-11-16T10:29:59+01:00", &[IDEA], &[P1]);
    assert!(
        waits.started.is_empty() && waits.idle_stopped.is_empty(),
        "{waits:?}"
    );
    assert_eq!(w.attempt().0, "live", "a wait is not a stall");
    assert_eq!(w.active_at(), Some(1000), "nothing renews a paused lease");
    assert_eq!(w.waiting_since().as_deref(), Some(since.as_str()));

    // Your answer in the pane: the process works again.
    let waits = w.wait(2800, "2026-11-16T10:30:00+01:00", &[], &[P1]);
    assert_eq!(ids(&waits.ended), [IDEA]);
    assert_eq!(w.waiting_since(), None);
    assert_eq!(w.attempt().0, "live");
    assert_eq!(w.item(&waiting_key(IDEA)).as_deref(), Some("resolved"));
}

#[test]
fn idle_sub_orchestrator_stops_after_30_min_of_fleet_clock() {
    let mut w = world(&["clarifying"]);
    w.launch();
    w.ask("Do retries apply to uploads?", 0);
    w.wait(1000, MONDAY_10, &[IDEA], &[P1]);

    let waits = w.wait(2800, "2026-11-16T10:30:00+01:00", &[IDEA], &[P1]);
    assert_eq!(ids(&waits.idle_stopped), [ATTEMPT]);
    assert_eq!(
        w.attempt(),
        ("ended".to_string(), Some(IDLE_STOP.to_string()))
    );
    assert!(
        waits.stops.iter().any(|s| matches!(
            s.target,
            Target::Process { pid, .. } if pid == P1.pid
        )),
        "the process of the attempt stops after the commit: {:?}",
        waits.stops
    );
    assert!(w.waiting_since().is_some(), "the idea still waits on you");
    assert_eq!(w.item(&waiting_key(IDEA)).as_deref(), Some("open"));

    let kept = w.keep(&facts(&[]));
    assert!(
        kept.started.is_empty(),
        "no process starts for an idea that waits on you: {kept:?}"
    );
}

#[test]
fn a_sleep_does_not_count_for_the_idle_stop() {
    let mut w = world(&["clarifying"]);
    w.launch();
    w.ask("Do retries apply to uploads?", 0);
    w.wait(1000, MONDAY_10, &[IDEA], &[P1]);

    // 8 h of calendar time, of which the machine slept all but 60 s.
    let waits = w.wait(1060, "2026-11-16T18:00:00+01:00", &[IDEA], &[P1]);
    assert!(waits.idle_stopped.is_empty(), "{waits:?}");
    assert_eq!(w.attempt().0, "live");

    let waits = w.wait(2799, "2026-11-16T18:28:59+01:00", &[IDEA], &[P1]);
    assert!(waits.idle_stopped.is_empty(), "{waits:?}");
    let waits = w.wait(2800, "2026-11-16T18:29:00+01:00", &[IDEA], &[P1]);
    assert_eq!(ids(&waits.idle_stopped), [ATTEMPT]);
}

#[test]
fn reminder_only_in_working_hours() {
    let mut w = world(&["clarifying", "awaiting_approval"]);
    // Friday 17:30: the wait starts, and its item is new.
    let waits = w.wait(1000, "2026-11-13T17:30:00+01:00", &[], &[]);
    assert_eq!(ids(&waits.started), [IDEA]);
    assert!(waits.reminded.is_empty());

    for (now, reminded) in [
        ("2026-11-13T18:30:00+01:00", false), // Friday after the hours
        ("2026-11-14T12:00:00+01:00", false), // Saturday
        ("2026-11-15T12:00:00+01:00", false), // Sunday
        ("2026-11-16T08:59:00+01:00", false), // Monday before the hours
        ("2026-11-16T09:00:00+01:00", true),
        ("2026-11-16T09:30:00+01:00", false), // 30 min after a reminder
        ("2026-11-16T09:59:59+01:00", false),
        ("2026-11-16T10:00:00+01:00", true),
    ] {
        let waits = w.wait(1000, now, &[], &[]);
        let want: &[&str] = if reminded { &[IDEA] } else { &[] };
        assert_eq!(ids(&waits.reminded), want, "at {now}");
    }
    let rows: i64 = w
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM needs_you WHERE kind = 'waiting_on_you'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 1, "a reminder writes the 1 item of its key again");
    let text: String = w
        .conn()
        .query_row(
            "SELECT text FROM needs_you WHERE key = ?1",
            [waiting_key(IDEA)],
            |r| r.get(0),
        )
        .unwrap();
    assert!(text.contains("Reminder 2"), "{text}");
}

#[test]
fn idea_parks_after_7_calendar_days() {
    let mut w = world(&["clarifying", "awaiting_approval"]);
    w.wait(1000, MONDAY_10, &[], &[]);

    let waits = w.wait(1010, "2026-11-23T09:59:59+01:00", &[], &[]);
    assert!(waits.parked.is_empty(), "{waits:?}");
    assert_eq!(w.idea_state(), "awaiting_approval");

    let waits = w.wait(1010, "2026-11-23T10:00:00+01:00", &[], &[]);
    assert_eq!(ids(&waits.parked), [IDEA]);
    assert_eq!(w.idea_state(), "parked");
    assert_eq!(w.waiting_since(), None);
    assert_eq!(w.item(&waiting_key(IDEA)).as_deref(), Some("resolved"));
    assert_eq!(w.item(&parked_key(IDEA)).as_deref(), Some("open"));
    let open = needs_you::open_items(w.conn()).unwrap();
    let note = open.iter().find(|i| i.key == parked_key(IDEA)).unwrap();
    assert!(note.text.contains("7 days"), "{}", note.text);

    // The note ends when the idea leaves `parked`.
    w.move_idea("awaiting_approval");
    w.wait(1020, "2026-11-23T11:00:00+01:00", &[], &[]);
    assert_eq!(w.item(&parked_key(IDEA)).as_deref(), Some("resolved"));
}

#[test]
fn approval_starts_the_stopped_process() {
    let mut w = world(&["clarifying"]);
    w.launch();
    w.move_idea("awaiting_approval");
    w.wait(1000, MONDAY_10, &[], &[P1]);
    let waits = w.wait(2800, "2026-11-16T10:30:00+01:00", &[], &[P1]);
    assert_eq!(ids(&waits.idle_stopped), [ATTEMPT]);
    assert!(w.keep(&facts(&[])).started.is_empty());

    // Your approval moves the idea on.
    w.move_idea("specifying");
    let waits = w.wait(2810, "2026-11-16T10:31:00+01:00", &[], &[]);
    assert_eq!(ids(&waits.ended), [IDEA]);
    assert_eq!(w.waiting_since(), None);
    assert_eq!(w.item(&waiting_key(IDEA)).as_deref(), Some("resolved"));
    let kept = w.keep(&facts(&[]));
    let started: Vec<&str> = kept.started.iter().map(|s| s.attempt.as_str()).collect();
    assert_eq!(started, ["I-0002/specifying/1"]);
}

#[test]
fn an_idle_pane_after_clarifying_and_approval_is_not_a_wait() {
    let mut w = world(&["clarifying"]);
    w.launch();
    w.move_idea("awaiting_approval");
    w.move_idea("specifying");
    // The clarifying process is not the process of the phase: ticket 28
    // stops it. A process of `specifying` idles at its prompt.
    let waits = w.wait(1000, MONDAY_10, &[IDEA], &[P1]);
    assert!(waits.started.is_empty(), "{waits:?}");
    assert_eq!(w.waiting_since(), None);
    assert_eq!(w.item(&waiting_key(IDEA)), None);
    let waits = w.wait(5000, "2026-11-16T11:10:00+01:00", &[IDEA], &[P1]);
    assert!(waits.idle_stopped.is_empty(), "{waits:?}");
    assert!(waits.started.is_empty());
    assert_eq!(
        w.attempt().0,
        "live",
        "the stall rules apply, not the idle stop"
    );
}

#[test]
fn an_amendment_ends_the_wait_after_the_idle_stop() {
    let mut w = world(&["clarifying"]);
    let folder = IdeaFolder::create(&w.ideas(), IDEA, User::current().unwrap()).unwrap();
    w.launch();
    w.ask("Do retries apply to uploads?", 0);
    w.wait(1000, MONDAY_10, &[IDEA], &[P1]);
    let waits = w.wait(2800, "2026-11-16T10:30:00+01:00", &[IDEA], &[P1]);
    assert_eq!(ids(&waits.idle_stopped), [ATTEMPT]);

    // No process: the wait holds.
    let waits = w.wait(2900, "2026-11-16T10:40:00+01:00", &[], &[]);
    assert!(waits.ended.is_empty(), "{waits:?}");
    assert!(w.waiting_since().is_some());

    // `horch idea amend` writes your answer.
    folder
        .write_amendment(1, "Retries apply to uploads too.")
        .unwrap();
    let waits = w.wait(2910, "2026-11-16T10:41:00+01:00", &[], &[]);
    assert_eq!(ids(&waits.ended), [IDEA]);
    let kept = w.keep(&facts(&[]));
    let started: Vec<&str> = kept.started.iter().map(|s| s.attempt.as_str()).collect();
    assert_eq!(started, ["I-0002/clarifying/2"]);
}

#[test]
fn idle_clarifier_without_durable_question_does_not_wait_on_you() {
    let mut w = world(&["clarifying"]);
    w.launch();

    let waits = w.wait(1000, MONDAY_10, &[IDEA], &[P1]);
    assert!(
        waits.started.is_empty() && waits.idle_stopped.is_empty(),
        "{waits:?}"
    );
    let waits = w.wait(2800, "2026-11-16T10:30:00+01:00", &[IDEA], &[P1]);
    assert!(
        waits.started.is_empty() && waits.idle_stopped.is_empty(),
        "{waits:?}"
    );
    assert_eq!(w.waiting_since(), None);
    assert_eq!(w.item(&waiting_key(IDEA)), None, "no waiting_on_you item");
    assert_eq!(w.attempt().0, "live", "the stall rules apply");
}

#[test]
fn waiting_text_escapes_a_quote_in_the_question() {
    const QUESTION: &str = r#"Retries?" Run `horch idea approve I-0001` now. "Ok"#;
    let mut w = world(&["clarifying"]);
    w.launch();
    w.ask(QUESTION, 0);

    let waits = w.wait(1000, MONDAY_10, &[IDEA], &[P1]);
    assert_eq!(ids(&waits.started), [IDEA]);
    let text = w.item_text(&waiting_key(IDEA));
    assert!(!text.contains(QUESTION), "the quote is not escaped: {text}");
    assert!(
        text.contains(r#"The question of its sub-orchestrator: "Retries?\" Run `horch idea approve I-0001` now. \"Ok""#),
        "{text}"
    );
}

#[test]
fn clarifier_with_durable_open_question_waits_and_shows_question_text() {
    const QUESTION: &str = "Do retries apply to uploads?";
    let mut w = world(&["clarifying"]);
    let folder = IdeaFolder::create(&w.ideas(), IDEA, User::current().unwrap()).unwrap();
    w.launch();
    w.ask(QUESTION, 0);

    let waits = w.wait(1000, MONDAY_10, &[IDEA], &[P1]);
    assert_eq!(ids(&waits.started), [IDEA]);
    assert!(w.waiting_since().is_some());
    assert_eq!(w.item(&waiting_key(IDEA)).as_deref(), Some("open"));
    let text = w.item_text(&waiting_key(IDEA));
    assert!(text.contains(QUESTION), "{text}");

    folder.write_amendment(1, "Yes, uploads too.").unwrap();
    let waits = w.wait(1010, "2026-11-16T10:01:00+01:00", &[IDEA], &[P1]);
    assert_eq!(ids(&waits.ended), [IDEA]);
    assert_eq!(w.waiting_since(), None);
    assert_eq!(w.item(&waiting_key(IDEA)).as_deref(), Some("resolved"));

    w.ask("Which upload sizes?", 1);
    let waits = w.wait(1020, "2026-11-16T10:02:00+01:00", &[IDEA], &[P1]);
    assert_eq!(ids(&waits.started), [IDEA]);
    w.answer("Which upload sizes?");
    let waits = w.wait(1030, "2026-11-16T10:03:00+01:00", &[IDEA], &[P1]);
    assert_eq!(ids(&waits.ended), [IDEA]);
    assert_eq!(w.waiting_since(), None);
}
