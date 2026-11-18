//! `horch feature abandon <feature>`: the operator ends a feature
//! (`docs/specs/fleetd/design.md`, "The feature state machine", row
//! `abandoned`; "A cancel"; I10; FM-43; ticket 40 of
//! `docs/tickets/fleetd-p4/`).
//!
//! fleetd writes a rescue ref of each task branch of the feature that is
//! not landed and reads it back. Then 1 transaction moves each such branch
//! to `abandoned`, cancels each task of the run that is not terminal, and
//! moves the feature to `abandoned`. After the commit the command stops the
//! worker of each live attempt, and a task is `cancelled` only when its
//! process is gone. Else the next pass of fleetd completes the cancel. A
//! crash before the commit is completed by the next abandon, with the same
//! rescue refs.
//!
//! It refuses to run inside a fleet attempt (SR-5; ticket 23 of
//! fleetd-p6). A caller that is a ledger worker and not the orchestrator is
//! refused: before Phase 5 this check is on the client side (the server
//! check SR-8 is Phase 5).

use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::Subcommand;
use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::application::commands::{self, FeatureAbandoned};
use horch_core::fleetd::integration::application::landing::GitBranchRescues;
use horch_core::fleetd::task_execution::application::cancel::{self, Cancel};
use horch_core::fleetd::worktree_custody::adapters::git::{repo_git_dir, CliWorktreeGit};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::model::WorkerKind;
use horch_core::taskledger::{self, finish, workers, Store};

use super::taskledger::{
    resolve_run, seen, stop_holders, worker_id_opt, HerdrPanes, RunArg, Stopper,
};
use crate::output;

#[derive(Subcommand)]
pub enum FeatureCommand {
    /// Abandon a feature. Each branch of the feature that is not landed
    /// gets a rescue ref first, and each task of the feature that is not
    /// done is cancelled.
    Abandon {
        /// The feature id, for example `F-3`.
        feature: String,
    },
}

