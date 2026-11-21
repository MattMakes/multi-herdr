#![cfg(unix)]

use std::fs::{self, OpenOptions, Permissions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use horch_core::fleetd::fleet_runtime::adapters::request_spool::{
    scan, Handled, ScanOutcome, Spool,
};
use horch_core::fleetd::fleet_runtime::adapters::scans::{requests_dir, ScanClock};
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::taskledger::model::{Actor, RunStatus};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{util, Store};
use horch_platform::channel::User;
use rusqlite::{params, Connection};
use serde_json::json;
use tempfile::TempDir;

const IDEA_ADD: &str = r#"Add an idea: horch idea add "<idea>""#;
const RUN_CLOSE: &str = "Merge the pull request of the feature: fleetd sees the merge. To drop a feature: horch feature abandon <feature>";
const RUN_GATE: &str = "fleetd runs the full gate of a feature when its task branches are landed";
const DIRECT: &str =
    "Add a ticket that you wrote: horch idea add --direct <ticket file> [--to <feature>]";
const ADD_MAP: &str = r#"Add an idea: horch idea add "<idea>". For 1 ticket that you wrote: horch idea add --direct <ticket file>. To check a map: horch ledger add-map --dry-run <file>"#;
const PLAN_WORKERS: &str =
    "fleetd dispatches each ready task: horch ideas and horch ledger status show each hold";

const RETIRED_KINDS: [(&str, &str); 7] = [
    ("run_new", IDEA_ADD),
    ("run_close", RUN_CLOSE),
    ("run_gate", RUN_GATE),
    ("task_add", DIRECT),
    ("map_add", ADD_MAP),
    ("task_solo", DIRECT),
    ("plan_workers", PLAN_WORKERS),
];

const MAP: &str =
    "# Map\n\n| # | Title | Blocked by | Wave |\n|---|---|---|---|\n| 01 | [Alpha task](01-a.md) | | 1 |\n";
const TICKET: &str = "# 01: Alpha task\n\n<!-- horch-ticket: 1 -->\n**Persona:** primary `dev` · **Parallel:** yes\n**Kind:** feature\n**Check:** `true`\n**Own:** `src/a.rs`\n\n## Goal\n\nThe goal.\n";

struct Out {
    code: i32,
    out: String,
    err: String,
}

struct World {
    dir: TempDir,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let w = World { dir };
        fs::create_dir_all(w.root().join("home")).unwrap();
        fs::create_dir_all(w.project().join("tickets")).unwrap();
        fs::write(w.project().join("tickets/MAP.md"), MAP).unwrap();
        fs::write(w.project().join("tickets/01-a.md"), TICKET).unwrap();
        w
    }

    fn root(&self) -> PathBuf {
        horch_platform::path::canonicalize(self.dir.path()).unwrap()
    }

    fn project(&self) -> PathBuf {
        self.root().join("proj")
    }

    fn ledger(&self) -> PathBuf {
        self.root().join("ledger.sqlite3")
    }

    fn fleet(&self) -> PathBuf {
        self.root().join("fleet")
    }

    fn horch(&self, args: &[&str]) -> Out {
        let root = self.root();
        let out = Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(self.project())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.ledger())
            .env("HORCH_PROJECT_DIR", self.project())
            .env("FLEET_ROOT", self.fleet())
            .env("HORCH_WORKSPACE_ID", "w1")
            .env("HORCH_NO_WATCHDOG", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        Out {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    fn assert_retired(&self, args: &[&str], command: &str, instead: &str) {
        let o = self.horch(args);
        let line = format!("{command} is retired. {instead}");
        assert_eq!(
            o.code, 1,
            "{args:?}\nstdout:\n{}\nstderr:\n{}",
            o.out, o.err
        );
        assert!(
            o.err.lines().any(|l| l == line),
            "{args:?} must print the line\n{line}\nstderr:\n{}",
            o.err
        );
        assert!(!self.ledger().exists(), "{args:?} made a ledger");
        assert!(
            !requests_dir(&self.fleet()).exists(),
            "{args:?} wrote a request file"
        );
    }
}

#[test]
fn run_new_is_retired_and_names_idea_add() {
    let w = World::new();
    w.assert_retired(
        &["ledger", "run", "new", "alpha", "--base", "main"],
        "horch ledger run new",
        IDEA_ADD,
    );
    w.assert_retired(&["ledger", "run", "new"], "horch ledger run new", IDEA_ADD);
}

#[test]
fn run_close_is_retired_and_names_feature_abandon() {
    let w = World::new();
    w.assert_retired(
        &["ledger", "run", "close", "--abandon", "wrong plan"],
        "horch ledger run close",
        RUN_CLOSE,
    );
}

#[test]
fn run_gate_is_retired_and_names_the_gate_of_fleetd() {
    let w = World::new();
    w.assert_retired(
        &["ledger", "run", "gate", "--check", "true"],
        "horch ledger run gate",
        RUN_GATE,
    );
}

#[test]
fn add_is_retired_and_names_idea_add_direct() {
    let w = World::new();
    w.assert_retired(
        &["ledger", "add", "tickets/01-a.md", "--persona", "dev"],
        "horch ledger add",
        DIRECT,
    );
}

#[test]
fn add_map_is_retired_and_its_dry_run_stays() {
    let w = World::new();
    w.assert_retired(
        &["ledger", "add-map", "tickets/MAP.md"],
        "horch ledger add-map",
        ADD_MAP,
    );
    let o = w.horch(&["ledger", "add-map", "--dry-run", "tickets/MAP.md"]);
    assert!(
        !o.err.contains("is retired") && !o.out.contains("is retired"),
        "add-map --dry-run stays (ticket 13)\nstdout:\n{}\nstderr:\n{}",
        o.out,
        o.err
    );
}

#[test]
fn task_is_retired_and_names_idea_add_direct() {
    let w = World::new();
    w.assert_retired(
        &["ledger", "task", "Fix the docs", "--check", "none"],
        "horch ledger task",
        DIRECT,
    );
}

#[test]
fn plan_workers_is_retired_and_names_the_dispatch_of_fleetd() {
    let w = World::new();
    w.assert_retired(
        &["ledger", "plan-workers", "--json"],
        "horch ledger plan-workers",
        PLAN_WORKERS,
    );
}

#[test]
fn retired_kind_request_file_is_refused_with_the_new_command() {
    let temp = tempfile::tempdir().unwrap();
    let fleet = temp.path().join("fleet");
    let token = token_file::ensure(&fleet).unwrap();
    let db = temp.path().join("ledger.sqlite3");
    Store::open(&db).unwrap();
    let mut conn = Connection::open(&db).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let dir = requests_dir(&fleet);
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, Permissions::from_mode(0o700)).unwrap();
    for (n, (kind, _)) in RETIRED_KINDS.iter().enumerate() {
        let id = format!("6f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e{n:02}");
        let line = json!({
            "request_id": id,
            "kind": kind,
            "payload": {},
            "token": token.text(),
        });
        write_0600(&dir.join(format!("{id}.json")), &format!("{line}\n"));
    }
    let owner = User::current().unwrap();
    let spool = Spool {
        fleet_root: &fleet,
        owner: &owner,
        token: Some(&token),
        max_bytes: 64 * 1024,
        guard: Generation(1),
    };
    let start = Instant::now();
    let mut now = move || start;
    let mut clock = ScanClock {
        deadline: Deadline::pass(start, 3600),
        now: &mut now,
        stamp: "2026-11-18T00:00:00Z",
    };
    let outcome = scan(
        &mut conn,
        &spool,
        &mut Cursor::new("2026-11-18T00:00:00Z"),
        &mut clock,
        &Handlers::new(),
    )
    .unwrap();
    let ScanOutcome::Read(read) = outcome else {
        panic!("expected a scan of the folder, got {outcome:?}");
    };
    assert_eq!(read.facts.len(), RETIRED_KINDS.len());
    for (n, (kind, instead)) in RETIRED_KINDS.iter().enumerate() {
        let name = format!("6f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e{n:02}.json");
        let fact = read.facts.iter().find(|f| f.name == name).unwrap();
        assert_eq!(
            fact.handled,
            Handled::Rejected(format!("retired kind: {instead}")),
            "the request file of {kind}"
        );
    }
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM requests", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0, "a retired kind gets no requests row");
}

#[test]
fn two_features_in_1_workspace_are_allowed() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("proj");
    let store = Store::open(&temp.path().join("ledger.sqlite3")).unwrap();
    let conn = store.conn();
    let old = |slug: &str| NewRun {
        slug: slug.to_string(),
        workspace_id: Some("w1".to_string()),
        base_branch: Some("main".to_string()),
        ..NewRun::default()
    };
    runs::create(conn, &root, &old("alpha")).unwrap();
    let first = runs::create_feature_run(conn, &root, "main").unwrap();
    let second = runs::create_feature_run(conn, &root, "main").unwrap();
    runs::create(conn, &root, &old("beta")).unwrap();
    assert_ne!(first, second);
    let active: Vec<_> = runs::of_project(conn, &root)
        .unwrap()
        .into_iter()
        .filter(|r| r.status == RunStatus::Active.as_str())
        .map(|r| r.slug)
        .collect();
    assert_eq!(active.len(), 4, "each run stays active: {active:?}");
}

