#![cfg(unix)]

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::fleetd::short_tempdir;

use horch_core::taskledger::map::ticket_facts;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::Store;
use horch_core::taskledger::{schema, util};

use horch_core::fleetd::task_execution::adapters::task_files::{write_attempt_files, AttemptFiles};
use horch_core::fleetd::task_execution::domain::result::{validate, ResultStatus};
use horch_core::fleetd::task_execution::domain::task_files::TaskFacts;

const HEALTHY_MACHINE: &str = r#"{"os":"linux","arch":"x86_64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

const PATH: &str = "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin";

const RESULT_MAX_BYTES: u64 = 64 * 1024;

const FIRST: &str = "run `horch result` first";

struct Out {
    code: i32,
    out: String,
    err: String,
}

fn seed_run_and_task(git: impl Fn(&[&str]), proj: &Path, ledger: &Path) {
    git(&["branch", "fleet/r1", "main"]);
    let new = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        run_branch: Some("fleet/r1".into()),
        ..Default::default()
    };
    let ticket = proj.join("t1.md");
    let text = std::fs::read_to_string(&ticket).unwrap();
    let facts = ticket_facts(&text);
    let title = facts.title.clone().unwrap_or_else(|| "task".to_string());
    let task = NewTask {
        land_check: facts.check.clone(),
        ticket_snapshot: Some(text.clone()),
        ..NewTask::work(&title, &ticket.to_string_lossy(), "dev")
    };
    let mut store = Store::open(ledger).unwrap();
    store
        .immediate(|tx| {
            let run = runs::create(tx, proj, &new)?;
            schema::run_idea(tx, &run.run_id, &util::now(), true)?;
            tasks::add(tx, &run, &task, Actor::Orchestrator)
        })
        .unwrap();
}

struct Agent(std::process::Child);

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct World {
    _agent: Option<Agent>,
    _tmp: tempfile::TempDir,
    root: PathBuf,
    task_id: String,
    files: AttemptFiles,
}

impl World {
    fn new() -> World {
        World::with_agent(false)
    }

    fn with_agent(outside: bool) -> World {
        let tmp = short_tempdir();
        let root = horch_platform::path::canonicalize(tmp.path()).unwrap();
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
            _agent: None,
            _tmp: tmp,
            root,
            task_id: String::new(),
            files: AttemptFiles {
                task_yaml: PathBuf::new(),
                start_md: PathBuf::new(),
            },
        };
        seed_run_and_task(
            |args| drop(git(&w.root, &proj, args)),
            &proj,
            &w.root.join("ledger.sqlite3"),
        );
        let new = horch_core::taskledger::workers::NewWorker {
            persona: "dev".into(),
            role: Some("dev1".into()),
            ..Default::default()
        };
        let agent = outside.then(|| Command::new("sleep").arg("3600").spawn().unwrap());
        let pid = agent.as_ref().map_or_else(std::process::id, |a| a.id());
        support::bind::bind(&w.root.join("ledger.sqlite3"), new, Some(pid));
        w._agent = agent.map(Agent);
        git(
            &w.root,
            &proj,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "fleet/r1-T1-ticket-one",
                ".worktrees/r1-T1-ticket-one",
                "fleet/r1",
            ],
        );
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
            branch: "fleet/r1-T1-ticket-one".into(),
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

    fn tree(&self) -> PathBuf {
        self.root.join("proj/.worktrees/r1-T1-ticket-one")
    }

    fn outbox(&self) -> PathBuf {
        self.files.task_yaml.parent().unwrap().join("outbox")
    }

    fn result_json(&self) -> PathBuf {
        self.outbox().join("result.json")
    }

    fn commit(&self, name: &str) -> String {
        let tree = self.tree();
        std::fs::write(tree.join(name), "x\n").unwrap();
        git(&self.root, &tree, &["add", name]);
        git(&self.root, &tree, &["commit", "-q", "-m", name]);
        git(&self.root, &tree, &["rev-parse", "HEAD"])
            .trim()
            .to_string()
    }

    fn command(&self, task_yaml: Option<&Path>, worker: Option<&str>) -> Command {
        let root = &self.root;
        let proj = root.join("proj");
        let cwd = if self.tree().is_dir() {
            self.tree()
        } else {
            proj.clone()
        };
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.current_dir(cwd)
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
        cmd
    }

    fn horch(&self, task_yaml: Option<&Path>, worker: Option<&str>, args: &[&str]) -> Out {
        let mut cmd = self.command(task_yaml, worker);
        cmd.args(args);
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

    fn result(&self, args: &[&str]) -> Out {
        let mut all = vec!["result"];
        all.extend_from_slice(args);
        self.horch(Some(&self.files.task_yaml), None, &all)
    }
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
    let w = World::with_agent(true);
    w.commit("one.txt");
    let write = ["result", "--status", "completed", "--summary", "s"];

    let out = w.horch(None, None, &write);
    assert_eq!(out.code, 1, "{}{}", out.out, out.err);
    assert!(out.err.contains("no live attempt"), "{}", out.err);
    let out = w.horch(None, None, &["result", "--check"]);
    assert_eq!(
        out.code, 1,
        "a pane that is not a push attempt is never held: {}",
        out.err
    );

    let out = w.horch(Some(&w.files.task_yaml), None, &write);
    assert_eq!(out.code, 1, "{}{}", out.out, out.err);
    assert!(out.err.contains("no live attempt"), "{}", out.err);

    assert!(!w.result_json().exists());
}

#[test]
fn oversized_result_is_refused() {
    let w = World::new();
    w.commit("one.txt");
    let summary = "x".repeat(RESULT_MAX_BYTES as usize);
    let out = w.result(&["--status", "completed", "--summary", &summary]);
    assert_eq!(out.code, 1, "{}", out.err);
    assert!(out.err.contains("the limit is 65536 bytes"), "{}", out.err);
    assert!(!w.result_json().exists());

    let toml = w.root.join("fleet/fleet.toml");
    let mut text = std::fs::read_to_string(&toml).unwrap();
    text.push_str("result_max_bytes = 2000\n");
    std::fs::write(&toml, text).unwrap();
    let summary = "x".repeat(3000);
    let out = w.result(&["--status", "completed", "--summary", &summary]);
    assert_eq!(out.code, 1, "{}", out.err);
    assert!(out.err.contains("the limit is 2000 bytes"), "{}", out.err);
    assert!(!w.result_json().exists());
    assert!(!w.outbox().exists() || names(&w.outbox()).is_empty());
}
