//! A QUESTION of a push attempt also writes its question file in
//! `outbox/questions/` (ticket 16 of fleetd-p4).
//!
//! Each test runs the real binary on its own temporary git project, with
//! its own ledger, state and fleet folders, a fake herdr, and the watchdog
//! off. The push attempt is attempt A1 of T1: its `task.yaml` is under the
//! fleet root, and the pane names it in `HORCH_TASK_YAML`.

#![cfg(unix)]

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::fleetd::{short_tempdir, TestFleetd};

use horch_core::fleetd::task_execution::adapters::task_files::{write_attempt_files, AttemptFiles};
use horch_core::fleetd::task_execution::domain::task_files::TaskFacts;

const HEALTHY_MACHINE: &str = r#"{"os":"linux","arch":"x86_64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

const PATH: &str = "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin";

const WORKSPACE: &str = "w1";
const ROLE: &str = "dev1";

/// The starting value of `result_max_bytes` of `fleet.toml`.
const MAX_BYTES: u64 = 64 * 1024;

/// The fleetd drops first: fields drop in order.
struct World {
    _fleetd: Option<TestFleetd>,
    _tmp: tempfile::TempDir,
    root: PathBuf,
    worker: String,
    task_id: String,
    files: AttemptFiles,
}

impl World {
    /// A project with 1 run, 1 task T1 claimed by worker `dev1`, and the
    /// `task.yaml` of attempt A1 of T1 under the fleet root.
    fn new() -> World {
        let tmp = short_tempdir();
        let root = tmp.path().canonicalize().unwrap();
        let proj = root.join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        git(&root, &proj, &["init", "-q", "-b", "main"]);
        std::fs::write(proj.join("t1.md"), "# Ticket One\n\n**Check:** none\n").unwrap();
        git(&root, &proj, &["add", "t1.md"]);
        git(&root, &proj, &["commit", "-q", "-m", "init"]);
        std::fs::write(root.join("machine.json"), HEALTHY_MACHINE).unwrap();
        std::fs::create_dir_all(root.join("fleet")).unwrap();
        std::fs::write(
            root.join("fleet/fleet.toml"),
            "health_max_process_share = 1.0\nhealth_max_open_file_share = 1.0\n",
        )
        .unwrap();
        let herdr = root.join("herdr");
        std::fs::write(
            &herdr,
            "#!/bin/sh\nprintf '%s\\n' '{\"result\":{\"workspaces\":[],\"panes\":[]}}'\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut w = World {
            _fleetd: None,
            _tmp: tmp,
            root,
            worker: String::new(),
            task_id: String::new(),
            files: AttemptFiles {
                task_yaml: PathBuf::new(),
                start_md: PathBuf::new(),
            },
        };
        w._fleetd = Some(TestFleetd::start(w.command(None), &w.root.join("fleet")));
        w.ok(&["ledger", "run", "new", "r1"]);
        w.ok(&["ledger", "add", "--file", "t1.md", "--primary", "dev"]);
        // A spawned worker has the workspace of its pane.
        let worker = horch_core::taskledger::workers::NewWorker {
            persona: "dev".into(),
            role: Some(ROLE.into()),
            workspace_id: Some(WORKSPACE.into()),
            ..Default::default()
        };
        w.worker = support::bind::bind(&w.root.join("ledger.sqlite3"), worker, None).worker_id;
        w.task_id = w
            .ledger()
            .query_row("SELECT task_id FROM tasks", [], |r| r.get(0))
            .unwrap();
        let facts = TaskFacts {
            task: "T1".into(),
            task_id: w.task_id.clone(),
            run: "r1".into(),
            attempt: "A1".into(),
            profile: "dev".into(),
            worktree: w.root.join("proj").display().to_string(),
            branch: "fleet/r1-T1-feat".into(),
            read_first: "t1.md".into(),
            goal: "Ticket One".into(),
            check: None,
            kind: None,
            ticket_snapshot: "# Ticket One\n".into(),
            extra: None,
            handoff: None,
            idea: None,
            feature: None,
            covers: Vec::new(),
            verdict: None,
        };
        w.files = write_attempt_files(&w.root.join("fleet"), &facts, &[]).unwrap();
        w
    }

