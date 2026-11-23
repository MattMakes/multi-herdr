use super::{
    finish::{self, WorktreeRow},
    git::Git,
    model::{Actor, WorktreeStatus},
    runs,
    sql_words::{self, word},
    store::{optional, Store},
    tasks, util, workers,
};
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::worktree_custody::adapters::sqlite::{NewWorktree, WorktreeMode};
use crate::fleetd::worktree_custody::application::create::{JobProcess, Lease, RunJob};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

pub fn valid_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 60
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("worktree name must contain 1 to 60 letters, digits, '-' or '_'");
    }
    Ok(())
}
pub fn create(
    store: &mut Store,
    git: &Git,
    worker_id: &str,
    name: &str,
    from: Option<&str>,
    scratch: bool,
) -> Result<WorktreeRow> {
    let jobs = JobProcess::of_this_process()?;
    let kind = if scratch {
        CreateKind::Scratch { from }
    } else {
        CreateKind::Task { from }
    };
    create_with(store, git, worker_id, name, kind, &jobs)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateKind<'a> {
    Task { from: Option<&'a str> },
    Scratch { from: Option<&'a str> },
}

impl<'a> CreateKind<'a> {
    fn from(self) -> Option<&'a str> {
        match self {
            CreateKind::Task { from } | CreateKind::Scratch { from } => from,
        }
    }
}

pub fn create_with(
    store: &mut Store,
    git: &Git,
    worker_id: &str,
    name: &str,
    kind: CreateKind<'_>,
    jobs: &dyn RunJob,
) -> Result<WorktreeRow> {
    if kind == (CreateKind::Task { from: None }) {
        return finish::worktree_new_with(store, git, worker_id, name, jobs);
    }
    valid_name(name)?;
    let worker = workers::get(store.conn(), worker_id)?;
    let run = runs::get(
        store.conn(),
        worker.run_id.as_deref().context("worker has no run")?,
    )?;
    let task = match kind {
        CreateKind::Scratch { .. } => None,
        CreateKind::Task { .. } => Some(owned_task(store, &worker, worker_id)?),
    };
    let existing = match &task {
        Some(t) => finish::worktree_of(store.conn(), &t.task_id)?,
        None => None,
    };
    let new = match existing {
        Some(_) => None,
        None => Some(new_tree(
            git,
            &run,
            task.as_ref(),
            worker_id,
            name,
            kind.from(),
        )?),
    };
    let attempt = finish::live_attempt(store.conn(), worker_id)?;
    let lease = Lease {
        task_id: task.as_ref().map(|t| t.task_id.as_str()),
        worker_id,
        attempt: attempt.as_deref(),
        from: kind.from(),
        new: new.as_ref(),
    };
    let id = finish::lease_tree(store, jobs, &lease)?;
    finish::worktree(store.conn(), &id)
}

fn owned_task(store: &Store, worker: &workers::Worker, worker_id: &str) -> Result<tasks::Task> {
    let id = worker
        .task_id
        .as_deref()
        .context("run horch ledger claim first")?;
    let t = tasks::get(store.conn(), id)?;
    if t.current_worker_id.as_deref() != Some(worker_id) {
        bail!("this worker no longer owns the task");
    }
    if !t.uses_worktree() {
        bail!("this task works in the shared checkout");
    }
    Ok(t)
}

fn new_tree(
    git: &Git,
    run: &runs::Run,
    task: Option<&tasks::Task>,
    worker_id: &str,
    name: &str,
    from: Option<&str>,
) -> Result<NewWorktree> {
    let root = Path::new(&run.project_root);
    let base = from
        .or(run.run_branch.as_deref())
        .or(run.base_branch.as_deref())
        .context("run has no git base")?;
    if base.starts_with('-') || base.contains(['\n', '\r']) {
        bail!("invalid base ref");
    }
    let commit = git.rev(root, base)?;
    let (path, branch) = tree_names(run, task, name);
    if path.exists() || git.branch_exists(root, &branch) {
        bail!("worktree path or branch already exists; pick another name");
    }
    Ok(NewWorktree {
        worktree_id: util::new_id(),
        task_id: task.map(|t| t.task_id.clone()),
        worker_id: Some(worker_id.to_string()),
        repo_root: run.project_root.clone(),
        path: path.to_string_lossy().into_owned(),
        branch,
        base: base.to_string(),
        base_commit: Some(commit),
        status: WorktreeStatus::Open,
        mode: Some(WorktreeMode {
            run_id: run.run_id.clone(),
            name: name.to_string(),
            scratch: task.is_none(),
        }),
    })
}

pub fn tree_names(run: &runs::Run, task: Option<&tasks::Task>, name: &str) -> (PathBuf, String) {
    let (slug, branch) = match task {
        None => (
            format!("_scratch/{name}"),
            format!("fleet-scratch-{}-{name}", run.slug),
        ),
        Some(t) => (
            format!("T{}-{name}", t.seq),
            format!(
                "{}-T{}-{name}",
                run.run_branch.as_deref().unwrap_or("fleet"),
                t.seq
            ),
        ),
    };
    let path = Path::new(&run.project_root).join(".worktrees").join(slug);
    (path, branch)
}

pub fn find(store: &Store, run_id: &str, name: &str) -> Result<WorktreeRow> {
    find_in(store.conn(), run_id, name)
}

pub fn find_in(conn: &Connection, run_id: &str, name: &str) -> Result<WorktreeRow> {
    let mut stmt=conn.prepare(&format!("SELECT w.worktree_id,w.path,m.name FROM worktrees w LEFT JOIN tasks t ON t.task_id=w.task_id LEFT JOIN worktree_modes m ON m.worktree_id=w.worktree_id WHERE (t.run_id=?1 OR m.run_id=?1) AND w.status NOT IN ({gone})",gone=sql_words::list(sql_words::WORKTREES_GONE)))?;
    let rows = stmt
        .query_map([run_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let ids: Vec<_> = rows
        .into_iter()
        .filter(|(id, path, mode)| {
            id == name
                || mode.as_deref() == Some(name)
                || Path::new(path).file_name().is_some_and(|p| p == name)
        })
        .map(|r| r.0)
        .collect();
    if ids.len() != 1 {
        bail!(
            "expected one registered worktree named {name}, found {}",
            ids.len()
        );
    }
    finish::worktree(conn, &ids[0])
}

fn verified_head(git: &Git, root: &Path, wt: &WorktreeRow) -> Result<String> {
    let path = Path::new(&wt.path);
    let recorded = git.rev(root, &wt.branch)?;
    if !path.exists() {
        return Ok(recorded);
    }
    let branch = git.current_branch(path).with_context(|| {
        format!("worktree HEAD is detached or unreadable; reattach the recorded branch {} before dropping it; nothing removed", wt.branch)
    })?;
    if branch != wt.branch {
        bail!("worktree is on {branch}, not its recorded branch {}; nothing removed (including with --keep-branch)", wt.branch);
    }
    let actual = git.rev(path, "HEAD")?;
    if actual != recorded {
        bail!(
            "worktree HEAD differs from its recorded branch {}; nothing removed",
            wt.branch
        );
    }
    Ok(actual)
}

pub fn drop_worktree(
    store: &mut Store,
    git: &Git,
    run_id: &str,
    name: &str,
    reason: &str,
    keep_branch: bool,
) -> Result<WorktreeRow> {
    let jobs = JobProcess::of_this_process()?;
    drop_worktree_with(store, git, run_id, name, reason, keep_branch, &jobs)
}

pub fn drop_worktree_with(
    store: &mut Store,
    git: &Git,
    run_id: &str,
    name: &str,
    reason: &str,
    keep_branch: bool,
    jobs: &dyn RunJob,
) -> Result<WorktreeRow> {
    if reason.trim().is_empty() {
        bail!("give a nonempty --reason");
    }
    let wt = find(store, run_id, name)?;
    let root = PathBuf::from(&wt.repo_root);
    let path = Path::new(&wt.path);
    if path.exists() && !git.dirty(path)?.is_empty() {
        bail!("worktree has uncommitted or untracked changes; nothing removed");
    }
    if !git.stashes_on(&root, &wt.branch)?.is_empty() {
        bail!("worktree branch has a stash; nothing removed");
    }
    let refs = git.run(
        &root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )?;
    let own = format!("refs/heads/{}", wt.branch);
    let head = verified_head(git, &root, &wt)?;
    let mut args = vec!["rev-list", head.as_str(), "--not"];
    args.extend(refs.lines().filter(|r| *r != own));
    let unique = git.run(&root, &args)?;
    if !keep_branch && !unique.is_empty() {
        bail!("worktree has unpublished commits that exist nowhere else; use --keep-branch; nothing removed");
    }
    if verified_head(git, &root, &wt)? != head {
        bail!("worktree HEAD changed during the drop checks; nothing removed");
    }
    if path.exists() {
        let cause = Cause {
            actor: Actor::Worker,
            text: "dropped",
        };
        if let Err(why) = finish::remove_tree(store, jobs, &wt.worktree_id, cause, keep_branch)? {
            bail!("the worktree stays: {why}; nothing removed");
        }
    }
    if !keep_branch && git.branch_exists(&root, &wt.branch) {
        let _ = git.try_run(&root, &["branch", "-d", &wt.branch]);
    }
    record_drop(store.conn(), &wt.worktree_id, reason)?;
    finish::worktree(store.conn(), &wt.worktree_id)
}

pub fn record_drop(conn: &Connection, id: &str, reason: &str) -> Result<()> {
    conn.execute(
        &format!(
            "UPDATE worktrees SET status={abandoned},reason=?2,cleaned_at=?3 WHERE worktree_id=?1",
            abandoned = word(WorktreeStatus::Abandoned)
        ),
        params![id, reason, util::now()],
    )?;
    Ok(())
}

pub fn is_scratch(store: &Store, id: &str) -> Result<bool> {
    Ok(optional(store.conn().query_row(
        "SELECT scratch FROM worktree_modes WHERE worktree_id=?1",
        [id],
        |r| r.get::<_, bool>(0),
    ))?
    .unwrap_or(false))
}
