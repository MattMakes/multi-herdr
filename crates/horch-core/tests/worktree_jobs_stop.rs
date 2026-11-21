//! Each stop of a worktree job ends the sandboxed git of the job
//! (docs/tickets/fleetd-finish/208-linux-job-stop-reaches-each-sandbox.md).
//!
//! The job process is this test binary, started through the launcher as
//! part 3 of a pass starts `horch job run <id>`. Its git is a fake git that
//! writes a heartbeat into the git folder of the repository and never
//! ends. The path of the fake git holds a marker, so a search of the
//! command lines of the host finds each process of the sandbox.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::{self, ProcessFact, ScanClock};
use horch_core::fleetd::fleet_runtime::application::holds;
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, JobCtx, JobKey, JobRunner, Queued, SandboxProfiles, Stop,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::budget::{Cursor, Deadline};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::{EnvRule, LaunchSpec};
use horch_core::fleetd::worktree_custody::adapters::git::{worktree_add_key, CliWorktreeGit};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, NewWorktree};
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::schema;
use horch_core::taskledger::workers::Worker;
use rusqlite::{Connection, Transaction, TransactionBehavior};

/// The variable that makes this test binary the job process: `<ledger
/// path>|<job id>`.
const JOB_CHILD: &str = "HORCH_WORKTREE_JOBS_STOP_CHILD";

const LIMIT_S: u64 = 10;

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    repo: PathBuf,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = open(&dir.path().join("ledger.sqlite3"));
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let repo = repo.canonicalize().unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    let mut config = FleetConfig::default();
    config.job_limit_s.worktree_add = LIMIT_S;
    World {
        dir,
        conn,
        config,
        repo,
    }
}

fn open(db: &Path) -> Connection {
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    conn
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?} failed: {out:?}");
}

struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(
        &self,
        _worker: &Worker,
        _dir: &Path,
        _check: &str,
    ) -> anyhow::Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

impl World {
    fn db(&self) -> PathBuf {
        self.dir.path().join("ledger.sqlite3")
    }

    fn beat_file(&self) -> PathBuf {
        self.repo.join(".git").join("beat")
    }

    fn tx<T>(&mut self, f: impl FnOnce(&JobCtx<'_>, &Transaction<'_>) -> T) -> T {
        let config = self.config.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let out = f(&ctx, &tx);
        tx.commit().unwrap();
        out
    }

    /// A fake git whose file name holds `marker`: each call writes a
    /// count to [`World::beat_file`] each 100 ms and never ends.
    fn hanging_git(&self, marker: &str) -> PathBuf {
        let path = self.dir.path().join(marker);
        let beat = self.beat_file();
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\ni=0\nwhile :; do i=$((i+1)); echo $i > '{}'; sleep 0.1; done\n",
                beat.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.canonicalize().unwrap()
    }

    /// Queue the `worktree_add` job of a new `reserved` row.
    fn queue_add(&mut self) -> i64 {
        let new = NewWorktree {
            worktree_id: "w1".to_string(),
            task_id: None,
            worker_id: None,
            repo_root: self.repo.display().to_string(),
            path: self
                .repo
                .join(".worktrees")
                .join("w1")
                .display()
                .to_string(),
            branch: "fleet/w1".to_string(),
            base: "main".to_string(),
            // The fake git hangs at its first call, before a read of the
            // base tip.
            base_commit: Some("0".repeat(40)),
            status: WorktreeStatus::Open,
            mode: None,
        };
        let cause = Cause {
            actor: Actor::Horch,
            text: "test",
        };
        let repo = self.repo.display().to_string();
        let q = self.tx(|ctx, tx| {
            trees::insert_reserved(tx, None, &new, cause).unwrap();
            let row = trees::get(tx, "w1").unwrap().unwrap();
            let input_key = worktree_add_key(&row);
            let key = JobKey {
                kind: Kind::WorktreeAdd,
                subject: "w1",
                input_key: &input_key,
            };
            jobs::queue(ctx, tx, &key, Some(&repo), FleetSeconds(0)).unwrap()
        });
        match q {
            Queued::New(id) => id,
            other => panic!("expected a new run, got {other:?}"),
        }
    }

