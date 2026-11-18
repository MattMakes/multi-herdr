//! `horch worker` - the port of `scripts/herdr-fleet/worker.sh`.
//!
//! `horch spawn` writes a brief (the resolved teammate, model, task, session and
//! record ids) into the workspace mailbox and then runs this command in the
//! fresh pane. `execution::lifecycle::run_worker` registers the role, renders
//! that teammate's briefing, launches the agent CLI and records how it ended.
//!
//! The bash version `exec`ed the agent. There is no `exec` on Windows, so the
//! agent runs as a child and its exit status is propagated.
//!
//! Until the fleet-run wrapper of Phase 5, this command is the wrapper of a
//! push attempt. When its agent exits with commits since the base and with
//! no `result.json` and no question file, it runs the repair pass of FM-6:
//! 1 short resume of the same session with the fixed line of
//! `domain/repair.rs`, before it records the exit. The wrapper writes no
//! ledger row (RR-5): `exit.json` of the attempt says that the repair ran.
//!
//! An exec attempt (fleetd-p7 ticket 17) has the same repair pass, with
//! its resume in exec mode. An exec attempt that ended on a usage limit
//! has none: its `exit.json` has the outcome hint `quota`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result};
use horch_core::execution::lifecycle::{run_worker, PaneWorker, WorkerSteps};
use horch_core::fleetd::fleet_runtime::adapters::sqlite::has_fact;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::fleetd::task_execution::domain::repair::{
    repair_pass, RepairFacts, RepairPort, Repaired, REPAIR_CAUSE,
};
use horch_core::fleetd::worktree_custody::adapters::sqlite as trees;
use horch_core::messaging::brief::Brief;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::EventKind;
use horch_core::taskledger::{runs, tasks, Store};
use rusqlite::OptionalExtension;

use super::resultcmd::{has_question, is_regular};

pub fn worker(ctx: &mut RuntimeContext, role: &str) -> Result<ExitCode> {
    // The process's working directory, not an environment variable: the
    // agent starts here, and a relative roster path resolves from here.
    let enter_dir = |dir: &Path| Ok(std::env::set_current_dir(dir)?);
    let code = run_worker(&mut Repairing(PaneWorker::new(ctx, role, &enter_dir)))?;
    Ok(match code {
        0 => ExitCode::SUCCESS,
        code => ExitCode::from(code.clamp(1, 255) as u8),
    })
}

/// The pane worker, with the repair pass after its agent exits.
struct Repairing<'a>(PaneWorker<'a>);

impl WorkerSteps for Repairing<'_> {
    fn load_brief(&mut self) -> Result<Brief> {
        self.0.load_brief()
    }

    fn enter_context(&mut self, brief: &Brief) -> Result<()> {
        self.0.enter_context(brief)
    }

    fn register(&mut self, brief: &Brief) -> Result<()> {
        self.0.register(brief)
    }

    fn set_running(&mut self, brief: &Brief) -> Result<()> {
        self.0.set_running(brief)
    }

    /// The agent, then the repair pass. The code is the one of the first
    /// exit: the repair is not an attempt. An exec attempt that ended on a
    /// usage limit ends with `quota`, not without its result: no repair.
    fn launch(&mut self, brief: &Brief) -> Result<Option<i32>> {
        let code = self.0.launch(brief)?;
        if self.0.exec_end().is_some_and(|end| !end.repair_applies()) {
            return Ok(code);
        }
        if let Err(e) = self.repair(brief) {
            eprintln!(
                "horch worker[{}]: the repair pass failed: {e:#}",
                brief.role
            );
        }
        Ok(code)
    }

    fn agent_exited(&mut self, brief: &Brief, code: Option<i32>) -> Result<()> {
        self.0.agent_exited(brief, code)
    }

    fn startup_failed(&mut self, brief: &Brief, error: &str) -> Result<()> {
        self.0.startup_failed(brief, error)
    }

    fn close_pane(&mut self) -> Result<()> {
        self.0.close_pane()
    }

    fn leave_startup_error(&mut self, error: &str, brief: Option<&Brief>) -> Result<()> {
        self.0.leave_startup_error(error, brief)
    }
}

