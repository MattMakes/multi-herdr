use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::clock_store;
use crate::fleetd::fleet_runtime::adapters::config_file;
use crate::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use crate::fleetd::fleet_runtime::adapters::sqlite::{drop_reserved, state_of};
use crate::fleetd::fleet_runtime::application::jobs::{
    self, outcome, result_data, JobCtx, JobKey, NewItem, Queued,
};
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause, RecordRefusal};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::domain::task::AttemptState;
use crate::fleetd::worker_profiles::adapters::launcher::Launcher;
use crate::fleetd::worktree_custody::adapters::executors;
use crate::fleetd::worktree_custody::adapters::git::{
    worktree_add_key, BranchRefused, CliWorktreeGit, NoSandbox,
};
use crate::fleetd::worktree_custody::adapters::rescue_git::rescue_key;
use crate::fleetd::worktree_custody::adapters::run_now::{run_now, Starter};
use crate::fleetd::worktree_custody::adapters::sqlite::{
    self as store, NewWorktree, WorktreeRow, WorktreeState,
};
use crate::fleetd::worktree_custody::application::ports::WorktreeGitGateway;
use crate::fleetd::worktree_custody::application::{remove, rescue};
use crate::runtime::bins::{git_bin, BinOverrides};
use crate::runtime::context::{EnvSource, ProcessEnv};
use crate::runtime::paths::nonempty_path;
use crate::taskledger::model::{Actor, WorkerStatus};
use crate::taskledger::sql_words::WORKTREES_GONE;
use crate::taskledger::workers;

