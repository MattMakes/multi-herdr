//! The wiring of Phase 6 into the pass of fleetd, part 1: the release duty,
//! the request effects and the notes of the idea folders, the answers, the
//! question flow, the dispatcher with the plan commit, the replans, the
//! roll-up, the sub-orchestrators, the waits and the needs-you mirror
//! (docs/tickets/fleetd-p6/51-pass-ideas.md).
//!
//! `docs/specs/fleetd/design.md`, "Supervision, liveness and
//! reconciliation" (The reconcile pass, Order inside 1 pass) and "Ledger"
//! (Three rules); RR-4, RR-7 and PL-5 for the idea duties.
//!
//! Each test uses a temp ledger at generation 1, a temp fleet root and a
//! temp project. The time of a budget comes from a test clock, the
//! calendar time from the idea ports, never from the host. No test starts
//! an agent CLI: a launch runs `/bin/sh`, which writes 1 line and sleeps.
#![cfg(unix)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, FixedOffset};
use horch_core::fleetd::fleet_runtime::adapters::effects;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::application::pass::{
    IdeaPorts, NoDuties, Outside, Pass, PassReport, Ports, Primary, Settings,
    ABORT_AFTER_IDEA_COMMIT,
};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use horch_core::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use horch_core::fleetd::ideas::adapters::needs_you_folder::Mirrored;
use horch_core::fleetd::ideas::adapters::questions_store;
use horch_core::fleetd::ideas::application::intake::{add_idea, Target};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::ideas::domain::plan_validator::{DeclaredGates, RosterProfile};
use horch_core::fleetd::ideas::domain::questions::{Question, State as QuestionState};
use horch_core::fleetd::integration::application::ports::{
    BranchTip, Candidate, IntegrationGitGateway, Member, RefMove,
};
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::runtime::fault::{Faults, ABORT_EXIT_CODE};
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

/// The env var that names the world folder of a child pass.
const CHILD: &str = "HORCH_P6A_CHILD";

/// The first feature, its idea from the release backfill, and the pane of
/// its sub-orchestrator.
const FEATURE: &str = "F-0001";
/// The idea that `horch idea add` adds to [`FEATURE`].
const ADDED: &str = "I-0003";
/// The second feature, and its idea that waits for your approval.
const FEATURE_2: &str = "F-0002";
const WAITING: &str = "I-0004";
const PANE: &str = "w1:p7";

// ─── the world ──────────────────────────────────────────────────────────

struct World {
    _temp: Option<tempfile::TempDir>,
    dir: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Operator,
        text,
    }
}

/// A temp ledger at generation 1, a fleet root, and 2 runs with their
/// features and ideas from the release backfill.
fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let mut conn = open(&dir.join("ledger.sqlite3"));
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 100, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let project = dir.join("project");
    fs::create_dir_all(&project).unwrap();
    let mut run = None;
    for slug in ["r1", "r2"] {
        let new_run = NewRun {
            slug: slug.into(),
            ..Default::default()
        };
        let r = runs::create(&conn, &project, &new_run).unwrap();
        schema::run_idea(&conn, &r.run_id, &util::now(), true).unwrap();
        run.get_or_insert(r);
    }
    fs::create_dir_all(dir.join("fleet")).unwrap();
    let config = FleetConfig {
        pass_budget: 200,
        ..FleetConfig::default()
    };
    World {
        _temp: Some(temp),
        dir,
        conn,
        run: run.unwrap(),
        config,
    }
}

