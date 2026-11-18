//! `horch task requeue` and `horch task cancel`: the commands of the
//! operator on 1 task (`docs/specs/fleetd/design.md`, "The task state
//! machine" and "A cancel").
//!
//! `horch ledger requeue` and `horch ledger cancel` run the same code, with
//! the same arguments, lines and exit codes. Before Phase 5 the commands
//! write the ledger directly through `transition()`.

use std::process::ExitCode;

use anyhow::Result;
use clap::Subcommand;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger;

use super::taskledger::{cancel_cmd, requeue_cmd, RunArg};

#[derive(Subcommand)]
pub enum TaskCommand {
    /// Queue a held task again. An escalated task gets its attempt count
    /// at 0; a failed task gets its 1 replan. A cancelled task is terminal.
    Requeue {
        task: String,
        reason: String,
        #[command(flatten)]
        run: RunArg,
    },
    /// Cancel a task. A live attempt is stopped first by its recorded PIDs
    /// and pane; the task is cancelled when its process is gone.
    Cancel {
        task: String,
        reason: String,
        #[command(flatten)]
        run: RunArg,
    },
}

/// `horch task`.
pub fn run(ctx: &RuntimeContext, command: TaskCommand) -> Result<ExitCode> {
    let mut store = taskledger::open(ctx)?;
    match command {
        TaskCommand::Requeue { task, reason, run } => {
            requeue_cmd(ctx, &mut store, &task, &reason, &run)
        }
        TaskCommand::Cancel { task, reason, run } => {
            cancel_cmd(ctx, &mut store, &task, &reason, &run)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use horch_core::runtime::{MapEnv, RuntimeContext};
    use horch_core::taskledger::model::Actor;
    use horch_core::taskledger::runs::{self, NewRun};
    use horch_core::taskledger::tasks::{self, NewTask};
    use horch_core::taskledger::workers::{self, NewWorker};
    use horch_core::taskledger::Store;

    use super::*;
    use crate::cmd::taskledger::LedgerCommand;

    struct World {
        _dir: tempfile::TempDir,
        ctx: RuntimeContext,
        db: PathBuf,
        root: PathBuf,
        /// The worker that claimed T-1.
        worker: String,
    }

    /// A ledger with the run "r1" and 2 tasks: T-1, and T-2 that waits for
    /// T-1. A worker with no recorded process claimed T-1. `shape` then
    /// changes the ledger.
    fn world(shape: impl FnOnce(&mut Store, &str)) -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let db = dir.path().join("ledger.sqlite3");
        let mut store = Store::open(&db).unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| runs::create(tx, &root, &new_run))
            .unwrap();
        let t1 = store
            .immediate(|tx| {
                let a = NewTask::work("a", "tickets/a.md", "dev");
                let t1 = tasks::add(tx, &run, &a, Actor::Orchestrator)?;
                let mut b = NewTask::work("b", "tickets/b.md", "dev");
                b.blocked_by = vec!["T-1".into()];
                tasks::add(tx, &run, &b, Actor::Orchestrator)?;
                Ok(t1.task_id)
            })
            .unwrap();
        let w = NewWorker {
            run_id: run.run_id.clone(),
            persona: "dev".into(),
            ..Default::default()
        };
        let worker = store
            .immediate(|tx| {
                let id = workers::insert(tx, &w)?;
                tasks::claim(tx, &id)?;
                Ok(id)
            })
            .unwrap();
        shape(&mut store, &t1);
        let env = MapEnv::new(&root)
            .with("HORCH_PROJECT_DIR", root.to_str().unwrap())
            .with("HORCH_LEDGER_DB", db.to_str().unwrap());
        World {
            ctx: RuntimeContext::from_env(&env).unwrap(),
            _dir: dir,
            db,
            root,
            worker,
        }
    }

    fn rows_of(db: &Path, sql: &str) -> Vec<String> {
        let store = Store::open(db).unwrap();
        let mut stmt = store.conn().prepare(sql).unwrap();
        let n = stmt.column_count();
        let rows = stmt
            .query_map([], |r| {
                let mut cells = Vec::new();
                for i in 0..n {
                    let v: rusqlite::types::Value = r.get(i)?;
                    cells.push(format!("{v:?}"));
                }
                Ok(cells.join(" | "))
            })
            .unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    }

    /// The rows that a cancel or a requeue writes, with no id and no time.
    fn rows(w: &World) -> Vec<String> {
        [
            "SELECT seq, status, attempts, attempt_floor, replans, held_reason, summary,
                cancel_requested, current_worker_id IS NULL, finished_at IS NULL
             FROM tasks ORDER BY seq",
            "SELECT state, outcome FROM attempts ORDER BY rowid",
            "SELECT status FROM workers ORDER BY rowid",
            "SELECT entity, kind, from_state, to_state FROM events ORDER BY rowid",
            "SELECT kind, state FROM needs_you ORDER BY rowid",
            "SELECT kind, run, abandoned_runs, state FROM jobs ORDER BY id",
            "SELECT count(*) FROM leases",
            "SELECT target, result FROM stops ORDER BY rowid",
        ]
        .iter()
        .flat_map(|sql| rows_of(&w.db, sql))
        .collect()
    }

    fn run_args() -> RunArg {
        RunArg {
            run: Some("r1".into()),
        }
    }

    #[test]
    fn task_cancel_and_ledger_cancel_give_the_same_rows() {
        let (task, ledger) = (world(|_, _| {}), world(|_, _| {}));
        let by_task = TaskCommand::Cancel {
            task: "T-1".into(),
            reason: "dropped".into(),
            run: run_args(),
        };
        let by_ledger = LedgerCommand::Cancel {
            task: "T-1".into(),
            reason: "dropped".into(),
            stop: false,
            run: run_args(),
        };

        assert_eq!(run(&task.ctx, by_task).unwrap(), ExitCode::SUCCESS);
        assert_eq!(
            crate::cmd::taskledger::run(&ledger.ctx, by_ledger).unwrap(),
            ExitCode::SUCCESS
        );

        let rows = rows(&task);
        assert_eq!(rows, self::rows(&ledger));
        assert!(
            rows.iter()
                .any(|r| r.starts_with("Integer(1) | Text(\"cancelled\")")),
            "{rows:#?}"
        );
        assert!(
            rows.contains(&"Text(\"ended\") | Text(\"killed\")".to_string()),
            "{rows:#?}"
        );
    }

    #[test]
    fn task_requeue_and_ledger_requeue_give_the_same_rows() {
        let escalated = |store: &mut Store, t1: &str| {
            store.immediate(|tx| tasks::escalate(tx, t1)).unwrap();
        };
        let (task, ledger) = (world(escalated), world(escalated));
        let by_task = TaskCommand::Requeue {
            task: "T-1".into(),
            reason: "again".into(),
            run: run_args(),
        };
        let by_ledger = LedgerCommand::Requeue {
            task: "T-1".into(),
            reason: "again".into(),
            run: run_args(),
        };

        assert_eq!(run(&task.ctx, by_task).unwrap(), ExitCode::SUCCESS);
        assert_eq!(
            crate::cmd::taskledger::run(&ledger.ctx, by_ledger).unwrap(),
            ExitCode::SUCCESS
        );

        let rows = rows(&task);
        assert_eq!(rows, self::rows(&ledger));
        assert!(
            rows.iter()
                .any(|r| r.starts_with("Integer(1) | Text(\"ready\") | Integer(1) | Integer(1)")),
            "{rows:#?}"
        );
    }

    /// `task_cancel` and `task_requeue` are commands of the operator: a
    /// worker pane is refused, and the ledger does not change.
    #[test]
    fn a_worker_cannot_cancel_or_requeue_a_task() {
        let w = world(|_, _| {});
        let before = rows(&w);
        let env = MapEnv::new(&w.root)
            .with("HORCH_PROJECT_DIR", w.root.to_str().unwrap())
            .with("HORCH_LEDGER_DB", w.db.to_str().unwrap())
            .with("HORCH_WORKER_ID", &w.worker);
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let cancel = TaskCommand::Cancel {
            task: "T-1".into(),
            reason: "mine".into(),
            run: run_args(),
        };
        let requeue = TaskCommand::Requeue {
            task: "T-1".into(),
            reason: "mine".into(),
            run: run_args(),
        };

        let err = run(&ctx, cancel).unwrap_err();
        assert_eq!(
            err.to_string(),
            "horch ledger cancel is for the orchestrator, not a worker"
        );
        let err = run(&ctx, requeue).unwrap_err();
        assert_eq!(
            err.to_string(),
            "horch ledger requeue is for the orchestrator, not a worker"
        );
        assert_eq!(rows(&w), before);
    }
}
