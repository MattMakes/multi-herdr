//! `horch branch abandon <branch>`: the operator ends a task branch that
//! its resolver or fix attempts did not repair
//! (`docs/specs/fleetd/design.md`, "Landing a task branch on its feature
//! branch", I10; ticket 35 of `docs/tickets/fleetd-p4/`).
//!
//! fleetd writes a rescue ref of the branch tip and reads it back, then
//! moves the branch to `abandoned` in 1 transaction. The task stays as it
//! is. A `landed` branch is refused. An abandon that dies after the rescue
//! is completed by the next abandon, with the same ref. `horch task cancel`
//! abandons the task branch of the task that it cancels in the same way.
//!
//! A caller that is a ledger worker and not the orchestrator is refused:
//! before Phase 5 this check is on the client side (the server check SR-8
//! is Phase 5).

use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::Subcommand;
use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::application::commands::{self, Abandoned};
use horch_core::fleetd::integration::application::landing::GitBranchRescues;
use horch_core::fleetd::worktree_custody::adapters::git::{repo_git_dir, CliWorktreeGit};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::model::WorkerKind;
use horch_core::taskledger::tasks::Task;
use horch_core::taskledger::{self, finish, workers, Store};

use super::taskledger::{resolve_run, worker_id_opt, RunArg};
use crate::output;

#[derive(Subcommand)]
pub enum BranchCommand {
    /// Abandon a task branch. fleetd writes a rescue ref of the branch
    /// first.
    Abandon {
        /// The task branch.
        branch: String,
    },
}