/// The world of a child pass, in the folder `dir` of its parent.
fn child_world(dir: &Path) -> World {
    let conn = open(&dir.join("ledger.sqlite3"));
    let id: String = conn
        .query_row("SELECT run_id FROM runs WHERE slug = 'r1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let run = runs::get(&conn, &id).unwrap();
    World {
        _temp: None,
        dir: dir.to_path_buf(),
        conn,
        run,
        config: FleetConfig {
            pass_budget: 200,
            ..FleetConfig::default()
        },
    }
}

impl World {
    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    fn ideas(&self) -> PathBuf {
        self.root().join("ideas")
    }

    fn settings(&self, faults: Faults) -> Settings {
        Settings {
            fleet_root: self.root(),
            generation: Generation(1),
            config: self.config.clone(),
            horch: PathBuf::from("/bin/true"),
            faults,
        }
    }

    fn job(&self) -> JobCtx<'_> {
        JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }

    /// `horch idea add` of an idea in `feature`, as its handler writes it.
    fn add_idea(&self, feature: &str) -> String {
        let tx = self.conn.unchecked_transaction().unwrap();
        let added = add_idea(
            &tx,
            &self.job(),
            "an idea",
            "horch idea add",
            Target::Join(feature),
            cause("idea added"),
        )
        .unwrap();
        tx.commit().unwrap();
        added.file.id
    }

    fn move_idea(&self, idea: &str, to: &str) {
        let tx = self.conn.unchecked_transaction().unwrap();
        transition(
            &tx,
            None,
            Entity::Idea,
            idea,
            to,
            cause("test"),
            &NeedsYouList,
        )
        .unwrap();
        tx.commit().unwrap();
    }

    fn idea_state(&self, idea: &str) -> String {
        self.conn
            .query_row("SELECT state FROM ideas WHERE id = ?1", [idea], |r| {
                r.get(0)
            })
            .unwrap()
    }

    /// A task of the first run with 1 open question of its feature whose
    /// time limit is `due_at` on the fleet clock. Gives the question id.
    fn question(&self, name: &str, due_at: i64) -> String {
        self.question_of(name, due_at, FEATURE)
    }

    fn question_of(&self, name: &str, due_at: i64, feature: &str) -> String {
        let new = NewTask::work(name, &format!("tickets/{name}.md"), "dev");
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator).unwrap();
        let q = Question {
            id: format!("{}/A1/Q1", task.task_id),
            task_id: task.task_id.clone(),
            feature_id: feature.into(),
            owner: format!("idea-lead:{feature}"),
            state: QuestionState::Open,
            due_at,
            asked_at: util::now(),
            answered_at: None,
        };
        questions_store::insert(&self.conn, &q).unwrap();
        q.id
    }

    /// The live sub-orchestrator of `idea` in its `phase`, in the pane
    /// [`PANE`]. Gives its attempt id.
    fn live_sub_orchestrator(&self, idea: &str, phase: &str) -> String {
        let new = NewWorker {
            run_id: self.run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: "idea-lead".into(),
            pane_id: Some(PANE.into()),
            ..Default::default()
        };
        let worker = workers::insert(&self.conn, &new).unwrap();
        self.conn
            .execute(
                "UPDATE workers SET status = 'working' WHERE worker_id = ?1",
                [&worker],
            )
            .unwrap();
        let id = format!("{idea}/{phase}/1");
        self.conn
            .execute(
                "INSERT INTO attempts (id, task_id, idea_id, worker_id, state, started_at)
                 VALUES (?1, NULL, ?2, ?3, 'live', ?4)",
                params![id, idea, worker, util::now()],
            )
            .unwrap();
        id
    }

    fn ask_note(&self, idea: &str, attempt: &str, question: &str) {
        let notes = self.root().join("ideas").join(idea).join("notes");
        fs::create_dir_all(&notes).unwrap();
        for dir in [notes.parent().unwrap(), notes.as_path()] {
            fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let note = serde_json::json!({
            "idea": idea, "attempt": attempt, "kind": "ask", "text": question,
        });
        let path = notes.join("1.json");
        fs::write(&path, note.to_string()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    /// An applied `idea_amend` request of `idea` whose result names the
    /// amendment `n` with `text`.
    fn amend_request(&self, idea: &str, n: u32, text: &str) {
        let result = serde_json::json!({
            "idea": idea,
            "effects": { "idea": idea, "amendment": { "n": n, "text": text } },
        });
        self.conn
            .execute(
                "INSERT INTO requests (request_id, kind, payload_json, payload_sha256, caller, class,
                     auth, state, result_json, received_at, decided_at)
                 VALUES (?1, 'idea_amend', '{}', 'x', 'test', 'operator', 'token', 'applied', ?2, ?3, ?3)",
                params![format!("req-{idea}-{n}-{text}"), result.to_string(), util::now()],
            )
            .unwrap();
    }

    /// The events of the ledger, in order: (event id, entity, entity id,
    /// kind, cause).
    fn events(&self) -> Vec<(i64, String, String, String, String)> {
        let mut stmt = self
            .conn
            .prepare("SELECT event_id, COALESCE(entity, ''), COALESCE(entity_id, task_id, worker_id, ''), kind, text
                 FROM events ORDER BY event_id")
            .unwrap();
        stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    /// The id of the first event after `after` that `want` matches.
    fn first_event(
        &self,
        after: i64,
        want: impl Fn(&(i64, String, String, String, String)) -> bool,
    ) -> i64 {
        let events = self.events();
        events
            .iter()
            .filter(|e| e.0 > after)
            .find(|e| want(e))
            .map(|e| e.0)
            .unwrap_or_else(|| panic!("no such event after {after}: {events:#?}"))
    }

    fn launched(&self) -> Vec<String> {
        lines(&self.dir.join("launched"))
    }

    fn rung(&self) -> Vec<String> {
        lines(&self.dir.join("rung"))
    }
}

fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

// ─── the fakes ──────────────────────────────────────────────────────────

/// The outside of a pass: no herdr snapshot, a launch that writes its
/// attempt id to `launched`, and a doorbell that writes its pane to `rung`.
struct Fake {
    dir: PathBuf,
    /// The herdr status of each worker.
    status: BTreeMap<String, String>,
}

impl Fake {
    fn new(w: &World) -> Fake {
        Fake {
            dir: w.dir.clone(),
            status: BTreeMap::new(),
        }
    }
}

impl Outside for Fake {
    fn panes(&self) -> Option<BTreeMap<String, InPane>> {
        None
    }

    fn close_pane(&self, _pane: &str) -> Result<()> {
        Ok(())
    }

    fn holds(&self) -> Vec<Hold> {
        Vec::new()
    }

    fn worktree_folders(
        &self,
        _conn: &Connection,
    ) -> Result<Vec<horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem>> {
        Ok(Vec::new())
    }

    fn attempt_spec(&self, attempt: &AttemptRow) -> Result<LaunchSpec> {
        let script = format!(
            "echo '{}' >> '{}'; sleep 30",
            attempt.id,
            self.dir.join("launched").display()
        );
        Ok(LaunchSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: None,
            env_rule: Default::default(),
            detach: true,
            output: None,
        })
    }

    fn observe(
        &self,
        w: &horch_core::taskledger::workers::Worker,
    ) -> Option<horch_core::taskledger::watchdog::Observation> {
        let status = self.status.get(&w.worker_id)?;
        Some(horch_core::taskledger::watchdog::Observation {
            agent_alive: true,
            herdr_status: Some(status.clone()),
            herdr_seq: None,
            transcript_bytes: None,
            transcript_mtime: None,
            screen: None,
        })
    }

    fn doorbell(&self, pane: &str) -> Result<()> {
        let path = self.dir.join("rung");
        let mut text = fs::read_to_string(&path).unwrap_or_default();
        text.push_str(pane);
        text.push('\n');
        fs::write(&path, text)?;
        Ok(())
    }
}

/// A git with refs in memory.
#[derive(Default)]
struct Git {
    refs: RefCell<BTreeMap<String, String>>,
}

impl IntegrationGitGateway for Git {
    fn tip(&self, _repo: &Path, name: &str) -> Result<Option<String>> {
        Ok(self.refs.borrow().get(name).cloned())
    }

    fn update_ref(
        &self,
        _repo: &Path,
        name: &str,
        new: &str,
        expected_old: Option<&str>,
    ) -> Result<RefMove> {
        let mut refs = self.refs.borrow_mut();
        let actual = refs.get(name).cloned();
        if actual.as_deref() != expected_old {
            return Ok(RefMove::Mismatch { actual });
        }
        refs.insert(name.into(), new.into());
        Ok(RefMove::Moved)
    }

    fn delete_ref(&self, _repo: &Path, name: &str, _expected_old: &str) -> Result<RefMove> {
        self.refs.borrow_mut().remove(name);
        Ok(RefMove::Moved)
    }

    /// Each feature tip contains the primary tip in this test.
    fn contains(&self, _: &Path, _: &str, _: &str) -> Result<bool> {
        Ok(true)
    }

    fn changes_in(&self, _: &Path, _: &str, _: &str, _: &str) -> Result<bool> {
        Ok(false)
    }

    fn is_ancestor(&self, _: &Path, a: &str, b: &str) -> Result<bool> {
        Ok(a == b)
    }

    fn build_candidate(&self, _: &Path, _: &str, _: &[Member]) -> Result<Candidate> {
        anyhow::bail!("no candidate in this test")
    }

    fn fleet_branches(&self, _: &Path) -> Result<Vec<BranchTip>> {
        Ok(Vec::new())
    }

    fn task_branches(&self, _: &Path, _: &str) -> Result<Vec<BranchTip>> {
        Ok(Vec::new())
    }

    fn stash_branches(&self, _: &Path) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    fn fleet_toml(&self, _: &Path, _: &str) -> Result<Option<String>> {
        Ok(None)
    }

    fn ensure_settings(&self, _: &Path) -> Result<()> {
        Ok(())
    }
}

/// The idea ports of a test: no roster, no gates, no primary tips, a
/// Wednesday at 10:00 inside the working hours, and a count of the
/// notifications.
#[derive(Default)]
struct Ideas {
    git: Git,
    notified: Cell<usize>,
}

impl IdeaPorts for Ideas {
    fn roster(&self) -> Vec<RosterProfile> {
        Vec::new()
    }

    fn gates(&self, _conn: &Connection) -> Result<DeclaredGates> {
        Ok(DeclaredGates::default())
    }

    fn git(&self) -> &dyn IntegrationGitGateway {
        &self.git
    }

    fn primaries(&self, _conn: &Connection) -> Result<Vec<Primary>> {
        Ok(Vec::new())
    }

    fn now(&self) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-18T10:00:00+00:00").unwrap()
    }

    fn notify(&self, mirrored: &Mirrored, _seq: u64) -> Result<()> {
        if mirrored.new > 0 {
            self.notified.set(self.notified.get() + 1);
        }
        Ok(())
    }
}

/// A test clock: each read moves it by `step`. The cell counts the reads.
fn test_clock<'a>(step: Duration) -> (Box<dyn FnMut() -> Instant + 'a>, Rc<Cell<u32>>) {
    let start = Instant::now();
    let reads = Rc::new(Cell::new(0u32));
    let count = Rc::clone(&reads);
    let clock = Box::new(move || {
        count.set(count.get() + 1);
        start + step * count.get()
    });
    (clock, reads)
}