    fn ledger(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.join("ledger.sqlite3")).unwrap()
    }

    fn questions_dir(&self) -> PathBuf {
        self.files
            .task_yaml
            .parent()
            .unwrap()
            .join("outbox/questions")
    }

    /// The ids of the `questions` rows, oldest first.
    fn question_rows(&self) -> Vec<String> {
        let conn = self.ledger();
        let mut stmt = conn
            .prepare("SELECT id FROM questions ORDER BY rowid")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// The `horch` binary with the environment of the test.
    fn command(&self, task_yaml: Option<&Path>) -> Command {
        let root = &self.root;
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.current_dir(root.join("proj"))
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", root.join("ledger.sqlite3"))
            .env("HORCH_PROJECT_DIR", root.join("proj"))
            .env("HORCH_MACHINE_FILE", root.join("machine.json"))
            .env("HORCH_HERDR_BIN", root.join("herdr"))
            .env("HORCH_WORKSPACE_ID", WORKSPACE)
            .env("FLEET_ROOT", root.join("fleet"))
            .env("HORCH_NO_WATCHDOG", "1");
        if !self.worker.is_empty() {
            cmd.env("HORCH_WORKER_ID", &self.worker)
                .env("HORCH_ROLE", ROLE);
        }
        if let Some(path) = task_yaml {
            cmd.env("HORCH_TASK_YAML", path);
        }
        cmd
    }

    /// `horch <args>` as the worker; `task_yaml` is `HORCH_TASK_YAML`.
    fn horch(&self, task_yaml: Option<&Path>, args: &[&str]) -> (i32, String, String) {
        let mut cmd = self.command(task_yaml);
        cmd.args(args);
        let o = cmd.output().unwrap();
        (
            o.status.code().unwrap(),
            String::from_utf8(o.stdout).unwrap(),
            String::from_utf8(o.stderr).unwrap(),
        )
    }

    fn ok(&self, args: &[&str]) -> String {
        let (code, out, err) = self.horch(None, args);
        assert_eq!(code, 0, "horch {args:?}: {out}{err}");
        out
    }

    /// `horch tell orchestrator <text>` of the worker, in the push attempt
    /// A1 when `push` is true, else in its claim attempt.
    fn ask(&self, push: bool, text: &str) {
        let yaml = push.then_some(self.files.task_yaml.as_path());
        let (code, out, err) = self.horch(yaml, &["tell", "orchestrator", text]);
        assert_eq!(code, 0, "horch tell: {out}{err}");
    }
}

fn read_json(path: &Path) -> serde_json::Value {
    let bytes = std::fs::read(path).unwrap();
    assert!(
        (bytes.len() as u64) < MAX_BYTES,
        "{} has {} bytes",
        path.display(),
        bytes.len()
    );
    serde_json::from_slice(&bytes).unwrap()
}

fn files_of(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

#[test]
fn question_of_a_push_attempt_writes_1_question_file() {
    let w = World::new();
    w.ask(true, "QUESTION: which port? I recommend 8080.");
    let rows = w.question_rows();
    assert_eq!(rows.len(), 1, "1 questions row, as in Phase 2");
    let dir = w.questions_dir();
    assert_eq!(files_of(&dir), vec!["1.json".to_string()]);
    let file = read_json(&dir.join("1.json"));
    assert_eq!(file["task"], w.task_id.as_str());
    assert_eq!(file["attempt"], "A1");
    assert_eq!(file["question_id"], rows[0].as_str());
    assert!(
        file["text"]
            .as_str()
            .unwrap()
            .contains("QUESTION: which port? I recommend 8080."),
        "{file}"
    );
    assert!(
        file["at"].as_str().is_some_and(|at| !at.is_empty()),
        "{file}"
    );
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(dir.join("1.json")).unwrap().permissions(),
    );
    assert_eq!(mode & 0o777, 0o600);
    // The Stop hook test accepts the question file in place of result.json.
    let (code, out, err) = w.horch(Some(&w.files.task_yaml), &["result", "--check"]);
    assert_eq!(code, 0, "horch result --check: {out}{err}");
}

#[test]
fn two_questions_write_2_files() {
    let w = World::new();
    w.ask(true, "QUESTION: which port?");
    w.ask(true, "QUESTION: which host?");
    let rows = w.question_rows();
    assert_eq!(rows.len(), 2);
    let dir = w.questions_dir();
    assert_eq!(
        files_of(&dir),
        vec!["1.json".to_string(), "2.json".to_string()]
    );
    let first = read_json(&dir.join("1.json"));
    let second = read_json(&dir.join("2.json"));
    assert_eq!(first["question_id"], rows[0].as_str());
    assert_eq!(second["question_id"], rows[1].as_str());
    assert!(first["text"].as_str().unwrap().contains("which port?"));
    assert!(second["text"].as_str().unwrap().contains("which host?"));
}

#[test]
fn a_long_question_file_stays_under_64_kib() {
    let w = World::new();
    let text = format!("QUESTION: {}", "x".repeat(100 * 1024));
    w.ask(true, &text);
    let dir = w.questions_dir();
    assert_eq!(files_of(&dir), vec!["1.json".to_string()]);
    read_json(&dir.join("1.json"));
}

#[test]
fn question_of_a_claim_attempt_writes_no_file() {
    let w = World::new();
    w.ask(false, "QUESTION: which port?");
    assert_eq!(w.question_rows().len(), 1, "1 questions row, as in Phase 2");
    assert_eq!(files_of(&w.questions_dir()), Vec::<String>::new());
    let tasks = w.root.join("fleet/tasks");
    let outbox = w.files.task_yaml.parent().unwrap().join("outbox");
    assert!(
        !outbox.exists(),
        "a claim attempt writes no outbox under {}",
        tasks.display()
    );
}

fn git(root: &Path, dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", PATH)
        .env("HOME", root.join("home"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "n")
        .env("GIT_AUTHOR_EMAIL", "a@b")
        .env("GIT_COMMITTER_NAME", "n")
        .env("GIT_COMMITTER_EMAIL", "a@b")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