/// `horch branch`.
pub fn run(ctx: &RuntimeContext, command: BranchCommand) -> Result<ExitCode> {
    match command {
        BranchCommand::Abandon { branch } => {
            let mut store = taskledger::open(ctx)?;
            refuse_worker(ctx, &store)?;
            let done = abandon(ctx, &mut store, &branch, "the operator abandons it")?;
            output::println(&line(&done));
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Abandon the task branch `branch` with the cause `why`: its rescue ref
/// first, then the move.
fn abandon(ctx: &RuntimeContext, store: &mut Store, branch: &str, why: &str) -> Result<Abandoned> {
    let repo = commands::repo_of(store.conn(), branch)?;
    let git = CliWorktreeGit::new(&ctx.bins.harness.git, &ctx.paths.fleet_root)
        .reading(std::slice::from_ref(&repo))
        .writing(&[repo_git_dir(&repo)?]);
    if let Some(gap) = git.sandbox_gap() {
        bail!("{gap}; horch writes a rescue ref only in the sandbox, so {branch} is not abandoned");
    }
    let config = config_file::load(&ctx.paths.fleet_root)?;
    let jobs = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let rescues = GitBranchRescues { git: &git };
    commands::abandon(store, &jobs, &rescues, &repo, branch, why)
}

/// The last line of an abandon.
fn line(done: &Abandoned) -> String {
    let kept = match &done.rescue_ref {
        Some(name) => format!("its rescue ref {name} keeps its tip"),
        None => "git has no such branch, so it has no tip to keep".to_string(),
    };
    if done.already {
        format!("{} was abandoned already; {kept}.", done.branch)
    } else {
        format!("{} is abandoned; {kept}.", done.branch)
    }
}

/// Abandon each task branch of the cancelled task `task` that is not
/// `landed` or `abandoned`, after the commit of its cancel. A failure does
/// not undo the cancel: the landing part of the pass rescues and abandons
/// the branch of a cancelled task.
pub(super) fn abandon_branches_of(ctx: &RuntimeContext, store: &mut Store, task: &Task) {
    let why = format!("its task {} is cancelled", task.short());
    let names = match commands::open_branches_of_task(store.conn(), &task.task_id) {
        Ok(names) => names,
        Err(e) => {
            output::println(&format!(
                "The task branches of {} are not read: {e:#}. The next pass of fleetd abandons them.",
                task.short()
            ));
            return;
        }
    };
    for name in names {
        match abandon(ctx, store, &name, &why) {
            Ok(done) => output::println(&line(&done)),
            Err(e) => output::println(&format!(
                "{name} is not abandoned yet: {e:#}. The next pass of fleetd writes its rescue ref and abandons it."
            )),
        }
    }
}

/// Refuse a caller that is a ledger worker and not the orchestrator. The
/// caller is found as `horch ledger waive` finds it: its worker id, else the
/// worker of the active run in its pane, so a worker that unsets
/// `HORCH_WORKER_ID` is still known.
fn refuse_worker(ctx: &RuntimeContext, store: &Store) -> Result<()> {
    let pane = ctx.herdr.pane.as_ref().map(|p| p.as_str().to_string());
    let caller = worker_id_opt(ctx).or_else(|| {
        let run = resolve_run(ctx, store, &RunArg::default()).ok()?;
        finish::codesweep::worker_in_pane(store.conn(), &run.run_id, pane.as_deref())
    });
    if let Some(w) = caller {
        let w = workers::get(store.conn(), &w)?;
        if w.kind != WorkerKind::Orchestrator.as_str() {
            bail!(
                "only the operator abandons a task branch; worker {} cannot",
                w.worker_id
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
    use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
    use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
    use horch_core::fleetd::integration::application::landing;
    use horch_core::runtime::MapEnv;
    use horch_core::taskledger::model::Actor;
    use horch_core::taskledger::runs::{self, NewRun};
    use horch_core::taskledger::tasks::{self, NewTask};
    use horch_core::taskledger::workers::NewWorker;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const RUN_BRANCH: &str = "fleet/r1";

    /// A temp project with 1 commit on `main` and the run branch, its
    /// ledger with 1 active run, its feature, and the state folders. No
    /// path is the live ledger or state.
    struct World {
        dir: tempfile::TempDir,
        project: PathBuf,
        run_id: String,
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
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
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&project).unwrap();
        git(&project, &["init", "-q", "-b", "main"]);
        std::fs::write(project.join("a.txt"), "a\n").unwrap();
        git(&project, &["add", "a.txt"]);
        git(&project, &["commit", "-q", "-m", "first"]);
        git(&project, &["branch", RUN_BRANCH]);
        let mut w = World {
            dir,
            project,
            run_id: String::new(),
        };
        let mut store = taskledger::open(&w.ctx(&[])).unwrap();
        let run = store
            .immediate(|tx| {
                let new_run = NewRun {
                    slug: "r1".into(),
                    base_branch: Some("main".into()),
                    run_branch: Some(RUN_BRANCH.into()),
                    ..Default::default()
                };
                let run = runs::create(tx, &w.project, &new_run)?;
                let feature = NewFeature {
                    id: "F-0001",
                    name: Some("r1"),
                    branch: Some(RUN_BRANCH),
                    primary_branch: "main",
                };
                let cause = Cause {
                    actor: Actor::Horch,
                    text: "feature accepted",
                };
                rows::insert_feature(tx, None, &feature, cause)?;
                Ok(run)
            })
            .unwrap();
        w.run_id = run.run_id;
        w
    }

    impl World {
        /// A context in the project, with the temp ledger and state, and
        /// the environment `extra`.
        fn ctx(&self, extra: &[(&str, &str)]) -> RuntimeContext {
            let s = |p: PathBuf| p.to_string_lossy().into_owned();
            let root = self.dir.path();
            let env = MapEnv::new(self.project.to_str().unwrap())
                .with_exe("/opt/horch/bin/horch")
                .with("HORCH_STATE_DIR", &s(root.join("state")))
                .with("HORCH_DATA_DIR", &s(root.join("data")))
                .with("HORCH_PROJECT_DIR", self.project.to_str().unwrap())
                .with("HORCH_LEDGER_DB", &s(root.join("ledger.sqlite3")))
                .with("FLEET_ROOT", &s(root.join("fleet")));
            let env = extra.iter().fold(env, |e, (k, v)| e.with(k, v));
            RuntimeContext::from_env(&env).unwrap()
        }

        fn store(&self) -> Store {
            taskledger::open(&self.ctx(&[])).unwrap()
        }

        /// Add a worker row of `kind` in `pane` to the run, and give its id.
        fn worker(&self, kind: WorkerKind, pane: &str) -> String {
            self.store()
                .immediate(|tx| {
                    workers::insert(
                        tx,
                        &NewWorker {
                            run_id: self.run_id.clone(),
                            kind: Some(kind),
                            persona: "opus".into(),
                            pane_id: Some(pane.into()),
                            ..Default::default()
                        },
                    )
                })
                .unwrap()
        }

        /// A task `title` with its branch row moved along `path`, and the
        /// branch in git with 1 commit. Gives (task id, branch, tip).
        fn task(&self, title: &str, path: &[&str]) -> (String, String, String) {
            let branch = format!("{RUN_BRANCH}-{title}");
            let config = horch_core::fleetd::fleet_runtime::domain::config::FleetConfig::default();
            let task = self
                .store()
                .immediate(|tx| {
                    let run = runs::get(tx, &self.run_id)?;
                    let new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
                    let task = tasks::add(tx, &run, &new, Actor::Orchestrator)?.task_id;
                    let jobs = JobCtx {
                        guard: None,
                        config: &config,
                        needs_you: &NeedsYouList,
                    };
                    landing::bind_branch(&jobs, tx, &task, &branch, FleetSeconds(0))?;
                    for to in path {
                        let to = landing::state_named(to)?;
                        landing::enter(&jobs, tx, &branch, to, FleetSeconds(1), "test move")?;
                    }
                    Ok(task)
                })
                .unwrap();
            git(
                &self.project,
                &["checkout", "-q", "-b", &branch, RUN_BRANCH],
            );
            std::fs::write(self.project.join(format!("{title}.txt")), "work\n").unwrap();
            git(&self.project, &["add", "."]);
            git(&self.project, &["commit", "-q", "-m", title]);
            git(&self.project, &["checkout", "-q", "main"]);
            let tip = git(&self.project, &["rev-parse", &branch]);
            (task, branch, tip)
        }

        /// The landing state and the rescue ref of the row of `branch`.
        fn row(&self, branch: &str) -> (String, Option<String>) {
            let row = rows::branch(self.store().conn(), branch).unwrap().unwrap();
            (row.landing_state.unwrap(), row.rescue_ref)
        }

        /// The rescue refs of branches in git.
        fn rescue_refs(&self) -> Vec<String> {
            git(
                &self.project,
                &[
                    "for-each-ref",
                    "--format=%(refname)",
                    "refs/horch/rescue/branch/",
                ],
            )
            .lines()
            .map(str::to_string)
            .collect()
        }
    }

    const CONFLICTED: [&str; 4] = ["verifying", "ready", "in_bundle", "conflicted"];

    fn abandon_as(ctx: &RuntimeContext, branch: &str) -> Result<ExitCode> {
        run(
            ctx,
            BranchCommand::Abandon {
                branch: branch.to_string(),
            },
        )
    }

    #[test]
    fn operator_abandon_writes_the_rescue_ref_then_abandoned() {
        let w = world();
        let (task, branch, tip) = w.task("a", &CONFLICTED);
        let before = tasks::get(w.store().conn(), &task).unwrap().status;
        abandon_as(&w.ctx(&[]), &branch).unwrap();
        let refs = w.rescue_refs();
        assert_eq!(refs.len(), 1, "{refs:?}");
        assert_eq!(git(&w.project, &["rev-parse", &refs[0]]), tip);
        assert_eq!(w.row(&branch), ("abandoned".into(), Some(refs[0].clone())));
        let status = tasks::get(w.store().conn(), &task).unwrap().status;
        assert_eq!(status, before, "the task stays as it is");
    }

    #[test]
    fn worker_caller_is_refused() {
        let w = world();
        let (_, branch, _) = w.task("a", &CONFLICTED);
        let worker = w.worker(WorkerKind::Worker, "p_7");
        // By HORCH_WORKER_ID.
        let err = abandon_as(&w.ctx(&[("HORCH_WORKER_ID", &worker)]), &branch).unwrap_err();
        assert!(format!("{err:#}").contains("only the operator"), "{err:#}");
        // By its pane, with no HORCH_WORKER_ID.
        let err = abandon_as(&w.ctx(&[("HERDR_PANE_ID", "p_7")]), &branch).unwrap_err();
        assert!(format!("{err:#}").contains("only the operator"), "{err:#}");
        assert_eq!(w.row(&branch), ("conflicted".into(), None));
        assert!(w.rescue_refs().is_empty());

        // The orchestrator can abandon.
        let orchestrator = w.worker(WorkerKind::Orchestrator, "p_1");
        abandon_as(&w.ctx(&[("HORCH_WORKER_ID", &orchestrator)]), &branch).unwrap();
        assert_eq!(w.row(&branch).0, "abandoned");
    }

    #[test]
    fn task_cancel_abandons_the_task_branch_after_its_rescue_ref() {
        let w = world();
        let (task, branch, tip) = w.task("a", &[]);
        let ctx = w.ctx(&[]);
        let mut store = w.store();
        let run = RunArg {
            run: Some("r1".into()),
        };
        super::super::taskledger::cancel_cmd(&ctx, &mut store, &task, "not needed", &run).unwrap();
        assert_eq!(tasks::get(store.conn(), &task).unwrap().status, "cancelled");
        let refs = w.rescue_refs();
        assert_eq!(refs.len(), 1, "{refs:?}");
        assert_eq!(git(&w.project, &["rev-parse", &refs[0]]), tip);
        assert_eq!(w.row(&branch), ("abandoned".into(), Some(refs[0].clone())));
    }
}
