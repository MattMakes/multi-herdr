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
use horch_platform::channel::User;
use rusqlite::{params, Connection};

const CHILD_JOB: &str = "FLEETD_P3_JOB";
const CHILD_DB: &str = "FLEETD_P3_DB";

struct World {
    _temp: tempfile::TempDir,
    top: PathBuf,
    conn: Connection,
    config: FleetConfig,
    git: PathBuf,
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let top = horch_platform::path::canonicalize(temp.path()).unwrap();
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
        horch_core::taskledger::schema::run_idea(
            &self.conn,
            &run.run_id,
            &horch_core::taskledger::util::now(),
            true,
        )
        .unwrap();
        (path.display().to_string(), run)
    }

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

    fn ended(&self, id: i64) -> JobRow {
        let until = Instant::now() + Duration::from_secs(30);
        let conn = open(&self.db());
        let owner = User::current().unwrap();
        loop {
            let row = self.job(id);
            if !matches!(row.state, State::Queued | State::Running) {
                return row;
            }
            let reads = jobs::read_results(
                &conn,
                &self.top.join("fleet"),
                &owner,
                self.config.job_result_max_bytes,
            )
            .unwrap();
            conn.execute_batch("BEGIN IMMEDIATE").unwrap();
            jobs::ingest_results(&ctx(&self.config), &conn, &reads).unwrap();
            conn.execute_batch("COMMIT").unwrap();
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

#[test]
fn child_job() {
    let Some(id) = std::env::var_os(CHILD_JOB) else {
        return;
    };
    let id: i64 = id.to_str().unwrap().parse().unwrap();
    let db = PathBuf::from(std::env::var_os(CHILD_DB).unwrap());
    let conn = open(&db);
    let config = FleetConfig::default();
    let profiles = CodexSandbox::default();
    let checks = CheckRunner {
        host: &Launcher,
        profiles: &profiles,
        sweep: None,
    };
    jobs::run_job(&ctx(&config), &conn, id, std::process::id(), &checks).unwrap();
}

#[test]
fn slow_worktree_add_job_keeps_the_pass_under_its_time_budget() {
    let mut w = world();
    w.slow_git();
    let (repo, _) = w.repo("a");
    let add = w.reserved(&repo, "wt1");
    let host = Watching::new(&w, None);
    let mut pass = w.pass(&Trees, &host);
    let budget = Duration::from_secs(w.config.pass_time_budget_s);

    let until = Instant::now() + Duration::from_secs(40);
    let mut passes = 0;
    while passes < 5 {
        assert!(Instant::now() < until, "{passes} passes in 40 s");
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

    assert!(passes >= 5, "{passes} passes");
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

#[test]
fn worktree_scan_continues_at_its_cursor_after_a_restart() {
    let mut w = world();
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

#[test]
fn worktree_list_job_writes_no_index_of_a_tree() {
    let mut w = world();
    let (repo, _) = w.repo("a");
    let tree = format!("{repo}/.worktrees/T1-feat");
    git(
        Path::new(&repo),
        &["worktree", "add", "-q", "-b", "feat", &tree],
    );
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