fn production_sources() -> Vec<(String, String)> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for entry in fs::read_dir(crates).unwrap() {
        let src = entry.unwrap().path().join("src");
        if src.is_dir() {
            walk(&src, &mut files);
        }
    }
    files
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap();
            let prod: Vec<&str> = text
                .lines()
                .take_while(|l| !matches!(l.trim(), "#[cfg(test)]" | "#[cfg(all(test, unix))]"))
                .collect();
            let rel = path.strip_prefix(crates).unwrap().display().to_string();
            (rel, prod.join("\n"))
        })
        .collect()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_code_path_creates_a_task_without_an_idea() {
    const ALLOWED: [&str; 4] = [
        "horch-core/src/fleetd/task_execution/application/planned_tasks.rs",
        "horch-core/src/taskledger/tasks.rs",
        "horch-core/src/taskledger/import.rs",
        "horch-core/src/taskledger/schema.rs",
    ];
    let sources = production_sources();
    assert!(
        sources.iter().any(|(f, _)| f.ends_with("planned_tasks.rs")),
        "the scan reads the production code"
    );
    const E2E_TEST_FIXTURE: &str = "horch-e2e/src/ledger_seed.rs";
    let mut inserts = Vec::new();
    let mut ideas = Vec::new();
    for (file, text) in &sources {
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.contains("INSERT INTO tasks (") || flat.contains("INSERT INTO tasks(") {
            inserts.push(file.clone());
        }
        if flat.contains("run_idea(")
            && !file.ends_with("taskledger/schema.rs")
            && file != E2E_TEST_FIXTURE
        {
            ideas.push(file.clone());
        }
    }
    let outside: Vec<_> = inserts
        .iter()
        .filter(|f| !ALLOWED.contains(&f.as_str()))
        .collect();
    assert!(
        outside.is_empty(),
        "an insert into tasks outside the paths that give an idea: {outside:?}"
    );
    assert!(
        ideas.is_empty(),
        "a code path writes the idea of a run for its task: {ideas:?}"
    );
}

