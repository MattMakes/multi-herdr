//! `horch job requeue`, `horch task`, `horch worktree restore` and
//! `triage apply`, `horch feature abandon` and `horch branch abandon` send
//! 1 request to fleetd and write no row themselves (RR-5, FM-24; ticket 25
//! of fleetd-p5).
//!
//! Each test runs the development `horch` with temporary
//! `HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB` and fleet root,
//! and the watchdog off. The test process plays fleetd: it writes the
//! `fleetd` row with its own PID and start time, makes the operator token,
//! and serves the channel in-process with the task and custody handlers.
//!
//! The tests are unix only: the socket path must be short.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, NewRun as NewJob};
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::handlers_custody::{self, Custody};
use horch_core::fleetd::fleet_runtime::application::handlers_task::{self, TaskHandlers};
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::Kind as JobKind;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::sqlite as rows;
use horch_core::fleetd::integration::application::commands::GitRefTips;
use horch_core::fleetd::integration::application::landing;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};

const PATH: &str = "/usr/bin:/bin";
const GIT: &str = "/usr/bin/git";
const FEATURE: &str = "F-0001";
const RUN_BRANCH: &str = "fleet/r1";

struct Out {
    code: i32,
    out: String,
    err: String,
}

/// A fleet in temporary folders: a git project with 1 commit on `main`
/// and the run branch, a ledger with the run `r1` and its feature, and
/// the channel served by this process as fleetd.
struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    run_id: String,
    server: Option<ChannelServer>,
}

