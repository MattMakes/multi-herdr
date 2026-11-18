//! The holds on dispatch in `horch ledger status` (ticket 29 of fleetd-p7;
//! OR-1, FM-45): a ready task that does not run shows the cause of its
//! hold, for a health floor, pacing, an unhealthy profile, a git job that
//! holds the repository, a missing fast gate and `not_before`. The board
//! shows each lease and the last signal of its worker, and fleetd records
//! an event when a global hold starts and when it ends.
//!
//! Each test runs the real binary with its own ledger, state and fleet
//! folders, and the watchdog off. The facts are ledger rows that the test
//! writes; no test reads the real machine or the real clock.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::holds::record_hold_changes;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::health::{Fact, Hold};
use horch_core::fleetd::worker_profiles::application::health::mark_unhealthy;
use horch_core::fleetd::worktree_custody::adapters::sqlite::{insert_reserved, NewWorktree};
use horch_core::routing::quota::pool_for;
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;

const HEALTHY_MACHINE: &str = r#"{"os":"linux","arch":"x86_64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

const PATH: &str = "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin";

const AT: &str = "2026-11-01T00:00:00Z";

/// The persona of each task. The built-in roster gives it its pool.
const PERSONA: &str = "opus";

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    /// A git project with 1 run `r1`.
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let w = World { _tmp: tmp, root };
        let proj = w.proj();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(w.root.join("home")).unwrap();
        w.git(&["init", "-q", "-b", "main"]);
        std::fs::write(proj.join("a.txt"), "a\n").unwrap();
        w.git(&["add", "a.txt"]);
        w.git(&["commit", "-q", "-m", "init"]);
        std::fs::write(w.root.join("machine.json"), HEALTHY_MACHINE).unwrap();
        std::fs::create_dir_all(w.root.join("fleet")).unwrap();
        std::fs::write(
            w.root.join("fleet/fleet.toml"),
            "health_max_process_share = 1.0\nhealth_max_open_file_share = 1.0\n",
        )
        .unwrap();
        let store = Store::open(&w.ledger()).unwrap();
        let new = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        runs::create(store.conn(), &w.proj(), &new).unwrap();
        // A task needs an idea in flight of the feature of its run (FR-12).
        store
            .conn()
            .execute_batch(&format!(
                "INSERT OR IGNORE INTO features (id, name, branch, primary_branch, state,
                    created_at, updated_at)
                 VALUES ('F-0001', 'r1', 'fleet/r1', 'main', 'open', '{AT}', '{AT}');
                 INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
                 VALUES ('I-0001', 'F-0001', 'in_progress', '{AT}', '{AT}');"
            ))
            .unwrap();
        w
    }

    fn proj(&self) -> PathBuf {
        self.root.join("proj")
    }

    fn ledger(&self) -> PathBuf {
        self.root.join("ledger.sqlite3")
    }

    fn store(&self) -> Store {
        Store::open(&self.ledger()).unwrap()
    }

    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", self.root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "n")
            .env("GIT_AUTHOR_EMAIL", "a@b")
            .env("GIT_COMMITTER_NAME", "n")
            .env("GIT_COMMITTER_EMAIL", "a@b")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
    }

    /// Run `horch <args>` as the operator; return stdout. A board with work
    /// left exits 1, so each exit code but 0 and 1 fails.
    fn horch(&self, args: &[&str]) -> String {
        let root = &self.root;
        let o = Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.ledger())
            .env("HORCH_PROJECT_DIR", self.proj())
            .env("HORCH_MACHINE_FILE", root.join("machine.json"))
            .env("FLEET_ROOT", root.join("fleet"))
            .env("HORCH_NO_WATCHDOG", "1")
            .output()
            .unwrap();
        let out = String::from_utf8(o.stdout).unwrap();
        assert!(
            matches!(o.status.code(), Some(0 | 1)),
            "{args:?}: {out}{}",
            String::from_utf8_lossy(&o.stderr)
        );
        out
    }

    fn status(&self) -> String {
        self.horch(&["ledger", "status"])
    }

    fn sql(&self, sql: &str) {
        self.store().conn().execute_batch(sql).unwrap();
    }

    /// Add the ready task `title` of [`PERSONA`]; return its id.
    fn task(&self, title: &str) -> String {
        let store = self.store();
        let run = runs::resolve(store.conn(), &self.proj(), Some("r1"), None)
            .unwrap()
            .0;
        let mut new = NewTask::work(title, &format!("tickets/{title}.md"), PERSONA);
        new.ticket_snapshot = Some(format!("# {title}\n"));
        let id = tasks::add(store.conn(), &run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        let status: String = store
            .conn()
            .query_row("SELECT status FROM tasks WHERE task_id = ?1", [&id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "ready");
        id
    }

    /// The `fleetd` row of a fleetd that ran, with the fleet clock at
    /// `clock` and a pid that no process has.
    fn fleet_clock(&self, clock: i64) {
        self.sql(&format!(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s,
                clock_ticked_at)
             VALUES (1, 1, 999999, 1, '{AT}', {clock}, '{AT}');"
        ));
    }

    fn project_id(&self) -> String {
        self.store()
            .conn()
            .query_row("SELECT project_id FROM runs WHERE slug = 'r1'", [], |r| {
                r.get(0)
            })
            .unwrap()
    }

    /// The hold events, as `(kind, cause)`.
    fn hold_events(&self) -> Vec<(String, String)> {
        let store = self.store();
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT kind, cause FROM events
                 WHERE kind IN ('hold_started', 'hold_ended') ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }
}

