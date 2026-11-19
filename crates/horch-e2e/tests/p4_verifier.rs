//! The e2e tests of the verifier of Phase 4 (`docs/specs/fleetd/design.md`,
//! FM-9 (the part test of Phase 4), RR-3, SR-4;
//! `docs/tickets/fleetd-p4/51-p4-verifier-e2e.md`).
//!
//! Each test records 1 live push attempt in a temp ledger, as fleetd does
//! before a launch, and plays the worker with the fake agent `claude`: it
//! runs `horch result` in the worktree of the attempt and says "done" in
//! `result.json`. Then a real `horch fleetd` runs on that ledger. Only its
//! verifier moves the task, so each test reads the ledger, the file
//! `verdict-<n>.json` and the files of the check, and never a time.
//!
//! Each process runs with temporary `HORCH_DATA_DIR`, `HORCH_STATE_DIR`,
//! `HORCH_LEDGER_DB` and `FLEET_ROOT`. The load, the limits and the clock of
//! this machine are not read: the harness gives the machine facts through
//! `HORCH_MACHINE_FILE`, and the test `fleet.toml` gives the pass period.

#![cfg(unix)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::task_execution::adapters::task_files::{attempt_dir, write_attempt_files};
use horch_core::fleetd::task_execution::domain::task_files::TaskFacts;
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, NewWorktree};
use horch_core::procid;
use horch_core::taskledger::model::{Actor, VerifyProfile, WorkKind, WorkerKind, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, ClaimOutcome, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for_or, Harness};
use rusqlite::{params, Connection};
use serde_json::Value;

/// The persona of each worker: a Claude persona, so its check runs in the
/// platform sandbox and the fake agent is `claude`.
const PERSONA: &str = "opus";

/// The 1 file that the fake agent commits, `fake-<fake>.txt` (`agent.rs`).
const FAKE_FILE: &str = "fake-claude.txt";

/// Link `bwrap` of this host into the `bin/` of `h`, so that fleetd and its
/// jobs find the Linux sandbox on their sealed `PATH`. macOS needs no link:
/// its sandbox is `/usr/bin/sandbox-exec`.
fn link_bwrap(h: &Harness) {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("bwrap"))
            .find(|p| p.is_file())
    });
    if let Some(bwrap) = found {
        std::os::unix::fs::symlink(bwrap, h.bin.join("bwrap")).unwrap();
    }
}

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// What a test gives a task: its check, its kind and its owned files.
struct Spec<'a> {
    check: &'a str,
    kind: WorkKind,
    own: &'a [&'a str],
}

/// 1 test: a harness with its own ledger, fleet root and fleet.toml, and 1
/// task in a live push attempt `A1` with its worktree. The fleetds and the
/// agent stop before the harness removes its folder.
struct World {
    fleetds: Fleetds,
    h: Harness,
    agent: Child,
    task_id: String,
    worker: String,
    worktree: PathBuf,
    started: usize,
}

