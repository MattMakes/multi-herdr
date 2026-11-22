use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use rusqlite::{params, Connection};

use super::{
    finish::{self, WorktreeRow},
    git::Git,
    model::{Actor, EventKind, TaskStatus, WorktreeStatus},
    runs,
    store::{optional, Store},
    tasks::{self, Task},
    util::{self, About},
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Undo {
    Keep,
    DeleteBranch,
    RestoreBranch(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prestaged {
    pub path: PathBuf,
    pub task_id: String,
    run_id: String,
    worker_id: String,
    worktree_id: String,
    repo_root: PathBuf,
    branch: String,
    start: String,
    undo: Undo,
}

pub fn prestage(
    store: &mut Store,
    git: &Git,
    worker_id: &str,
    name: &dyn Fn(&Task) -> String,
) -> Result<Option<Prestaged>> {
    let Some(task) = store.immediate(|tx| tasks::reserve_for_worktree(tx, worker_id))? else {
        return Ok(None);
    };
    if let Some(row) = finish::worktree_of(store.conn(), &task.task_id)? {
        let found = Path::new(&row.path).is_dir();
        return Ok(found.then(|| adopted(row, &task, worker_id)));
    }
    create(store, git, worker_id, &task, &name(&task)).map(Some)
}

fn adopted(row: WorktreeRow, task: &Task, worker_id: &str) -> Prestaged {
    Prestaged {
        path: PathBuf::from(&row.path),
        task_id: task.task_id.clone(),
        run_id: task.run_id.clone(),
        worker_id: worker_id.to_string(),
        worktree_id: row.worktree_id,
        repo_root: PathBuf::from(&row.repo_root),
        branch: row.branch,
        start: row.base_commit.unwrap_or_default(),
        undo: Undo::Keep,
    }
}

fn create(
    store: &mut Store,
    git: &Git,
    worker_id: &str,
    task: &Task,
    name: &str,
) -> Result<Prestaged> {
    let run = runs::get(store.conn(), &task.run_id)?;
    let root = PathBuf::from(&run.project_root);
    let base = finish::worktree_base(&run)?;
    let (path, branch) = finish::worktree_place(&run, task, name);
    if path.exists() {
        bail!(
            "{} exists but the ledger has no row for it; remove it or rename the task",
            path.display()
        );
    }
    let start = git.rev(&root, &base)?;
    let path_text = path.to_string_lossy().into_owned();
    let undo = add_worktree(git, &root, &branch, &path_text, &start)?;
    let row = WorktreeRow {
        worktree_id: util::new_id(),
        task_id: Some(task.task_id.clone()),
        worker_id: None,
        repo_root: run.project_root.clone(),
        path: path_text,
        branch: branch.clone(),
        base,
        base_commit: Some(start.clone()),
        status: WorktreeStatus::Open.as_str().to_string(),
        landed_commit: None,
        bytes_on_disk: None,
        reason: None,
    };
    let prestaged = Prestaged {
        path,
        task_id: task.task_id.clone(),
        run_id: task.run_id.clone(),
        worker_id: worker_id.to_string(),
        worktree_id: row.worktree_id.clone(),
        repo_root: root,
        branch,
        start,
        undo,
    };
    if let Err(e) = finish::insert_worktree(store, &row, &task.run_id, Actor::Horch) {
        if let Err(undo) = undo_git(git, &prestaged) {
            eprintln!("horch spawn: removing the new worktree failed: {undo:#}");
        }
        return Err(e);
    }
    Ok(prestaged)
}

fn add_worktree(git: &Git, root: &Path, branch: &str, path: &str, start: &str) -> Result<Undo> {
    if !git.branch_exists(root, branch) {
        git.run(root, &["worktree", "add", "-b", branch, path, start])?;
        return Ok(Undo::DeleteBranch);
    }
    let tip = git.rev(root, &format!("refs/heads/{branch}"))?;
    if !git.is_ancestor(root, &tip, start) {
        bail!("branch {branch} has commits that are not in {start}; land or remove them first");
    }
    git.run(root, &["worktree", "add", path, branch])?;
    if tip != start {
        if let Err(e) = git.run(Path::new(path), &["merge", "--ff-only", start]) {
            if let Err(undo) = git.run(root, &["worktree", "remove", path]) {
                eprintln!("horch spawn: removing the new worktree failed: {undo:#}");
            }
            return Err(e);
        }
    }
    Ok(Undo::RestoreBranch(tip))
}

pub fn release(store: &mut Store, git: &Git, p: &Prestaged) -> Result<()> {
    store.immediate(|tx| {
        tx.execute(
            "UPDATE workers SET target_task_id = NULL
             WHERE worker_id = ?1 AND claimed_at IS NULL",
            [&p.worker_id],
        )?;
        if p.undo == Undo::Keep || !unheld(tx, p)? {
            return Ok(());
        }
        undo_git(git, p)?;
        tx.execute(
            "DELETE FROM worktrees WHERE worktree_id = ?1",
            [&p.worktree_id],
        )?;
        util::event(
            tx,
            &About {
                run: Some(&p.run_id),
                task: Some(&p.task_id),
                worker: Some(&p.worker_id),
            },
            Actor::Horch,
            EventKind::Note,
            &format!(
                "Removed worktree {} after a failed launch.",
                p.path.display()
            ),
        )
    })
}

fn unheld(conn: &Connection, p: &Prestaged) -> Result<bool> {
    let row: Option<(Option<String>, String)> = optional(conn.query_row(
        "SELECT w.worker_id, t.status FROM worktrees w
         JOIN tasks t ON t.task_id = w.task_id WHERE w.worktree_id = ?1",
        params![p.worktree_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    Ok(row
        .is_some_and(|(worker, status)| worker.is_none() && status == TaskStatus::Queued.as_str()))
}

fn undo_git(git: &Git, p: &Prestaged) -> Result<()> {
    let head = git.rev(&p.path, "HEAD")?;
    if head != p.start {
        bail!(
            "worktree {} has new commits; it stays in place",
            p.path.display()
        );
    }
    let path = p.path.to_string_lossy();
    git.run(&p.repo_root, &["worktree", "remove", &path])?;
    let reference = format!("refs/heads/{}", p.branch);
    match &p.undo {
        Undo::DeleteBranch => {
            git.run(&p.repo_root, &["update-ref", "-d", &reference, &p.start])?;
        }
        Undo::RestoreBranch(tip) if *tip != p.start => {
            git.run(&p.repo_root, &["update-ref", &reference, tip, &p.start])?;
        }
        Undo::RestoreBranch(_) | Undo::Keep => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::taskledger::model::{Actor, WorkerKind};
    use crate::taskledger::runs::{NewRun, Run};
    use crate::taskledger::tasks::NewTask;
    use crate::taskledger::workers::{self, NewWorker};

    struct World {
        _dir: tempfile::TempDir,
        root: PathBuf,
        store: Store,
        git: Git,
        run: Run,
        task: Task,
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn world() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&root).unwrap();
        sh(&root, &["init", "-q", "-b", "main"]);
        sh(&root, &["commit", "-q", "--allow-empty", "-m", "init"]);
        sh(&root, &["branch", "fleet/run"]);
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        run_branch: Some("fleet/run".into()),
                        base_branch: Some("main".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        let task = store
            .immediate(|tx| {
                tasks::add(
                    tx,
                    &run,
                    &NewTask::work("Import it", "tickets/a.md", "importer"),
                    Actor::Orchestrator,
                )
            })
            .unwrap();
        World {
            _dir: dir,
            root,
            store,
            git: Git::new("git"),
            run,
            task,
        }
    }

    fn worker(w: &mut World) -> String {
        let run_id = w.run.run_id.clone();
        w.store
            .immediate(|tx| {
                workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run_id.clone(),
                        kind: Some(WorkerKind::Worker),
                        persona: "importer".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap()
    }

    fn stage(w: &mut World, id: &str) -> Result<Option<Prestaged>> {
        prestage(&mut w.store, &w.git, id, &|_| "import-it".to_string())
    }

    fn fail(w: &mut World, id: &str) {
        w.store
            .immediate(|tx| workers::launch_failed(tx, id, "no pane"))
            .unwrap();
    }

    const BRANCH: &str = "fleet/run-T1-import-it";

    #[test]
    fn the_worktree_exists_before_the_worker_claims_and_the_claim_adopts_it() {
        let mut w = world();
        let id = worker(&mut w);
        let p = stage(&mut w, &id).unwrap().expect("a worktree");
        assert!(p.path.join(".git").is_file());
        assert_eq!(p.task_id, w.task.task_id);
        let row = finish::worktree_of(w.store.conn(), &w.task.task_id)
            .unwrap()
            .unwrap();
        assert_eq!(row.worker_id, None);
        assert_eq!(
            tasks::get(w.store.conn(), &w.task.task_id).unwrap().status,
            "queued"
        );

        let claimed = w.store.immediate(|tx| tasks::claim(tx, &id)).unwrap();
        let tasks::ClaimOutcome::Claimed(c) = claimed else {
            panic!("no claim: {claimed:?}");
        };
        assert_eq!(c.task.task_id, w.task.task_id);
        assert_eq!(c.worktree.as_deref(), Some(row.path.as_str()));
        let again = finish::worktree_new(&mut w.store, &w.git, &id, "other").unwrap();
        assert_eq!(again.worktree_id, row.worktree_id, "no second worktree");
        assert_eq!(again.worker_id.as_deref(), Some(id.as_str()));
    }

    #[test]
    fn two_spawns_cannot_take_the_same_task() {
        let mut w = world();
        let (a, b) = (worker(&mut w), worker(&mut w));
        assert!(stage(&mut w, &a).unwrap().is_some());
        assert_eq!(stage(&mut w, &b).unwrap(), None);
        let list = sh(&w.root, &["worktree", "list", "--porcelain"]);
        assert_eq!(list.matches("worktree ").count(), 2, "{list}");
    }

    #[test]
    fn a_failed_launch_frees_the_task_and_leaves_no_worktree_or_branch() {
        let mut w = world();
        let id = worker(&mut w);
        let p = stage(&mut w, &id).unwrap().unwrap();
        fail(&mut w, &id);
        release(&mut w.store, &w.git, &p).unwrap();

        assert!(!p.path.exists());
        assert!(!w.git.branch_exists(&w.root, BRANCH));
        assert_eq!(
            finish::worktree_of(w.store.conn(), &w.task.task_id).unwrap(),
            None
        );
        let next = worker(&mut w);
        let again = stage(&mut w, &next).unwrap().expect("the task is free");
        assert_eq!(again.path, p.path, "the same path can be made again");
    }

    #[test]
    fn a_release_keeps_a_worktree_that_a_later_worker_claimed() {
        let mut w = world();
        let id = worker(&mut w);
        let p = stage(&mut w, &id).unwrap().unwrap();
        fail(&mut w, &id);
        let next = worker(&mut w);
        w.store.immediate(|tx| tasks::claim(tx, &next)).unwrap();

        release(&mut w.store, &w.git, &p).unwrap();

        assert!(p.path.is_dir());
        assert!(w.git.branch_exists(&w.root, BRANCH));
    }

    #[test]
    fn a_release_keeps_a_worktree_with_uncommitted_work() {
        let mut w = world();
        let id = worker(&mut w);
        let p = stage(&mut w, &id).unwrap().unwrap();
        std::fs::write(p.path.join("work.txt"), "keep").unwrap();
        fail(&mut w, &id);

        assert!(release(&mut w.store, &w.git, &p).is_err());

        assert!(p.path.join("work.txt").is_file());
        assert!(finish::worktree_of(w.store.conn(), &w.task.task_id)
            .unwrap()
            .is_some());
    }

    #[test]
    fn a_leftover_branch_with_no_commits_is_reused_and_restored() {
        let mut w = world();
        sh(&w.root, &["branch", BRANCH, "fleet/run"]);
        let old = sh(&w.root, &["rev-parse", BRANCH]);
        sh(&w.root, &["switch", "-q", "fleet/run"]);
        sh(&w.root, &["commit", "-q", "--allow-empty", "-m", "moved"]);
        sh(&w.root, &["switch", "-q", "main"]);
        let moved = sh(&w.root, &["rev-parse", "fleet/run"]);
        let id = worker(&mut w);

        let p = stage(&mut w, &id).unwrap().unwrap();
        assert_eq!(sh(&p.path, &["rev-parse", "HEAD"]), moved);

        fail(&mut w, &id);
        release(&mut w.store, &w.git, &p).unwrap();
        assert_eq!(sh(&w.root, &["rev-parse", BRANCH]), old);
    }

    #[test]
    fn a_leftover_branch_with_its_own_commits_is_refused() {
        let mut w = world();
        sh(&w.root, &["switch", "-q", "-c", BRANCH, "fleet/run"]);
        sh(&w.root, &["commit", "-q", "--allow-empty", "-m", "work"]);
        sh(&w.root, &["switch", "-q", "main"]);
        let tip = sh(&w.root, &["rev-parse", BRANCH]);
        let id = worker(&mut w);

        let err = stage(&mut w, &id).unwrap_err().to_string();

        assert!(err.contains("has commits"), "{err}");
        assert_eq!(sh(&w.root, &["rev-parse", BRANCH]), tip);
        assert!(!w.root.join(".worktrees/T1-import-it").exists());
    }

    #[test]
    fn an_existing_worktree_of_the_task_is_adopted_and_survives_a_failed_launch() {
        let mut w = world();
        let first = worker(&mut w);
        let made = stage(&mut w, &first).unwrap().unwrap();
        fail(&mut w, &first);
        let second = worker(&mut w);

        let p = stage(&mut w, &second).unwrap().unwrap();
        assert_eq!(p.path, made.path);
        fail(&mut w, &second);
        release(&mut w.store, &w.git, &p).unwrap();

        assert!(p.path.is_dir());
        assert!(w.git.branch_exists(&w.root, BRANCH));
    }
}