impl Repairing<'_> {
    /// The repair pass of the push attempt of this worker. A worker with no
    /// task ledger or no push attempt has none.
    fn repair(&mut self, brief: &Brief) -> Result<()> {
        let Some((db, worker_id)) = self
            .0
            .ledger()
            .map(|(d, w)| (d.to_path_buf(), w.to_string()))
        else {
            return Ok(());
        };
        let Some(attempt) = PushAttempt::of_worker(&db, &worker_id, &self.0.ctx.paths.fleet_root)?
        else {
            return Ok(());
        };
        let git = Git::new(&self.0.ctx.bins.harness.git);
        let worker_repaired = self.0.repaired();
        let worker = &mut self.0;
        let mut resume = |line: &str| worker.resume(brief, line);
        let mut port = LedgerRepair {
            db,
            attempt,
            git,
            resume: &mut resume,
            recorded: worker_repaired,
        };
        let repaired = repair_pass(&mut port);
        let (recorded, id) = (port.recorded, port.attempt.id);
        if recorded {
            self.0.mark_repaired();
        }
        if let Repaired::Resumed(code) = repaired? {
            let how = code.map_or("a signal".to_string(), |c| format!("code {c}"));
            eprintln!(
                "horch worker[{}]: the repair resume of {id} ended with {how}",
                brief.role
            );
        }
        Ok(())
    }
}

/// The push attempt of a worker: its tree, the base of its branch and its
/// outbox.
#[derive(Debug)]
struct PushAttempt {
    id: String,
    tree: PathBuf,
    base: String,
    outbox: PathBuf,
}

impl PushAttempt {
    /// The newest attempt of `worker_id` with `via = push`. `None` for a
    /// worker of a claim attempt, or of no task.
    fn of_worker(db: &Path, worker_id: &str, fleet_root: &Path) -> Result<Option<PushAttempt>> {
        let store = Store::open_read_only(db)?;
        let conn = store.conn();
        let row: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT id, task_id FROM attempts WHERE worker_id = ?1 AND via = 'push'
                 ORDER BY rowid DESC LIMIT 1",
                [worker_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((id, Some(task_id))) = row else {
            return Ok(None);
        };
        let (tree, base) = match trees::of_task(conn, &task_id)? {
            Some(row) => (row.path, row.base_commit.unwrap_or(row.base)),
            None => {
                let run = runs::get(conn, &tasks::get(conn, &task_id)?.run_id)?;
                let base = run
                    .base_branch
                    .with_context(|| format!("the run of {id} has no base branch"))?;
                (run.project_root, base)
            }
        };
        let n = id
            .strip_prefix(&format!("{task_id}/"))
            .with_context(|| format!("the attempt id {id} does not start with its task"))?;
        let outbox = attempt_dir(fleet_root, &task_id, n).join("outbox");
        Ok(Some(PushAttempt {
            tree: PathBuf::from(tree),
            base,
            outbox,
            id,
        }))
    }
}

/// The repair pass of 1 push attempt on the task ledger (read-only), its
/// tree and its outbox.
struct LedgerRepair<'r> {
    db: PathBuf,
    attempt: PushAttempt,
    git: Git,
    resume: &'r mut dyn FnMut(&str) -> Result<Option<i32>>,
    /// The repair is recorded: this wrapper ran it, and its `exit.json`
    /// says so.
    recorded: bool,
}

impl RepairPort for LedgerRepair<'_> {
    type Error = anyhow::Error;

    /// Read after the agent exited. The outbox rule is the one of the Stop
    /// hook (`horch result --check`).
    fn facts(&mut self) -> Result<RepairFacts> {
        let a = &self.attempt;
        let range = format!("{}..HEAD", a.base);
        let count = self.git.run(&a.tree, &["rev-list", "--count", &range])?;
        let count: u64 = count
            .trim()
            .parse()
            .with_context(|| format!("git rev-list --count {range} printed {count:?}"))?;
        let store = Store::open_read_only(&self.db)?;
        Ok(RepairFacts {
            exited: true,
            commits_since_base: count > 0,
            result_json: is_regular(&a.outbox.join("result.json")),
            question_file: has_question(&a.outbox.join("questions")),
            repaired: self.recorded
                || has_fact(
                    store.conn(),
                    Entity::Attempt,
                    &a.id,
                    EventKind::Note,
                    REPAIR_CAUSE,
                )?,
        })
    }

    /// No ledger row (RR-5): `exit.json` carries the repair, and fleetd
    /// writes its event.
    fn record(&mut self) -> Result<()> {
        self.recorded = true;
        Ok(())
    }

    fn resume(&mut self, line: &str) -> Result<Option<i32>> {
        (self.resume)(line)
    }
}