fn pass<'a>(w: &World, outside: &'a Fake, ideas: &'a Ideas, faults: Faults) -> Pass<'a> {
    let ports = Ports {
        outside,
        duties: &NoDuties,
        needs_you: &NeedsYouList,
        host: &Launcher,
    };
    Pass::new(w.settings(faults), ports)
        .with_idea_work(ideas)
        .with_clock(test_clock(Duration::from_micros(1)).0)
}

fn run(p: &mut Pass<'_>, conn: &mut Connection) -> PassReport {
    p.run(conn)
        .unwrap_or_else(|e| panic!("the pass failed: {e}"))
}

/// Wait until `f` gives `n` lines, at most 10 s: a launched process writes
/// its line after the pass.
fn wait_lines(f: impl Fn() -> Vec<String>, n: usize) -> Vec<String> {
    for _ in 0..200 {
        let got = f();
        if got.len() >= n {
            return got;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    f()
}

// ─── the tests ──────────────────────────────────────────────────────────

/// 1 pass runs each idea duty in the order of ticket 51: the release, the
/// question flow, the dispatcher, the sub-orchestrators, the waits, then
/// the needs-you mirror, which writes the file of each item that the
/// duties before it opened in the same pass.
#[test]
fn pass_runs_the_idea_duties_in_order() {
    let mut w = world();
    let added = w.add_idea(FEATURE);
    assert_eq!(added, ADDED);
    let waiting = w.add_idea(FEATURE_2);
    assert_eq!(waiting, WAITING);
    w.move_idea(WAITING, "clarifying");
    w.move_idea(WAITING, "awaiting_approval");
    let question = w.question("t1", 0);
    let before = w.events().last().map_or(0, |e| e.0);

    let fake = Fake::new(&w);
    let ideas = Ideas::default();
    let mut p = pass(&w, &fake, &ideas, Faults::default());
    let report = run(&mut p, &mut w.conn);
    assert!(report.heartbeat, "{report:?}");

    let released = w.first_event(before, |e| e.1 == "idea" && e.4.contains("idea released"));
    let escalated = w.first_event(before, |e| {
        e.2 == question.split("/A1").next().unwrap() && e.3 == "blocked"
    });
    let dispatched = w.first_event(before, |e| e.1 == "idea" && e.2 == ADDED);
    let started = w.first_event(before, |e| {
        e.1 == "attempt" && e.2.starts_with(&format!("{ADDED}/"))
    });
    let waits = w.first_event(before, |e| e.2 == format!("waiting_on_you:{WAITING}"));
    assert!(
        released < escalated,
        "the release comes before the question flow"
    );
    assert!(
        escalated < dispatched,
        "the question flow comes before the dispatcher"
    );
    assert!(
        dispatched < started,
        "the dispatcher comes before the sub-orchestrators"
    );
    assert!(
        started < waits,
        "the sub-orchestrators come before the waits"
    );
    assert_eq!(w.idea_state(ADDED), "clarifying");
    let files: Vec<String> = fs::read_dir(w.root().join("needs-you"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        files.iter().any(|f| f.starts_with("question-")),
        "{files:?}"
    );
    assert!(files.iter().any(|f| f.contains("I-0004")), "{files:?}");
    assert_eq!(ideas.notified.get(), 1, "1 notification of the new items");
    assert_eq!(wait_lines(|| w.launched(), 1).len(), 1);
}

/// The child of a crash test: 1 pass on the world of its parent with the
/// fault of `HORCH_FAULT`.
#[test]
fn child_pass() {
    let Ok(dir) = std::env::var(CHILD) else {
        return;
    };
    let mut w = child_world(Path::new(&dir));
    let fake = Fake::new(&w);
    let ideas = Ideas::default();
    let faults = Faults::parse(std::env::var("HORCH_FAULT").ok().as_deref());
    let mut p = pass(&w, &fake, &ideas, faults);
    run(&mut p, &mut w.conn);
}

fn crash(w: &World, fault: &str) {
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_pass", "--nocapture", "--test-threads=1"])
        .env(CHILD, &w.dir)
        .env("HORCH_FAULT", fault)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(ABORT_EXIT_CODE), "the fault {fault}");
}

/// A crash after the last commit of the idea duties and before their
/// effects: the next pass launches the new sub-orchestrator 1 time and
/// types the ring of the question flow 1 time, and a later pass does
/// neither again. The effects of a committed idea request are written 1
/// time, and the next pass does not write them again.
#[test]
fn crash_after_a_commit_completes_each_idea_effect_once() {
    let mut w = world();
    let asking = w.add_idea(FEATURE);
    w.move_idea(&asking, "clarifying");
    let sub = w.live_sub_orchestrator(&asking, "clarifying");
    w.question("t1", 1_000_000);
    let captured = w.add_idea(FEATURE_2);
    w.amend_request(&asking, 1, "the amended text");

    crash(&w, ABORT_AFTER_IDEA_COMMIT);
    assert_eq!(w.idea_state(&captured), "clarifying", "the rows committed");
    let launching: i64 = w
        .conn
        .query_row(
            "SELECT COUNT(*) FROM attempts WHERE idea_id = ?1 AND state = 'launching'",
            [&captured],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(launching, 1, "the new sub-orchestrator is in launching");
    assert!(w.launched().is_empty(), "no launch before the crash");
    assert!(w.rung().is_empty(), "no ring before the crash");
    let amendment = w.ideas().join(&asking).join("amendments");
    let files = || fs::read_dir(&amendment).map_or(0, |d| d.count());
    assert_eq!(
        files(),
        1,
        "the request effect is written before the duties"
    );

    let fake = Fake::new(&w);
    let ideas = Ideas::default();
    let mut p = pass(&w, &fake, &ideas, Faults::default());
    run(&mut p, &mut w.conn);
    run(&mut p, &mut w.conn);
    let launched = wait_lines(|| w.launched(), 1);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(w.launched(), launched);
    assert_eq!(launched.len(), 1, "1 launch: {launched:?}");
    assert!(
        launched[0].starts_with(&format!("{captured}/")),
        "{launched:?}"
    );
    assert_eq!(w.rung(), vec![PANE.to_string()], "1 ring of {sub}");
    assert_eq!(files(), 1, "the amendment is written 1 time");
}

/// A request whose effects fleetd cannot write gets 1 needs-you item, and
/// the effects of the later requests are written: 1 bad row never stops
/// the pass (design.md, "Ledger", Three rules, rule 1; ticket 58 of
/// fleetd-build, F3).
#[test]
fn a_request_effect_that_cannot_be_written_does_not_block_the_next_one() {
    let mut w = world();
    let idea = w.add_idea(FEATURE);
    // 2 requests with 1 number: the file of the second conflicts with the
    // file of the first.
    w.amend_request(&idea, 1, "first");
    w.amend_request(&idea, 1, "second");
    w.amend_request(&idea, 2, "third");

    let fake = Fake::new(&w);
    let ideas = Ideas::default();
    let mut p = pass(&w, &fake, &ideas, Faults::default());
    run(&mut p, &mut w.conn);
    run(&mut p, &mut w.conn);

    let dir = w.ideas().join(&idea).join("amendments");
    let read = |name: &str| fs::read_to_string(dir.join(name)).unwrap();
    assert_eq!(read("0001.md"), "first");
    assert_eq!(read("0002.md"), "third", "the later request is written");
    let items: Vec<(String, String)> = w
        .conn
        .prepare("SELECT key, entity_id FROM needs_you WHERE kind = ?1 AND state = 'open'")
        .unwrap()
        .query_map(["request_effects_unwritten"], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(items.len(), 1, "1 item for the bad row: {items:?}");
    assert_eq!(items[0].1, idea);
}

/// The pass starts a sub-orchestrator and ends: it does not wait for the
/// process, which still runs after the pass. A live sub-orchestrator that
/// herdr shows at its prompt waits on you, and the pass records that wait
/// and ends: it does not wait for your answer either.
#[test]
fn pass_never_waits_for_a_sub_orchestrator() {
    let mut w = world();
    let asking = w.add_idea(FEATURE);
    w.move_idea(&asking, "clarifying");
    let sub = w.live_sub_orchestrator(&asking, "clarifying");
    w.ask_note(&asking, &sub, "Do retries apply to uploads?");
    let captured = w.add_idea(FEATURE_2);
    let mut fake = Fake::new(&w);
    let worker: String = w
        .conn
        .query_row(
            "SELECT worker_id FROM attempts WHERE id = ?1",
            [&sub],
            |r| r.get(0),
        )
        .unwrap();
    fake.status.insert(worker, "idle".into());
    let ideas = Ideas::default();

    let mut p = pass(&w, &fake, &ideas, Faults::default());
    let report = run(&mut p, &mut w.conn);
    assert!(report.heartbeat, "{report:?}");
    let attempt = format!("{captured}/clarifying/1");
    assert!(
        report
            .done
            .iter()
            .any(|d| format!("{d:?}").contains(&attempt)),
        "the launch of {attempt}: {report:?}"
    );
    let since: Option<String> = w
        .conn
        .query_row(
            "SELECT waiting_on_you_since FROM ideas WHERE id = ?1",
            [&asking],
            |r| r.get(0),
        )
        .unwrap();
    assert!(since.is_some(), "the idea at its prompt waits on you");
    let record = effects::read_launch(&w.root(), &attempt)
        .unwrap()
        .expect("a launch record");
    let pid = record.process.expect("a launched process").pid;
    let alive = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .unwrap()
        .success();
    let _ = Command::new("kill")
        .args(["-9", &format!("-{pid}")])
        .status();
    assert!(alive, "the process of {attempt} runs after the pass");
}

/// 10000 idea folders: the scan of their notes stops at the scan budget on
/// the test clock, the pass writes its heartbeat within 30 s of that
/// clock, and the next pass continues the scan at its cursor.
#[test]
fn ten_thousand_idea_folders_keep_the_heartbeat_under_30_s() {
    let mut w = world();
    w.config.pass_time_budget_s = 25;
    w.config.scan_budget_ms = 2000;
    let owner = horch_platform::channel::User::current().unwrap();
    for n in 1..=10_000 {
        let id = format!("I-{n:05}");
        let folder = IdeaFolder::create(&w.ideas(), &id, owner.clone()).unwrap();
        let notes = folder.dir().join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::set_permissions(&notes, fs::Permissions::from_mode(0o700)).unwrap();
        let note = serde_json::json!({
            "idea": id,
            "attempt": format!("{id}/clarifying/1"),
            "kind": "interview",
            "question": "Q?",
            "text": "A.",
        });
        let path = notes.join("1.json");
        fs::write(&path, serde_json::to_vec(&note).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let fake = Fake::new(&w);
    let ideas = Ideas::default();
    let ports = Ports {
        outside: &fake,
        duties: &NoDuties,
        needs_you: &NeedsYouList,
        host: &Launcher,
    };
    let step = Duration::from_millis(1);
    let (clock, reads) = test_clock(step);
    let mut p = Pass::new(w.settings(Faults::default()), ports)
        .with_idea_work(&ideas)
        .with_clock(clock);
    let ingested = |w: &World| -> i64 {
        w.conn
            .query_row("SELECT COUNT(*) FROM ingested", [], |r| r.get(0))
            .unwrap()
    };

    let report = run(&mut p, &mut w.conn);
    assert!(report.heartbeat, "{report:?}");
    let elapsed = step * reads.get();
    assert!(
        elapsed < Duration::from_secs(30),
        "the pass took {elapsed:?}"
    );
    let first = ingested(&w);
    assert!(
        first > 0 && first < 10_000,
        "the scan stops at its budget: {first}"
    );

    let before = reads.get();
    let report = run(&mut p, &mut w.conn);
    assert!(report.heartbeat, "{report:?}");
    let elapsed = step * (reads.get() - before);
    assert!(
        elapsed < Duration::from_secs(30),
        "the pass took {elapsed:?}"
    );
    assert!(
        ingested(&w) > first,
        "the next pass continues at the cursor"
    );
}
