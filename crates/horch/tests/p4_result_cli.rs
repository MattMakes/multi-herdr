//! `horch result` and `horch result --check` (ticket 12 of fleetd-p4).
//!
//! Each test runs the real binary on its own temporary git project, with
//! its own ledger, state and fleet folders, and the watchdog off. The
//! attempt is a push attempt: its `task.yaml` is under the fleet root, and
//! the pane names it in `HORCH_TASK_YAML`.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::task_execution::adapters::task_files::{write_attempt_files, AttemptFiles};
use horch_core::fleetd::task_execution::domain::result::{
    validate, ResultStatus, RESULT_MAX_BYTES,
};
use horch_core::fleetd::task_execution::domain::task_files::TaskFacts;

const HEALTHY_MACHINE: &str = r#"{"os":"linux","arch":"x86_64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

const PATH: &str = "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin";

const FIRST: &str = "run `horch result` first";

struct Out {
    code: i32,
    out: String,
    err: String,
}

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    task_id: String,
    files: AttemptFiles,
}

impl World {
    /// A project with 1 run, 1 task T1 claimed by a worker, its tree, and
    /// the `task.yaml` of attempt A1 of T1 under the fleet root.
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let proj = root.join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        git(&root, &proj, &["init", "-q", "-b", "main"]);
        std::fs::write(proj.join("a.txt"), "a\n").unwrap();
        std::fs::write(proj.join("t1.md"), "# Ticket One\n\n**Check:** none\n").unwrap();
        git(&root, &proj, &["add", "a.txt", "t1.md"]);
        git(&root, &proj, &["commit", "-q", "-m", "init"]);
        std::fs::write(root.join("machine.json"), HEALTHY_MACHINE).unwrap();
        std::fs::create_dir_all(root.join("fleet")).unwrap();
        std::fs::write(
            root.join("fleet/fleet.toml"),
            "health_max_process_share = 1.0\nhealth_max_open_file_share = 1.0\n",
        )
        .unwrap();
        let mut w = World {
            _tmp: tmp,
            root,
            task_id: String::new(),
            files: AttemptFiles {
                task_yaml: PathBuf::new(),
                start_md: PathBuf::new(),
            },
        };
        w.ok(None, &["ledger", "run", "new", "r1"]);
        w.ok(
            None,
            &["ledger", "add", "--file", "t1.md", "--primary", "dev"],
        );
        let worker = w
            .ok(
                None,
                &[
                    "ledger",
                    "worker",
                    "new",
                    "--persona",
                    "dev",
                    "--role",
                    "dev1",
                ],
            )
            .trim()
            .to_string();
        w.ok(Some(&worker), &["ledger", "claim"]);
        w.ok(Some(&worker), &["ledger", "worktree", "new", "feat"]);
        let conn = rusqlite::Connection::open(w.root.join("ledger.sqlite3")).unwrap();
        w.task_id = conn
            .query_row("SELECT task_id FROM tasks", [], |r| r.get(0))
            .unwrap();
        let facts = TaskFacts {
            task: "T1".into(),
            task_id: w.task_id.clone(),
            run: "r1".into(),
            attempt: "A1".into(),
            profile: "dev".into(),
            worktree: w.tree().display().to_string(),
            branch: "fleet/r1-T1-feat".into(),
            read_first: "t1.md".into(),
            goal: "Ticket One".into(),
            check: None,
            kind: None,
            ticket_snapshot: "# Ticket One\n".into(),
            extra: None,
        };
        w.files = write_attempt_files(&w.root.join("fleet"), &facts, &[]).unwrap();
        w
    }

    fn tree(&self) -> PathBuf {
        self.root.join("proj/.worktrees/T1-feat")
    }

    fn outbox(&self) -> PathBuf {
        self.files.task_yaml.parent().unwrap().join("outbox")
    }

    fn result_json(&self) -> PathBuf {
        self.outbox().join("result.json")
    }

    /// Commit `name` in the tree of T1; its sha.
    fn commit(&self, name: &str) -> String {
        let tree = self.tree();
        std::fs::write(tree.join(name), "x\n").unwrap();
        git(&self.root, &tree, &["add", name]);
        git(&self.root, &tree, &["commit", "-q", "-m", name]);
        git(&self.root, &tree, &["rev-parse", "HEAD"])
            .trim()
            .to_string()
    }

    /// `horch <args>` in the tree of T1. `task_yaml` is `HORCH_TASK_YAML`.
    fn horch(&self, task_yaml: Option<&Path>, worker: Option<&str>, args: &[&str]) -> Out {
        let root = &self.root;
        let proj = root.join("proj");
        let cwd = if self.tree().is_dir() {
            self.tree()
        } else {
            proj.clone()
        };
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.args(args)
            .current_dir(cwd)
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", root.join("ledger.sqlite3"))
            .env("HORCH_PROJECT_DIR", &proj)
            .env("HORCH_MACHINE_FILE", root.join("machine.json"))
            .env("FLEET_ROOT", root.join("fleet"))
            .env("HORCH_NO_WATCHDOG", "1");
        if let Some(w) = worker {
            cmd.env("HORCH_WORKER_ID", w);
        }
        if let Some(path) = task_yaml {
            cmd.env("HORCH_TASK_YAML", path);
        }
        let o = cmd.output().unwrap();
        Out {
            code: o.status.code().unwrap(),
            out: String::from_utf8(o.stdout).unwrap(),
            err: String::from_utf8(o.stderr)
                .unwrap()
                .lines()
                .filter(|l| !l.starts_with("horch: NOTE: the health guard "))
                .map(|l| format!("{l}\n"))
                .collect(),
        }
    }

    fn ok(&self, worker: Option<&str>, args: &[&str]) -> String {
        let out = self.horch(None, worker, args);
        assert_eq!(out.code, 0, "horch {args:?}: {}{}", out.out, out.err);
        out.out
    }

    /// `horch result <args>` in the push attempt A1 of T1.
    fn result(&self, args: &[&str]) -> Out {
        let mut all = vec!["result"];
        all.extend_from_slice(args);
        self.horch(Some(&self.files.task_yaml), None, &all)
    }
}