impl World {
    fn new(name: &str, spec: &Spec) -> World {
        let mut h = Harness::new(name).with_git();
        std::os::unix::fs::symlink(Harness::horch_bin(), h.bin.join("horch")).unwrap();
        link_bwrap(&h);
        let db = h.root.join("ledger.sqlite3");
        let fleet = h.root.join("fleet");
        std::fs::create_dir_all(&fleet).unwrap();
        h.set("HORCH_LEDGER_DB", db.to_string_lossy());
        h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set("HORCH_TEST_ASSERT_READ_ONLY", "1");
        // A pass each second. The stand-in agent writes no heartbeat file, so
        // no stall rung stops it during the test.
        std::fs::write(
            fleet.join("fleet.toml"),
            "pass_period_s = 1\nheartbeat_max_age_s = 300\n",
        )
        .unwrap();
        let base = h.head_sha().expect("the head of the project");
        let worktree = fleet.join("work").join("t0");
        let mut add = h.git_cmd(&["worktree", "add", "-q", "-b", "fleet/r1-t0"]);
        add.arg(&worktree).current_dir(&h.project);
        let o = add.output().unwrap();
        assert!(o.status.success(), "git worktree add: {}", text(&o));
        // The wrapper of the attempt: a real `sh` that waits for its keys,
        // then runs the fake `claude` as its child, as the wrapper of a
        // pane runs its agent. `horch result` finds the attempt by the
        // parent chain, so the PID of this `sh` is the one that the
        // attempt records.
        let mut cmd = Command::new("/bin/sh");
        h.seal(&mut cmd);
        let agent = cmd
            .args([
                "-c",
                "read s; read w; read y; \
                 HORCH_FAKE_SCENARIO=\"$s\" HORCH_WORKER_ID=\"$w\" HORCH_TASK_YAML=\"$y\" \"$@\"; \
                 exec sleep 600",
                "sh",
            ])
            .arg(h.bin.join("claude"))
            .current_dir(&worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let (task_id, worker) = World::record(&h, spec, &worktree, &base, &agent);
        let fleetds = Fleetds::new(&h);
        World {
            fleetds,
            h,
            agent,
            task_id,
            worker,
            worktree,
            started: 0,
        }
    }

    /// Write the rows that fleetd writes before a launch: the run (the
    /// verification profile `minimal`: the check, then land, so no review
    /// step runs), the idea, the task, its worker, the claimed push attempt
    /// `A1`, the worktree, and the 2 files of the attempt.
    fn record(
        h: &Harness,
        spec: &Spec,
        worktree: &Path,
        base: &str,
        agent: &Child,
    ) -> (String, String) {
        let mut conn = Connection::open(h.root.join("ledger.sqlite3")).unwrap();
        schema::migrate_to(&mut conn, schema::VERSION).unwrap();
        let run = runs::create(
            &conn,
            &h.project,
            &NewRun {
                slug: "r1".into(),
                finish_steps: VerifyProfile::Minimal.steps(),
                ..Default::default()
            },
        )
        .unwrap();
        // FR-12: a task needs an idea in flight of the feature of its run.
        schema::run_idea(&conn, &run.run_id, &util::now(), true)
            .unwrap()
            .expect("the idea of r1");
        let mut new = NewTask::work("t0", "tickets/t0.md", PERSONA);
        new.land_check = Some(spec.check.into());
        new.work_kind = Some(spec.kind);
        new.own = spec.own.iter().map(|s| s.to_string()).collect();
        let task = tasks::add(&conn, &run, &new, Actor::Orchestrator).unwrap();
        let worker = workers::insert(
            &conn,
            &NewWorker {
                run_id: run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona: PERSONA.into(),
                ..Default::default()
            },
        )
        .unwrap();
        conn.execute(
            "UPDATE workers SET status = 'spawning' WHERE worker_id = ?1",
            [&worker],
        )
        .unwrap();
        match tasks::claim(&conn, &worker).unwrap() {
            ClaimOutcome::Claimed(_) => {}
            other => panic!("no claim: {other:?}"),
        }
        let pid = agent.id();
        let started = procid::start_time(pid).expect("the start time of the agent");
        let id = format!("{}/A1", task.task_id);
        let set = conn
            .execute(
                "UPDATE attempts SET via = 'push', pid = ?2, pid_start = ?3 WHERE id = ?1",
                params![id, i64::from(pid), i64::try_from(started).unwrap()],
            )
            .unwrap();
        assert_eq!(set, 1, "the claim writes the attempt {id}");
        let tree = NewWorktree {
            worktree_id: "wt-t0".into(),
            task_id: Some(task.task_id.clone()),
            worker_id: Some(worker.clone()),
            repo_root: h.project.to_string_lossy().into_owned(),
            path: worktree.to_string_lossy().into_owned(),
            branch: "fleet/r1-t0".into(),
            base: base.into(),
            base_commit: Some(base.into()),
            status: WorktreeStatus::Open,
            mode: None,
        };
        let cause = Cause {
            actor: Actor::Orchestrator,
            text: "the worktree of t0",
        };
        let tx = conn.unchecked_transaction().unwrap();
        trees::insert_reserved(&tx, None, &tree, cause).unwrap();
        tx.commit().unwrap();
        let facts = TaskFacts {
            task: "T1".into(),
            task_id: task.task_id.clone(),
            run: "r1".into(),
            attempt: "A1".into(),
            profile: PERSONA.into(),
            worktree: worktree.to_string_lossy().into_owned(),
            branch: "fleet/r1-t0".into(),
            read_first: "tickets/t0.md".into(),
            goal: "t0".into(),
            check: Some(spec.check.into()),
            kind: Some(spec.kind.as_str().into()),
            ticket_snapshot: "# t0\n".into(),
            extra: None,
            handoff: None,
            idea: None,
            feature: None,
            covers: Vec::new(),
            verdict: None,
        };
        write_attempt_files(&h.root.join("fleet"), &facts, &[]).unwrap();
        (task.task_id, worker)
    }

    fn conn(&self) -> Connection {
        let conn = Connection::open(self.h.root.join("ledger.sqlite3")).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        conn
    }

    fn attempt_folder(&self) -> PathBuf {
        attempt_dir(&self.h.root.join("fleet"), &self.task_id, "A1")
    }

    /// Play the worker: the wrapper of the attempt runs the fake `claude`
    /// in the worktree with the Phase 4 keys `scenario`. With the key
    /// `result`, the fake runs `horch result --status completed`: it says
    /// "done".
    fn worker_says_done(&mut self, scenario: &str) {
        let yaml = self.attempt_folder().join("task.yaml");
        let keys = format!("{scenario}\n{}\n{}\n", self.worker, yaml.display());
        self.agent
            .stdin
            .take()
            .expect("the stdin of the wrapper")
            .write_all(keys.as_bytes())
            .unwrap();
        // The wrapper stays alive after the fake ended, so the attempt is
        // live until the test ends.
        let result = self.attempt_folder().join("outbox").join("result.json");
        wait_for_or(
            "the fake claude to write result.json",
            || result.is_file().then_some(()),
            || format!("calls: {:?}", self.h.calls_of("claude")),
        );
        let said: Value = serde_json::from_slice(&std::fs::read(&result).unwrap()).unwrap();
        assert_eq!(said["status"], "completed", "{said}");
    }

    /// Commit `body` as `path` in the worktree.
    fn commit_file(&self, path: &str, body: &str) {
        let file = self.worktree.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, body).unwrap();
        for args in [
            vec!["add", path],
            vec!["commit", "-q", "--no-gpg-sign", "-m", "the work"],
        ] {
            let mut c = self.h.git_cmd(&args);
            c.current_dir(&self.worktree);
            let o = c.output().unwrap();
            assert!(o.status.success(), "git {args:?}: {}", text(&o));
        }
    }