/// The board line of the task `title`.
fn task_line(board: &str, title: &str) -> String {
    board
        .lines()
        .find(|l| l.contains(title))
        .unwrap_or_else(|| panic!("no line of {title}:\n{board}"))
        .to_string()
}

fn floor() -> Hold {
    Hold::Floor {
        fact: Fact::FreeDisk,
        cause: "free disk 1 GB is below the floor 10 GB".into(),
    }
}

/// Make 1 hold of `cause` for the ready task `title`; return the text that
/// its task line must show.
fn make_hold(w: &World, cause: &str, task_id: &str) -> String {
    let store = w.store();
    let conn = store.conn();
    match cause {
        "health floor" => {
            record_hold_changes(conn, &[], &[floor()]).unwrap();
            "hold on dispatch: free disk 1 GB is below the floor 10 GB".into()
        }
        "pacing" => {
            let pool = pool_for("claude", "claude-opus-5-5").to_string();
            let hold = Hold::Pacing {
                pool: pool.clone(),
                cause: format!("pool {pool} window 7d is 63% used at pace 1.3x, above 1.25x"),
            };
            record_hold_changes(conn, &[], &[hold]).unwrap();
            format!(
                "hold on dispatch: persona {PERSONA}: pool {pool} window 7d is 63% used at pace 1.3x, above 1.25x"
            )
        }
        "unhealthy profile" => {
            let tx = conn.unchecked_transaction().unwrap();
            mark_unhealthy(&tx, None, PERSONA, "conformance failed: dialog").unwrap();
            tx.commit().unwrap();
            format!("hold on dispatch: unhealthy profile {PERSONA}")
        }
        "git job" => {
            let repo = w.proj().display().to_string();
            let new = NewWorktree {
                worktree_id: "W-1".into(),
                task_id: Some(task_id.to_string()),
                worker_id: None,
                repo_root: repo.clone(),
                path: w.root.join("trees/W-1").display().to_string(),
                branch: "fleet/t1".into(),
                base: "main".into(),
                base_commit: None,
                status: WorktreeStatus::Open,
                mode: None,
            };
            let cause = Cause {
                actor: Actor::Horch,
                text: "test",
            };
            let tx = conn.unchecked_transaction().unwrap();
            insert_reserved(&tx, None, &new, cause).unwrap();
            tx.commit().unwrap();
            conn.execute_batch(&format!(
                "INSERT INTO jobs (id, kind, subject, repo, input_key, state, time_limit_s,
                    created_at)
                 VALUES (7, 'rescue', 'W-0', '{repo}', 'k0', 'running', 60, '{AT}'),
                        (8, 'worktree_add', 'W-1', '{repo}', 'k1', 'queued', 60, '{AT}');"
            ))
            .unwrap();
            format!("hold on dispatch: git job rescue 7 holds {repo}")
        }
        "missing fast gate" => {
            let p = w.project_id();
            conn.execute_batch(&format!(
                "INSERT INTO needs_you (key, kind, entity, entity_id, state, text, created_at)
                 VALUES ('missing_fast_gate:{p}', 'missing_fast_gate', 'task', '{p}', 'open',
                    'no fast gate', '{AT}');"
            ))
            .unwrap();
            format!(
                "hold on dispatch: project {p}: missing fast gate: add [gates] fast to fleet.toml in {p}"
            )
        }
        "not_before" => {
            w.fleet_clock(100);
            conn.execute(
                "UPDATE tasks SET not_before = 160 WHERE task_id = ?1",
                [task_id],
            )
            .unwrap();
            "hold on dispatch: retry after 60 s".into()
        }
        other => panic!("no cause {other}"),
    }
}

