//! The operator task and worker commands of `horch ledger` send 1 request
//! to fleetd and write no row themselves (RR-5, FM-24; ticket 22 of
//! fleetd-p5).
//!
//! Each test runs the development `horch` with temporary
//! `HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB` and fleet root,
//! and the watchdog off. The test process plays fleetd: it writes the
//! `fleetd` row with its own PID and start time, makes the operator token,
//! and serves the channel in-process with the handlers of the run, task,
//! worker and custody kinds.
//!
//! The tests are unix only: the socket path must be short.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{parse_reply, Outcome, Request};
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::handlers_custody::{self, Custody};
use horch_core::fleetd::fleet_runtime::application::handlers_run;
use horch_core::fleetd::fleet_runtime::application::handlers_task::{self, TaskHandlers};
use horch_core::fleetd::fleet_runtime::application::handlers_worker::{self, WorkerDeps};
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::integration::application::commands::RefTips;
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};

const PATH: &str = "/usr/bin:/bin";

const TICKET: &str = "# 01: Alpha task\n\n<!-- horch-ticket: 1 -->\n**Persona:** primary `dev` · **Parallel:** yes\n**Kind:** feature\n**Check:** `true`\n**Own:** `src/a.rs`\n\n## Goal\n\nDo the alpha work.\n";

struct Out {
    code: i32,
    out: String,
    err: String,
}

/// git has no ref but `HEAD`: the HEAD of the folder, which the drop
/// handler reads.
struct HeadOnly;

impl RefTips for HeadOnly {
    fn tip(&self, repo: &Path, name: &str) -> anyhow::Result<Option<String>> {
        if name != "HEAD" {
            return Ok(None);
        }
        let out = Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])
            .current_dir(repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()?;
        Ok(out
            .status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()))
    }
}

/// A fleet in temporary folders. With [`World::up`], the test process
/// serves the channel as fleetd.
struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    server: Option<ChannelServer>,
}

