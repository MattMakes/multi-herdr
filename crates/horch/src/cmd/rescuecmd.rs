//! `horch rescue drop <ref>`: the operator deletes a rescue ref
//! (`docs/specs/fleetd/design.md`, "No work is lost", I10, FM-40; ticket 43
//! of `docs/tickets/fleetd-p4/`).
//!
//! The command shows the ref, its class, its size and its files. It commits
//! 1 event, then deletes the ref with `git update-ref -d <ref> <tip>` and
//! reads it back. A drop that dies after the commit is completed by the next
//! drop, with no second event. A caller that is a ledger worker and not the
//! orchestrator is refused: before Phase 5 this check is on the client side
//! (the server check SR-8 is Phase 5).

use std::collections::BTreeSet;
use std::path::Path;
use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::Subcommand;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::worktree_custody::adapters::git::{repo_git_dir, CliWorktreeGit};
use horch_core::fleetd::worktree_custody::adapters::rescue_refs_git as refs_git;
use horch_core::fleetd::worktree_custody::application::rescue_refs;
use horch_core::fleetd::worktree_custody::domain::rescue_class::RefOwner;
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::{
    self, finish, model::WorkerKind, triage::human_bytes, workers, Store,
};

use super::taskledger::{project_root, resolve_run, worker_id_opt, RunArg};

#[derive(Subcommand)]
pub enum RescueCommand {
    /// Delete a rescue ref. No collection deletes a `unique` ref: only
    /// this command or `horch worktree restore` ends it.
    Drop {
        /// The rescue ref.
        #[arg(value_name = "REF")]
        rescue_ref: String,
    },
}

/// `horch rescue`.
pub fn run(ctx: &RuntimeContext, command: RescueCommand) -> Result<ExitCode> {
    match command {
        RescueCommand::Drop { rescue_ref } => {
            let repo = project_root(ctx)?;
            println!("{}", drop_ref(ctx, &repo, &rescue_ref)?);
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Drop the rescue ref `name` of the repository `repo`, and give the last
/// line.
fn drop_ref(ctx: &RuntimeContext, repo: &Path, name: &str) -> Result<String> {
    let mut store = taskledger::open(ctx)?;
    refuse_worker(ctx, &store)?;
    let git = CliWorktreeGit::new(&ctx.bins.harness.git, &ctx.paths.fleet_root)
        .reading(&[repo.to_path_buf()]);
    if let Some(gap) = git.sandbox_gap() {
        bail!("{gap}; horch runs git on a rescue ref only in the sandbox, so nothing is deleted");
    }
    let owner = |o: &RefOwner| rescue_refs::owner_of(store.conn(), o).ok().flatten();
    let refs = refs_git::read_facts(&git, repo, &owner)?;
    let plan = rescue_refs::plan_drop(&repo.display().to_string(), name, &refs)?;
    let d = &plan.deletion;
    println!("ref:   {}", d.name);
    println!("tip:   {}", d.tip);
    println!("class: {}", plan.class.as_str());
    println!(
        "size:  {}",
        plan.bytes.map_or_else(|| "unknown".into(), human_bytes)
    );
    let files = files_of(&git, repo, &d.tip)?;
    println!("files: {}", files.len());
    for f in &files {
        println!("  {f}");
    }
    store.immediate(|tx| rescue_refs::record_drop(tx, &plan))?;
    let gw = CliGitGateway::new(git.writing(&[repo_git_dir(repo)?]));
    rescue_refs::drop_ref(&gw, d)?;
    Ok(format!(
        "dropped {} at {}; it reads back no tip",
        d.name, d.tip
    ))
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
                "only the operator drops a rescue ref; worker {} cannot",
                w.worker_id
            );
        }
    }
    Ok(())
}