impl World {
    fn new() -> World {
        // A short folder: `sun_path` holds about 104 bytes on macOS.
        let tmp = tempfile::Builder::new()
            .prefix("ro")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for dir in ["home", "fleet", "proj"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let mut w = World {
            _tmp: tmp,
            root,
            run_id: String::new(),
            server: None,
        };
        let proj = w.proj();
        git(&proj, &["init", "-q", "-b", "main"]);
        std::fs::write(proj.join("a.txt"), "a\n").unwrap();
        git(&proj, &["add", "a.txt"]);
        git(&proj, &["commit", "-q", "-m", "first"]);
        git(&proj, &["branch", RUN_BRANCH]);
        let mut store = Store::open(&w.ledger()).unwrap();
        let run = store
            .immediate(|tx| {
                let new_run = NewRun {
                    slug: "r1".into(),
                    base_branch: Some("main".into()),
                    run_branch: Some(RUN_BRANCH.into()),
                    ..Default::default()
                };
                let run = runs::create(tx, &proj, &new_run)?;
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        w.run_id = run.run_id;
        let pid = std::process::id();
        let start = horch_platform::process::start_time(pid).expect("the start time of the test");
        store
            .conn()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, ?1, ?2, ?3, 0, ?3)",
                params![pid, i64::try_from(start).unwrap(), util::now()],
            )
            .unwrap();
        token_file::ensure(&w.fleet()).unwrap();
        w.server = Some(w.serve());
        w
    }

    /// The channel with the task and custody handlers of fleetd.
    fn serve(&self) -> ChannelServer {
        let mut handlers = Handlers::new();
        let config = FleetConfig::default();
        handlers_task::register(
            &mut handlers,
            TaskHandlers {
                config: Arc::new(config.clone()),
                evidence_root: self.root.join("evidence"),
            },
        );
        let refs = CliWorktreeGit::new(GIT, self.fleet()).reading(&[self.proj()]);
        handlers_custody::register(
            &mut handlers,
            Arc::new(Custody {
                guard: Generation(1),
                config,
                refs: Box::new(GitRefTips { git: refs }),
            }),
        );
        let conn = Connection::open(self.ledger()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        ChannelServer::start(&self.fleet(), conn, Generation(1), handlers).unwrap()
    }

    fn ledger(&self) -> PathBuf {
        self.root.join("ledger.sqlite3")
    }

    fn fleet(&self) -> PathBuf {
        self.root.join("fleet")
    }

    fn proj(&self) -> PathBuf {
        self.root.join("proj")
    }

    fn horch_with(&self, args: &[&str], extra: &[(&str, &str)]) -> Out {
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
        for (k, v) in extra {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        Out {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// `horch` with `args`, which must exit 0.
    fn ok(&self, args: &[&str]) -> Out {
        self.ok_with(args, &[])
    }

    fn ok_with(&self, args: &[&str], extra: &[(&str, &str)]) -> Out {
        let o = self.horch_with(args, extra);
        assert_eq!(
            o.code, 0,
            "{args:?}\nstdout:\n{}\nstderr:\n{}",
            o.out, o.err
        );
        o
    }

    fn conn(&self) -> Connection {
        Connection::open(self.ledger()).unwrap()
    }

    fn sql(&self, sql: &str) {
        self.conn().execute_batch(sql).unwrap();
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

    fn one(&self, sql: &str) -> String {
        self.conn()
            .query_row(sql, [], |r| {
                let v: rusqlite::types::Value = r.get(0)?;
                Ok(match v {
                    rusqlite::types::Value::Text(t) => t,
                    rusqlite::types::Value::Integer(i) => i.to_string(),
                    other => format!("{other:?}"),
                })
            })
            .unwrap()
    }

    /// 2 queued tasks of the run: T-1 and T-2.
    fn tasks(&self) {
        let mut store = Store::open(&self.ledger()).unwrap();
        store
            .immediate(|tx| {
                let run = runs::get(tx, &self.run_id)?;
                for t in ["a", "b"] {
                    let new = NewTask::work(t, &format!("tickets/{t}.md"), "dev");
                    tasks::add(tx, &run, &new, Actor::Orchestrator)?;
                }
                Ok(())
            })
            .unwrap();
    }

    /// A task `title` with its branch row in `building`, and the branch in
    /// git with 1 commit. Gives the branch and its tip.
    fn task_branch(&self, title: &str) -> (String, String) {
        let branch = format!("{RUN_BRANCH}-{title}");
        let config = FleetConfig::default();
        let mut store = Store::open(&self.ledger()).unwrap();
        store
            .immediate(|tx| {
                let run = runs::get(tx, &self.run_id)?;
                let new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
                let task = tasks::add(tx, &run, &new, Actor::Orchestrator)?.task_id;
                let jobs = JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                };
                landing::bind_branch(&jobs, tx, &task, &branch, FleetSeconds(0))
            })
            .unwrap();
        let proj = self.proj();
        git(&proj, &["checkout", "-q", "-b", &branch, RUN_BRANCH]);
        std::fs::write(proj.join(format!("{title}.txt")), "work\n").unwrap();
        git(&proj, &["add", "."]);
        git(&proj, &["commit", "-q", "-m", title]);
        git(&proj, &["checkout", "-q", "main"]);
        let tip = git(&proj, &["rev-parse", &branch]);
        (branch, tip)
    }

    /// The landing state and the rescue ref of the row of `branch`.
    fn branch_row(&self, branch: &str) -> (String, Option<String>) {
        let row = rows::branch(&self.conn(), branch).unwrap().unwrap();
        (row.landing_state.unwrap(), row.rescue_ref)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.stop();
        }
    }
}

fn applied(kinds: &[&str]) -> Vec<(String, String)> {
    kinds
        .iter()
        .map(|k| ((*k).to_string(), "applied".to_string()))
        .collect()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(GIT)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A `push` job of `F-1` that ended abandoned 3 times. Gives its id.
fn held_job(w: &World) -> i64 {
    let id = jobs_store::insert(
        &w.conn(),
        &NewJob {
            kind: JobKind::Push,
            subject: "F-1",
            repo: None,
            input_key: "tipA",
            run: 3,
            abandoned_runs: 3,
            not_before: None,
            time_limit_s: 60,
        },
    )
    .unwrap();
    w.sql(&format!(
        "UPDATE jobs SET state = 'abandoned' WHERE id = {id}"
    ));
    id
}

#[test]
fn job_requeue_sends_job_requeue() {
    let w = World::new();
    let held = held_job(&w);
    let o = w.ok(&["job", "requeue", &held.to_string()]);
    let new = w.one("SELECT max(id) FROM jobs");
    assert_eq!(
        o.out.trim(),
        format!("Queued run 4 of the push job of F-1 as job {new}.")
    );
    assert_eq!(w.requests(), applied(&["job_requeue"]));
    assert_eq!(
        w.one(&format!(
            "SELECT run || '|' || abandoned_runs || '|' || state FROM jobs WHERE id = {new}"
        )),
        "4|0|queued"
    );

    // A refused request prints `refused: <reason>` and exits 1.
    let o = w.horch_with(&["job", "requeue", &held.to_string()], &[]);
    assert_eq!(o.code, 1, "{}", o.err);
    assert!(
        o.err.contains(&format!(
            "refused: job {new} of the same kind and subject is queued"
        )),
        "{}",
        o.err
    );
}

#[test]
fn task_requeue_and_cancel_send_their_kinds() {
    let w = World::new();
    w.tasks();
    w.sql(
        "UPDATE tasks SET status = 'escalated', attempts = 3, held_reason = 'stuck' WHERE seq = 1",
    );
    let o = w.ok(&["task", "requeue", "T-1", "try again"]);
    assert_eq!(o.out.trim(), "T-1 is queued again.");
    let o = w.ok(&["task", "cancel", "T-2", "dropped"]);
    assert!(o.out.contains("T-2 cancelled."), "{}", o.out);
    assert_eq!(w.requests(), applied(&["task_requeue", "task_cancel"]));
    assert_eq!(
        w.one("SELECT group_concat(status, ',') FROM (SELECT status FROM tasks ORDER BY seq)"),
        "ready,cancelled"
    );
}

#[test]
fn worktree_restore_and_triage_send_their_kinds() {
    let w = World::new();
    let proj = w.proj();
    // A folder that git knows and the ledger does not: triage row W1.
    let tree = proj.join(".worktrees").join("feat");
    git(
        &proj,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat",
            tree.to_str().unwrap(),
        ],
    );
    let reply = w.root.join("reply.md");
    std::fs::write(
        &reply,
        "| ID | Worktree | Suggested | Decision | Direction |\n|----|----|----|----|----|\n| W1 | feat | | KEEP | |\n",
    )
    .unwrap();
    let o = w.ok(&["worktree", "triage", "apply", reply.to_str().unwrap()]);
    assert!(o.out.starts_with("W1 feat KEEP"), "{}", o.out);

    // An archive ref of a folder that is gone.
    let tip = git(&proj, &["rev-parse", "main"]);
    git(&proj, &["update-ref", "refs/horch/archive/old", &tip]);
    let o = w.ok(&["worktree", "restore", "old"]);
    assert!(
        o.out.contains(&format!(
            "on restored/old at {} from refs/horch/archive/old",
            &tip[..7]
        )),
        "{}",
        o.out
    );
    assert_eq!(
        w.requests(),
        applied(&["worktree_triage", "worktree_restore"])
    );
    assert_eq!(
        w.one("SELECT branch || '|' || state FROM worktrees WHERE path LIKE '%/.worktrees/old'"),
        "restored/old|reserved"
    );
}

/// `ARCHIVE` is 1 `worktree_triage` request, and fleetd queues 1
/// `triage_apply` job for it (T-888). The command runs no git write: the
/// folder stays until the job runs.
#[test]
fn worktree_triage_archive_sends_1_request_and_queues_1_job() {
    let w = World::new();
    let proj = w.proj();
    let tree = proj.join(".worktrees").join("feat");
    git(
        &proj,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat",
            tree.to_str().unwrap(),
        ],
    );
    std::fs::write(tree.join("f.txt"), "work\n").unwrap();
    git(&tree, &["add", "f.txt"]);
    git(&tree, &["commit", "-q", "-m", "feat"]);
    let reply = w.root.join("reply.md");
    std::fs::write(
        &reply,
        "| ID | Worktree | Suggested | Decision | Direction |\n|----|----|----|----|----|\n| W1 | feat | | ARCHIVE | |\n",
    )
    .unwrap();
    let o = w.horch_with(
        &["worktree", "triage", "apply", reply.to_str().unwrap()],
        &[],
    );
    assert_eq!(o.code, 0, "{}{}", o.out, o.err);
    let job = w.one("SELECT id FROM jobs WHERE kind = 'triage_apply' AND state = 'queued'");
    assert!(
        o.out.contains(&format!(
            "W1 feat ARCHIVE: queued in the triage_apply job {job}"
        )),
        "{}",
        o.out
    );
    assert_eq!(w.requests(), applied(&["worktree_triage"]));
    assert!(tree.exists());
    let archive = git(
        &w.proj(),
        &["for-each-ref", "--format=%(refname)", "refs/horch/archive/"],
    );
    assert!(archive.is_empty(), "{archive}");
}

#[test]
fn feature_branch_and_rescue_commands_send_their_kinds() {
    let w = World::new();
    let (lone, lone_tip) = w.task_branch("lone");
    let o = w.ok(&["branch", "abandon", &lone]);
    let (state, rescue) = w.branch_row(&lone);
    let rescue = rescue.expect("the rescue ref of the branch");
    assert_eq!(state, "abandoned");
    assert_eq!(git(&w.proj(), &["rev-parse", &rescue]), lone_tip);
    assert_eq!(
        o.out.trim(),
        format!("{lone} is abandoned; its rescue ref {rescue} keeps its tip.")
    );

    let (other, other_tip) = w.task_branch("other");
    let o = w.ok(&["feature", "abandon", FEATURE]);
    let (state, rescue) = w.branch_row(&other);
    let rescue = rescue.expect("the rescue ref of the branch");
    assert_eq!(state, "abandoned");
    assert_eq!(git(&w.proj(), &["rev-parse", &rescue]), other_tip);
    assert!(
        o.out.contains(&format!(
            "{other} is abandoned; its rescue ref {rescue} keeps its tip."
        )),
        "{}",
        o.out
    );
    assert!(o.out.contains("F-0001 is abandoned."), "{}", o.out);
    assert_eq!(
        w.requests(),
        applied(&["branch_abandon", "feature_abandon"])
    );
    assert_eq!(
        w.one(&format!(
            "SELECT state FROM features WHERE id = '{FEATURE}'"
        )),
        "abandoned"
    );
    // `horch rescue drop` sends 1 `rescue_drop` request, then deletes the
    // ref (T-885).
    w.ok(&["rescue", "drop", &rescue]);
    assert_eq!(
        w.requests(),
        applied(&["branch_abandon", "feature_abandon", "rescue_drop"])
    );
    assert_eq!(
        w.one(&format!(
            "SELECT count(*) FROM events WHERE entity_id = '{rescue}' AND kind = 'note'"
        )),
        "1"
    );
}

/// `HORCH_ROLE` and `HORCH_WORKER_ID` decide nothing in the command: each
/// command sends its request, and fleetd decides.
#[test]
fn each_operator_command_that_sets_horch_role_is_still_decided_by_fleetd() {
    let w = World::new();
    w.tasks();
    let held = held_job(&w);
    let (branch, _) = w.task_branch("lone");
    let worker = [("HORCH_ROLE", "sonnet-1"), ("HORCH_WORKER_ID", "w-1")];
    w.ok_with(&["job", "requeue", &held.to_string()], &worker);
    w.ok_with(&["task", "cancel", "T-1", "dropped"], &worker);
    w.ok_with(&["branch", "abandon", &branch], &worker);
    w.ok_with(&["feature", "abandon", FEATURE], &worker);
    assert_eq!(
        w.requests(),
        applied(&[
            "job_requeue",
            "task_cancel",
            "branch_abandon",
            "feature_abandon"
        ])
    );
}
