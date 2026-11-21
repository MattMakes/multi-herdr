#![cfg(unix)]

use std::path::{Path, PathBuf};

use horch_core::digest::sha256_bytes;
use horch_core::fleetd::fleet_runtime::domain::budget::PassBudget;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use horch_core::fleetd::ideas::adapters::{ideas_store, questions_store};
use horch_core::fleetd::ideas::application::approval::{check_i7, trusted_approval};
use horch_core::fleetd::ideas::application::questions::{self, Ask};
use horch_core::fleetd::ideas::application::release::{
    pending_approvals, release, Release, Released, SOURCE,
};
use horch_core::fleetd::ideas::application::sub_orchestrators::{keep_sub_orchestrators, Keep};
use horch_core::fleetd::ideas::domain::approval::RELEASE;
use horch_core::fleetd::ideas::domain::idea_folder::IDEA_MD;
use horch_core::fleetd::ideas::domain::questions::State;
use horch_core::fleetd::task_execution::domain::task::Facts;
use horch_core::taskledger::{runs, schema, tasks, workers, Store};
use horch_platform::channel::User;
use rusqlite::Connection;

const RUN: &str = "r1";
const FEATURE: &str = "F-0001";
const IDEA: &str = "I-0001";
const SOLO_IDEA: &str = "I-0002";
const ORCHESTRATOR: &str = "orch-1";
const QUESTION: &str = "t1/A1/Q1";
const SNAPSHOT: &str = "# 01: a\n\nThe text that add-map stored.\n";
const MAP_TEXT: &str = "# Map: alpha\n\n| # | Title |\n|---|---|\n| 01 | [a](01-a.md) |\n";

struct World {
    _dir: tempfile::TempDir,
    store: Store,
    root: PathBuf,
    ideas: PathBuf,
}

fn owner() -> User {
    User::current().unwrap()
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    let tickets = root.join("docs/tickets/alpha");
    std::fs::create_dir_all(&tickets).unwrap();
    std::fs::write(tickets.join("MAP.md"), MAP_TEXT).unwrap();
    std::fs::write(tickets.join("01-a.md"), "# 01: a, edited after add-map\n").unwrap();
    let path = dir.path().join("ledger.sqlite3");
    {
        let mut conn = Connection::open(&path).unwrap();
        schema::migrate_to(&mut conn, schema::PHASE_6_VERSION - 1).unwrap();
        conn.execute(
            "INSERT INTO projects VALUES ('p', ?1, 'then')",
            [root.to_str().unwrap()],
        )
        .unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO runs (run_id, project_id, slug, base_branch, run_branch, status, started_at)
                VALUES ('{RUN}', 'p', 'alpha', 'main', 'fleet/a', 'active', '2026-11-01T00:00:00Z'),
                       ('r2', 'p', 'beta', 'main', NULL, 'active', '2026-11-02T00:00:00Z');
             INSERT INTO features (id, name, branch, primary_branch, state, created_at, updated_at)
                VALUES ('{FEATURE}', 'alpha', 'fleet/a', 'main', 'open', 'then', 'then');
             INSERT INTO workers (worker_id, run_id, kind, persona, role, host, status)
                VALUES ('{ORCHESTRATOR}', '{RUN}', 'orchestrator', 'orchestrator', 'orchestrator', 'host-1', 'working');
             UPDATE runs SET orchestrator_worker_id = '{ORCHESTRATOR}' WHERE run_id = '{RUN}';
             INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by,
                    status, ticket_snapshot, ticket_sha, created_at, updated_at)
                VALUES ('t1', '{RUN}', 1, 'a', 'docs/tickets/alpha/01-a.md', 'opus', 'opus', 'running',
                        '{SNAPSHOT}', '{sha}', 'now', 'now'),
                       ('t2', '{RUN}', 2, 'b', 'docs/tickets/alpha/02-b.md', 'opus', 'opus', 'done',
                        NULL, NULL, 'now', 'now'),
                       ('t3', 'r2', 1, 'c', 'notes/solo.md', 'opus', 'opus', 'ready',
                        NULL, NULL, 'now', 'now');
             INSERT INTO questions (id, task_id, feature_id, owner, state, due_at, asked_at)
                VALUES ('{QUESTION}', 't1', '{RUN}', 'orchestrator', 'open', 100, 'now');",
            sha = tasks::ticket_sha(SNAPSHOT),
        ))
        .unwrap();
    }
    let store = Store::open(&path).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, 1, 1, 'then', 1000, 'then')",
            [],
        )
        .unwrap();
    World {
        ideas: dir.path().join("ideas"),
        root,
        _dir: dir,
        store,
    }
}