    fn job(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    fn facts(&self) -> Vec<ProcessFact> {
        let items = jobs::processes(&self.conn).unwrap();
        let start = Instant::now();
        let mut now = Instant::now;
        let mut clock = ScanClock {
            deadline: Deadline::pass(start, 60).scan(start, 60_000),
            now: &mut now,
            stamp: "pass",
        };
        scans::scan_processes(&items, &mut Cursor::new("pass"), &mut clock).facts
    }

    /// Reap and scan until the process of the run `id` is gone.
    fn wait_gone(&self, runner: &mut JobRunner<'_>, id: i64) -> Vec<ProcessFact> {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            runner.reap();
            let facts = self.facts();
            let key = id.to_string();
            let fact = facts.iter().find(|f| f.key == key).expect("a fact");
            if fact.liveness == scans::Liveness::Dead {
                return facts;
            }
            assert!(Instant::now() < until, "the process of job {id} ended");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// The job process: `horch job run <id>` of the ledger and the run of
/// [`JOB_CHILD`].
fn job_child(value: &str) {
    let (db, id) = value.rsplit_once('|').unwrap();
    let conn = open(Path::new(db));
    let config = FleetConfig::default();
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let checks = CheckRunner {
        host: &Launcher,
        profiles: &NoProfiles,
        sweep: None,
    };
    let ran = jobs::run_job(
        &ctx,
        &conn,
        id.parse().unwrap(),
        std::process::id(),
        &checks,
    );
    eprintln!("job child: {ran:?}");
}

/// Whether this host runs git in the sandbox.
fn has_sandbox(w: &World) -> bool {
    let gap = CliWorktreeGit::new("git", w.dir.path().join("fleet")).sandbox_gap();
    if let Some(gap) = &gap {
        eprintln!("skipped: {gap}");
    }
    gap.is_none()
}

/// Start the `worktree_add` job of `w` as this test binary `test`, with
/// the hanging git of `marker`; stop it with the TERM of a pass at the
/// clock `stop_at`, or with a KILL of the job group when `kill`; then
/// assert that the job is abandoned and that no process of its sandbox
/// lives on.
fn stop_ends_the_git_sandbox(test: &str, stop_at: i64, kill: bool) {
    if let Ok(value) = std::env::var(JOB_CHILD) {
        job_child(&value);
        return;
    }
    let mut w = world();
    if !has_sandbox(&w) {
        return;
    }
    let marker = format!("git-stop-marker-{}-{test}", std::process::id());
    let fake_git = w.hanging_git(&marker);
    let id = w.queue_add();
    let spec = LaunchSpec {
        program: std::env::current_exe().unwrap(),
        args: vec![test.into(), "--exact".into(), "--nocapture".into()],
        cwd: None,
        env_rule: EnvRule {
            vars: vec![
                (
                    JOB_CHILD.into(),
                    format!("{}|{id}", w.db().display()).into(),
                ),
                ("HORCH_GIT_BIN".into(), fake_git.into_os_string()),
            ],
        },
        detach: false,
        output: Some(w.dir.path().join("job.log")),
    };
    let job = w.job(id);
    let config = w.config.clone();
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let mut runner = JobRunner::new(&Launcher);
    assert!(runner
        .start(&ctx, &mut w.conn, &job, &spec, FleetSeconds(0))
        .unwrap());
    let limit = i64::try_from(LIMIT_S).unwrap();
    assert_eq!(w.job(id).deadline, Some(FleetSeconds(limit)));

    let beat_file = w.beat_file();
    let until = Instant::now() + Duration::from_secs(60);
    let first = loop {
        if let Some(beat) = beat(&beat_file) {
            break beat;
        }
        assert!(
            Instant::now() < until,
            "the git of the job started: job {:?}, log {:?}",
            w.job(id).state,
            std::fs::read_to_string(w.dir.path().join("job.log"))
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    while beat(&beat_file) == Some(first) {
        assert!(Instant::now() < until, "the heartbeat of git advances");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!marked(&marker).is_empty(), "the git of the job runs");

    let stop = if kill {
        let row = w.job(id);
        Stop {
            job_id: id,
            pid: row.pid.unwrap(),
            pid_start: row.pid_start,
            kill: true,
        }
    } else {
        let facts = w.facts();
        let seen = w
            .tx(|ctx, tx| jobs::observe(ctx, tx, &facts, FleetSeconds(stop_at)))
            .unwrap();
        assert_eq!(seen.stops.len(), 1);
        assert!(!seen.stops[0].kill, "the first stop is the TERM");
        seen.stops[0]
    };
    assert!(jobs::stop(&stop));
    let facts = w.wait_gone(&mut runner, id);
    let seen = w
        .tx(|ctx, tx| jobs::observe(ctx, tx, &facts, FleetSeconds(stop_at + 1)))
        .unwrap();
    assert_eq!(seen.ended, vec![(id, State::Abandoned)]);
    assert_no_process_left(&marker, &beat_file);
}

/// No process of the command line `marker` is alive, and the heartbeat in
/// `beat_file` stops.
fn assert_no_process_left(marker: &str, beat_file: &Path) {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let left = marked(marker);
        if left.is_empty() {
            break;
        }
        assert!(
            Instant::now() < until,
            "each process of the git sandbox ended: {left:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let stopped = beat(beat_file);
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(beat(beat_file), stopped, "the heartbeat of git stops");
}

#[test]
fn worktree_job_stopped_at_its_deadline_ends_its_git_sandbox() {
    let limit = i64::try_from(LIMIT_S).unwrap();
    stop_ends_the_git_sandbox(
        "worktree_job_stopped_at_its_deadline_ends_its_git_sandbox",
        limit,
        false,
    );
}

#[test]
fn worktree_job_first_seen_past_its_grace_gets_the_term_and_ends_its_git_sandbox() {
    let past_grace = i64::try_from(LIMIT_S + FleetConfig::default().stop_grace_s).unwrap();
    stop_ends_the_git_sandbox(
        "worktree_job_first_seen_past_its_grace_gets_the_term_and_ends_its_git_sandbox",
        past_grace,
        false,
    );
}

#[test]
fn kill_of_the_job_group_ends_the_git_sandbox_of_a_worktree_job() {
    let past_grace = i64::try_from(LIMIT_S + FleetConfig::default().stop_grace_s).unwrap();
    stop_ends_the_git_sandbox(
        "kill_of_the_job_group_ends_the_git_sandbox_of_a_worktree_job",
        past_grace,
        true,
    );
}

/// The leader of the group of a job ends, and a member of its group lives
/// on. With the leader gone, no recorded start time proves that the member
/// is of the job (the group id can be another's by reuse): the pass sends
/// it no signal and records a notice.
#[test]
fn abandoned_job_whose_group_has_a_member_gets_a_notice_and_no_signal() {
    let mut w = world();
    let id = w.queue_add();
    let member_file = w.dir.path().join("member");
    let spec = LaunchSpec {
        program: "sh".into(),
        args: vec![
            "-c".into(),
            format!("sleep 60 & echo $! > '{}'", member_file.display()).into(),
        ],
        cwd: None,
        env_rule: EnvRule::default(),
        detach: false,
        output: None,
    };
    let job = w.job(id);
    let config = w.config.clone();
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let mut runner = JobRunner::new(&Launcher);
    assert!(runner
        .start(&ctx, &mut w.conn, &job, &spec, FleetSeconds(0))
        .unwrap());
    let facts = w.wait_gone(&mut runner, id);
    let member: u32 = std::fs::read_to_string(&member_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let alive = |pid: u32| {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    assert!(alive(member), "the member lives on");

    let seen = w
        .tx(|ctx, tx| jobs::observe(ctx, tx, &facts, FleetSeconds(1)))
        .unwrap();
    assert_eq!(seen.ended, vec![(id, State::Abandoned)]);
    assert_eq!(seen.stops, Vec::new(), "no signal to an unproven group");
    let notices: i64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM events WHERE entity = 'job' AND entity_id = ?1
             AND kind = 'note' AND cause LIKE '%its group id has members%'",
            [id.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(notices, 1, "the pass records 1 notice");
    std::thread::sleep(Duration::from_millis(200));
    assert!(alive(member), "the member got no signal");
    let repo = job.repo.clone().expect("a worktree add has a repository");
    assert!(
        jobs::git_writer_busy(&w.conn, &repo).unwrap(),
        "no git writer of the repository starts while the member runs"
    );

    // The next run of the add waits: the hold of its task names the group,
    // the member and its start time, and the end by hand.
    // The hold reads only the task id of the tree, so the test names a task
    // that has no row.
    w.conn
        .execute_batch(
            "PRAGMA foreign_keys = OFF;
             UPDATE worktrees SET task_id = 'T-1' WHERE worktree_id = 'w1';
             PRAGMA foreign_keys = ON;",
        )
        .unwrap();
    let held = holds::task_holds(&w.conn, &|_| None).unwrap();
    let cause = held.get("T-1").expect("the task of the tree is held");
    let start = horch_platform::process::start_time(member).unwrap();
    for part in [
        format!("the group {}", w.job(id).pid.unwrap()),
        format!("pid {member} (start time {start})"),
        "end them by hand".to_string(),
    ] {
        assert!(cause.contains(&part), "the hold names {part:?}: {cause}");
    }

    // 1 needs-you item, only when the member runs past the time limit and
    // the stop grace after the notice, and only 1 time.
    let grace = i64::try_from(config.stop_grace_s).unwrap();
    let bound = 1 + i64::try_from(LIMIT_S).unwrap() + grace;
    let items = |conn: &Connection, state: &str| -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT text FROM needs_you WHERE key LIKE 'job_abandoned:group:%' AND state = ?1",
            )
            .unwrap();
        stmt.query_map([state], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    w.tx(|ctx, tx| jobs::observe(ctx, tx, &[], FleetSeconds(bound - 1)))
        .unwrap();
    assert_eq!(
        items(&w.conn, "open"),
        Vec::<String>::new(),
        "no item before the bound"
    );
    for clock in [bound, bound + 1] {
        w.tx(|ctx, tx| jobs::observe(ctx, tx, &[], FleetSeconds(clock)))
            .unwrap();
    }
    let open = items(&w.conn, "open");
    assert_eq!(open.len(), 1, "1 item at the bound: {open:?}");
    assert!(open[0].contains(&format!("pid {member}")), "{}", open[0]);
    w.conn
        .execute(
            "UPDATE needs_you SET state = 'resolved' WHERE key LIKE 'job_abandoned:group:%'",
            [],
        )
        .unwrap();
    w.tx(|ctx, tx| jobs::observe(ctx, tx, &[], FleetSeconds(bound + 2)))
        .unwrap();
    assert_eq!(
        items(&w.conn, "open"),
        Vec::<String>::new(),
        "a later pass opens no second item"
    );

    // The test started the member, so the test ends it.
    let ended = Command::new("kill")
        .args(["-KILL", &member.to_string()])
        .status()
        .unwrap();
    assert!(ended.success());
    let gone_by = std::time::Instant::now() + Duration::from_secs(10);
    while alive(member) && std::time::Instant::now() < gone_by {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(member), "the member is gone");
    assert!(
        !jobs::git_writer_busy(&w.conn, &repo).unwrap(),
        "the next git writer starts when the member is gone"
    );
}

/// A job process that ignores TERM: it gets the TERM at its deadline,
/// nothing in the grace after the recorded TERM, then the KILL. The record
/// is in the ledger, so a new connection (a restarted fleetd) counts the
/// same grace.
#[test]
fn job_stop_sends_the_term_first_and_the_kill_after_the_grace_of_the_recorded_term() {
    let mut w = world();
    let id = w.queue_add();
    let ready = w.dir.path().join("ready");
    let spec = LaunchSpec {
        program: "sh".into(),
        args: vec![
            "-c".into(),
            format!("trap '' TERM; : > '{}'; exec sleep 60", ready.display()).into(),
        ],
        cwd: None,
        env_rule: EnvRule::default(),
        detach: false,
        output: None,
    };
    let job = w.job(id);
    let config = w.config.clone();
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let mut runner = JobRunner::new(&Launcher);
    assert!(runner
        .start(&ctx, &mut w.conn, &job, &spec, FleetSeconds(0))
        .unwrap());
    let limit = i64::try_from(LIMIT_S).unwrap();
    let grace = i64::try_from(w.config.stop_grace_s).unwrap();
    let late = limit + grace + 100;
    let until = Instant::now() + Duration::from_secs(20);
    while !ready.exists() {
        assert!(Instant::now() < until, "the job process ignores TERM");
        std::thread::sleep(Duration::from_millis(20));
    }

    // The first pass sees the run long past its deadline and its grace.
    let facts = w.facts();
    let seen = w
        .tx(|ctx, tx| jobs::observe(ctx, tx, &facts, FleetSeconds(late)))
        .unwrap();
    assert_eq!(seen.stops.len(), 1);
    assert!(!seen.stops[0].kill, "the first stop is the TERM");
    assert!(jobs::stop(&seen.stops[0]));
    std::thread::sleep(Duration::from_millis(200));
    runner.reap();

    // A restarted fleetd reads the TERM from the ledger: no stop in the
    // grace, the kill at its end.
    w.conn = open(&w.db());
    let facts = w.facts();
    let key = id.to_string();
    let fact = facts.iter().find(|f| f.key == key).expect("a fact");
    assert_eq!(fact.liveness, scans::Liveness::Alive, "TERM is ignored");
    let seen = w
        .tx(|ctx, tx| jobs::observe(ctx, tx, &facts, FleetSeconds(late + grace - 1)))
        .unwrap();
    assert_eq!(seen.stops, Vec::new(), "no stop in the grace of the TERM");
    let seen = w
        .tx(|ctx, tx| jobs::observe(ctx, tx, &facts, FleetSeconds(late + grace)))
        .unwrap();
    assert_eq!(seen.stops.len(), 1);
    assert!(seen.stops[0].kill, "the kill after the grace");
    assert!(jobs::stop(&seen.stops[0]));
    let facts = w.wait_gone(&mut runner, id);
    let seen = w
        .tx(|ctx, tx| jobs::observe(ctx, tx, &facts, FleetSeconds(late + grace + 1)))
        .unwrap();
    assert_eq!(seen.ended, vec![(id, State::Abandoned)]);
}

fn beat(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn marked(marker: &str) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let line = String::from_utf8_lossy(&raw).replace('\0', " ");
        if line.contains(marker) {
            found.push(format!("{} {line}", entry.file_name().to_string_lossy()));
        }
    }
    found
}

#[cfg(not(target_os = "linux"))]
fn marked(marker: &str) -> Vec<String> {
    let out = Command::new("ps")
        .args(["-axww", "-o", "pid=,command="])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| line.contains(marker))
        .map(|line| line.trim().to_string())
        .collect()
}
