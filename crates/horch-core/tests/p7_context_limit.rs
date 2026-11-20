//! The 256k context limit of a CLI with no compaction setting (fleetd-p7
//! ticket 18; FM-20, FR-9, MAP.md Q2).
//!
//! The wrapper ends an Antigravity attempt at 256000 context tokens with
//! the outcome hint `context_limit`. fleetd ends the attempt with the
//! outcome `context_limit`, burns no attempt, and moves the task to `ready`
//! at once, with the handoff file of rung 6 for the next attempt in the
//! same worktree. When 2 attempts in a row end with `context_limit` and the
//! task branch has no new commit between them, the task does not go to
//! `ready` again, and 1 needs-you item opens.
//!
//! Each test runs the fake agents of `crates/horch-e2e` in a temporary
//! fleet root, with a temporary ledger and a temporary git worktree. No
//! test reads the clock of this machine or waits for a time: each waits
//! for the end of the fake or for a line of its stream.

#[path = "support/fakes.rs"]
mod fakes;

use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::sync::mpsc;
use std::time::Duration;

use horch_core::execution::lifecycle::{
    run_exec, write_exec_exit, ExecAttempt, ExecCli, ExecEnd, StreamEvent, TRANSCRIPT_JSONL,
};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::task_execution::adapters::result_reader::AttemptRef;
use horch_core::fleetd::task_execution::application::outbox_ingest;
use horch_core::fleetd::task_execution::application::retry::{
    on_context_limit, ContextLimitMove, TreeAtEnd,
};
use horch_core::fleetd::task_execution::domain::context_limit::CONTEXT_LIMIT;
use horch_core::harness::antigravity::{self, ExecLaunch};
use horch_core::taskledger::model::{Actor, WorkerKind, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::supervisor::{
    AgentFacts, AttemptFiles, ExitFile, HeartbeatFile, OutcomeHint, EXIT_JSON, HEARTBEAT_JSON,
};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{schema, testing, util};
use horch_platform::start::{self, Command};
use rusqlite::{params, Connection};

const GUARD: Generation = Generation(1);

/// The fleet clock of the `fleetd` row in each test.
const CLOCK: i64 = 4242;

/// The fakes that the tests run.
const FAKES: [&str; 2] = ["fake-claude", "fake-antigravity"];

// ─── the fakes ──────────────────────────────────────────────────────────

fn fake(name: &str) -> PathBuf {
    fakes::fake(&FAKES, name)
}

/// A pane that sends each complete line that it gets to the test.
struct Pane {
    send: mpsc::Sender<String>,
    open: Vec<u8>,
}

impl std::io::Write for Pane {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.open.extend_from_slice(buf);
        while let Some(end) = self.open.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.open.drain(..=end).collect();
            let _ = self
                .send
                .send(String::from_utf8_lossy(&line).trim_end().to_string());
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The failure bound of 1 wait of a test. A test never passes by it: it
/// only turns a hang into a failure.
const BOUND: Duration = Duration::from_secs(120);

/// End the CLI of the attempt in `files` with `kill -KILL`, by the pid of
/// its heartbeat. No heartbeat: nothing to end.
fn kill_cli(files: &AttemptFiles) {
    let Ok(text) = std::fs::read_to_string(files.dir().join(HEARTBEAT_JSON)) else {
        return;
    };
    let beat: HeartbeatFile = serde_json::from_str(&text).unwrap();
    if let Some(pid) = beat.pid {
        let _ = StdCommand::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
}

/// End the CLI that the wrapper started with `kill -KILL`, by the pid that
/// the wrapper took at its spawn. A bounded wait that fails calls it, so
/// the scoped wrapper thread ends and the test fails, not hangs.
fn kill_agent(facts: &AgentFacts) {
    if let Some(pid) = facts.pid() {
        let _ = StdCommand::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status();
    }
}

/// Run `cmd` through the wrapper. `watch` gets each pane line until it
/// returns `true`, then `act` runs. A run that does not end within
/// [`BOUND`] after that is ended, and the test fails.
fn run_watched(
    cli: ExecCli,
    cmd: Command,
    files: &AttemptFiles,
    watch: impl Fn(&str) -> bool,
    act: impl FnOnce(),
) -> ExecEnd {
    let facts = AgentFacts::new(Some("w1:p1".into()));
    let (send, lines) = mpsc::channel();
    let (done, end) = mpsc::channel();
    std::thread::scope(|s| {
        let facts = &facts;
        s.spawn(move || {
            let mut pane = Pane {
                send,
                open: Vec::new(),
            };
            let mut attempt = ExecAttempt {
                cli,
                files,
                facts,
                pane: &mut pane,
            };
            let _ = done.send(run_exec(cmd, &mut attempt).unwrap());
        });
        let seen = loop {
            match lines.recv_timeout(BOUND) {
                Ok(line) if watch(&line) => break true,
                Ok(_) => {}
                Err(_) => break false,
            }
        };
        if !seen {
            kill_agent(facts);
            panic!("the stream showed no awaited line within {BOUND:?}");
        }
        act();
        match end.recv_timeout(BOUND) {
            Ok(end) => end,
            Err(_) => {
                kill_agent(facts);
                panic!("the wrapper did not end the CLI within {BOUND:?}");
            }
        }
    })
}

/// A pane line with a token count at or past the limit.
fn past_the_limit(line: &str) -> bool {
    line.split_whitespace()
        .any(|word| word.parse::<u64>().is_ok_and(|n| n >= CONTEXT_LIMIT))
}

// ─── the world: a ledger, a fleet root and a git worktree ───────────────

/// A temp ledger at generation 1 and fleet clock [`CLOCK`], 1 run, a fleet
/// root and the git worktree of the task.
struct World {
    dir: PathBuf,
    _temp: tempfile::TempDir,
    conn: Connection,
    run: Run,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let mut conn = Connection::open(dir.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, ?3, ?2)",
        params![std::process::id(), util::now(), CLOCK],
    )
    .unwrap();
    let run = runs::create(
        &conn,
        &dir,
        &NewRun {
            slug: "r1".into(),
            ..Default::default()
        },
    )
    .unwrap();
    // A task needs an idea in flight of the feature of its run (FR-12).
    schema::run_idea(&conn, &run.run_id, &util::now(), true).unwrap();
    std::fs::create_dir_all(dir.join("fleet")).unwrap();
    let w = World {
        dir,
        _temp: temp,
        conn,
        run,
    };
    std::fs::create_dir_all(w.tree()).unwrap();
    w.git(&["init", "-q", "-b", "main"]);
    std::fs::write(w.tree().join("a.txt"), "one\n").unwrap();
    w.commit("first");
    w
}

impl World {
    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    /// The worktree of the task.
    fn tree(&self) -> PathBuf {
        self.dir.join("tree")
    }

    /// Run git in the worktree, with no config of this machine.
    fn git(&self, args: &[&str]) -> String {
        let out = StdCommand::new("git")
            .current_dir(self.tree())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    /// What fleetd knows of the worktree at the end of an attempt.
    fn tree_at_end(&self) -> TreeAtEnd {
        TreeAtEnd {
            head: Some(self.git(&["rev-parse", "HEAD"]).trim().to_string()),
            diff: self.git(&["diff"]),
        }
    }

    /// Add 1 task of the shared checkout, and bind it to a new live
    /// attempt. The test keeps its own git worktree in [`World::tree`].
    fn running_task(&self) -> AttemptRef {
        let mut new = NewTask::work("t", "tickets/t.md", "agy");
        new.worktree_mode = WorktreeMode::Shared;
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator).unwrap();
        self.claim(&task.task_id)
    }

    /// A new worker binds the task `task_id`: its new live attempt.
    fn claim(&self, task_id: &str) -> AttemptRef {
        let worker = NewWorker {
            run_id: self.run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: "agy".into(),
            ..Default::default()
        };
        let bound = testing::bind_live(&self.conn, task_id, &worker, None).unwrap();
        AttemptRef {
            task_id: task_id.to_string(),
            attempt_id: bound.attempt_id,
        }
    }

    /// The wrapper files of the attempt `a`, with its `inbox/START.md`.
    fn files(&self, a: &AttemptRef) -> AttemptFiles {
        let files = AttemptFiles::new(&self.root(), a.clone());
        std::fs::create_dir_all(files.dir().join("inbox")).unwrap();
        std::fs::create_dir_all(files.dir().join("outbox")).unwrap();
        std::fs::write(files.dir().join("inbox/START.md"), "# T-1\nDo the task.\n").unwrap();
        files
    }

    /// The wrapper runs the attempt `a` with the fake Antigravity and its
    /// `long_context` key, and writes `exit.json`.
    fn run_agy_long_context(&self, a: &AttemptRef) -> ExecEnd {
        let files = self.files(a);
        let start_md = files.dir().join("inbox/START.md");
        let mut cmd = antigravity::exec_command(&ExecLaunch {
            bin: &fake("fake-antigravity"),
            model: "gemini-3-pro",
            effort: None,
            home: &self.dir,
            workdir: &self.tree(),
            prompt: &start_md.to_string_lossy(),
        });
        cmd.env("HORCH_FAKE_LOG", self.dir.join("fake.log"))
            .env("HORCH_FAKE_SCENARIO", "long_context");
        // The wrapper ends the fake by itself: the test does nothing.
        let end = run_watched(ExecCli::Antigravity, cmd, &files, past_the_limit, || {});
        write_exec_exit(&files, end, false).unwrap();
        end
    }

    /// fleetd ingests the end of the attempt `a` at the context limit.
    fn on_context_limit(&mut self, a: &AttemptRef) -> Option<ContextLimitMove> {
        let tree = self.tree_at_end();
        let root = self.root();
        let tail = FleetConfig::default().transcript_tail_bytes;
        let tx = self.conn.transaction().unwrap();
        let moved = on_context_limit(&tx, Some(GUARD), &root, tail, a, &tree).unwrap();
        tx.commit().unwrap();
        moved
    }

    fn task(&self, a: &AttemptRef) -> tasks::Task {
        tasks::get(&self.conn, &a.task_id).unwrap()
    }

    /// The state and the outcome of the attempt `a`.
    fn attempt(&self, a: &AttemptRef) -> (String, Option<String>) {
        self.conn
            .query_row(
                "SELECT state, outcome FROM attempts WHERE id = ?1",
                [&a.attempt_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    /// The open needs-you items: kind, entity id and text.
    fn needs_you(&self) -> Vec<(String, String, String)> {
        let mut st = self
            .conn
            .prepare("SELECT kind, entity_id, text FROM needs_you WHERE state = 'open'")
            .unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// The attempts that the task of `a` used: `attempts - attempt_floor`.
    fn used(&self, a: &AttemptRef) -> i64 {
        let t = self.task(a);
        t.attempts - t.attempt_floor
    }
}

/// 1 attempt of a task that ends at the context limit: the wrapper run,
/// then the ingest of fleetd.
fn context_limit_attempt(w: &mut World, a: &AttemptRef) -> Option<ContextLimitMove> {
    let end = w.run_agy_long_context(a);
    assert!(matches!(end, ExecEnd::ContextLimit(_)), "{end:?}");
    w.on_context_limit(a)
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn agy_attempt_at_256000_tokens_ends_with_context_limit() {
    assert_eq!(CONTEXT_LIMIT, 256_000, "FR-9");
    let mut w = world();
    let a = w.running_task();
    let end = w.run_agy_long_context(&a);

    // The fake does not stop by itself: the wrapper ended it.
    assert!(matches!(end, ExecEnd::ContextLimit(_)), "{end:?}");
    assert_eq!(end.outcome_hint(), Some(OutcomeHint::ContextLimit));
    assert!(
        !end.repair_applies(),
        "an attempt at the limit has no repair"
    );
    let files = AttemptFiles::new(&w.root(), a.clone());
    let exit: ExitFile =
        serde_json::from_str(&std::fs::read_to_string(files.dir().join(EXIT_JSON)).unwrap())
            .unwrap();
    assert_eq!(exit.outcome_hint, Some(OutcomeHint::ContextLimit));
    let transcript = std::fs::read_to_string(files.dir().join(TRANSCRIPT_JSONL)).unwrap();
    assert!(
        transcript.lines().any(|l| matches!(
            ExecCli::Antigravity.read(l),
            StreamEvent::ContextTokens(n) if n >= CONTEXT_LIMIT
        )),
        "the token line is in the transcript: {transcript}"
    );

    // fleetd ends the attempt with the outcome context_limit.
    assert!(w.on_context_limit(&a).is_some());
    assert_eq!(
        w.attempt(&a),
        ("ended".to_string(), Some("context_limit".to_string()))
    );
}

#[test]
fn context_limit_burns_no_attempt_and_moves_the_task_to_ready_at_once() {
    let mut w = world();
    let a = w.running_task();
    let used = w.used(&a);
    let moved = context_limit_attempt(&mut w, &a);

    assert!(
        matches!(moved, Some(ContextLimitMove::Ready { .. })),
        "{moved:?}"
    );
    let t = w.task(&a);
    assert_eq!(t.status, "ready");
    assert_eq!(t.not_before, None, "no backoff: the task is ready at once");
    assert_eq!(t.current_worker_id, None);
    assert_eq!(w.used(&a), used - 1, "the attempt is given back");
    assert!(w.needs_you().is_empty());

    // The next claim uses the same attempt number of the limit.
    let next = w.claim(&a.task_id);
    assert_eq!(w.used(&next), used, "no attempt is burned");

    // A second ingest of the same attempt changes nothing.
    assert_eq!(w.on_context_limit(&a), None);
}

#[test]
fn next_attempt_gets_the_handoff_file_in_the_same_worktree() {
    let mut w = world();
    let a = w.running_task();
    // The attempt changed the worktree and wrote 1 progress line.
    std::fs::write(w.tree().join("a.txt"), "one\ntwo from A1\n").unwrap();
    let files = w.files(&a);
    std::fs::write(
        files.dir().join("outbox/progress.jsonl"),
        "{\"text\":\"step 1 of 3 is done\"}\n",
    )
    .unwrap();
    let Some(ContextLimitMove::Ready { handoff }) = context_limit_attempt(&mut w, &a) else {
        panic!("the task goes on");
    };

    let text = std::fs::read_to_string(&handoff).unwrap();
    assert!(
        text.contains("step 1 of 3 is done"),
        "progress.jsonl: {text}"
    );
    assert!(text.contains("+two from A1"), "git diff: {text}");
    let transcript = std::fs::read_to_string(files.dir().join(TRANSCRIPT_JSONL)).unwrap();
    let last = transcript.lines().last().unwrap();
    assert!(text.contains(last), "the transcript tail: {text}");
    assert!(handoff.starts_with(files.dir()), "{}", handoff.display());

    // The next attempt names the handoff file, and the worktree keeps the
    // work of the old attempt.
    let next = w.claim(&a.task_id);
    assert_ne!(next.attempt_id, a.attempt_id);
    assert_eq!(
        outbox_ingest::handoff_of(&w.conn, &a.task_id).unwrap(),
        Some(handoff.display().to_string())
    );
    assert_eq!(
        std::fs::read_to_string(w.tree().join("a.txt")).unwrap(),
        "one\ntwo from A1\n"
    );
}

#[test]
fn two_context_limits_with_no_new_commit_open_1_needs_you_item() {
    let mut w = world();
    let a1 = w.running_task();
    let used = w.used(&a1);
    assert!(matches!(
        context_limit_attempt(&mut w, &a1),
        Some(ContextLimitMove::Ready { .. })
    ));
    // A2 changes the worktree, but commits nothing.
    let a2 = w.claim(&a1.task_id);
    std::fs::write(w.tree().join("a.txt"), "one\nno commit\n").unwrap();
    let moved = context_limit_attempt(&mut w, &a2);

    assert_eq!(moved, Some(ContextLimitMove::Held));
    let t = w.task(&a2);
    assert_ne!(t.status, "ready", "the loop stops");
    assert_eq!(t.status, "escalated");
    assert_eq!(t.current_worker_id, None);
    assert_eq!(
        w.attempt(&a2),
        ("ended".to_string(), Some("context_limit".to_string()))
    );
    assert_eq!(w.used(&a2), used - 1, "no attempt is burned");
    let items = w.needs_you();
    assert_eq!(items.len(), 1, "{items:?}");
    let (kind, entity, text) = &items[0];
    assert_eq!(kind, "context_limit_loop");
    assert_eq!(entity, &a1.task_id);
    assert!(text.contains(&a1.task_id), "{text}");
    assert!(text.contains(&a1.attempt_id), "{text}");
    assert!(text.contains(&a2.attempt_id), "{text}");

    // A second ingest opens no second item.
    assert_eq!(w.on_context_limit(&a2), None);
    assert_eq!(w.needs_you().len(), 1);
}

#[test]
fn two_context_limits_with_a_new_commit_go_on() {
    let mut w = world();
    let a1 = w.running_task();
    let used = w.used(&a1);
    assert!(matches!(
        context_limit_attempt(&mut w, &a1),
        Some(ContextLimitMove::Ready { .. })
    ));
    // A2 commits its work before it reaches the limit.
    let a2 = w.claim(&a1.task_id);
    std::fs::write(w.tree().join("a.txt"), "one\ncommitted by A2\n").unwrap();
    w.commit("A2 work");
    let moved = context_limit_attempt(&mut w, &a2);

    assert!(
        matches!(moved, Some(ContextLimitMove::Ready { .. })),
        "{moved:?}"
    );
    let t = w.task(&a2);
    assert_eq!(t.status, "ready");
    assert_eq!(t.not_before, None);
    assert_eq!(w.used(&a2), used - 1, "no attempt is burned");
    assert!(w.needs_you().is_empty());
}

/// The test ends the fake with `kill`, a command of Unix.
#[cfg(unix)]
#[test]
fn claude_attempt_is_not_ended_by_the_wrapper_limit() {
    let w = world();
    let a = w.running_task();
    let files = w.files(&a);
    let start_md = files.dir().join("inbox/START.md");
    let mut cmd: Command = start::command(fake("fake-claude"));
    cmd.args([
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-hook-events",
        "--session-id",
        "s-1",
        &start_md.to_string_lossy(),
    ])
    .env("HORCH_FAKE_LOG", w.dir.join("fake.log"))
    .env("HORCH_FAKE_SCENARIO", "long_context");

    // Claude compacts by its own setting, so the CLI goes on past the
    // limit. The test ends it: the end says who stopped it.
    let end = run_watched(ExecCli::Claude, cmd, &files, past_the_limit, || {
        kill_cli(&files)
    });

    assert!(matches!(end, ExecEnd::Exited(_)), "{end:?}");
    assert_eq!(end.outcome_hint(), None);
}