/// `git <args>` in `dir`, with no config of the operator's.
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

/// The names in `dir`, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn result_writes_a_valid_file_once() {
    let w = World::new();
    let first = w.commit("one.txt");
    let second = w.commit("two.txt");
    let out = w.result(&["--status", "completed", "--summary", "Added 2 files."]);
    assert_eq!(out.code, 0, "{}{}", out.out, out.err);
    assert!(
        out.out.contains(&w.result_json().display().to_string()),
        "{}",
        out.out
    );
    let bytes = std::fs::read(w.result_json()).unwrap();
    let file = validate(&bytes, &w.task_id, "A1", RESULT_MAX_BYTES).unwrap();
    assert_eq!(file.status, ResultStatus::Completed);
    assert_eq!(file.summary, "Added 2 files.");
    assert_eq!(
        file.commits,
        [first, second],
        "the commits since the base, oldest first"
    );
    assert_eq!(file.files_changed, ["one.txt", "two.txt"]);
    assert_eq!(names(&w.outbox()), ["result.json"], "no temp file is left");

    // The commits and the files given on the command line win.
    let w = World::new();
    w.commit("one.txt");
    let out = w.result(&[
        "--status",
        "blocked",
        "--summary",
        "s",
        "--commit",
        "abc",
        "--file",
        "x.rs",
        "--notes",
        "n",
    ]);
    assert_eq!(out.code, 0, "{}{}", out.out, out.err);
    let file = validate(
        &std::fs::read(w.result_json()).unwrap(),
        &w.task_id,
        "A1",
        RESULT_MAX_BYTES,
    )
    .unwrap();
    assert_eq!(file.commits, ["abc"]);
    assert_eq!(file.files_changed, ["x.rs"]);
    assert_eq!(file.notes_for_next.as_deref(), Some("n"));
}

#[test]
fn result_json_with_another_status_is_refused() {
    let w = World::new();
    w.commit("one.txt");
    let out = w.result(&["--status", "done", "--summary", "s"]);
    assert_ne!(out.code, 0, "{}{}", out.out, out.err);
    assert!(out.err.contains("done"), "{}", out.err);
    assert!(!w.result_json().exists());
    // No status at all writes nothing either.
    let out = w.result(&["--summary", "s"]);
    assert_eq!(out.code, 1, "{}{}", out.out, out.err);
    assert!(out.err.contains("--status"), "{}", out.err);
    assert!(!w.result_json().exists());
}