    /// Start `horch fleetd` on the ledger.
    fn start_fleetd(&mut self) -> u32 {
        let name = format!("fleetd-{}", self.started);
        self.started += 1;
        self.fleetds.start(&self.h, &name)
    }

    fn task_status(&self) -> String {
        self.conn()
            .query_row(
                "SELECT status FROM tasks WHERE task_id = ?1",
                [&self.task_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// The state and the outcome of attempt `A1`.
    fn attempt(&self) -> (String, Option<String>) {
        self.conn()
            .query_row(
                "SELECT state, outcome FROM attempts WHERE id = ?1",
                [format!("{}/A1", self.task_id)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    /// Each status that the task had, in order, with the text of its event.
    fn moves(&self) -> Vec<(String, String)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT to_state, COALESCE(cause, '') FROM events
                 WHERE entity = 'task' AND entity_id = ?1 AND to_state IS NOT NULL
                 ORDER BY event_id",
            )
            .unwrap();
        let rows = stmt
            .query_map([&self.task_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        rows
    }

    /// The path of `verdict-<n>.json` of the task.
    fn verdict_path(&self, n: u32) -> PathBuf {
        self.h
            .root
            .join("fleet")
            .join("tasks")
            .join(&self.task_id)
            .join(format!("verdict-{n}.json"))
    }

    /// Wait for `verdict-<n>.json` and read it.
    fn verdict(&self, n: u32) -> Value {
        let path = self.verdict_path(n);
        wait_for_or(
            &format!("verdict-{n}.json of the task"),
            || std::fs::read(&path).ok(),
            || self.evidence(),
        );
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
    }

    /// Whether a `task_check` job ended: it is neither queued nor running.
    fn check_job_ended(&self) -> bool {
        self.conn()
            .query_row(
                "SELECT count(*) FROM jobs
                 WHERE kind = 'task_check' AND state NOT IN ('queued', 'running')",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            > 0
    }

    /// The facts of the ledger and of fleetd, for a failure message.
    fn evidence(&self) -> String {
        let conn = self.conn();
        let jobs: Vec<String> = conn
            .prepare("SELECT kind || ' ' || state || ' ' || COALESCE(result_json, '') FROM jobs")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        format!(
            "task status: {}\nattempt: {:?}\nmoves: {:?}\njobs: {jobs:?}\nfleetd stderr: {}",
            self.task_status(),
            self.attempt(),
            self.moves(),
            self.fleetds.output("fleetd-0", "stderr")
        )
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = self.agent.kill();
        let _ = self.agent.wait();
    }
}

/// The task is not done, and no event moved it to `done`.
fn assert_not_done(w: &World) {
    assert_ne!(w.task_status(), "done", "{}", w.evidence());
    assert!(
        !w.moves().iter().any(|(to, _)| to == "done"),
        "{}",
        w.evidence()
    );
}

#[test]
fn passing_code_task_reaches_done_only_through_the_verifier() {
    let mut w = World::new(
        "p4v-pass",
        &Spec {
            check: "test -s fake-claude.txt",
            kind: WorkKind::Feature,
            own: &[],
        },
    );
    w.worker_says_done("commit,result");
    // The worker said "done". The task is not done: only the verifier moves
    // it, and no fleetd ran yet.
    assert_ne!(w.task_status(), "done");
    w.start_fleetd();
    let verdict = w.verdict(1);
    assert_eq!(verdict["decision"], "passed", "{verdict}");
    wait_for_or(
        "the task to be done",
        || (w.task_status() == "done").then_some(()),
        || w.evidence(),
    );
    // The only move to `done` names the verdict that judged the tip.
    let to_done: Vec<_> = w
        .moves()
        .into_iter()
        .filter(|(to, _)| to == "done")
        .collect();
    assert_eq!(to_done.len(), 1, "{}", w.evidence());
    assert!(
        to_done[0].1.contains("the verifier: verdict-1.json judged"),
        "{}",
        w.evidence()
    );
    assert!(w.worktree.join(FAKE_FILE).is_file());
}

#[test]
fn fm9_code_task_with_a_failing_check_stays_not_done() {
    let mut w = World::new(
        "p4v-fm9-code",
        &Spec {
            check: "test -f the-file-that-the-worker-never-wrote",
            kind: WorkKind::Feature,
            own: &[],
        },
    );
    w.worker_says_done("commit,result");
    w.start_fleetd();
    let verdict = w.verdict(1);
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    wait_for_or(
        "attempt A1 to end rejected",
        || (w.attempt().1.as_deref() == Some("rejected")).then_some(()),
        || w.evidence(),
    );
    assert_not_done(&w);
}

#[test]
fn fm9_docs_task_without_its_section_stays_not_done() {
    let mut w = World::new(
        "p4v-fm9-docs",
        &Spec {
            // The check passes, so only the structural check can fail.
            check: "true",
            kind: WorkKind::Docs,
            own: &["docs/guide.md"],
        },
    );
    w.commit_file("docs/guide.md", "The guide has text and no section.\n");
    w.worker_says_done("result");
    w.start_fleetd();
    wait_for_or(
        "attempt A1 to end rejected",
        || (w.attempt().1.as_deref() == Some("rejected")).then_some(()),
        || w.evidence(),
    );
    // The check passed (verdict 1), so the structural check gave the
    // failure (verdict 2).
    let checked = w.verdict(1);
    assert_eq!(checked["decision"], "passed", "{checked}");
    let verdict = w.verdict(2);
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    assert!(
        verdict["reason"]
            .as_str()
            .unwrap()
            .contains("docs/guide.md"),
        "{verdict}"
    );
    assert_not_done(&w);
}

#[test]
fn sr4_check_that_writes_outside_its_paths_is_refused() {
    // A folder outside each writable path of a check: not in the worktree,
    // not in the fleet root, not in the temp folder of the harness.
    let outside = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let escaped = outside.path().join("escaped.txt");
    let check = format!(
        "echo inside > written-inside.txt && echo outside > {}",
        escaped.display()
    );
    let mut w = World::new(
        "p4v-sr4",
        &Spec {
            check: &check,
            kind: WorkKind::Feature,
            own: &[],
        },
    );
    w.worker_says_done("commit,result");
    w.start_fleetd();
    // The sandbox of the check on the `PATH` that fleetd and its jobs get.
    if let Some(gap) = horch_platform::sandbox::host_problem(Some(w.h.bin.as_os_str())) {
        // Orchestrator answer Q-A, rule 3: a host with no sandbox (Linux
        // without `bwrap`) gives NotSupported, and the check does not run at
        // all. SR-4 holds by refusal, not by a denied write.
        wait_for_or(
            "the task_check job to end with no sandbox",
            || w.check_job_ended().then_some(()),
            || w.evidence(),
        );
        assert!(
            !escaped.exists() && !w.worktree.join("written-inside.txt").exists(),
            "SR-4 on a host with NotSupported ({gap:?}): the check ran with no sandbox\n{}",
            w.evidence()
        );
        assert_not_done(&w);
        return;
    }
    let verdict = w.verdict(1);
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    // The check ran in its sandbox: its write inside the worktree stands,
    // and its write outside is refused.
    assert!(
        w.worktree.join("written-inside.txt").is_file(),
        "the check did not run, so its refusal proves nothing\n{verdict}\n{}",
        w.evidence()
    );
    assert!(
        !escaped.exists(),
        "SR-4: the check wrote {} outside its paths\n{}",
        escaped.display(),
        w.evidence()
    );
    assert_not_done(&w);
}
