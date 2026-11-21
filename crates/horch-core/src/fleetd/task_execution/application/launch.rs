use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::fleetd::fleet_runtime::adapters::effects::{self, LaunchRecord};
use crate::fleetd::fleet_runtime::adapters::jobs_store;
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::fleet_runtime::domain::pass::START_TIMEOUT_S;
use crate::fleetd::ideas::adapters::questions_store;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::adapters::sqlite as store;
use crate::fleetd::task_execution::application::commands;
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::task::{
    AttemptState, ProcessScope, Recorded, RowRef, Stop, Target,
};
use crate::fleetd::worker_profiles::application::ports::{LaunchSpec, Launched, ProcessHost};
use crate::fleetd::worktree_custody::adapters::sqlite::{self as trees, WorktreeState};
use crate::taskledger::model::{Actor, ProgressSource, TaskStatus};
use crate::taskledger::sql_words::WORKTREES_GONE;
use crate::taskledger::{runs, tasks, util, workers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeFact {
    Ready,
    Adding,
    AddFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Launch,
    WaitTree,
    Stop(Vec<Recorded>),
    Crash(String),
}

const TREE_FAILED: &str = "its worktree_add job failed, so its tree does not exist";

pub fn decide(tree: TreeFact, older_alive: &[Recorded]) -> Decision {
    match tree {
        TreeFact::AddFailed => Decision::Crash(TREE_FAILED.to_string()),
        _ if !older_alive.is_empty() => Decision::Stop(older_alive.to_vec()),
        TreeFact::Adding => Decision::WaitTree,
        TreeFact::Ready => Decision::Launch,
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub launches: Vec<String>,
    pub stops: Vec<Stop>,
    pub crashed: Vec<String>,
}

struct Pending {
    id: String,
    task_id: String,
    worker_id: String,
}

fn pending(conn: &Connection) -> Result<Vec<Pending>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, worker_id FROM attempts
         WHERE state = 'launching' AND via = 'push' AND task_id IS NOT NULL
         ORDER BY rowid",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Pending {
            id: r.get(0)?,
            task_id: r.get(1)?,
            worker_id: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn pending_one(conn: &Connection, id: &str) -> Result<Option<Pending>> {
    let row = conn.query_row(
        "SELECT id, task_id, worker_id FROM attempts
         WHERE id = ?1 AND state = 'launching' AND via = 'push' AND task_id IS NOT NULL",
        [id],
        |r| {
            Ok(Pending {
                id: r.get(0)?,
                task_id: r.get(1)?,
                worker_id: r.get(2)?,
            })
        },
    );
    Ok(row.optional()?)
}

fn tree_fact(conn: &Connection, task_id: &str) -> Result<TreeFact> {
    if !tasks::get(conn, task_id)?.uses_worktree() {
        return Ok(TreeFact::Ready);
    }
    let Some(row) = trees::of_task(conn, task_id)?.filter(|r| !WORKTREES_GONE.contains(&r.status))
    else {
        return Ok(TreeFact::AddFailed);
    };
    if row.state != WorktreeState::Reserved {
        return Ok(TreeFact::Ready);
    }
    let job = jobs_store::last(conn, Kind::WorktreeAdd, &row.worktree_id)?;
    Ok(match job.map(|j| j.state) {
        Some(State::Failed) => TreeFact::AddFailed,
        _ => TreeFact::Adding,
    })
}

fn older_processes(
    conn: &Connection,
    task_id: &str,
    attempt_id: &str,
) -> Result<Vec<(String, Recorded)>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, COALESCE(a.pid, w.agent_pid),
            CASE WHEN a.pid IS NULL THEN w.agent_started ELSE a.pid_start END
         FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
         WHERE a.task_id = ?1 AND a.id <> ?2
         ORDER BY a.rowid",
    )?;
    let rows = stmt.query_map(params![task_id, attempt_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<i64>>(1)?,
            r.get::<_, Option<i64>>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, pid, start) = row?;
        if let Some(pid) = pid.and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 0) {
            let pid_start = start.and_then(|s| u64::try_from(s).ok());
            out.push((id, Recorded { pid, pid_start }));
        }
    }
    Ok(out)
}

