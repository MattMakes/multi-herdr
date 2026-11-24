use super::{
    finish::{self, WorktreeRow},
    git::Git,
    model::WorktreeStatus,
    runs,
    sql_words::{self, word},
    store::{optional, Store},
    tasks, util, workers,
};
use anyhow::{bail, Context, Result};
use rusqlite::params;
use std::path::{Path, PathBuf};

fn valid_name(name: &str) -> Result<()> {
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

pub(crate) fn add_worktree(
    git: &Git,
    root: &Path,
    branch: &str,
    path: &str,
    commit: &str,
) -> Result<()> {
    let existed = git.branch_exists(root, branch);
    let added = git.run(root, &["worktree", "add", "-b", branch, path, commit]);
    if added.is_err() && !existed && git.branch_exists(root, branch) {
        let reference = format!("refs/heads/{branch}");
        let _ = git.try_run(root, &["update-ref", "-d", &reference, commit]);
    }
    added.map(|_| ())
}

pub fn create(
    store: &mut Store,
    git: &Git,
    worker_id: &str,
    name: &str,
    from: Option<&str>,
    scratch: bool,
) -> Result<WorktreeRow> {
    if !scratch && from.is_none() {
        return finish::worktree_new(store, git, worker_id, name);
    }
    valid_name(name)?;
    let worker = workers::get(store.conn(), worker_id)?;
    let run = runs::get(
        store.conn(),
        worker.run_id.as_deref().context("worker has no run")?,
    )?;
    let task = worker
        .task_id
        .as_deref()
        .map(|id| tasks::get(store.conn(), id))
        .transpose()?;
    if !scratch {
        let t = task.as_ref().context("run horch ledger claim first")?;
        if t.current_worker_id.as_deref() != Some(worker_id) {
            bail!("this worker no longer owns the task");
        }
        if !t.uses_worktree() {
            bail!("this task works in the shared checkout");
        }
        if let Some(wt) = finish::worktree_of(store.conn(), &t.task_id)? {
            if from.is_some_and(|f| f != wt.base) {
                bail!(
                    "existing worktree base is {}; drop it before changing base",
                    wt.base
                );
            }
            store.immediate(|tx| {
                tx.execute(
                    "UPDATE worktrees SET worker_id=?2 WHERE worktree_id=?1",
                    params![wt.worktree_id, worker_id],
                )?;
                Ok(())
            })?;
            return finish::worktree(store.conn(), &wt.worktree_id);
        }
    }
    let root = Path::new(&run.project_root);
    let base = from
        .or(run.run_branch.as_deref())
        .or(run.base_branch.as_deref())
        .context("run has no git base")?;
    if base.starts_with('-') || base.contains(['\n', '\r']) {
        bail!("invalid base ref");
    }
    let commit = git.rev(root, base)?;
    let slug = if scratch {
        format!("_scratch/{name}")
    } else {
        format!("T{}-{name}", task.as_ref().unwrap().seq)
    };
    let path = root.join(".worktrees").join(slug);
    let branch = if scratch {
        format!("fleet-scratch-{}-{name}", run.slug)
    } else {
        format!(
            "{}-T{}-{name}",
            run.run_branch.as_deref().unwrap_or("fleet"),
            task.as_ref().unwrap().seq
        )
    };
    if path.exists() || git.branch_exists(root, &branch) {
        bail!("worktree path or branch already exists; pick another name");
    }
    let id = util::new_id();
    let path_text = path.to_string_lossy().into_owned();
    store.immediate(|tx| {
        add_worktree(git, root, &branch, &path_text, &commit)?;
        let persisted=(|| -> Result<()> {
            tx.execute(&format!("INSERT INTO worktrees (worktree_id,task_id,worker_id,repo_root,path,branch,base,base_commit,status,origin,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,{open},'native',?9)",open=word(WorktreeStatus::Open)),params![id,if scratch {None}else{task.as_ref().map(|t|t.task_id.as_str())},worker_id,run.project_root,path_text,branch,base,commit,util::now()])?;
            tx.execute("INSERT INTO worktree_modes(worktree_id,run_id,name,scratch) VALUES (?1,?2,?3,?4)",params![id,run.run_id,name,scratch])?;
            Ok(())
        })();
        if let Err(err)=persisted {
            let _=git.try_run(root,&["worktree","remove",&path_text]);
            let _=git.try_run(root,&["branch","-d",&branch]);
            return Err(err);
        }
        Ok(())
    })?;
    finish::worktree(store.conn(), &id)
}

pub fn find(store: &Store, run_id: &str, name: &str) -> Result<WorktreeRow> {
    let mut stmt=store.conn().prepare(&format!("SELECT w.worktree_id,w.path,m.name FROM worktrees w LEFT JOIN tasks t ON t.task_id=w.task_id LEFT JOIN worktree_modes m ON m.worktree_id=w.worktree_id WHERE (t.run_id=?1 OR m.run_id=?1) AND w.status NOT IN ({gone})",gone=sql_words::list(sql_words::WORKTREES_GONE)))?;
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
    finish::worktree(store.conn(), &ids[0])
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
    store.immediate(|tx| {
        if verified_head(git, &root, &wt)? != head {
            bail!("worktree HEAD changed during the drop checks; nothing removed");
        }
        if path.exists() {
            git.run(&root, &["worktree", "remove", &wt.path])?;
        }
        if !keep_branch && git.branch_exists(&root, &wt.branch) {
            let _ = git.try_run(&root, &["branch", "-d", &wt.branch]);
        }
        tx.execute(
            &format!("UPDATE worktrees SET status={abandoned},reason=?2,cleaned_at=?3 WHERE worktree_id=?1",abandoned=word(WorktreeStatus::Abandoned)),
            params![wt.worktree_id, reason, util::now()],
        )?;
        Ok(())
    })?;
    finish::worktree(store.conn(), &wt.worktree_id)
}

pub fn is_scratch(store: &Store, id: &str) -> Result<bool> {
    Ok(optional(store.conn().query_row(
        "SELECT scratch FROM worktree_modes WHERE worktree_id=?1",
        [id],
        |r| r.get::<_, bool>(0),
    ))?
    .unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn repo() -> (tempfile::TempDir, PathBuf, Git, String) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap().join("repo");
        let git = Git::new("git");
        std::fs::create_dir(&root).unwrap();
        git.run(&root, &["init", "-q"]).unwrap();
        git.run(
            &root,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        )
        .unwrap();
        let commit = git.rev(&root, "HEAD").unwrap();
        (tmp, root, git, commit)
    }

    #[test]
    fn a_failed_worktree_add_removes_the_branch_it_made() {
        let (_tmp, root, git, commit) = repo();
        let admin = root.join(".git").join("worktrees");
        std::fs::create_dir(&admin).unwrap();
        std::fs::set_permissions(&admin, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::create_dir(admin.join("probe")).is_ok() {
            return;
        }
        let path = root.join(".worktrees").join("T1-x");

        let added = add_worktree(&git, &root, "fleet-T1-x", &path.to_string_lossy(), &commit);
        std::fs::set_permissions(&admin, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(added.is_err());
        assert!(!git.branch_exists(&root, "fleet-T1-x"));
        assert!(!path.exists());
    }

    #[test]
    fn a_failed_worktree_add_keeps_a_branch_that_was_there_before() {
        let (_tmp, root, git, commit) = repo();
        git.run(&root, &["branch", "fleet-T1-x", &commit]).unwrap();
        let path = root.join(".worktrees").join("T1-x");

        let added = add_worktree(&git, &root, "fleet-T1-x", &path.to_string_lossy(), &commit);

        assert!(added.is_err());
        assert!(git.branch_exists(&root, "fleet-T1-x"));
    }

    #[test]
    fn a_worktree_add_makes_the_branch_and_the_folder() {
        let (_tmp, root, git, commit) = repo();
        let path = root.join(".worktrees").join("T1-x");

        add_worktree(&git, &root, "fleet-T1-x", &path.to_string_lossy(), &commit).unwrap();

        assert!(git.branch_exists(&root, "fleet-T1-x"));
        assert!(path.join(".git").exists());
    }
}