impl World {
    fn release(&mut self) -> Released {
        let ctx = Release {
            guard: None,
            ideas: &self.ideas,
            owner: owner(),
            budget: PassBudget::new(100),
        };
        self.store.immediate(|tx| release(tx, &ctx)).unwrap()
    }

    fn first_pass(&mut self) -> Released {
        let released = self.release();
        for a in &released.approvals {
            a.write(&self.ideas, owner()).unwrap();
        }
        released
    }

    fn folder(&self, idea: &str) -> IdeaFolder {
        IdeaFolder::new(&self.ideas, idea, owner()).unwrap()
    }

    fn map(&self) -> PathBuf {
        self.root.join("docs/tickets/alpha/MAP.md")
    }

    fn events(&self) -> i64 {
        self.store
            .conn()
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap()
    }

    fn approval(&mut self, idea: &str) -> (bool, String, String) {
        let folder = self.folder(idea);
        self.store
            .immediate(|tx| {
                let holds = check_i7(tx, None, &folder, idea)?;
                let a = trusted_approval(tx, None, &folder, idea)?.expect("a trusted approval");
                Ok((holds, a.file, a.sha256))
            })
            .unwrap()
    }
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[test]
fn first_pass_writes_the_folder_of_each_backfilled_idea_once() {
    let mut w = world();
    let released = w.first_pass();
    let mut ideas: Vec<&str> = released.approvals.iter().map(|a| a.idea.as_str()).collect();
    ideas.sort_unstable();
    assert_eq!(ideas, [IDEA, SOLO_IDEA]);

    let idea = w.folder(IDEA).read_idea().unwrap();
    assert_eq!(idea.id, IDEA);
    assert_eq!(idea.source, SOURCE);
    assert!(idea.text.contains("alpha"), "{}", idea.text);
    assert!(
        idea.text.contains(w.map().to_str().unwrap()),
        "{}",
        idea.text
    );

    let (holds, file, sha) = w.approval(IDEA);
    assert!(holds);
    assert_eq!(Path::new(&file), w.map());
    assert_eq!(sha, sha256_bytes(MAP_TEXT.as_bytes()).hex());
    let row = ideas_store::idea(w.store.conn(), IDEA).unwrap().unwrap();
    assert_eq!(row.approved_sha256.as_deref(), Some(RELEASE));

    let solo = w.folder(SOLO_IDEA).read_idea().unwrap();
    assert_eq!(solo.source, SOURCE);
    assert!(solo.text.contains("beta"), "{}", solo.text);
    let (holds, file, sha) = w.approval(SOLO_IDEA);
    assert!(holds);
    assert_eq!(file, IDEA_MD);
    assert_eq!(sha, w.folder(SOLO_IDEA).sha256_of(IDEA_MD).unwrap());

    let files = |w: &World| {
        [IDEA, SOLO_IDEA].map(|i| {
            let dir = w.ideas.join(i);
            (read(&dir.join("idea.md")), read(&dir.join("approval.json")))
        })
    };
    let before = (files(&w), w.events());
    let again = w.first_pass();
    assert!(again.approvals.is_empty(), "{again:?}");
    assert!(again.orchestrators.is_empty(), "{again:?}");
    assert_eq!((files(&w), w.events()), before);
    assert!(pending_approvals(w.store.conn(), &w.ideas)
        .unwrap()
        .is_empty());
}

#[test]
fn crash_after_the_commit_writes_each_approval_once() {
    let mut w = world();
    let released = w.release();
    let pending = pending_approvals(w.store.conn(), &w.ideas).unwrap();
    assert_eq!(pending, released.approvals);
    for a in &pending {
        a.write(&w.ideas, owner()).unwrap();
    }
    assert!(w.approval(IDEA).0);
    assert!(w.approval(SOLO_IDEA).0);
    assert!(pending_approvals(w.store.conn(), &w.ideas)
        .unwrap()
        .is_empty());
    assert!(w.release().approvals.is_empty());
}

#[test]
fn open_question_of_a_run_gets_the_sub_orchestrator_as_its_1_owner() {
    let mut w = world();
    w.first_pass();
    let facts = Facts::default();
    let ideas = w.ideas.clone();
    let ctx = Keep {
        guard: None,
        ideas: &ideas,
        facts: &facts,
        stamp: "2026-11-18T00:00:00Z",
        budget: PassBudget::new(100),
    };
    let kept = w
        .store
        .immediate(|tx| keep_sub_orchestrators(tx, &ctx))
        .unwrap();
    let of_feature: Vec<_> = kept
        .started
        .iter()
        .filter(|s| s.feature == FEATURE)
        .collect();
    assert_eq!(of_feature.len(), 1, "{kept:?}");
    assert_eq!(of_feature[0].idea, IDEA);
    assert!(of_feature[0].folder.join(IDEA_MD).is_file());
    let q = questions_store::get(w.store.conn(), QUESTION)
        .unwrap()
        .unwrap();
    assert_eq!(q.state, State::Open);
    assert_eq!(q.feature_id, FEATURE);
    assert_eq!(q.owner, format!("idea-lead:{FEATURE}"));
}

#[test]
fn old_orchestrator_gets_no_more_questions() {
    let mut w = world();
    let run = runs::get(w.store.conn(), RUN).unwrap();
    assert_eq!(run.orchestrator_worker_id.as_deref(), Some(ORCHESTRATOR));

    let released = w.first_pass();
    assert_eq!(released.orchestrators, [ORCHESTRATOR]);
    let run = runs::get(w.store.conn(), RUN).unwrap();
    assert_eq!(run.orchestrator_worker_id, None);
    let orch = workers::get(w.store.conn(), ORCHESTRATOR).unwrap();
    assert_eq!(orch.status, "working");

    let ask = Ask {
        task_id: "t1",
        attempt_id: "t1/A1",
        mail_id: Some("m-2"),
        text: "Which file?",
        asked_at: "2026-11-18T00:00:00Z",
    };
    let q = questions::ask(w.store.conn(), &ask, FleetSeconds(1000), 900).unwrap();
    assert_eq!(q.owner, format!("idea-lead:{FEATURE}"));
    let owners: Vec<String> = questions_store::open(w.store.conn())
        .unwrap()
        .into_iter()
        .map(|q| q.owner)
        .collect();
    assert_eq!(owners.len(), 2);
    assert!(owners.iter().all(|o| o == &format!("idea-lead:{FEATURE}")));
}

#[test]
fn task_of_a_run_in_flight_keeps_its_ticket_snapshot() {
    let mut w = world();
    w.first_pass();
    let t = tasks::get(w.store.conn(), "t1").unwrap();
    assert_eq!(t.ticket_snapshot.as_deref(), Some(SNAPSHOT));
    assert_eq!(
        t.ticket_sha.as_deref(),
        Some(tasks::ticket_sha(SNAPSHOT).as_str())
    );
    let idea: String = w
        .store
        .conn()
        .query_row("SELECT idea_id FROM tasks WHERE task_id = 't1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(idea, IDEA);
    assert_eq!(read(&w.map()), MAP_TEXT.as_bytes());
    assert!(w.approval(IDEA).0);
}