fn older_alive(
    conn: &Connection,
    p: &Pending,
    alive: &dyn Fn(Recorded) -> bool,
) -> Result<Vec<(String, Recorded)>> {
    Ok(older_processes(conn, &p.task_id, &p.id)?
        .into_iter()
        .filter(|(_, r)| alive(*r))
        .collect())
}

fn decision_of(
    conn: &Connection,
    p: &Pending,
    alive: &dyn Fn(Recorded) -> bool,
) -> Result<(Decision, Vec<(String, Recorded)>)> {
    let older = older_alive(conn, p, alive)?;
    let live: Vec<Recorded> = older.iter().map(|(_, r)| *r).collect();
    Ok((decide(tree_fact(conn, &p.task_id)?, &live), older))
}

fn stop_of(attempt: &str, r: Recorded) -> Option<Stop> {
    Some(Stop {
        row: RowRef::Attempt(attempt.to_string()),
        target: Target::Process {
            pid: r.pid,
            pid_start: r.pid_start?,
            scope: ProcessScope::LeaderGroup,
        },
    })
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn crash(tx: &Connection, guard: Option<Generation>, p: &Pending, why: &str) -> Result<()> {
    let outcome = Outcome::Crashed.as_str();
    store::set_ended(tx, &p.id, outcome, &util::now())?;
    let text = format!("attempt {} ended {outcome}: {why}", p.id);
    transition(
        tx,
        guard,
        Entity::Attempt,
        &p.id,
        AttemptState::Ended.as_str(),
        by_fleetd(&text),
        &NeedsYouList,
    )?;
    workers::launch_failed(tx, &p.worker_id, why)?;
    commands::schedule_retry(tx, &p.task_id, why)
}

pub fn end_failed_spawn(
    tx: &Connection,
    guard: Option<Generation>,
    worker_id: &str,
    why: &str,
) -> Result<Vec<String>> {
    let mut ended = Vec::new();
    for p in pending(tx)?
        .into_iter()
        .filter(|p| p.worker_id == worker_id)
    {
        crash(tx, guard, &p, why)?;
        ended.push(p.id);
    }
    Ok(ended)
}

pub fn plan(
    tx: &Connection,
    guard: Option<Generation>,
    alive: &dyn Fn(Recorded) -> bool,
) -> Result<Plan> {
    if tx.is_autocommit() {
        bail!("the launch plan writes its rows in 1 open transaction of the ledger");
    }
    let mut out = Plan::default();
    for p in pending(tx)? {
        let (decision, older) = decision_of(tx, &p, alive)?;
        match decision {
            Decision::Launch => out.launches.push(p.id),
            Decision::WaitTree => {}
            Decision::Stop(_) => {
                for (attempt, r) in older {
                    out.stops.extend(stop_of(&attempt, r));
                }
            }
            Decision::Crash(why) => {
                crash(tx, guard, &p, &why)?;
                out.crashed.push(p.id);
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushLaunch {
    pub attempt: String,
    pub task_id: String,
    pub worker_id: String,
    pub persona: String,
    pub run_slug: String,
    pub project_root: String,
    pub from_pane: Option<String>,
    pub workspace_id: Option<String>,
}

fn push_launch(conn: &Connection, p: &Pending) -> Result<PushLaunch> {
    let w = workers::get(conn, &p.worker_id)?;
    let run = runs::get(conn, &tasks::get(conn, &p.task_id)?.run_id)?;
    let from_pane = match &run.orchestrator_worker_id {
        Some(id) => workers::find(conn, id)?.and_then(|o| o.pane_id),
        None => None,
    };
    Ok(PushLaunch {
        attempt: p.id.clone(),
        task_id: p.task_id.clone(),
        worker_id: w.worker_id.clone(),
        persona: w.persona.clone().unwrap_or_default(),
        run_slug: run.slug,
        project_root: run.project_root,
        from_pane,
        workspace_id: run.workspace_id.or(w.workspace_id),
    })
}

pub trait Launches {
    fn alive(&self, p: Recorded) -> bool {
        crate::procid::alive(p.pid, p.pid_start)
            || p.pid_start
                .is_some_and(|s| crate::procid::is_same_group(p.pid, s))
    }

    fn spec(&self, l: &PushLaunch) -> Result<LaunchSpec>;

    fn resuming_harness(&self, _persona: &str) -> Option<String> {
        None
    }
}

pub const SESSION_ID_MAX_BYTES: usize = 128;

pub fn session_id_ok(id: &str) -> bool {
    (1..=SESSION_ID_MAX_BYTES).contains(&id.len())
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn resume_of(conn: &Connection, p_id: &str, ports: &dyn Launches) -> Result<Option<String>> {
    let Some(p) = pending_one(conn, p_id)? else {
        return Ok(None);
    };
    let prev = conn
        .query_row(
            "SELECT a.id, a.outcome, w.session_id, w.persona
             FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
             WHERE a.task_id = ?1 AND a.id <> ?2
             ORDER BY a.rowid DESC LIMIT 1",
            params![p.task_id, p.id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((prev_id, outcome, Some(session), prev_persona)) = prev else {
        return Ok(None);
    };
    if outcome.as_deref() != Some(Outcome::Asked.as_str()) || !session_id_ok(&session) {
        return Ok(None);
    }
    let answered = questions_store::answered_of_attempt(conn, &prev_id)?;
    let persona = workers::get(conn, &p.worker_id)?
        .persona
        .unwrap_or_default();
    let now = ports.resuming_harness(&persona);
    let then = ports.resuming_harness(prev_persona.as_deref().unwrap_or_default());
    Ok((answered && now.is_some() && now == then).then_some(session))
}

pub fn resume_refused(conn: &Connection, attempt_id: &str) -> Result<bool> {
    let row = conn
        .query_row(
            "SELECT a.started_at, w.resumed_from_session, w.last_progress_source,
                (SELECT b.outcome FROM attempts b
                 WHERE b.task_id = a.task_id AND b.rowid < a.rowid
                 ORDER BY b.rowid DESC LIMIT 1)
             FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
             WHERE a.id = ?1",
            params![attempt_id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((Some(started_at), Some(_), progress, Some(before))) = row else {
        return Ok(false);
    };
    let in_window =
        util::seconds_between(&started_at, &util::now()).is_some_and(|s| s <= START_TIMEOUT_S);
    Ok(before == Outcome::Asked.as_str()
        && in_window
        && progress.as_deref() != Some(ProgressSource::Agent.as_str()))
}

pub fn on_resume_refused(
    tx: &Connection,
    guard: Option<Generation>,
    task_id: &str,
    attempt_id: &str,
) -> Result<()> {
    if tasks::cancel_requested(tx, task_id)?.is_some() {
        return Ok(());
    }
    tx.execute(
        "UPDATE tasks SET attempt_floor = attempt_floor + 1, current_worker_id = NULL,
            not_before = NULL, updated_at = ?2
         WHERE task_id = ?1",
        params![task_id, util::now()],
    )?;
    let text = format!(
        "RR-12: the CLI refused the resume of attempt {attempt_id}; a fresh attempt \
         starts from the question, the answer and the committed work"
    );
    let cause = Cause {
        actor: Actor::Horch,
        text: &text,
    };
    let ready = TaskStatus::Ready.as_str();
    transition(
        tx,
        guard,
        Entity::Task,
        task_id,
        ready,
        cause,
        &NeedsYouList,
    )?;
    Ok(())
}

#[derive(Debug)]
pub enum Launch {
    Launched(Launched),
    Done,
    Waiting(String),
}

pub fn launch(
    conn: &Connection,
    fleet_root: &Path,
    clock: FleetSeconds,
    attempt_id: &str,
    host: &dyn ProcessHost,
    ports: &dyn Launches,
) -> Result<Launch> {
    if !conn.is_autocommit() {
        bail!("attempt {attempt_id} launches after the commit, never inside a transaction");
    }
    let Some(p) = pending_one(conn, attempt_id)? else {
        return Ok(Launch::Done);
    };
    if effects::read_launch(fleet_root, attempt_id)?.is_some() {
        return Ok(Launch::Done);
    }
    let (decision, _) = decision_of(conn, &p, &|r| ports.alive(r))?;
    match decision {
        Decision::Launch => {}
        Decision::WaitTree => return Ok(Launch::Waiting("its tree is reserved".into())),
        Decision::Stop(alive) => {
            let pids: Vec<String> = alive.iter().map(|r| r.pid.to_string()).collect();
            return Ok(Launch::Waiting(format!(
                "a process of an older attempt is alive: pid {}",
                pids.join(", ")
            )));
        }
        Decision::Crash(why) => {
            return Ok(Launch::Waiting(format!("the next pass ends it: {why}")))
        }
    }
    let l = push_launch(conn, &p)?;
    let resume = resume_of(conn, attempt_id, ports)?;
    conn.execute(
        "UPDATE workers SET resumed_from_session = ?2 WHERE worker_id = ?1",
        params![l.worker_id, resume],
    )?;
    let record = LaunchRecord {
        attempt: attempt_id.to_string(),
        launched_at: clock,
        process: None,
    };
    effects::write_launch(fleet_root, &record)?;
    let launched = ports
        .spec(&l)
        .and_then(|spec| host.launch(&spec).map_err(anyhow::Error::from));
    match launched {
        Ok(started) => Ok(Launch::Launched(started)),
        Err(e) => {
            let reason = format!("the launch of attempt {attempt_id} did not start: {e:#}");
            util::atomic(conn, |tx| end_failed_spawn(tx, None, &l.worker_id, &reason))?;
            Err(e).with_context(|| format!("launching attempt {attempt_id}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;
    use crate::fleetd::fleet_runtime::adapters::effects;
    use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
    use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
    use crate::fleetd::ideas::application::needs_you::NeedsYouList;
    use crate::fleetd::task_execution::application::commands;
    use crate::fleetd::task_execution::application::dispatch::{bind, Bound, DispatchCtx};
    use crate::fleetd::task_execution::domain::task::{ProcessScope, Reap, RowRef, Target};
    use crate::fleetd::worker_profiles::adapters::launcher::Launcher;
    use crate::fleetd::worker_profiles::application::ports::{EnvRule, LaunchError};
    use crate::fleetd::worktree_custody::adapters::sqlite as trees;
    use crate::fleetd::worktree_custody::adapters::sqlite::WorktreeState;
    use crate::fleetd::worktree_custody::application::create::{self, TestJobs};
    use crate::taskledger::git::Git;
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun, Run};
    use crate::taskledger::tasks::{self, NewTask};
    use crate::taskledger::workers::{self, NewWorker};
    use crate::taskledger::Store;

    const RUN_BRANCH: &str = "fleet/r1";
    const ORCHESTRATOR_PANE: &str = "w0:p0";
    const OLD: Recorded = Recorded {
        pid: 4_242,
        pid_start: Some(77),
    };

    fn git(dir: &Path, args: &[&str]) {
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
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }

    struct World {
        _tmp: tempfile::TempDir,
        fleet_root: PathBuf,
        ledger: PathBuf,
        store: Store,
        run: Run,
        config: FleetConfig,
    }

    impl World {
        fn new() -> World {
            let tmp = tempfile::tempdir().unwrap();
            let top = horch_platform::path::for_child(&tmp.path().canonicalize().unwrap());
            let root = top.join("repo");
            let fleet_root = top.join("fleet");
            std::fs::create_dir_all(&root).unwrap();
            std::fs::create_dir_all(&fleet_root).unwrap();
            git(&root, &["init", "--quiet", "-b", "main"]);
            std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
            git(&root, &["add", ".gitignore"]);
            git(&root, &["commit", "--quiet", "-m", "base"]);
            git(&root, &["branch", RUN_BRANCH]);
            let ledger = top.join("ledger.sqlite3");
            let mut store = Store::open(&ledger).unwrap();
            let run = store
                .immediate(|tx| {
                    let run = runs::create(
                        tx,
                        &root,
                        &NewRun {
                            slug: "r1".into(),
                            workspace_id: Some("w0".into()),
                            base_branch: Some("main".into()),
                            run_branch: Some(RUN_BRANCH.into()),
                            ..Default::default()
                        },
                    )?;
                    crate::taskledger::schema::run_idea(
                        tx,
                        &run.run_id,
                        &crate::taskledger::util::now(),
                        true,
                    )?;
                    let orchestrator = workers::insert(
                        tx,
                        &NewWorker {
                            run_id: run.run_id.clone(),
                            persona: "orchestrator".into(),
                            pane_id: Some(ORCHESTRATOR_PANE.into()),
                            workspace_id: Some("w0".into()),
                            ..Default::default()
                        },
                    )?;
                    tx.execute(
                        "UPDATE runs SET orchestrator_worker_id = ?2 WHERE run_id = ?1",
                        [&run.run_id, &orchestrator],
                    )?;
                    runs::get(tx, &run.run_id)
                })
                .unwrap();
            World {
                _tmp: tmp,
                fleet_root,
                ledger,
                store,
                run,
                config: FleetConfig::default(),
            }
        }

        fn jobs(&self) -> JobCtx<'_> {
            JobCtx {
                guard: None,
                config: &self.config,
                needs_you: &NeedsYouList,
            }
        }

        fn add(&mut self, title: &str) -> String {
            let run = self.run.clone();
            let mut t = NewTask::work(title, &format!("docs/{title}.md"), "opus");
            t.ticket_snapshot = Some("# A ticket\n".into());
            self.store
                .immediate(|tx| Ok(tasks::add(tx, &run, &t, Actor::Orchestrator)?.task_id))
                .unwrap()
        }

        fn bind(&mut self, task_id: &str) -> Bound {
            let config = FleetConfig::default();
            let git = Git::new("git");
            let ctx = DispatchCtx {
                jobs: JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                },
                clock: FleetSeconds(0),
                git: &git,
            };
            let run_id = self.run.run_id.clone();
            self.store
                .immediate(|tx| {
                    let worker = NewWorker {
                        run_id: run_id.clone(),
                        ..Default::default()
                    };
                    bind(tx, &ctx, task_id, "opus", &worker)
                })
                .unwrap()
        }

        fn add_tree(&self, b: &Bound) -> Result<String> {
            let tree = b.worktree.as_ref().expect("a tree");
            let mut conn = Connection::open(&self.ledger).unwrap();
            conn.busy_timeout(std::time::Duration::from_secs(30))
                .unwrap();
            create::finish_reserved(&mut conn, &self.jobs(), &TestJobs, &tree.worktree_id)
        }

        fn plan(&mut self, alive: &dyn Fn(Recorded) -> bool) -> Plan {
            self.store.immediate(|tx| plan(tx, None, alive)).unwrap()
        }

        fn launch(&self, attempt: &str, ports: &Fake) -> Result<Launch> {
            launch(
                self.store.conn(),
                &self.fleet_root,
                FleetSeconds(5),
                attempt,
                ports,
                ports,
            )
        }

        fn attempt_state(&self, attempt: &str) -> (String, Option<String>) {
            self.store
                .conn()
                .query_row(
                    "SELECT state, outcome FROM attempts WHERE id = ?1",
                    [attempt],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
        }

        fn task_status(&self, task_id: &str) -> String {
            tasks::get(self.store.conn(), task_id).unwrap().status
        }
    }

    #[derive(Default)]
    struct Fake {
        alive: RefCell<BTreeSet<u32>>,
        specs: RefCell<Vec<PushLaunch>>,
        launches: Cell<usize>,
    }

    impl Fake {
        fn with_alive(pid: u32) -> Fake {
            let f = Fake::default();
            f.alive.borrow_mut().insert(pid);
            f
        }

        fn alive_fn(&self) -> impl Fn(Recorded) -> bool + '_ {
            |p| self.alive.borrow().contains(&p.pid)
        }
    }

    impl Launches for Fake {
        fn alive(&self, p: Recorded) -> bool {
            self.alive.borrow().contains(&p.pid)
        }

        fn spec(&self, l: &PushLaunch) -> Result<LaunchSpec> {
            self.specs.borrow_mut().push(l.clone());
            Ok(LaunchSpec {
                program: "true".into(),
                args: vec!["spawn".into(), "--attempt".into(), l.attempt.clone().into()],
                cwd: None,
                env_rule: EnvRule::default(),
                detach: false,
                output: None,
            })
        }

        fn resuming_harness(&self, _: &str) -> Option<String> {
            Some("claude".into())
        }
    }

    impl ProcessHost for Fake {
        fn launch(&self, spec: &LaunchSpec) -> Result<Launched, LaunchError> {
            self.launches.set(self.launches.get() + 1);
            Launcher.launch(spec)
        }
    }

    fn reap(l: Launch) {
        match l {
            Launch::Launched(mut l) => {
                l.child.wait().unwrap();
            }
            other => panic!("not launched: {other:?}"),
        }
    }

    fn bound_with_tree(w: &mut World, title: &str) -> Bound {
        let task = w.add(title);
        let b = w.bind(&task);
        w.add_tree(&b).unwrap();
        b
    }

    #[test]
    fn launching_attempt_is_launched_once_after_the_commit() {
        let mut w = World::new();
        let b = bound_with_tree(&mut w, "once");
        let f = Fake::default();
        let plan = w.plan(&f.alive_fn());
        assert_eq!(plan.launches, std::slice::from_ref(&b.attempt_id));
        assert!(plan.stops.is_empty() && plan.crashed.is_empty());

        let refused = w.store.immediate(|tx| {
            launch(tx, &w.fleet_root, FleetSeconds(5), &b.attempt_id, &f, &f).map(|_| ())
        });
        assert!(refused.is_err(), "a launch inside a transaction is refused");
        assert_eq!(f.launches.get(), 0);

        reap(w.launch(&b.attempt_id, &f).unwrap());
        assert_eq!(f.launches.get(), 1);
        let spec = f.specs.borrow()[0].clone();
        assert_eq!(
            spec,
            PushLaunch {
                attempt: b.attempt_id.clone(),
                task_id: b.task_id.clone(),
                worker_id: b.worker_id.clone(),
                persona: "opus".into(),
                run_slug: "r1".into(),
                project_root: w.run.project_root.clone(),
                from_pane: Some(ORCHESTRATOR_PANE.into()),
                workspace_id: Some("w0".into()),
            }
        );
        let record = effects::read_launch(&w.fleet_root, &b.attempt_id)
            .unwrap()
            .expect("the launch record");
        assert_eq!(record.launched_at, FleetSeconds(5));
        assert_eq!(record.process, None, "the spawn is not the agent");

        assert!(matches!(w.launch(&b.attempt_id, &f).unwrap(), Launch::Done));
        assert_eq!(f.launches.get(), 1);
    }

    #[test]
    fn crash_after_commit_before_launch_launches_once_on_the_next_pass() {
        let mut w = World::new();
        let task = w.add("fault");
        let f = Fake::default();

        let config = FleetConfig::default();
        let git = Git::new("git");
        let ctx = DispatchCtx {
            jobs: JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            },
            clock: FleetSeconds(0),
            git: &git,
        };
        let run_id = w.run.run_id.clone();
        let rolled_back: Result<()> = w.store.immediate(|tx| {
            let worker = NewWorker {
                run_id: run_id.clone(),
                ..Default::default()
            };
            bind(tx, &ctx, &task, "opus", &worker)?;
            bail!("fleetd dies before the commit")
        });
        assert!(rolled_back.is_err());
        assert_eq!(w.task_status(&task), "ready");
        assert_eq!(w.plan(&f.alive_fn()), Plan::default());
        assert_eq!(f.launches.get(), 0);

        let b = w.bind(&task);
        w.add_tree(&b).unwrap();
        assert!(effects::read_launch(&w.fleet_root, &b.attempt_id)
            .unwrap()
            .is_none());
        let next = w.plan(&f.alive_fn());
        assert_eq!(next.launches, std::slice::from_ref(&b.attempt_id));
        reap(w.launch(&b.attempt_id, &f).unwrap());
        assert!(matches!(w.launch(&b.attempt_id, &f).unwrap(), Launch::Done));
        assert_eq!(f.launches.get(), 1, "exactly 1 launch of the attempt");
    }

    #[test]
    fn crash_after_launch_does_not_launch_again() {
        let mut w = World::new();
        let b = bound_with_tree(&mut w, "after");
        let f = Fake::default();
        reap(w.launch(&b.attempt_id, &f).unwrap());

        let again = Fake::default();
        let _ = w.plan(&again.alive_fn());
        assert!(matches!(
            w.launch(&b.attempt_id, &again).unwrap(),
            Launch::Done
        ));
        assert_eq!(again.launches.get(), 0);
        assert_eq!(w.attempt_state(&b.attempt_id).0, "launching");
    }

    #[test]
    fn reserved_tree_waits_for_its_worktree_add_job() {
        let mut w = World::new();
        let task = w.add("tree");
        let b = w.bind(&task);
        let tree = b.worktree.clone().unwrap();
        assert!(tree.add_job.is_some(), "a new tree is reserved");
        let f = Fake::default();

        let plan = w.plan(&f.alive_fn());
        assert!(plan.launches.is_empty(), "{plan:?}");
        assert!(plan.crashed.is_empty(), "{plan:?}");
        assert!(matches!(
            w.launch(&b.attempt_id, &f).unwrap(),
            Launch::Waiting(_)
        ));
        assert_eq!(f.launches.get(), 0);
        assert!(effects::read_launch(&w.fleet_root, &b.attempt_id)
            .unwrap()
            .is_none());

        w.add_tree(&b).unwrap();
        let row = trees::get(w.store.conn(), &tree.worktree_id)
            .unwrap()
            .unwrap();
        assert_eq!(row.state, WorktreeState::Active);
        assert_eq!(
            w.plan(&f.alive_fn()).launches,
            std::slice::from_ref(&b.attempt_id)
        );
        reap(w.launch(&b.attempt_id, &f).unwrap());
        assert_eq!(f.launches.get(), 1);
    }

    #[test]
    fn failed_worktree_add_ends_the_attempt_crashed() {
        let mut w = World::new();
        let task = w.add("broken");
        let b = w.bind(&task);
        let tree = b.worktree.clone().unwrap();
        std::fs::create_dir_all(&tree.path).unwrap();
        std::fs::write(Path::new(&tree.path).join("x"), "x").unwrap();
        assert!(w.add_tree(&b).is_err(), "the worktree_add job fails");

        let f = Fake::default();
        let plan = w.plan(&f.alive_fn());
        assert_eq!(plan.crashed, std::slice::from_ref(&b.attempt_id));
        assert!(plan.launches.is_empty());
        let (state, outcome) = w.attempt_state(&b.attempt_id);
        assert_eq!(
            (state.as_str(), outcome.as_deref()),
            ("ended", Some("crashed"))
        );
        assert_eq!(w.task_status(&task), "ready", "rung 6 queues it again");
        let worker = workers::get(w.store.conn(), &b.worker_id).unwrap();
        assert_eq!(worker.status, "launch_failed");
        assert!(matches!(w.launch(&b.attempt_id, &f).unwrap(), Launch::Done));
        assert_eq!(f.launches.get(), 0);
        assert_eq!(w.plan(&f.alive_fn()), Plan::default());
    }

    #[test]
    fn handoff_with_a_live_old_process_launches_only_after_it_is_gone() {
        let mut w = World::new();
        let task = w.add("handoff");
        let run_id = w.run.run_id.clone();
        let handoff = w.fleet_root.join("handoff.md");
        std::fs::write(&handoff, "next step\n").unwrap();
        let old_worker = w
            .store
            .immediate(|tx| {
                let new = NewWorker {
                    run_id: run_id.clone(),
                    persona: "opus".into(),
                    ..Default::default()
                };
                let id =
                    crate::taskledger::testing::bind_live(tx, &task, &new, Some(OLD))?.worker_id;
                tx.execute(
                    "UPDATE workers SET supervisor_pid = 4241, supervisor_started = 76
                     WHERE worker_id = ?1",
                    [&id],
                )?;
                tasks::handoff(tx, &id, handoff.to_str().unwrap())?;
                Ok(id)
            })
            .unwrap();
        let old = crate::fleetd::task_execution::domain::task::attempt_id(&task, 1);
        w.store
            .immediate(|tx| {
                let reap = Reap {
                    attempt: old.clone(),
                    worker: old_worker.clone(),
                    outcome: "handed_off".into(),
                    end_worker: false,
                    task: None,
                    cleanup: Default::default(),
                };
                commands::end_attempt(tx, &reap).map(|_| ())
            })
            .unwrap();
        let b = w.bind(&task);
        w.add_tree(&b).unwrap();

        let f = Fake::with_alive(OLD.pid);
        let plan = w.plan(&f.alive_fn());
        assert!(plan.launches.is_empty(), "{plan:?}");
        assert_eq!(
            plan.stops,
            [Stop {
                row: RowRef::Attempt(old.clone()),
                target: Target::Process {
                    pid: OLD.pid,
                    pid_start: 77,
                    scope: ProcessScope::LeaderGroup,
                },
            }]
        );
        assert!(matches!(
            w.launch(&b.attempt_id, &f).unwrap(),
            Launch::Waiting(_)
        ));
        assert_eq!(f.launches.get(), 0);

        f.alive.borrow_mut().clear();
        let plan = w.plan(&f.alive_fn());
        assert_eq!(plan.launches, std::slice::from_ref(&b.attempt_id));
        assert!(plan.stops.is_empty());
        reap(w.launch(&b.attempt_id, &f).unwrap());
        assert_eq!(f.launches.get(), 1);
    }

    #[test]
    fn decide_puts_a_failed_tree_first_then_stops_then_the_tree() {
        assert!(matches!(
            decide(TreeFact::AddFailed, &[OLD]),
            Decision::Crash(_)
        ));
        assert_eq!(decide(TreeFact::Adding, &[OLD]), Decision::Stop(vec![OLD]));
        assert_eq!(decide(TreeFact::Ready, &[OLD]), Decision::Stop(vec![OLD]));
        assert_eq!(decide(TreeFact::Adding, &[]), Decision::WaitTree);
        assert_eq!(decide(TreeFact::Ready, &[]), Decision::Launch);
    }

    #[test]
    fn session_id_form_refuses_an_option_and_odd_bytes() {
        assert!(session_id_ok("5bd39a10-8a9f-43b8-a26f-3e68992c7d58"));
        assert!(session_id_ok("thread_1"));
        for bad in ["", "-c", "--resume", "a b", "a;b", "a/b", "a\nb", "é"] {
            assert!(!session_id_ok(bad), "{bad:?}");
        }
        assert!(session_id_ok(&"a".repeat(SESSION_ID_MAX_BYTES)));
        assert!(!session_id_ok(&"a".repeat(SESSION_ID_MAX_BYTES + 1)));
    }

    #[test]
    fn launch_with_no_question_records_no_resume() {
        let mut w = World::new();
        let b = bound_with_tree(&mut w, "fresh");
        let f = Fake::default();
        reap(w.launch(&b.attempt_id, &f).unwrap());
        let worker = workers::get(w.store.conn(), &b.worker_id).unwrap();
        assert_eq!(worker.resumed_from_session, None);
        assert_eq!(resume_of(w.store.conn(), &b.attempt_id, &f).unwrap(), None);
    }
}