/// `horch feature`.
pub fn run(ctx: &RuntimeContext, command: FeatureCommand) -> Result<ExitCode> {
    match command {
        FeatureCommand::Abandon { feature } => {
            let command = "horch feature abandon";
            if let Some(refused) = super::idea::refuse_inside_fleet_attempt(ctx, command) {
                return Ok(refused);
            }
            let mut store = taskledger::open(ctx)?;
            refuse_worker(ctx, &store)?;
            let done = abandon(ctx, &mut store, &feature)?;
            for line in lines(&done) {
                output::println(&line);
            }
            let herdr = HerdrPanes::new(ctx);
            stop_and_complete(&mut store, &done, &Stopper::real(&herdr))?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Abandon the feature `feature`: the rescue ref of each branch that is
/// not landed first, then the moves and the cancels in 1 transaction.
fn abandon(ctx: &RuntimeContext, store: &mut Store, feature: &str) -> Result<FeatureAbandoned> {
    let repo = commands::feature_repo(store.conn(), feature)?;
    let git = CliWorktreeGit::new(&ctx.bins.harness.git, &ctx.paths.fleet_root)
        .reading(std::slice::from_ref(&repo))
        .writing(&[repo_git_dir(&repo)?]);
    if let Some(gap) = git.sandbox_gap() {
        bail!(
            "{gap}; horch writes a rescue ref only in the sandbox, so {feature} is not abandoned"
        );
    }
    let config = config_file::load(&ctx.paths.fleet_root)?;
    let jobs = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let rescues = GitBranchRescues { git: &git };
    let why = format!("the operator abandons the feature {feature}");
    commands::abandon_feature(store, &jobs, &rescues, &repo, feature, &why)
}

/// The lines of an abandon: each branch, each task cancelled at once, and
/// the feature.
fn lines(done: &FeatureAbandoned) -> Vec<String> {
    let mut out: Vec<String> = done
        .branches
        .iter()
        .map(|b| match &b.rescue_ref {
            Some(name) => format!(
                "{} is abandoned; its rescue ref {name} keeps its tip.",
                b.branch
            ),
            None => format!(
                "{} is abandoned; git has no such branch, so it has no tip to keep.",
                b.branch
            ),
        })
        .collect();
    for c in each_cancel(&done.cancels) {
        if let Cancel::Cancelled { task, .. } = c {
            out.push(format!("{} cancelled.", task.short()));
        }
    }
    out.push(if done.already {
        format!("{} was abandoned already.", done.feature)
    } else {
        format!("{} is abandoned.", done.feature)
    });
    out
}

/// Each cancel of `cancels`, with the cancel of each of their judge tasks.
fn each_cancel(cancels: &[Cancel]) -> Vec<&Cancel> {
    let mut out = Vec::new();
    for c in cancels {
        out.push(c);
        out.extend(each_cancel(c.judges()));
    }
    out
}

/// After the commit: stop each worker of a live attempt, then cancel each
/// task whose processes are gone. A task with a process that still runs is
/// cancelled by the next pass of fleetd.
fn stop_and_complete(store: &mut Store, done: &FeatureAbandoned, s: &Stopper) -> Result<()> {
    let why = format!("feature abandoned: {}", done.feature);
    stop_holders(store, &done.to_stop(), &why, s)?;
    for c in each_cancel(&done.cancels) {
        let Cancel::Stopping { task, .. } = c else {
            continue;
        };
        let facts = seen(&cancel::watched(store.conn(), &task.task_id)?, s.procs);
        match store.immediate(|tx| cancel::complete(tx, &task.task_id, &facts))? {
            Some(t) => output::println(&format!("{} cancelled.", t.short())),
            None => output::println(&format!(
                "{} is not cancelled yet: a process of its attempt still runs. The next pass of fleetd cancels it when the process is gone.",
                task.short()
            )),
        }
    }
    Ok(())
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
                "only the operator abandons a feature; worker {} cannot",
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

    const FEATURE: &str = "F-0001";
    const RUN_BRANCH: &str = "fleet/r1";

    /// A temp project with 1 commit on `main` and the run branch, its
    /// ledger with 1 active run and its feature `F-0001`, and the state
    /// folders. No path is the live ledger or state.
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
                    id: FEATURE,
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

        /// A task `title` with its branch row in `building`, and the branch
        /// in git with 1 commit. Gives (task id, branch, tip).
        fn task(&self, title: &str) -> (String, String, String) {
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

        fn feature_state(&self) -> String {
            rows::feature(self.store().conn(), FEATURE)
                .unwrap()
                .unwrap()
                .state
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

    fn abandon_as(ctx: &RuntimeContext) -> Result<ExitCode> {
        run(
            ctx,
            FeatureCommand::Abandon {
                feature: FEATURE.to_string(),
            },
        )
    }

    #[test]
    fn operator_abandon_rescues_then_abandons_the_feature() {
        let w = world();
        let (task, branch, tip) = w.task("a");
        abandon_as(&w.ctx(&[])).unwrap();
        let refs = w.rescue_refs();
        assert_eq!(refs.len(), 1, "{refs:?}");
        assert_eq!(git(&w.project, &["rev-parse", &refs[0]]), tip);
        assert_eq!(w.row(&branch), ("abandoned".into(), Some(refs[0].clone())));
        let status = tasks::get(w.store().conn(), &task).unwrap().status;
        assert_eq!(status, "cancelled");
        assert_eq!(w.feature_state(), "abandoned");

        // A second abandon moves nothing and writes no second ref.
        abandon_as(&w.ctx(&[])).unwrap();
        assert_eq!(w.rescue_refs(), refs);
    }

    #[test]
    fn worker_caller_is_refused() {
        let w = world();
        let (task, branch, _) = w.task("a");
        let worker = w.worker(WorkerKind::Worker, "p_7");
        // By HORCH_WORKER_ID.
        let err = abandon_as(&w.ctx(&[("HORCH_WORKER_ID", &worker)])).unwrap_err();
        assert!(format!("{err:#}").contains("only the operator"), "{err:#}");
        // By its pane, with no HORCH_WORKER_ID.
        let err = abandon_as(&w.ctx(&[("HERDR_PANE_ID", "p_7")])).unwrap_err();
        assert!(format!("{err:#}").contains("only the operator"), "{err:#}");
        assert_eq!(w.row(&branch), ("building".into(), None));
        assert!(w.rescue_refs().is_empty());
        assert_eq!(w.feature_state(), "open");
        let status = tasks::get(w.store().conn(), &task).unwrap().status;
        assert_eq!(status, "ready");

        // The orchestrator can abandon.
        let orchestrator = w.worker(WorkerKind::Orchestrator, "p_1");
        abandon_as(&w.ctx(&[("HORCH_WORKER_ID", &orchestrator)])).unwrap();
        assert_eq!(w.feature_state(), "abandoned");
    }
}