#[derive(Debug, Clone)]
pub struct Lease<'a> {
    pub task_id: Option<&'a str>,
    pub worker_id: &'a str,
    pub attempt: Option<&'a str>,
    pub from: Option<&'a str>,
    pub new: Option<&'a NewWorktree>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    Attached(WorktreeRow),
    NeedsRescue(WorktreeRow),
    Reserved { row: WorktreeRow, job: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attach {
    Attached(WorktreeRow),
    NeedsRescue(WorktreeRow),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeCreated {
    pub worktree_id: String,
    pub task_id: Option<String>,
    pub path: String,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    Created(WorktreeCreated),
    Dropped(String),
    Kept(String),
}

fn by_worker(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Worker,
        text,
    }
}

pub fn create_worktree(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    lease: &Lease<'_>,
    clock: FleetSeconds,
) -> Result<CreateOutcome> {
    if let Some(row) = task_tree(tx, lease.task_id)? {
        if lease.from.is_some_and(|f| f != row.base) {
            bail!(
                "existing worktree base is {}; drop it before changing base",
                row.base
            );
        }
        return match attach(tx, &row, lease.worker_id, lease.attempt)? {
            Attach::Attached(row) if row.state == WorktreeState::Reserved => {
                reserved(ctx, tx, row, clock)
            }
            Attach::Attached(row) => Ok(CreateOutcome::Attached(row)),
            Attach::NeedsRescue(row) => Ok(CreateOutcome::NeedsRescue(row)),
        };
    }
    let new = lease
        .new
        .context("the task has no worktree now; run the command again")?;
    let cause = by_worker("the worker asked for a worktree");
    store::insert_reserved(tx, ctx.guard, new, cause)?;
    store::set_owner(tx, &new.worktree_id, lease.attempt)?;
    let row = store::get(tx, &new.worktree_id)?.context("the reserved row is gone")?;
    reserved(ctx, tx, row, clock)
}

fn task_tree(tx: &Connection, task_id: Option<&str>) -> Result<Option<WorktreeRow>> {
    let Some(task_id) = task_id else {
        return Ok(None);
    };
    Ok(store::of_task(tx, task_id)?.filter(|r| !WORKTREES_GONE.contains(&r.status)))
}

fn reserved(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    row: WorktreeRow,
    clock: FleetSeconds,
) -> Result<CreateOutcome> {
    let job = add_run(ctx, tx, &row, clock)?;
    Ok(CreateOutcome::Reserved { row, job })
}

fn add_run(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    row: &WorktreeRow,
    clock: FleetSeconds,
) -> Result<i64> {
    let input_key = worktree_add_key(row);
    let key = JobKey {
        kind: Kind::WorktreeAdd,
        subject: &row.worktree_id,
        input_key: &input_key,
    };
    queued_run(ctx, tx, &key, &row.repo_root, clock)
}

fn queued_run(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    key: &JobKey<'_>,
    repo: &str,
    clock: FleetSeconds,
) -> Result<i64> {
    match jobs::queue(ctx, tx, key, Some(repo), clock)? {
        Queued::New(id) => Ok(id),
        Queued::Active(r) | Queued::Reused(r) => Ok(r.id),
        Queued::Held(r) => bail!(
            "the {} job of {} ended abandoned {} times; run horch job requeue {} after you read why",
            key.kind,
            key.subject,
            r.abandoned_runs,
            r.id
        ),
    }
}

pub fn attach(
    tx: &Connection,
    row: &WorktreeRow,
    worker: &str,
    attempt: Option<&str>,
) -> Result<Attach> {
    let id = row.worktree_id.as_str();
    match row.state {
        WorktreeState::Orphaned => return Ok(Attach::NeedsRescue(row.clone())),
        WorktreeState::Removed => bail!("worktree {id} is removed"),
        _ => {}
    }
    refuse_live_owner(tx, row, attempt)?;
    refuse_while_removal_active(tx, row)?;
    tx.execute(
        "UPDATE worktrees SET worker_id = ?2 WHERE worktree_id = ?1",
        params![id, worker],
    )?;
    store::set_owner(tx, id, attempt)?;
    if matches!(row.state, WorktreeState::Created | WorktreeState::Rescued) {
        let refusals: &dyn RecordRefusal = &NeedsYouList;
        let cause = by_worker("a new attempt of its task takes it");
        transition(tx, None, Entity::Worktree, id, "active", cause, refusals)?;
    }
    let row = store::get(tx, id)?.with_context(|| format!("no worktree {id} in the ledger"))?;
    Ok(Attach::Attached(row))
}

fn refuse_live_owner(tx: &Connection, row: &WorktreeRow, attempt: Option<&str>) -> Result<()> {
    let Some(owner) = row.owner_attempt.as_deref().filter(|o| Some(*o) != attempt) else {
        return Ok(());
    };
    let not_ended =
        state_of(tx, Entity::Attempt, owner)?.is_some_and(|s| s != AttemptState::Ended.as_str());
    let worker_live = match row.worker_id.as_deref() {
        Some(w) => workers::find(tx, w)?
            .is_some_and(|w| WorkerStatus::parse(&w.status).is_ok_and(WorkerStatus::is_live)),
        None => false,
    };
    if not_ended && worker_live {
        bail!(
            "worktree {} is held by the live attempt {owner}; a new attempt takes it only after that attempt ends (FM-14)",
            row.worktree_id
        );
    }
    Ok(())
}

fn refuse_while_removal_active(tx: &Connection, row: &WorktreeRow) -> Result<()> {
    let id = row.worktree_id.as_str();
    let Some(run) = remove::latest_removal(tx, id, &row.path)? else {
        return Ok(());
    };
    if matches!(run.state, State::Queued | State::Running | State::Passed) {
        bail!(
            "worktree {id} has the worktree_remove job {} in {}; it takes no new owner until a pass ends that job. Run the command again after that",
            run.id,
            run.state.as_str()
        );
    }
    Ok(())
}

pub fn release(tx: &Connection, row: &WorktreeRow, why: &str) -> Result<()> {
    let id = row.worktree_id.as_str();
    tx.execute(
        "UPDATE worktrees SET worker_id = NULL WHERE worktree_id = ?1",
        [id],
    )?;
    store::set_owner(tx, id, None)?;
    if row.state == WorktreeState::Active {
        let refusals: &dyn RecordRefusal = &NeedsYouList;
        transition(
            tx,
            None,
            Entity::Worktree,
            id,
            "created",
            by_worker(why),
            refusals,
        )?;
    }
    Ok(())
}

pub fn observe(ctx: &JobCtx<'_>, tx: &Connection, job: &JobRow) -> Result<Observed> {
    if job.kind != Kind::WorktreeAdd {
        bail!(
            "job {} is a {} job, not a worktree_add job",
            job.id,
            job.kind
        );
    }
    let id = job.subject.as_str();
    let Some(row) = store::get(tx, id)? else {
        return Ok(Observed::Kept(format!("no worktree {id} in the ledger")));
    };
    if row.state != WorktreeState::Reserved {
        return Ok(Observed::Kept(format!("worktree {id} is {}", row.state)));
    }
    let reason = outcome(job).map(|o| o.text).unwrap_or_default();
    match job.state {
        State::Passed if Path::new(&row.path).is_dir() => created(ctx, tx, row),
        State::Passed => {
            drop_reserved(tx, ctx.guard, id, "worktree_add_failed", Actor::Horch)?;
            Ok(Observed::Dropped(format!(
                "the worktree_add job {} passed, but {} does not exist",
                job.id, row.path
            )))
        }
        State::Failed => {
            if let Some(refused) = branch_refused(job) {
                open_branch_refused(ctx, tx, id, &refused, &reason)?;
            }
            if no_sandbox(job).is_some() {
                open_no_sandbox(ctx, tx, Kind::WorktreeAdd, id, &reason)?;
            }
            drop_reserved(tx, ctx.guard, id, "worktree_add_failed", Actor::Horch)?;
            Ok(Observed::Dropped(reason))
        }
        State::Abandoned => Ok(Observed::Kept(format!(
            "the worktree_add job {} ended abandoned: its process is gone with no result",
            job.id
        ))),
        State::Queued | State::Running => Ok(Observed::Kept(format!(
            "the worktree_add job {} is {}",
            job.id,
            job.state.as_str()
        ))),
    }
}

pub const GIT_SANDBOX_KIND: &str = "git_sandbox";

pub fn open_no_sandbox(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    kind: Kind,
    id: &str,
    reason: &str,
) -> Result<()> {
    let key = format!("{GIT_SANDBOX_KIND}:{kind}");
    let text = format!(
        "{reason}. Run fleetd on a host with a sandbox (macOS, or Linux with \
         bubblewrap on the PATH), then ask again"
    );
    let item = NewItem {
        key: &key,
        kind: GIT_SANDBOX_KIND,
        entity: Entity::Worktree,
        entity_id: id,
        text: &text,
    };
    ctx.needs_you.open_item(tx, &item)
}

pub const BRANCH_REFUSED_KIND: &str = "worktree_branch_refused";

fn branch_refused(job: &JobRow) -> Option<BranchRefused> {
    serde_json::from_value(result_data(job)?).ok()
}

fn no_sandbox(job: &JobRow) -> Option<NoSandbox> {
    serde_json::from_value(result_data(job)?).ok()
}

fn open_branch_refused(
    ctx: &JobCtx<'_>,
    tx: &Connection,
    id: &str,
    refused: &BranchRefused,
    reason: &str,
) -> Result<()> {
    let key = format!("{BRANCH_REFUSED_KIND}:{}", refused.branch);
    let fix = match &refused.held_by {
        Some(tree) => {
            format!("Commit or move the work of {tree}, then run git worktree remove there")
        }
        None => format!(
            "Keep the work of {} on another branch, then delete it",
            refused.branch
        ),
    };
    let text = format!("{reason}. {fix}, and ask for the worktree again");
    let item = NewItem {
        key: &key,
        kind: BRANCH_REFUSED_KIND,
        entity: Entity::Worktree,
        entity_id: id,
        text: &text,
    };
    ctx.needs_you.open_item(tx, &item)
}

fn stays_created(row: &WorktreeRow) -> bool {
    row.worker_id.is_none() && row.owner_attempt.is_none()
}

fn created(ctx: &JobCtx<'_>, tx: &Connection, row: WorktreeRow) -> Result<Observed> {
    let id = row.worktree_id.as_str();
    let refusals: &dyn RecordRefusal = ctx.needs_you;
    let made = Cause {
        actor: Actor::Horch,
        text: "its worktree_add job passed and its folder exists",
    };
    transition(
        tx,
        ctx.guard,
        Entity::Worktree,
        id,
        "created",
        made,
        refusals,
    )?;
    if stays_created(&row) {
        return Ok(Observed::Created(WorktreeCreated {
            worktree_id: row.worktree_id,
            task_id: row.task_id,
            path: row.path,
            branch: row.branch,
        }));
    }
    let owned = Cause {
        actor: Actor::Horch,
        text: "the owner of the request takes it",
    };
    transition(
        tx,
        ctx.guard,
        Entity::Worktree,
        id,
        "active",
        owned,
        refusals,
    )?;
    Ok(Observed::Created(WorktreeCreated {
        worktree_id: row.worktree_id,
        task_id: row.task_id,
        path: row.path,
        branch: row.branch,
    }))
}

pub trait RunJob {
    fn run_job(&self, ctx: &JobCtx<'_>, conn: &mut Connection, job_id: i64) -> Result<JobRow>;
}

#[derive(Debug, Clone)]
pub struct JobProcess {
    horch: PathBuf,
}

impl JobProcess {
    pub fn new(horch: impl Into<PathBuf>) -> JobProcess {
        JobProcess {
            horch: horch.into(),
        }
    }

    pub fn of_this_process() -> Result<JobProcess> {
        let exe = ProcessEnv
            .current_exe()
            .context("cannot read the path of this process")?;
        if exe.file_stem().and_then(|s| s.to_str()) != Some("horch") {
            bail!(
                "{} is not horch, so it cannot start `horch job run`: run the horch command, \
                 or pass a RunJob to the _with function",
                exe.display()
            );
        }
        Ok(JobProcess::new(exe))
    }
}

impl RunJob for JobProcess {
    fn run_job(&self, ctx: &JobCtx<'_>, conn: &mut Connection, job_id: i64) -> Result<JobRow> {
        let starter = Starter {
            ctx,
            host: &Launcher,
            horch: &self.horch,
            output: None,
        };
        run_now(conn, job_id, &starter)
    }
}

pub fn fleet_config(ledger: &Path) -> Result<FleetConfig> {
    let root = nonempty_path(&ProcessEnv, "FLEET_ROOT")
        .unwrap_or_else(|| ledger.parent().unwrap_or(Path::new("")).join("fleet"));
    config_file::load(&root)
}

pub fn create_and_wait(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    jobs: &dyn RunJob,
    lease: &Lease<'_>,
) -> Result<String> {
    let tx = immediate(conn)?;
    let clock = fleet_clock(&tx)?;
    let outcome = create_worktree(ctx, &tx, lease, clock)?;
    tx.commit()?;
    match outcome {
        CreateOutcome::Attached(row) => Ok(row.worktree_id),
        CreateOutcome::NeedsRescue(row) => {
            rescue_now(conn, ctx, jobs, &row)?;
            let tx = immediate(conn)?;
            let id = &row.worktree_id;
            let row = store::get(&tx, id)?.with_context(|| format!("no worktree {id}"))?;
            match attach(&tx, &row, lease.worker_id, lease.attempt)? {
                Attach::Attached(row) => {
                    tx.commit()?;
                    Ok(row.worktree_id)
                }
                Attach::NeedsRescue(_) => {
                    bail!("worktree {id} is still orphaned after its rescue")
                }
            }
        }
        CreateOutcome::Reserved { row, job } => wait_added(conn, ctx, jobs, &row, job),
    }
}

pub fn finish_reserved(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    jobs: &dyn RunJob,
    id: &str,
) -> Result<String> {
    let tx = immediate(conn)?;
    let row = store::get(&tx, id)?.with_context(|| format!("no worktree {id}"))?;
    if row.state != WorktreeState::Reserved {
        return Ok(row.worktree_id);
    }
    let last = jobs_store::latest(&tx, Kind::WorktreeAdd, id, &worktree_add_key(&row))?;
    let job = match last {
        Some(r) if matches!(r.state, State::Passed | State::Failed) => r.id,
        _ => add_run(ctx, &tx, &row, fleet_clock(&tx)?)?,
    };
    tx.commit()?;
    wait_added(conn, ctx, jobs, &row, job)
}

fn wait_added(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    jobs: &dyn RunJob,
    row: &WorktreeRow,
    job: i64,
) -> Result<String> {
    let ended = jobs.run_job(ctx, conn, job)?;
    let tx = immediate(conn)?;
    let seen = observe(ctx, &tx, &ended)?;
    let now = store::get(&tx, &row.worktree_id)?;
    tx.commit()?;
    match (seen, now) {
        (Observed::Dropped(reason), _) => bail!(
            "git worktree add failed, so the ledger keeps no worktree {}: {reason}",
            row.path
        ),
        (_, Some(now)) if now.state == WorktreeState::Active => Ok(now.worktree_id),
        (_, Some(now)) if now.state == WorktreeState::Created && stays_created(&now) => {
            Ok(now.worktree_id)
        }
        (Observed::Kept(why), _) => bail!(
            "worktree {} is not made yet: {why}; run the command again",
            row.path
        ),
        (_, _) => bail!("worktree {} is not active", row.path),
    }
}

pub fn give_to_operator(tx: &Connection, id: &str) -> Result<()> {
    let cause = Cause {
        actor: Actor::Operator,
        text: "horch worktree restore gives it to the operator, with no owner",
    };
    transition(
        tx,
        None,
        Entity::Worktree,
        id,
        "created",
        cause,
        &NeedsYouList,
    )?;
    Ok(())
}

pub fn rescue_now(
    conn: &mut Connection,
    ctx: &JobCtx<'_>,
    jobs: &dyn RunJob,
    row: &WorktreeRow,
) -> Result<()> {
    let root = executors::fleet_root(conn)?;
    let git = CliWorktreeGit::new(git_bin(&BinOverrides::from_env(&ProcessEnv)), root)
        .reading(&[PathBuf::from(&row.repo_root)]);
    let head = git
        .tree(Path::new(&row.path))?
        .with_context(|| format!("the orphaned worktree {} is not a work tree", row.path))?
        .head;
    let input_key = rescue_key(&row.path, &head);
    let key = JobKey {
        kind: Kind::Rescue,
        subject: &row.worktree_id,
        input_key: &input_key,
    };
    let tx = immediate(conn)?;
    let clock = fleet_clock(&tx)?;
    let job = queued_run(ctx, &tx, &key, &row.repo_root, clock)?;
    tx.commit()?;
    let ended = jobs.run_job(ctx, conn, job)?;
    let tx = immediate(conn)?;
    let seen = rescue::observe(ctx, &tx, &ended)?;
    tx.commit()?;
    match seen {
        rescue::Observed::Rescued(_) => Ok(()),
        _ => bail!(
            "the rescue of the orphaned worktree {} did not pass ({}: {}); its tree stays",
            row.path,
            ended.state.as_str(),
            outcome(&ended).map(|o| o.text).unwrap_or_default()
        ),
    }
}

fn immediate(conn: &mut Connection) -> Result<Transaction<'_>> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .context("taking the task ledger's write lock")
}

pub fn fleet_clock(conn: &Connection) -> Result<FleetSeconds> {
    match clock_store::read(conn) {
        Ok(clock) => Ok(clock),
        Err(e)
            if matches!(
                e.downcast_ref::<rusqlite::Error>(),
                Some(rusqlite::Error::QueryReturnedNoRows)
            ) =>
        {
            Ok(FleetSeconds(0))
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
pub(crate) struct TestJobs;

#[cfg(test)]
impl RunJob for TestJobs {
    fn run_job(&self, ctx: &JobCtx<'_>, conn: &mut Connection, job_id: i64) -> Result<JobRow> {
        use crate::fleetd::fleet_runtime::adapters::jobs_store;
        use crate::fleetd::fleet_runtime::application::jobs::{
            CheckRunner, JobRunner, SandboxProfiles,
        };
        use crate::fleetd::worker_profiles::application::ports::LaunchSpec;
        use crate::taskledger::workers::Worker;

        struct NoProfiles;
        impl SandboxProfiles for NoProfiles {
            fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> Result<Option<LaunchSpec>> {
                Ok(None)
            }
        }
        let get = |conn: &Connection| -> Result<JobRow> {
            jobs_store::get(conn, job_id)?.with_context(|| format!("no job {job_id}"))
        };
        let job = get(conn)?;
        if job.state == State::Queued {
            let spec = LaunchSpec {
                program: "true".into(),
                args: Vec::new(),
                cwd: None,
                env_rule: Default::default(),
                detach: false,
                output: None,
            };
            JobRunner::new(&Launcher).start(ctx, conn, &job, &spec, FleetSeconds(0))?;
        }
        let job = get(conn)?;
        if job.state == State::Running {
            let checks = CheckRunner {
                host: &Launcher,
                profiles: &NoProfiles,
                sweep: None,
            };
            let pid = job.pid.context("a running job has a pid")?;
            jobs::run_job(ctx, conn, job_id, pid, &checks)?;
            jobs::end_from_files(ctx, conn)?;
        }
        get(conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::worktree_custody::adapters::git::{DETACHED, GATE_PREFIX};
    use crate::fleetd::worktree_custody::adapters::sqlite::WorktreeMode;
    use crate::taskledger::model::WorktreeStatus;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::{finish, util, Store};

    fn git(dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    #[test]
    fn create_and_wait_gives_a_gate_tree_that_stays_created() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "a"]);
        git(&root, &["branch", "fleet/r1"]);
        let tip = git(&root, &["rev-parse", "HEAD"]);
        let ledger = dir.path().join("ledger.sqlite3");
        let mut ledger_store = Store::open(&ledger).unwrap();
        let run = ledger_store
            .immediate(|tx| {
                runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some("fleet/r1".into()),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        let path = root.join(".worktrees").join(format!("{GATE_PREFIX}r1"));
        let new = NewWorktree {
            worktree_id: util::new_id(),
            task_id: None,
            worker_id: None,
            repo_root: run.project_root.clone(),
            path: path.to_string_lossy().into_owned(),
            branch: DETACHED.to_string(),
            base: "fleet/r1".into(),
            base_commit: Some(tip),
            status: WorktreeStatus::Open,
            mode: Some(WorktreeMode {
                run_id: run.run_id.clone(),
                name: format!("{GATE_PREFIX}r1"),
                scratch: true,
            }),
        };
        let lease = Lease {
            task_id: None,
            worker_id: "",
            attempt: None,
            from: None,
            new: Some(&new),
        };
        let id = finish::lease_and_wait(&ledger_store, &TestJobs, &lease).unwrap();
        assert_eq!(id, new.worktree_id);
        let row = store::get(ledger_store.conn(), &id).unwrap().unwrap();
        assert_eq!(row.state, WorktreeState::Created);
        assert!(path.is_dir());
    }

    fn repo_and_run(dir: &Path) -> (Store, runs::Run, String) {
        let root = dir.join("repo");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["commit", "-q", "--allow-empty", "-m", "a"]);
        let tip = git(&root, &["rev-parse", "HEAD"]);
        let mut ledger_store = Store::open(&dir.join("ledger.sqlite3")).unwrap();
        let run = ledger_store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        ..Default::default()
                    },
                )?;
                crate::taskledger::schema::run_idea(tx, &run.run_id, &util::now(), true)?;
                Ok(run)
            })
            .unwrap();
        (ledger_store, run, tip)
    }

    fn new_tree(run: &runs::Run, tip: &str, name: &str, worker_id: Option<String>) -> NewWorktree {
        let path = Path::new(&run.project_root).join(".worktrees").join(name);
        NewWorktree {
            worktree_id: util::new_id(),
            task_id: None,
            worker_id,
            repo_root: run.project_root.clone(),
            path: path.to_string_lossy().into_owned(),
            branch: name.to_string(),
            base: "main".into(),
            base_commit: Some(tip.to_string()),
            status: WorktreeStatus::Open,
            mode: None,
        }
    }

    fn lease_of<'a>(new: &'a NewWorktree, worker_id: &'a str) -> Lease<'a> {
        Lease {
            task_id: None,
            worker_id,
            attempt: None,
            from: None,
            new: Some(new),
        }
    }

    fn reconciled(ledger_store: &mut Store, id: &str) -> WorktreeState {
        use crate::fleetd::worktree_custody::application::reconcile::{reconcile_worktrees, Facts};
        let config = fleet_config(ledger_store.path()).unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        ledger_store
            .immediate(|tx| reconcile_worktrees(&ctx, tx, &Facts::default(), fleet_clock(tx)?))
            .unwrap();
        store::get(ledger_store.conn(), id).unwrap().unwrap().state
    }

    #[test]
    fn a_tree_with_no_worker_and_no_owner_stays_created_and_is_not_orphaned() {
        let dir = tempfile::tempdir().unwrap();
        let (mut ledger_store, run, tip) = repo_and_run(dir.path());
        let new = new_tree(&run, &tip, "feat", None);
        let id = finish::lease_and_wait(&ledger_store, &TestJobs, &lease_of(&new, "")).unwrap();
        let row = store::get(ledger_store.conn(), &id).unwrap().unwrap();
        assert_eq!(row.state, WorktreeState::Created);
        assert_eq!(reconciled(&mut ledger_store, &id), WorktreeState::Created);
    }

    #[test]
    fn a_tree_of_a_launching_attempt_goes_to_active_and_is_not_orphaned() {
        use crate::taskledger::tasks::{self, NewTask};
        let dir = tempfile::tempdir().unwrap();
        let (mut ledger_store, run, tip) = repo_and_run(dir.path());
        let (task_id, worker) = ledger_store
            .immediate(|tx| {
                let task = NewTask::work("t1", "t1.md", "opus");
                let task_id = tasks::add(tx, &run, &task, Actor::Orchestrator)?.task_id;
                let new = workers::NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "opus".into(),
                    ..Default::default()
                };
                let worker = workers::insert(tx, &new)?;
                tx.execute(
                    "INSERT INTO attempts (id, task_id, worker_id, state) VALUES ('A1', ?1, ?2, 'launching')",
                    [&task_id, &worker],
                )?;
                Ok((task_id, worker))
            })
            .unwrap();
        let mut new = new_tree(&run, &tip, "T1-feat", Some(worker.clone()));
        new.task_id = Some(task_id.clone());
        let lease = Lease {
            task_id: Some(&task_id),
            attempt: Some("A1"),
            ..lease_of(&new, &worker)
        };
        let id = finish::lease_and_wait(&ledger_store, &TestJobs, &lease).unwrap();
        let row = store::get(ledger_store.conn(), &id).unwrap().unwrap();
        assert_eq!(row.state, WorktreeState::Active);
        assert_eq!(row.owner_attempt.as_deref(), Some("A1"));
        assert_eq!(reconciled(&mut ledger_store, &id), WorktreeState::Active);
    }
}