/// The files of the commits that `tip` reaches and no branch reaches.
fn files_of(git: &CliWorktreeGit, repo: &Path, tip: &str) -> Result<Vec<String>> {
    let out = git.git(
        repo,
        &[
            "log",
            "--format=",
            "--name-only",
            tip,
            "--not",
            "--branches",
        ],
    )?;
    let files: BTreeSet<&str> = out.lines().filter(|l| !l.is_empty()).collect();
    Ok(files.into_iter().map(str::to_string).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::runtime::MapEnv;
    use horch_core::taskledger::runs::{self, NewRun};
    use horch_core::taskledger::workers::NewWorker;
    use std::path::PathBuf;
    use std::process::Command;

    /// A temp project with 1 commit on `main`, its ledger with 1 active run,
    /// and the state folders. No path is the live ledger or state.
    struct World {
        dir: tempfile::TempDir,
        project: PathBuf,
        run_id: String,
    }

    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&project).unwrap();
        git(&project, &["init", "-q", "-b", "main"]);
        std::fs::write(project.join("a.txt"), "a\n").unwrap();
        git(&project, &["add", "a.txt"]);
        git(&project, &["commit", "-q", "-m", "first"]);
        let mut w = World {
            dir,
            project,
            run_id: String::new(),
        };
        let ctx = w.ctx(&[]);
        let mut store = taskledger::open(&ctx).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    &w.project,
                    &NewRun {
                        slug: "r1".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        w.run_id = run.run_id;
        w
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

        /// Add a worker row of `kind` in `pane` to the run, and give its id.
        fn worker(&self, kind: WorkerKind, pane: &str) -> String {
            let mut store = taskledger::open(&self.ctx(&[])).unwrap();
            store
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

        /// Commit `file` on a side branch, write a rescue ref of that tip as
        /// fleetd does, and delete the branch: only the ref holds the work.
        fn unique_ref(&self, file: &str) -> (String, String) {
            git(&self.project, &["checkout", "-q", "-b", "side"]);
            std::fs::write(self.project.join(file), "x\n").unwrap();
            git(&self.project, &["add", file]);
            git(&self.project, &["commit", "-q", "-m", file]);
            let tip = git(&self.project, &["rev-parse", "HEAD"]);
            let name = format!("refs/horch/rescue/branch/side/{}", file.len());
            git(&self.project, &["update-ref", &name, &tip]);
            git(&self.project, &["checkout", "-q", "main"]);
            git(&self.project, &["branch", "-q", "-D", "side"]);
            (name, tip)
        }

        fn tip(&self, rev: &str) -> Option<String> {
            let out = Command::new("git")
                .arg("-C")
                .arg(&self.project)
                .args(["rev-parse", "--verify", "-q", rev])
                .output()
                .unwrap();
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        }
    }

    #[test]
    fn drop_deletes_the_ref_and_verifies_it() {
        let w = world();
        let (name, tip) = w.unique_ref("b.txt");
        let line = drop_ref(&w.ctx(&[]), &w.project, &name).unwrap();
        assert_eq!(
            line,
            format!("dropped {name} at {tip}; it reads back no tip")
        );
        assert_eq!(w.tip(&name), None);
        // A second drop finds no ref and changes nothing.
        let err = drop_ref(&w.ctx(&[]), &w.project, &name).unwrap_err();
        assert!(format!("{err:#}").contains("does not exist"), "{err:#}");
    }

    #[test]
    fn ref_outside_the_rescue_namespace_is_refused() {
        let w = world();
        let main = w.tip("main").unwrap();
        let err = drop_ref(&w.ctx(&[]), &w.project, "refs/heads/main").unwrap_err();
        assert!(format!("{err:#}").contains("refs/horch/rescue/"), "{err:#}");
        assert_eq!(w.tip("main"), Some(main));
    }

    #[test]
    fn worker_caller_is_refused() {
        let w = world();
        let (name, tip) = w.unique_ref("b.txt");
        let worker = w.worker(WorkerKind::Worker, "p_7");
        // By HORCH_WORKER_ID.
        let ctx = w.ctx(&[("HORCH_WORKER_ID", &worker)]);
        let err = drop_ref(&ctx, &w.project, &name).unwrap_err();
        assert!(format!("{err:#}").contains("only the operator"), "{err:#}");
        // By its pane, with no HORCH_WORKER_ID.
        let ctx = w.ctx(&[("HERDR_PANE_ID", "p_7")]);
        let err = drop_ref(&ctx, &w.project, &name).unwrap_err();
        assert!(format!("{err:#}").contains("only the operator"), "{err:#}");
        assert_eq!(w.tip(&name), Some(tip));

        // The orchestrator, and a caller that is no worker, can drop.
        let orchestrator = w.worker(WorkerKind::Orchestrator, "p_1");
        let ctx = w.ctx(&[("HORCH_WORKER_ID", &orchestrator)]);
        assert!(refuse_worker(&ctx, &taskledger::open(&ctx).unwrap()).is_ok());
        drop_ref(&w.ctx(&[]), &w.project, &name).unwrap();
        assert_eq!(w.tip(&name), None);
    }
}
