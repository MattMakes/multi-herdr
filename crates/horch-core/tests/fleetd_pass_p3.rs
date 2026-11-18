//! The pass of Phase 3: the worktree facts in part 1, the reconciler in
//! part 2, a `worktree_list` job in each period, and 1 git writer for each
//! repository (docs/specs/fleetd/design.md, "Supervision, liveness and
//! reconciliation", "Worktrees: leased, reconciled, never lost", "Features,
//! landing and integration"; RR-10, FM-12;
//! docs/tickets/fleetd-p3/10-pass-wiring-phase3.md).
//!
//! Each test has its own temp ledger, fleet root and repositories. The job
//! process of each job is this test binary again, with the test
//! [`child_job`]: it runs the executor of its job with the git of its world.
//! The job processes are shell scripts, so the tests run on Unix only.

#![cfg(unix)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem;
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, JobCtx, JobKey, Queued,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::application::pass::{
    NoDuties, Outside, Pass, PassReport, Ports, Settings,
};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::domain::task::Recorded;
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::{
    LaunchError, LaunchSpec, Launched, ProcessHost,
};
use horch_core::fleetd::worktree_custody::adapters::git::worktree_add_key;
use horch_core::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeState,
};
use horch_core::fleetd::worktree_custody::application::reconcile::WorkDir;
use horch_core::runtime::fault::Faults;
use horch_core::taskledger::finish::CodexSandbox;
use horch_core::taskledger::model::{Actor, WorktreeMode, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::testing::bind_live;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

/// The variable that makes [`child_job`] run: the id of its job.
const CHILD_JOB: &str = "FLEETD_P3_JOB";
/// The ledger of the job process of [`child_job`].
const CHILD_DB: &str = "FLEETD_P3_DB";

// ─── the world ──────────────────────────────────────────────────────────

/// A temp ledger at generation 1 and a fleet root. Its repositories are
/// folders of the temp folder, each with an active run.
struct World {
    _temp: tempfile::TempDir,
    /// The temp folder, as its canonical path.
    top: PathBuf,
    conn: Connection,
    config: FleetConfig,
    /// The git of each job process.
    git: PathBuf,
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let top = temp.path().canonicalize().unwrap();
    let mut conn = open(&top.join("ledger.sqlite3"));
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    std::fs::create_dir_all(top.join("fleet")).unwrap();
    World {
        _temp: temp,
        top,
        conn,
        config: FleetConfig::default(),
        git: PathBuf::from("git"),
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn write_script(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

impl World {
    fn db(&self) -> PathBuf {
        self.top.join("ledger.sqlite3")
    }

    /// The folder `name` with its work directory `.worktrees/`, and an
    /// active run in it. No git repository.
    fn project(&mut self, name: &str) -> (String, Run) {
        let path = self.top.join(name);
        std::fs::create_dir_all(path.join(".worktrees")).unwrap();
        let new_run = NewRun {
            slug: name.into(),
            base_branch: Some("main".into()),
            run_branch: Some(format!("fleet/{name}")),
            ..Default::default()
        };
        let run = runs::create(&self.conn, &path, &new_run).unwrap();
        // FR-12: a task needs an idea in flight of the feature of its run.
        horch_core::taskledger::schema::run_idea(
            &self.conn,
            &run.run_id,
            &horch_core::taskledger::util::now(),
            true,
        )
        .unwrap();
        (path.display().to_string(), run)
    }

    /// [`World::project`] as a git repository with 1 commit on `main`.
    fn repo(&mut self, name: &str) -> (String, Run) {
        let (path, run) = self.project(name);
        let dir = Path::new(&path);
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        std::fs::write(dir.join(".gitignore"), ".worktrees/\n").unwrap();
        git(dir, &["add", "a.txt", ".gitignore"]);
        git(dir, &["commit", "-q", "-m", "first"]);
        (path, run)
    }

    /// Each git call of a job process sleeps 60 s first.
    fn slow_git(&mut self) {
        let real = String::from_utf8(Command::new("which").arg("git").output().unwrap().stdout)
            .unwrap()
            .trim()
            .to_string();
        let path = self.top.join("slow-git");
        write_script(
            &path,
            &format!("#!/bin/sh\n/bin/sleep 60\nexec '{real}' \"$@\"\n"),
        );
        self.git = path;
    }

    /// The program of each job process: `<it> job run <id>` runs
    /// [`child_job`] for the job `<id>`.
    fn job_program(&self) -> PathBuf {
        let path = self.top.join("job.sh");
        let exe = std::env::current_exe().unwrap();
        let text = format!(
            "#!/bin/sh\nexport {CHILD_DB}='{}'\nexport HORCH_GIT_BIN='{}'\nexport {CHILD_JOB}=\"$3\"\n\
             exec '{}' --exact child_job --nocapture --test-threads=1\n",
            self.db().display(),
            self.git.display(),
            exe.display()
        );
        write_script(&path, &text);
        path
    }

    fn settings(&self) -> Settings {
        Settings {
            fleet_root: self.top.join("fleet"),
            generation: Generation(1),
            config: self.config.clone(),
            horch: self.job_program(),
            faults: Faults::default(),
        }
    }

    fn pass<'a>(&self, outside: &'a dyn Outside, host: &'a dyn ProcessHost) -> Pass<'a> {
        let ports = Ports {
            outside,
            duties: &NoDuties,
            needs_you: &NeedsYouList,
            host,
        };
        Pass::new(self.settings(), ports)
    }

    /// A `reserved` row `id` in `<repo>/.worktrees/<id>` and its queued
    /// `worktree_add` job, in 1 transaction. The id of the job.
    fn reserved(&mut self, repo: &str, id: &str) -> i64 {
        let path = format!("{repo}/.worktrees/{id}");
        let new = NewWorktree {
            base_commit: Some(git(Path::new(repo), &["rev-parse", "HEAD"])),
            ..self.project_row(repo, id, &path)
        };
        let config = self.config.clone();
        let tx = self.conn.transaction().unwrap();
        trees::insert_reserved(&tx, None, &new, cause()).unwrap();
        let row = trees::get(&tx, id).unwrap().unwrap();
        let key = worktree_add_key(&row);
        let job = JobKey {
            kind: Kind::WorktreeAdd,
            subject: id,
            input_key: &key,
        };
        let Queued::New(job_id) =
            jobs::queue(&ctx(&config), &tx, &job, Some(repo), FleetSeconds(0)).unwrap()
        else {
            panic!("a new run");
        };
        tx.commit().unwrap();
        job_id
    }

    /// An `orphaned` row `id` of `repo` at `path`.
    fn orphan(&mut self, repo: &str, id: &str, path: &str) {
        let new = NewWorktree {
            base_commit: Some("b0".to_string()),
            ..self.project_row(repo, id, path)
        };
        let tx = self.conn.transaction().unwrap();
        trees::insert_orphan(&tx, None, &new, cause()).unwrap();
        tx.commit().unwrap();
    }

    fn project_row(&self, repo: &str, id: &str, path: &str) -> NewWorktree {
        NewWorktree {
            worktree_id: id.to_string(),
            task_id: None,
            worker_id: None,
            repo_root: repo.to_string(),
            path: path.to_string(),
            branch: format!("fleet/{id}"),
            base: "main".to_string(),
            base_commit: None,
            status: WorktreeStatus::Open,
            mode: None,
        }
    }

    /// A new task, bound to a new worker with a live attempt whose
    /// processes do not run: (task id, worker id, attempt id).
    fn claimed(&mut self, run: &Run) -> (String, String, String) {
        let mut t = NewTask::work("t1", "t1.md", "opus");
        t.worktree_mode = WorktreeMode::Shared;
        let task = tasks::add(&self.conn, run, &t, Actor::Orchestrator).unwrap();
        let new = NewWorker {
            run_id: run.run_id.clone(),
            persona: "opus".into(),
            role: Some("opus-1".into()),
            ..Default::default()
        };
        let agent = Recorded {
            pid: 4243,
            pid_start: Some(2),
        };
        let bound = bind_live(&self.conn, &task.task_id, &new, Some(agent)).unwrap();
        let worker = bound.worker_id;
        self.conn
            .execute(
                "UPDATE workers SET supervisor_pid = 4242, supervisor_started = 1
                 WHERE worker_id = ?1",
                [&worker],
            )
            .unwrap();
        let attempt = bound.attempt_id;
        (task.task_id, worker, attempt)
    }

    fn job(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    /// The runs of the `worktree_list` job of `repo`, oldest first.
    fn lists(&self, repo: &str) -> Vec<JobRow> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id FROM jobs WHERE kind = 'worktree_list' AND subject = ?1 ORDER BY id",
            )
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([repo], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        ids.into_iter().map(|id| self.job(id)).collect()
    }

    /// Wait until the job `id` ended.
    fn ended(&self, id: i64) -> JobRow {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let row = self.job(id);
            if !matches!(row.state, State::Queued | State::Running) {
                return row;
            }
            assert!(Instant::now() < until, "job {id} ended: {row:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn set_fleet_clock(&self, s: i64) {
        self.conn
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [s])
            .unwrap();
    }
}

/// Each job process ends with its world: the group of each recorded pid
/// gets a kill.
impl Drop for World {
    fn drop(&mut self) {
        let Ok(mut stmt) = self
            .conn
            .prepare("SELECT pid FROM jobs WHERE pid IS NOT NULL")
        else {
            return;
        };
        let pids: Vec<u32> = stmt
            .query_map([], |r| r.get(0))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default();
        for pid in pids {
            let _ = horch_platform::group::kill_group_id(pid);
        }
    }
}

fn cause() -> Cause<'static> {
    Cause {
        actor: Actor::Horch,
        text: "a test",
    }
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

fn run(pass: &mut Pass<'_>, w: &mut World) -> PassReport {
    pass.run(&mut w.conn).unwrap()
}

// ─── the fakes ──────────────────────────────────────────────────────────

/// The worktree facts and the repositories as the composition root of
/// fleetd gives them: the folder of each row that is not `removed`, and the
/// entries of the work directory of each repository. No herdr.
struct Trees;

impl Outside for Trees {
    fn panes(&self) -> Option<BTreeMap<String, InPane>> {
        None
    }

    fn close_pane(&self, _pane: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn holds(&self) -> Vec<Hold> {
        Vec::new()
    }

    fn worktree_folders(&self, conn: &Connection) -> anyhow::Result<Vec<FolderItem>> {
        Ok(trees::in_states(conn, &WorktreeState::ALL)?
            .into_iter()
            .filter(|r| r.state != WorktreeState::Removed)
            .map(|r| FolderItem {
                key: r.worktree_id,
                path: PathBuf::from(r.path),
            })
            .collect())
    }

    fn work_dirs(&self, conn: &Connection) -> anyhow::Result<Vec<WorkDir>> {
        let mut out = Vec::new();
        for repo in self.repositories(conn)? {
            let Ok(entries) = std::fs::read_dir(Path::new(&repo).join(".worktrees")) else {
                continue;
            };
            let folders = entries
                .flatten()
                .map(|e| e.path().display().to_string())
                .collect();
            out.push(WorkDir { repo, folders });
        }
        Ok(out)
    }

    fn repositories(&self, conn: &Connection) -> anyhow::Result<Vec<String>> {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT p.root FROM runs r JOIN projects p ON p.project_id = r.project_id
             WHERE r.status NOT IN ('done', 'abandoned') ORDER BY p.root",
        )?;
        let roots = stmt.query_map([], |r| r.get(0))?;
        Ok(roots.collect::<rusqlite::Result<_>>()?)
    }

    fn attempt_spec(&self, attempt: &AttemptRow) -> anyhow::Result<LaunchSpec> {
        anyhow::bail!("no launch of attempt {} in these tests", attempt.id)
    }
}

/// The launcher, with the job id of each launch and, when it has a tree
/// to watch, the state of that tree in the ledger at the launch.
struct Watching {
    db: PathBuf,
    tree: Option<String>,
    launches: RefCell<Vec<(i64, Option<String>)>>,
}

impl Watching {
    fn new(w: &World, tree: Option<&str>) -> Watching {
        Watching {
            db: w.db(),
            tree: tree.map(str::to_string),
            launches: RefCell::new(Vec::new()),
        }
    }

    fn jobs(&self) -> Vec<i64> {
        self.launches.borrow().iter().map(|(id, _)| *id).collect()
    }
}

impl ProcessHost for Watching {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, LaunchError> {
        // The job process runs as `<horch> job run <id>`.
        let [job, run, id] = &spec.args[..] else {
            panic!("a job launch is `job run <id>`: {:?}", spec.args);
        };
        assert_eq!((job.to_str(), run.to_str()), (Some("job"), Some("run")));
        let id: i64 = id.to_str().unwrap().parse().unwrap();
        let state = self.tree.as_ref().map(|t| {
            let conn = open(&self.db);
            trees::get(&conn, t)
                .unwrap()
                .map_or("none".to_string(), |r| r.state.as_str().to_string())
        });
        self.launches.borrow_mut().push((id, state));
        Launcher.launch(spec)
    }
}

/// The job process: it runs the executor of the job [`CHILD_JOB`] on the
/// ledger [`CHILD_DB`], as `horch job run <id>` does. In a normal test run
/// it does nothing.
#[test]
fn child_job() {
    let Some(id) = std::env::var_os(CHILD_JOB) else {
        return;
    };
    let id: i64 = id.to_str().unwrap().parse().unwrap();
    let db = PathBuf::from(std::env::var_os(CHILD_DB).unwrap());
    let mut conn = open(&db);
    let config = FleetConfig::default();
    let profiles = CodexSandbox::default();
    let checks = CheckRunner {
        host: &Launcher,
        profiles: &profiles,
        sweep: None,
    };
    jobs::run_job(&ctx(&config), &mut conn, id, std::process::id(), &checks).unwrap();
}

// ─── the tests ──────────────────────────────────────────────────────────

/// FM-12: a `git worktree add` of 60 s runs in its job process. Each pass
/// in that time ends within `pass_time_budget_s` and writes its heartbeat,
/// and the `worktree_list` job of the repository, which only reads git,
/// starts beside it.
#[test]
fn slow_worktree_add_job_keeps_the_pass_under_its_time_budget() {
    let mut w = world();
    w.slow_git();
    let (repo, _) = w.repo("a");
    let add = w.reserved(&repo, "wt1");
    let host = Watching::new(&w, None);
    let mut pass = w.pass(&Trees, &host);
    let budget = Duration::from_secs(w.config.pass_time_budget_s);

    let until = Instant::now() + Duration::from_secs(3);
    let mut passes = 0;
    while Instant::now() < until {
        let began = Instant::now();
        let report = run(&mut pass, &mut w);
        assert!(
            began.elapsed() < budget,
            "pass {passes} took {:?}",
            began.elapsed()
        );
        assert!(report.heartbeat, "pass {passes} wrote its heartbeat");
        passes += 1;
        std::thread::sleep(Duration::from_millis(200));
    }

    assert!(passes >= 5, "{passes} passes in 3 s");
    assert_eq!(
        w.job(add).state,
        State::Running,
        "the git worktree add runs"
    );
    let lists = w.lists(&repo);
    assert_eq!(lists.len(), 1, "1 worktree_list job in the period");
    assert_eq!(
        lists[0].state,
        State::Running,
        "the list runs beside the add"
    );
    assert!(host.jobs().contains(&add) && host.jobs().contains(&lists[0].id));
}

/// The scan of the worktree folders reads at most what its budget allows,
/// from its cursor in `pass_cursors`. A new `Pass` (a restart) continues
/// after the saved cursor: 12 rows whose folders are gone, 4 for each scan,
/// are each seen within 3 passes, and none twice.
#[test]
fn worktree_scan_continues_at_its_cursor_after_a_restart() {
    let mut w = world();
    // A clock that moves 1 ms at each reading: a scan of 5 ms reads 4
    // folders.
    w.config.scan_budget_ms = 5;
    let (repo, _) = w.project("a");
    for n in 1..=12 {
        let id = format!("wt{n:02}");
        w.orphan(&repo, &id, &format!("{repo}/.worktrees/{id}"));
    }
    let vanished =
        |w: &World| w.count("SELECT count(*) FROM needs_you WHERE kind = 'worktree_vanished'");
    let clock = || {
        let start = Instant::now();
        let mut ms = 0;
        Box::new(move || {
            ms += 1;
            start + Duration::from_millis(ms)
        })
    };

    let mut seen = Vec::new();
    for _restart in 0..3 {
        let host = Watching::new(&w, None);
        let mut pass = w.pass(&Trees, &host).with_clock(clock());
        run(&mut pass, &mut w);
        seen.push(vanished(&w));
    }

    assert_eq!(seen, vec![4, 8, 12], "each pass reads the next 4 folders");
}

/// RR-10: 1 `worktree_list` job for each repository in each period of
/// `worktree_list_period_s` on the fleet clock, also after the run of the
/// period ended.
#[test]
fn worktree_list_job_is_queued_once_per_period_per_repository() {
    let mut w = world();
    let (a, _) = w.repo("a");
    let (b, _) = w.repo("b");
    let host = Watching::new(&w, None);
    let mut pass = w.pass(&Trees, &host);

    run(&mut pass, &mut w);
    let first: Vec<JobRow> = [&a, &b].iter().flat_map(|r| w.lists(r)).collect();
    assert_eq!(first.len(), 2, "1 list job for each repository");
    for job in &first {
        assert_eq!(job.input_key, "0", "the period 0");
        assert_eq!(w.ended(job.id).state, State::Passed);
    }
    run(&mut pass, &mut w);
    assert_eq!(w.lists(&a).len(), 1, "no second run in the period");
    assert_eq!(w.lists(&b).len(), 1, "no second run in the period");

    w.set_fleet_clock(i64::try_from(w.config.worktree_list_period_s).unwrap());
    run(&mut pass, &mut w);
    for repo in [&a, &b] {
        let lists = w.lists(repo);
        assert_eq!(lists.len(), 2, "1 run in the next period");
        assert_eq!(lists[1].input_key, "1");
    }
}

/// Q12: the pass starts at most 1 `worktree_add`, `worktree_remove` or
/// `rescue` job of 1 repository at a time; another repository has its
/// own writer.
#[test]
fn only_1_git_writing_job_of_1_repository_starts_in_1_pass() {
    let mut w = world();
    w.slow_git();
    let (a, _) = w.repo("a");
    let (b, _) = w.repo("b");
    let a1 = w.reserved(&a, "wt-a1");
    let a2 = w.reserved(&a, "wt-a2");
    let b1 = w.reserved(&b, "wt-b1");
    let host = Watching::new(&w, None);
    let mut pass = w.pass(&Trees, &host);

    run(&mut pass, &mut w);
    let launched = host.jobs();
    assert!(launched.contains(&a1), "the first writer of a starts");
    assert!(launched.contains(&b1), "the writer of b starts");
    assert!(!launched.contains(&a2), "the second writer of a waits");
    assert_eq!(w.job(a2).state, State::Queued);

    run(&mut pass, &mut w);
    assert!(!host.jobs().contains(&a2), "a writer of a still runs");
    assert_eq!(w.job(a1).state, State::Running);
    assert_eq!(w.job(a2).state, State::Queued);
}

/// RR-10: a tree made by hand with `git worktree add`, outside the work
/// directory, is adopted as `orphaned` from the result of the
/// `worktree_list` job of its period.
#[test]
fn pass_adopts_a_hand_made_worktree_within_1_list_period() {
    let mut w = world();
    let (repo, _) = w.repo("a");
    std::fs::create_dir_all(w.top.join("elsewhere")).unwrap();
    let hand = w.top.join("elsewhere").join("hand");
    let hand_s = hand.display().to_string();
    git(
        Path::new(&repo),
        &["worktree", "add", "-q", "-b", "hand", &hand_s],
    );
    let host = Watching::new(&w, None);
    let mut pass = w.pass(&Trees, &host);

    run(&mut pass, &mut w);
    let list = w.lists(&repo);
    assert_eq!(list.len(), 1);
    assert_eq!(w.ended(list[0].id).state, State::Passed);
    run(&mut pass, &mut w);

    let row = trees::by_path(&w.conn, &hand_s)
        .unwrap()
        .expect("a row for the hand-made tree");
    assert_eq!(row.state, WorktreeState::Orphaned);
    assert_eq!(row.repo_root, repo);
    assert!(hand.is_dir(), "the tree stays");
    assert_eq!(w.lists(&repo).len(), 1, "within 1 period");
}

/// FM-25: the attempt that owns an `active` tree has no process. The pass
/// ends the attempt and marks the tree `orphaned` in part 2, so the tree
/// is `orphaned` before the first launch of part 3.
#[test]
fn pass_marks_the_tree_of_a_dead_owner_orphaned_before_any_dispatch() {
    let mut w = world();
    let (repo, run_a) = w.repo("a");
    let (task, worker, attempt) = w.claimed(&run_a);
    let path = format!("{repo}/.worktrees/T1-feat");
    std::fs::create_dir_all(&path).unwrap();
    let new = NewWorktree {
        task_id: Some(task),
        worker_id: Some(worker),
        ..w.project_row(&repo, "wt1", &path)
    };
    let tx = w.conn.transaction().unwrap();
    trees::insert_reserved(&tx, None, &new, cause()).unwrap();
    tx.execute(
        "UPDATE worktrees SET state = 'active', owner_attempt = ?2 WHERE worktree_id = ?1",
        params!["wt1", attempt],
    )
    .unwrap();
    tx.commit().unwrap();
    let host = Watching::new(&w, Some("wt1"));
    let mut pass = w.pass(&Trees, &host);

    run(&mut pass, &mut w);

    let row = trees::get(&w.conn, "wt1").unwrap().unwrap();
    assert_eq!(row.state, WorktreeState::Orphaned, "in the same pass");
    let launches = host.launches.borrow();
    assert!(!launches.is_empty(), "the pass launched a job");
    assert_eq!(
        launches[0].1.as_deref(),
        Some("orphaned"),
        "the tree is orphaned at the first launch"
    );
}

/// The `worktree_list` job reads the dirty flag of each tree and writes
/// nothing in it: its `git status` takes no optional lock, so it never
/// rewrites the index of a tree while its worker runs git there.
#[test]
fn worktree_list_job_writes_no_index_of_a_tree() {
    let mut w = world();
    let (repo, _) = w.repo("a");
    let tree = format!("{repo}/.worktrees/T1-feat");
    git(
        Path::new(&repo),
        &["worktree", "add", "-q", "-b", "feat", &tree],
    );
    // A new mtime with the same content: a git status that may write
    // refreshes the stat data of the index.
    std::thread::sleep(Duration::from_millis(1100));
    std::fs::write(Path::new(&tree).join("a.txt"), "a\n").unwrap();
    let index = Path::new(&repo).join(".git/worktrees/T1-feat/index");
    let before = std::fs::read(&index).unwrap();
    let host = Watching::new(&w, None);
    let mut pass = w.pass(&Trees, &host);

    run(&mut pass, &mut w);
    let list = w.lists(&repo);
    assert_eq!(w.ended(list[0].id).state, State::Passed);

    assert_eq!(
        std::fs::read(&index).unwrap(),
        before,
        "the index is unchanged"
    );
}