struct Ledger {
    _temp: TempDir,
    store: Store,
}

impl Ledger {
    fn new() -> Ledger {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("ledger.sqlite3")).unwrap();
        let feature =
            runs::create_feature_run(store.conn(), &temp.path().join("proj"), "main").unwrap();
        assert_eq!(feature, "F-0001");
        store
            .conn()
            .execute(
                "INSERT INTO features (id, primary_branch, state, created_at, updated_at)
                 VALUES ('F-0001', 'main', 'open', ?1, ?1)",
                [util::now()],
            )
            .unwrap();
        Ledger { _temp: temp, store }
    }

    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    fn idea(&self, id: &str, state: &str, created_at: &str) {
        self.conn()
            .execute(
                "INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
                 VALUES (?1, 'F-0001', ?2, ?3, ?3)",
                [id, state, created_at],
            )
            .unwrap();
    }

    fn add(&self, new: &NewTask) -> anyhow::Result<String> {
        let run = runs::feature_run(self.conn(), "F-0001").unwrap().unwrap();
        Ok(tasks::add(self.conn(), &run, new, Actor::Orchestrator)?.task_id)
    }

    fn idea_of(&self, task: &str) -> String {
        self.conn()
            .query_row(
                "SELECT idea_id FROM tasks WHERE task_id = ?1",
                [task],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn count(&self, table: &str) -> i64 {
        self.conn()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}

#[test]
fn tasks_add_takes_the_idea_of_its_parent() {
    let l = Ledger::new();
    l.idea("I-0001", "in_progress", "2026-01-01T00:00:00Z");
    let parent = l
        .add(&NewTask::work("Parent", "tickets/01-a.md", "dev"))
        .unwrap();
    assert_eq!(l.idea_of(&parent), "I-0001");
    l.idea("I-0002", "in_progress", "2026-02-01T00:00:00Z");
    let mut review = NewTask::work("Review", "tickets/01-a.md", "dev");
    review.parent_task_id = Some(parent);
    let review = l.add(&review).unwrap();
    assert_eq!(l.idea_of(&review), "I-0001");
}

#[test]
fn tasks_add_takes_the_youngest_idea_in_flight_of_the_feature() {
    let l = Ledger::new();
    l.idea("I-0001", "in_progress", "2026-01-01T00:00:00Z");
    l.idea("I-0002", "ticketing", "2026-02-01T00:00:00Z");
    l.idea("I-0003", "parked", "2026-03-01T00:00:00Z");
    l.idea("I-0004", "merged", "2026-04-01T00:00:00Z");
    let task = l
        .add(&NewTask::work("Resolve", "tickets/01-a.md", "dev"))
        .unwrap();
    assert_eq!(l.idea_of(&task), "I-0002");
}

#[test]
fn tasks_add_with_no_idea_in_flight_is_refused_with_fr_12() {
    let l = Ledger::new();
    l.idea("I-0001", "merged", "2026-01-01T00:00:00Z");
    let err = l
        .add(&NewTask::work("Orphan", "tickets/01-a.md", "dev"))
        .unwrap_err();
    assert!(format!("{err:#}").contains("FR-12"), "{err:#}");
    assert_eq!(l.count("tasks"), 0);
    assert_eq!(l.count("ideas"), 1, "tasks::add writes no idea");
    assert_eq!(l.count("features"), 1, "tasks::add writes no feature");
}

fn write_0600(path: &Path, text: &str) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
    fs::set_permissions(path, Permissions::from_mode(0o600)).unwrap();
}