impl World {
    /// The ledger, the `fleetd` row of this process and the token, and no
    /// server: fleetd is down.
    fn down() -> World {
        // A short folder: `sun_path` holds about 104 bytes on macOS.
        let tmp = tempfile::Builder::new()
            .prefix("rt")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for dir in ["home", "fleet", "proj"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let w = World {
            _tmp: tmp,
            root,
            server: None,
        };
        let store = Store::open(&w.ledger()).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, ?1, ?2, ?3, 0, ?3)",
                params![std::process::id(), this_start(), util::now()],
            )
            .unwrap();
        token_file::ensure(&w.fleet()).unwrap();
        w
    }

    /// [`World::down`], then the channel served by this process.
    fn up() -> World {
        let mut w = World::down();
        let mut handlers = Handlers::new();
        handlers_run::register(&mut handlers);
        handlers_task::register(
            &mut handlers,
            TaskHandlers {
                config: Arc::new(FleetConfig::default()),
                evidence_root: w.evidence(),
            },
        );
        handlers_worker::register(
            &mut handlers,
            &WorkerDeps {
                guard: Generation(1),
            },
        );
        handlers_custody::register(
            &mut handlers,
            Arc::new(Custody {
                guard: Generation(1),
                config: FleetConfig::default(),
                refs: Box::new(HeadOnly),
            }),
        );
        let conn = Connection::open(w.ledger()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        w.server = Some(ChannelServer::start(&w.fleet(), conn, Generation(1), handlers).unwrap());
        w
    }

    fn ledger(&self) -> PathBuf {
        self.root.join("ledger.sqlite3")
    }

    /// The evidence folder that the command and the handlers read.
    fn evidence(&self) -> PathBuf {
        self.root.join("evidence")
    }

    fn fleet(&self) -> PathBuf {
        self.root.join("fleet")
    }

    fn proj(&self) -> PathBuf {
        self.root.join("proj")
    }

    /// `horch` with `args` and the extra environment `env`.
    fn horch_with(&self, args: &[&str], env: &[(&str, &str)]) -> Out {
        let root = &self.root;
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.ledger())
            .env("HORCH_PROJECT_DIR", self.proj())
            .env("FLEET_ROOT", self.fleet())
            .env("HORCH_WORKSPACE_ID", "w1")
            .env("HORCH_NO_WATCHDOG", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        Out {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// `horch` with `args` and `env`, which must exit 0.
    fn ok_with(&self, args: &[&str], env: &[(&str, &str)]) -> Out {
        let o = self.horch_with(args, env);
        assert_eq!(
            o.code, 0,
            "{args:?}\nstdout:\n{}\nstderr:\n{}",
            o.out, o.err
        );
        o
    }

    fn ok(&self, args: &[&str]) -> Out {
        self.ok_with(args, &[])
    }

    fn conn(&self) -> Connection {
        let conn = Connection::open(self.ledger()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        conn
    }

    fn sql(&self, sql: &str) {
        self.conn().execute_batch(sql).unwrap();
    }

    fn one(&self, sql: &str) -> String {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// The kind and state of each `requests` row, in the order of receipt.
    fn requests(&self) -> Vec<(String, String)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT kind, state FROM requests ORDER BY received_at, rowid")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn run(&self, base: Option<&str>) -> String {
        let mut store = Store::open(&self.ledger()).unwrap();
        store
            .immediate(|tx| {
                let new = NewRun {
                    slug: "r1".into(),
                    base_branch: base.map(Into::into),
                    ..Default::default()
                };
                let run = runs::create(tx, &self.proj(), &new)?;
                horch_core::taskledger::schema::run_idea(tx, &run.run_id, &util::now(), true)?;
                Ok(run.run_id)
            })
            .unwrap()
    }

    fn raw_request_refusal(&self, kind: Kind, payload: serde_json::Value) -> String {
        let serde_json::Value::Object(payload) = payload else {
            panic!("a payload is an object");
        };
        let token = token_file::read(&self.fleet()).unwrap();
        let request = Request {
            request_id: RequestId::parse("raw-in-attempt").unwrap(),
            kind,
            payload,
            token: Some(token.text().to_string()),
        };
        let stream =
            std::os::unix::net::UnixStream::connect(self.fleet().join("run/fleetd.sock")).unwrap();
        (&stream).write_all(request.to_line().as_bytes()).unwrap();
        let mut reply = String::new();
        BufReader::new(&stream).read_line(&mut reply).unwrap();
        match parse_reply(reply.as_bytes(), 1 << 20).unwrap().outcome {
            Outcome::Refused(reason) => reason,
            Outcome::Applied(result) => panic!("fleetd applied {kind:?}: {result}"),
        }
    }

    /// A run `r1` with the task T-1 of [`TICKET`].
    fn run_with_task(&self) {
        let run_id = self.run(None);
        let mut store = Store::open(&self.ledger()).unwrap();
        store
            .immediate(|tx| {
                let run = runs::get(tx, &run_id)?;
                let mut new = NewTask::work("Alpha task", "t1.md", "dev");
                new.ticket_snapshot = Some(TICKET.to_string());
                tasks::add(tx, &run, &new, Actor::Orchestrator)?;
                Ok(())
            })
            .unwrap();
    }

    /// A worker of the run that holds T-1 and has a live attempt whose
    /// wrapper is this test process: each `horch` that this test starts is
    /// in that attempt.
    fn attempt_of_this_process(&self) {
        let conn = self.conn();
        let run: String = conn
            .query_row("SELECT run_id FROM runs WHERE slug = 'r1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let worker = workers::insert(
            &conn,
            &NewWorker {
                run_id: run,
                kind: Some(WorkerKind::Worker),
                persona: "dev".into(),
                role: Some("dev-1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at)
             VALUES ('a1', (SELECT task_id FROM tasks WHERE seq = 1), ?1, ?2, ?3, 'live', ?4)",
            params![worker, std::process::id(), this_start(), util::now()],
        )
        .unwrap();
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.stop();
        }
    }
}

/// The start time of this test process.
fn this_start() -> i64 {
    let start = horch_platform::process::start_time(std::process::id())
        .expect("the start time of the test");
    i64::try_from(start).unwrap()
}

fn applied(kinds: &[&str]) -> Vec<(String, String)> {
    kinds
        .iter()
        .map(|k| ((*k).to_string(), "applied".to_string()))
        .collect()
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The value of the line `<key>: <value>` of `out`.
fn line_value(out: &str, key: &str) -> String {
    out.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}: ")))
        .unwrap_or_else(|| panic!("no {key} in {out}"))
        .to_string()
}

#[test]
fn requeue_cancel_and_waive_send_their_kinds() {
    let w = World::up();
    w.run_with_task();
    w.sql("UPDATE tasks SET status = 'escalated', held_reason = 'stuck' WHERE seq = 1");
    let o = w.ok(&["ledger", "requeue", "T-1", "try again"]);
    assert!(o.out.contains("T-1 is queued again."), "{}", o.out);
    assert_eq!(w.one("SELECT status FROM tasks WHERE seq = 1"), "ready");

    // Round 1 of the code sweep of T-1, whose report names F1.
    let task_id = w.one("SELECT task_id FROM tasks WHERE seq = 1");
    let round = w.evidence().join(&task_id).join("round-1");
    std::fs::create_dir_all(&round).unwrap();
    std::fs::write(
        round.join("report.md"),
        "# Code sweep\n\n- F1 [Important] a finding\n",
    )
    .unwrap();
    let state = serde_json::json!({ "round": 1, "tree": "abc", "dir": round, "reviews": [] });
    std::fs::write(
        w.evidence().join(&task_id).join("code-sweep-state.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let o = w.ok(&["ledger", "waive", "T-1", "F1", "out of scope"]);
    assert!(o.out.contains("T-1 round 1: F1 waived."), "{}", o.out);

    let o = w.ok(&["ledger", "cancel", "T-1", "not needed"]);
    assert!(o.out.contains("T-1 cancelled."), "{}", o.out);
    assert_eq!(w.one("SELECT status FROM tasks WHERE seq = 1"), "cancelled");
    assert_eq!(
        w.requests(),
        applied(&["task_requeue", "ledger_waive", "task_cancel"])
    );
}

#[test]
fn worker_that_sets_horch_role_orchestrator_is_refused_by_fleetd() {
    let w = World::up();
    w.run_with_task();
    w.attempt_of_this_process();
    let o = w.horch_with(
        &["ledger", "cancel", "T-1", "mine now"],
        &[("HORCH_ROLE", "orchestrator")],
    );
    assert_eq!(o.code, 1, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    assert!(
        o.err.contains("refused inside a fleet attempt")
            && o.err.contains("fleet process of attempt a1"),
        "{}",
        o.err
    );
    assert!(w.requests().is_empty(), "{:?}", w.requests());

    let reason = w.raw_request_refusal(
        Kind::TaskCancel,
        serde_json::json!({ "task": "T-1", "reason": "mine now" }),
    );
    assert!(
        reason.contains("the peer process is a fleet process of attempt a1"),
        "{reason}"
    );
    assert_eq!(
        w.requests(),
        vec![("task_cancel".to_string(), "refused".to_string())]
    );
    assert_ne!(w.one("SELECT status FROM tasks WHERE seq = 1"), "cancelled");
}

#[test]
fn worker_that_removes_horch_role_is_refused_by_fleetd() {
    let w = World::up();
    w.run_with_task();
    w.attempt_of_this_process();
    let o = w.horch_with(&["ledger", "requeue", "T-1", "again"], &[]);
    assert_eq!(o.code, 1, "stdout:\n{}\nstderr:\n{}", o.out, o.err);
    assert!(
        o.err.contains("refused inside a fleet attempt")
            && o.err.contains("fleet process of attempt a1"),
        "{}",
        o.err
    );
    assert!(w.requests().is_empty(), "{:?}", w.requests());

    let reason = w.raw_request_refusal(
        Kind::TaskRequeue,
        serde_json::json!({ "task": "T-1", "reason": "again" }),
    );
    assert!(
        reason.contains("the peer process is a fleet process of attempt a1"),
        "{reason}"
    );
    assert_eq!(
        w.requests(),
        vec![("task_requeue".to_string(), "refused".to_string())]
    );
    assert_eq!(w.one("SELECT status FROM tasks WHERE seq = 1"), "ready");
}

#[test]
fn worktree_drop_sends_worktree_drop() {
    let w = World::up();
    let proj = w.proj();
    git(&proj, &["init", "-q", "-b", "main"]);
    git(&proj, &["config", "user.email", "t@example.com"]);
    git(&proj, &["config", "user.name", "t"]);
    std::fs::write(proj.join("t1.md"), TICKET).unwrap();
    git(&proj, &["add", "t1.md"]);
    git(&proj, &["commit", "-q", "-m", "first"]);
    let run_id = w.run(Some("main"));
    let id = workers::insert(
        &w.conn(),
        &NewWorker {
            run_id,
            kind: Some(WorkerKind::Worker),
            persona: "dev".into(),
            role: Some("dev-1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let env = [("HORCH_WORKER_ID", id.as_str())];

    let o = w.ok_with(&["ledger", "worktree", "scratch", "try"], &env);
    let path = line_value(&o.out, "path");
    let branch = line_value(&o.out, "branch");
    // The `worktree_add` job of the scratch tree, as the job process runs it.
    git(
        &proj,
        &["worktree", "add", "-q", "-b", &branch, &path, "main"],
    );
    w.sql("UPDATE worktrees SET state = 'created'");

    let o = w.ok_with(
        &[
            "ledger",
            "worktree",
            "drop",
            "try",
            "--reason",
            "not needed",
        ],
        &env,
    );
    assert!(o.out.contains("abandoned: "), "{}", o.out);
    assert_eq!(w.one("SELECT status FROM worktrees"), "abandoned");
    assert_eq!(
        w.requests(),
        applied(&["worktree_scratch", "worktree_drop"])
    );
}

#[test]
fn fleetd_down_queues_a_request_file() {
    let w = World::down();
    let o = w.ok(&["ledger", "dismiss", "7", "old"]);
    assert!(o.err.contains("fleetd is down: request "), "{}", o.err);
    let files: Vec<_> = std::fs::read_dir(w.fleet().join("run").join("requests"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    let line = std::fs::read_to_string(&files[0]).unwrap();
    assert!(line.contains("\"ledger_dismiss\""), "{line}");
    assert!(w.requests().is_empty());
}