#[test]
fn ready_task_shows_the_cause_of_its_hold() {
    for cause in [
        "health floor",
        "pacing",
        "unhealthy profile",
        "git job",
        "missing fast gate",
        "not_before",
    ] {
        let w = World::new();
        let id = w.task("t1");
        let before = task_line(&w.status(), "t1");
        assert!(!before.contains("hold on dispatch"), "{cause}: {before}");
        let want = make_hold(&w, cause, &id);
        let line = task_line(&w.status(), "t1");
        assert!(line.contains(&format!("[{want}]")), "{cause}: {line}");
    }
}

#[test]
fn status_shows_each_lease_and_the_last_signal_of_each_worker() {
    let w = World::new();
    let id = w.task("t1");
    w.fleet_clock(100);
    let store = w.store();
    let conn = store.conn();
    let run = runs::resolve(conn, &w.proj(), Some("r1"), None).unwrap().0;
    let worker = workers::insert(
        conn,
        &NewWorker {
            run_id: run.run_id.clone(),
            persona: PERSONA.into(),
            role: Some("opus-1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    conn.execute_batch(&format!(
        "UPDATE workers SET status = 'working', task_id = '{id}',
            last_progress_at = '{AT}', last_progress_source = 'agent'
         WHERE worker_id = '{worker}';
         UPDATE tasks SET status = 'running', current_worker_id = '{worker}'
         WHERE task_id = '{id}';
         INSERT INTO attempts (id, task_id, worker_id, state, heartbeat_seq, started_at)
         VALUES ('A-1', '{id}', '{worker}', 'live', 3, '{AT}');
         INSERT INTO leases (task_id, attempt_id, expires_at, renewed_at, renew_basis)
         VALUES ('{id}', 'A-1', 220, '{AT}', 'process,heartbeat,activity');"
    ))
    .unwrap();
    drop(store);
    let board = w.status();
    let short = task_line(&board, "t1")
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let lease = format!(
        "Lease of {short}: attempt A-1 of opus-1, renewed at {AT} by process,heartbeat,activity, expires in 120 s; last signal agent at {AT}."
    );
    assert!(board.lines().any(|l| l.trim() == lease), "{lease}\n{board}");
}

#[test]
fn global_hold_records_start_and_end_events() {
    let w = World::new();
    w.task("t1");
    let disk = Hold::WorktreeDisk {
        cause: "the worktrees use 30 GB, above the budget of 20 GB".into(),
    };
    {
        let store = w.store();
        record_hold_changes(store.conn(), &[], &[floor(), disk.clone()]).unwrap();
    }
    let board = w.status();
    for cause in [floor().cause(), disk.cause()] {
        assert!(
            board
                .lines()
                .any(|l| l == format!("Hold on dispatch: {cause}.")),
            "{cause}\n{board}"
        );
    }
    assert_eq!(
        w.hold_events(),
        vec![
            ("hold_started".to_string(), floor().cause().to_string()),
            ("hold_started".to_string(), disk.cause().to_string()),
        ]
    );
    // A new fleetd starts with no holds in memory: the ledger keeps the
    // open holds, so the same holds start no new event, and a hold that
    // ended while no fleetd ran ends now.
    {
        let store = w.store();
        record_hold_changes(store.conn(), &[], &[floor()]).unwrap();
    }
    assert_eq!(
        w.hold_events()[2..],
        [("hold_ended".to_string(), disk.cause().to_string())]
    );
    {
        let store = w.store();
        record_hold_changes(store.conn(), &[], &[]).unwrap();
    }
    assert_eq!(
        w.hold_events()[3..],
        [("hold_ended".to_string(), floor().cause().to_string())]
    );
    let board = w.status();
    assert!(!board.contains("Hold on dispatch:"), "{board}");
    assert!(!board.contains("hold on dispatch"), "{board}");
}