#[test]
fn second_result_of_1_attempt_is_refused() {
    let w = World::new();
    w.commit("one.txt");
    let first = w.result(&["--status", "completed", "--summary", "first"]);
    assert_eq!(first.code, 0, "{}{}", first.out, first.err);
    let before = std::fs::read(w.result_json()).unwrap();
    for summary in ["second", "first"] {
        let again = w.result(&["--status", "completed", "--summary", summary]);
        assert_eq!(again.code, 1, "{}{}", again.out, again.err);
        assert!(
            again.err.contains(&w.result_json().display().to_string()),
            "the refusal names the first file: {}",
            again.err
        );
        assert_eq!(
            std::fs::read(w.result_json()).unwrap(),
            before,
            "never rewritten"
        );
    }
}

#[test]
fn check_exits_2_with_neither_file_and_0_with_one() {
    let w = World::new();
    let out = w.result(&["--check"]);
    assert_eq!(out.code, 2, "{}{}", out.out, out.err);
    assert!(out.err.contains(FIRST), "{}", out.err);

    // A question file of the attempt lets the turn end.
    let questions = w.outbox().join("questions");
    std::fs::create_dir_all(&questions).unwrap();
    std::fs::write(questions.join("q.json.tmp"), "{}").unwrap();
    assert_eq!(
        w.result(&["--check"]).code,
        2,
        "a temp file is not a question"
    );
    std::fs::write(questions.join("Q1.json"), "{}").unwrap();
    let out = w.result(&["--check"]);
    assert_eq!(out.code, 0, "{}{}", out.out, out.err);
    std::fs::remove_dir_all(&questions).unwrap();

    // So does result.json.
    w.commit("one.txt");
    assert_eq!(w.result(&["--status", "failed", "--summary", "s"]).code, 0);
    let out = w.result(&["--check"]);
    assert_eq!(out.code, 0, "{}{}", out.out, out.err);
    assert!(
        !w.outbox().join("questions").exists(),
        "--check writes nothing"
    );
}

#[test]
fn result_outside_a_push_attempt_is_refused() {
    let w = World::new();
    w.commit("one.txt");
    let write = ["result", "--status", "completed", "--summary", "s"];

    // No task.yaml in the pane.
    let out = w.horch(None, None, &write);
    assert_eq!(out.code, 1, "{}{}", out.out, out.err);
    assert!(out.err.contains("HORCH_TASK_YAML"), "{}", out.err);
    let out = w.horch(None, None, &["result", "--check"]);
    assert_eq!(
        out.code, 1,
        "a pane that is not a push attempt is never held: {}",
        out.err
    );

    // A task.yaml outside the fleet root.
    let stray = w.root.join("tasks/T9/attempts/A1/task.yaml");
    std::fs::create_dir_all(stray.parent().unwrap()).unwrap();
    std::fs::copy(&w.files.task_yaml, &stray).unwrap();
    let out = w.horch(Some(&stray), None, &write);
    assert_eq!(out.code, 1, "{}{}", out.out, out.err);
    assert!(out.err.contains("fleet root"), "{}", out.err);

    // A task.yaml whose ids are not those of its folder.
    let other = w.root.join("fleet/tasks/T-other/attempts/A2/task.yaml");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    std::fs::copy(&w.files.task_yaml, &other).unwrap();
    let out = w.horch(Some(&other), None, &write);
    assert_eq!(out.code, 1, "{}{}", out.out, out.err);
    assert!(out.err.contains("T-other"), "{}", out.err);

    // A missing task.yaml.
    let gone = w.root.join("fleet/tasks/x/attempts/A1/task.yaml");
    let out = w.horch(Some(&gone), None, &write);
    assert_eq!(out.code, 1, "{}{}", out.out, out.err);

    for dir in [stray.parent().unwrap(), other.parent().unwrap()] {
        assert_eq!(names(dir), ["task.yaml"], "nothing is written");
    }
    assert!(!w.result_json().exists());
}

#[test]
fn oversized_result_is_refused() {
    let w = World::new();
    w.commit("one.txt");
    let summary = "x".repeat(RESULT_MAX_BYTES as usize);
    let out = w.result(&["--status", "completed", "--summary", &summary]);
    assert_eq!(out.code, 1, "{}", out.err);
    assert!(out.err.contains("limit"), "{}", out.err);
    assert!(!w.result_json().exists());
    assert!(!w.outbox().exists() || names(&w.outbox()).is_empty());
}