// The tests make git worktrees, as the push dispatch tests do: Unix only.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
    use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
    use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
    use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
    use horch_core::fleetd::task_execution::application::dispatch::{bind, Bound, DispatchCtx};
    use horch_core::fleetd::task_execution::domain::repair::REPAIR_LINE;
    use horch_core::taskledger::model::Actor;
    use horch_core::taskledger::runs::NewRun;
    use horch_core::taskledger::tasks::NewTask;
    use horch_core::taskledger::workers::NewWorker;
    use std::process::Command;

    const RUN_BRANCH: &str = "fleet/r1";

    /// Run git in `dir` with no hook and no config of the user.
    fn git(dir: &Path, args: &[&str]) -> String {
        let o = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args([
                "-c",
                "commit.gpgSign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// A repository, a ledger, a fleet root, and 1 task bound to its first
    /// push attempt, with the tree of the attempt made.
    struct World {
        _tmp: tempfile::TempDir,
        db: PathBuf,
        fleet_root: PathBuf,
        bound: Bound,
        tree: PathBuf,
        /// The repairs that the wrapper recorded: `exit.json` says
        /// `repaired` when it is more than 0.
        repairs: std::cell::Cell<i64>,
    }

    impl World {
        fn new() -> World {
            let tmp = tempfile::tempdir().unwrap();
            let top = tmp.path().canonicalize().unwrap();
            let repo = top.join("repo");
            let fleet_root = top.join("fleet");
            std::fs::create_dir_all(&repo).unwrap();
            std::fs::create_dir_all(&fleet_root).unwrap();
            git(&repo, &["init", "--quiet", "-b", "main"]);
            std::fs::write(repo.join("README.md"), "base\n").unwrap();
            git(&repo, &["add", "README.md"]);
            git(&repo, &["commit", "--quiet", "-m", "base"]);
            git(&repo, &["branch", RUN_BRANCH]);
            let db = top.join("ledger.sqlite3");
            let mut store = Store::open(&db).unwrap();
            let config = FleetConfig::default();
            let git_bin = Git::new("git");
            let ctx = DispatchCtx {
                jobs: JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                },
                clock: FleetSeconds(0),
                git: &git_bin,
            };
            let bound = store
                .immediate(|tx| {
                    let run = runs::create(
                        tx,
                        &repo,
                        &NewRun {
                            slug: "r1".into(),
                            base_branch: Some("main".into()),
                            run_branch: Some(RUN_BRANCH.into()),
                            ..Default::default()
                        },
                    )?;
                    let task = NewTask::work("repair", "docs/repair.md", "opus");
                    let id = tasks::add(tx, &run, &task, Actor::Orchestrator)?.task_id;
                    bind(tx, &ctx, &id, "opus", &NewWorker::default())
                })
                .unwrap();
            let t = bound
                .worktree
                .clone()
                .expect("a worktree task binds a tree");
            git(
                &repo,
                &[
                    "worktree", "add", "--quiet", "-b", &t.branch, &t.path, "main",
                ],
            );
            World {
                _tmp: tmp,
                db,
                fleet_root,
                tree: PathBuf::from(t.path),
                bound,
                repairs: std::cell::Cell::new(0),
            }
        }

        fn commit(&self) {
            std::fs::write(self.tree.join("work.txt"), "work\n").unwrap();
            git(&self.tree, &["add", "work.txt"]);
            git(&self.tree, &["commit", "--quiet", "-m", "work"]);
        }

        fn outbox(&self) -> PathBuf {
            attempt_dir(
                &self.fleet_root,
                &self.bound.task_id,
                &format!("A{}", self.bound.attempt),
            )
            .join("outbox")
        }

        /// 1 repair pass of the attempt; the lines its resume got.
        fn pass(&self) -> (Repaired, Vec<String>) {
            let attempt = PushAttempt::of_worker(&self.db, &self.bound.worker_id, &self.fleet_root)
                .unwrap()
                .expect("the worker has a push attempt");
            let mut lines = Vec::new();
            let mut resume = |line: &str| {
                lines.push(line.to_string());
                Ok(Some(0))
            };
            let before = self.repairs.get() > 0;
            let mut port = LedgerRepair {
                db: self.db.clone(),
                attempt,
                git: Git::new("git"),
                resume: &mut resume,
                recorded: before,
            };
            let done = repair_pass(&mut port).unwrap();
            if port.recorded && !before {
                self.repairs.set(self.repairs.get() + 1);
            }
            (done, lines)
        }

        /// The repairs that the wrapper recorded for `exit.json`. The
        /// wrapper writes no ledger event of a repair (RR-5).
        fn repair_events(&self) -> i64 {
            let events: i64 = Store::open_read_only(&self.db)
                .unwrap()
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM events WHERE entity = 'attempt' AND entity_id = ?1
                     AND kind = 'note' AND cause = ?2",
                    [&self.bound.attempt_id, REPAIR_CAUSE],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(events, 0, "the wrapper writes no repair event");
            self.repairs.get()
        }

        /// The attempt count and status of the task, its attempt rows, and
        /// the state of the attempt.
        fn attempts(&self) -> (i64, String, i64, String) {
            let store = Store::open(&self.db).unwrap();
            let conn = store.conn();
            let task = tasks::get(conn, &self.bound.task_id).unwrap();
            let one = |sql: &str, arg: &str| -> rusqlite::Result<rusqlite::types::Value> {
                conn.query_row(sql, [arg], |r| r.get(0))
            };
            let rows = one(
                "SELECT COUNT(*) FROM attempts WHERE task_id = ?1",
                &self.bound.task_id,
            );
            let state = one(
                "SELECT state FROM attempts WHERE id = ?1",
                &self.bound.attempt_id,
            );
            let (rusqlite::types::Value::Integer(rows), rusqlite::types::Value::Text(state)) =
                (rows.unwrap(), state.unwrap())
            else {
                panic!("unexpected column types");
            };
            (task.attempts, task.status, rows, state)
        }
    }

    #[test]
    fn worker_repair_resumes_1_time_after_an_exit_with_commits_and_no_result() {
        let w = World::new();
        w.commit();
        assert_eq!(
            w.pass(),
            (Repaired::Resumed(Some(0)), vec![REPAIR_LINE.to_string()])
        );
        assert_eq!(w.repair_events(), 1);
    }

    #[test]
    fn worker_repair_skips_an_attempt_with_a_result() {
        let w = World::new();
        w.commit();
        std::fs::create_dir_all(w.outbox()).unwrap();
        std::fs::write(w.outbox().join("result.json"), "{}").unwrap();
        assert_eq!(w.pass(), (Repaired::No, Vec::new()));
        assert_eq!(w.repair_events(), 0);
    }

    #[test]
    fn worker_repair_skips_an_attempt_with_a_question_file() {
        let w = World::new();
        w.commit();
        let questions = w.outbox().join("questions");
        std::fs::create_dir_all(&questions).unwrap();
        // A temp file is not a question file (protocol rule 1).
        std::fs::write(questions.join("q1.json.tmp"), "{}").unwrap();
        assert_eq!(w.pass().0, Repaired::Resumed(Some(0)));
        let w = World::new();
        w.commit();
        let questions = w.outbox().join("questions");
        std::fs::create_dir_all(&questions).unwrap();
        std::fs::write(questions.join("q1.json"), "{}").unwrap();
        assert_eq!(w.pass(), (Repaired::No, Vec::new()));
    }

    #[test]
    fn worker_repair_skips_an_attempt_with_no_commits() {
        let w = World::new();
        assert_eq!(w.pass(), (Repaired::No, Vec::new()));
        assert_eq!(w.repair_events(), 0);
    }

    #[test]
    fn worker_second_repair_of_1_attempt_never_runs() {
        let w = World::new();
        w.commit();
        assert_eq!(w.pass().0, Repaired::Resumed(Some(0)));
        assert_eq!(w.pass(), (Repaired::No, Vec::new()));
        assert_eq!(w.repair_events(), 1);
    }

    #[test]
    fn worker_repair_burns_no_attempt() {
        let w = World::new();
        w.commit();
        let before = w.attempts();
        assert_eq!(before.0, 1, "the bind made attempt 1");
        w.pass();
        assert_eq!(w.attempts(), before);
    }

    #[test]
    fn worker_of_a_claim_attempt_has_no_push_attempt() {
        let w = World::new();
        Store::open(&w.db)
            .unwrap()
            .conn()
            .execute("UPDATE attempts SET via = 'claim'", [])
            .unwrap();
        let found = PushAttempt::of_worker(&w.db, &w.bound.worker_id, &w.fleet_root).unwrap();
        assert!(found.is_none(), "{found:?}");
    }
}
