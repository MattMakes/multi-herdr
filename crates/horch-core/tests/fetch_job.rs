//! The `fetch` job (docs/specs/fleetd/design.md, "Between features":
//! "Remote git state is level-triggered"; "The kinds of job": `fetch`;
//! "Push and pull request results are facts on the feature row"; F11;
//! docs/tickets/fleetd-p4/37-fetch-job.md).
//!
//! Each test makes its own temporary ledger, a local bare repository as the
//! remote, and a clone of it as the repository of the project. A job
//! process is `true`, started through the launcher; the executor runs in
//! the test process with the recorded pid, as `horch job run` runs it. The
//! fleet clock of each test is a number that the test gives.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, JobCtx, JobRunner, Queued, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Outcome, State};
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::integration::adapters::exec_fetch::{self, Tools};
use horch_core::fleetd::integration::adapters::project_file;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::integration::application::remote;
use horch_core::fleetd::integration::domain::project::Remote;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::runtime::MapEnv;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::schema;
use horch_core::taskledger::workers::Worker;
use horch_core::taskledger::Store;
use rusqlite::{Connection, TransactionBehavior};

const FEATURE: &str = "F-0001";
const FEATURE_BRANCH: &str = "fleet/run/x";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    /// The bare repository that is the remote `origin`.
    remote: PathBuf,
    /// The clone: the repository of the project, as its canonical path.
    repo: PathBuf,
    /// A second clone that writes new commits to the remote.
    writer: PathBuf,
}

/// A temp ledger of the current schema with 1 project, its run and the
/// feature of the run, and a remote with `main` and the feature branch.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();

    let remote = dir.path().join("remote.git");
    git(
        dir.path(),
        &["init", "-q", "--bare", "-b", "main", path(&remote)],
    );
    let writer = dir.path().join("writer");
    git(dir.path(), &["clone", "-q", path(&remote), path(&writer)]);
    commit(&writer, "a.txt", "a\n");
    git(&writer, &["push", "-q", "origin", "HEAD:main"]);
    git(
        &writer,
        &["push", "-q", "origin", &format!("HEAD:{FEATURE_BRANCH}")],
    );
    let repo = dir.path().join("repo");
    git(dir.path(), &["clone", "-q", path(&remote), path(&repo)]);
    let repo = repo.canonicalize().unwrap();

    conn.execute(
        "INSERT INTO projects (project_id, root, created_at) VALUES ('p', ?1, 'now')",
        [path(&repo)],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO runs (run_id, project_id, slug, base_branch, run_branch, status, started_at)
         VALUES ('r1', 'p', 'x', 'main', ?1, 'active', 'now')",
        [FEATURE_BRANCH],
    )
    .unwrap();
    let new = NewFeature {
        id: FEATURE,
        name: Some("x"),
        branch: Some(FEATURE_BRANCH),
        primary_branch: "main",
    };
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    rows::insert_feature(&tx, None, &new, cause("feature accepted")).unwrap();
    tx.commit().unwrap();

    World {
        dir,
        conn,
        config: FleetConfig::default(),
        remote,
        repo,
        writer,
    }
}

impl World {
    fn repo(&self) -> &str {
        path(&self.repo)
    }

    /// The tip of `branch` on the remote.
    fn remote_tip(&self, branch: &str) -> String {
        git(
            &self.remote,
            &["rev-parse", &format!("refs/heads/{branch}")],
        )
    }

    /// A new commit on `branch` of the remote.
    fn advance(&self, branch: &str) {
        git(&self.writer, &["fetch", "-q", "origin"]);
        git(
            &self.writer,
            &["checkout", "-q", "-B", branch, &format!("origin/{branch}")],
        );
        let n = git(&self.writer, &["rev-list", "--count", "HEAD"]);
        commit(&self.writer, &format!("{n}.txt"), "n\n");
        git(
            &self.writer,
            &["push", "-q", "origin", &format!("HEAD:{branch}")],
        );
    }

    /// Queue the fetch of the repository at `clock` with the period rule.
    fn queue(&mut self, remote: &Remote, clock: i64) -> Option<Queued> {
        let config = self.config.clone();
        let ctx = ctx(&config);
        let repo = self.repo().to_string();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let queued = remote::queue_fetch(&ctx, &tx, &repo, remote, FleetSeconds(clock)).unwrap();
        tx.commit().unwrap();
        queued
    }

    /// Queue the fetch at `clock`, start its process at `clock` and run its
    /// executor to its end. Give the ended row.
    fn fetch(&mut self, clock: i64) -> JobRow {
        let id = match self.queue(&Remote::default(), clock) {
            Some(Queued::New(id)) => id,
            other => panic!("no new fetch run at {clock} s: {other:?}"),
        };
        self.run(id, clock)
    }

    fn run(&mut self, id: i64, clock: i64) -> JobRow {
        let pid = self.start(id, clock);
        self.run_job(id, pid);
        self.ingest();
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    /// Start the run `id` at `clock` through the launcher. Give its pid.
    fn start(&mut self, id: i64, clock: i64) -> u32 {
        let config = self.config.clone();
        let ctx = ctx(&config);
        let job = jobs_store::get(&self.conn, id).unwrap().unwrap();
        let spec = LaunchSpec {
            program: "true".into(),
            args: Vec::new(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        assert!(JobRunner::new(&Launcher)
            .start(&ctx, &mut self.conn, &job, &spec, FleetSeconds(clock))
            .unwrap());
        jobs_store::get(&self.conn, id)
            .unwrap()
            .unwrap()
            .pid
            .unwrap()
    }

    /// The executor of the run `id` with the pid `pid`, as `horch job run`
    /// runs it: on a read-only connection to the ledger.
    fn run_job(&self, id: i64, pid: u32) {
        let config = self.config.clone();
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
            sweep: None,
        };
        let job = self.job_store();
        jobs::run_job(&ctx(&config), job.conn(), id, pid, &checks).unwrap();
    }

    /// The executor of the fetch `row` with the programs `tools` on a
    /// read-only connection, then its facts recorded in 1 transaction, as
    /// the ingest of its result file records them. Give its outcome.
    fn fetch_with(&mut self, row: &JobRow, tools: &Tools) -> Outcome {
        let ran = exec_fetch::run_with(self.job_store().conn(), row, tools, &Launcher).unwrap();
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        exec_fetch::record(&ctx(&config), &tx, row, &ran).unwrap();
        tx.commit().unwrap();
        ran.outcome
    }

    /// The ledger as the job process opens it: read-only.
    fn job_store(&self) -> Store {
        Store::open_read_only(&self.dir.path().join("ledger.sqlite3")).unwrap()
    }

    /// End each run from its result file, as a pass of fleetd ends it.
    fn ingest(&mut self) -> Vec<(i64, State)> {
        let config = self.config.clone();
        jobs::end_from_files(&ctx(&config), &mut self.conn).unwrap()
    }

    /// The result files of the archive of the fleet root.
    fn archived(&self) -> PathBuf {
        self.dir.path().join("fleet/run/jobs/archive")
    }

    fn fetch_runs(&self) -> Vec<JobRow> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM jobs WHERE kind = 'fetch' ORDER BY id")
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        ids.into_iter()
            .map(|id| jobs_store::get(&self.conn, id).unwrap().unwrap())
            .collect()
    }

    fn tip_row(&self, branch: &str) -> rows::RemoteTipRow {
        rows::remote_tip(&self.conn, self.repo(), branch)
            .unwrap()
            .unwrap_or_else(|| panic!("no remote_tips row of {branch}"))
    }
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// No worker of a test has a sandbox profile.
struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> anyhow::Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// Run git in `dir` for the test itself, with no config of the user, and
/// give its trimmed output. A failed call fails the test.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
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
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(dir: &Path, file: &str, text: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", file]);
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn fetch_records_the_tips_in_remote_tips() {
    let mut w = world();
    w.advance("main");
    w.advance(FEATURE_BRANCH);

    let run = w.fetch(0);

    assert_eq!(run.state, State::Passed, "{:?}", run.result_json);
    for branch in ["main", FEATURE_BRANCH] {
        let row = w.tip_row(branch);
        assert_eq!(row.tip, w.remote_tip(branch), "the fetched tip of {branch}");
        assert_eq!(row.stale_since, None);
    }
    // The fetch wrote the objects: the clone has the fetched tip.
    git(&w.repo, &["cat-file", "-e", &w.remote_tip("main")]);
}

#[test]
fn two_periods_make_2_fetch_runs() {
    let mut w = world();
    let period = i64::try_from(w.config.fetch_period_s).unwrap();

    let first = w.fetch(0);
    assert_eq!(first.state, State::Passed, "{:?}", first.result_json);
    assert_eq!(
        w.queue(&Remote::default(), period - 1),
        None,
        "inside 1 period"
    );

    w.advance("main");
    let second = w.fetch(period);

    assert_eq!(second.state, State::Passed, "{:?}", second.result_json);
    let runs = w.fetch_runs();
    assert_eq!(runs.len(), 2);
    assert_eq!((runs[0].run, runs[1].run), (1, 2));
    assert_eq!(runs[0].input_key, runs[1].input_key);
    assert_eq!(w.tip_row("main").tip, w.remote_tip("main"));
}

#[test]
fn failed_fetch_sets_stale_since_and_the_next_period_runs_again() {
    let mut w = world();
    let period = i64::try_from(w.config.fetch_period_s).unwrap();
    assert_eq!(w.fetch(0).state, State::Passed);
    let seen = w.tip_row("main").tip;

    // The remote is gone: each fetch fails.
    let moved = w.dir.path().join("moved.git");
    std::fs::rename(&w.remote, &moved).unwrap();
    let failed = w.fetch(period);
    assert_eq!(failed.state, State::Failed);
    let stale = w.tip_row("main");
    assert_eq!(stale.tip, seen, "a failed fetch keeps the last fetched tip");
    let since = stale.stale_since.expect("a failed fetch sets stale_since");

    // The next period is a new run, also after a failed run, and
    // stale_since keeps the time of the first failure.
    std::thread::sleep(Duration::from_millis(1100));
    let again = w.fetch(2 * period);
    assert_eq!(again.state, State::Failed);
    assert_eq!(again.run, failed.run + 1);
    assert_eq!(
        w.tip_row("main").stale_since.as_deref(),
        Some(since.as_str())
    );

    // The remote is back: a passed fetch clears stale_since.
    std::fs::rename(&moved, &w.remote).unwrap();
    w.advance("main");
    let passed = w.fetch(3 * period);
    assert_eq!(passed.state, State::Passed, "{:?}", passed.result_json);
    let row = w.tip_row("main");
    assert_eq!(row.stale_since, None);
    assert_eq!(row.tip, w.remote_tip("main"));
}

#[test]
fn remote_none_runs_no_fetch() {
    let mut w = world();
    let settings = project_file::parse("remote = \"none\"\n").unwrap();
    assert_eq!(settings.remote, Remote::None);

    for clock in [0, 60, 120] {
        assert_eq!(w.queue(&settings.remote, clock), None);
    }

    assert!(w.fetch_runs().is_empty());
    assert_eq!(rows::remote_tip(&w.conn, w.repo(), "main").unwrap(), None);
}

#[cfg(unix)]
#[test]
fn closed_unmerged_pull_request_opens_1_item() {
    let mut w = world();
    w.conn
        .execute(
            "UPDATE features SET pr = 'https://example.invalid/pull/7' WHERE id = ?1",
            [FEATURE],
        )
        .unwrap();
    // A fake `gh` on PATH: the pull request is closed and not merged.
    let bin = w.dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\n[ \"$1 $2\" = \"pr view\" ] || exit 2\necho '{\"state\":\"CLOSED\"}'\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    let env = MapEnv::new(w.dir.path()).with("PATH", path(&bin));
    let tools = Tools::from_env(&env);
    assert_eq!(tools.gh.as_deref(), Some(gh.as_path()));

    // 2 fetch periods read the same closed pull request.
    for clock in [0, 60] {
        let id = match w.queue(&Remote::default(), clock) {
            Some(Queued::New(id)) => id,
            other => panic!("no new fetch run: {other:?}"),
        };
        let row = jobs_store::get(&w.conn, id).unwrap().unwrap();
        let outcome = w.fetch_with(&row, &tools);
        assert!(outcome.passed, "{}", outcome.text);
        w.conn
            .execute("UPDATE jobs SET state = 'passed' WHERE id = ?1", [id])
            .unwrap();
    }

    let feature = rows::feature(&w.conn, FEATURE).unwrap().unwrap();
    assert_eq!(feature.pr_state, "closed");
    let items: Vec<_> = needs_you::open_items(&w.conn)
        .unwrap()
        .into_iter()
        .filter(|i| i.entity_id == FEATURE)
        .collect();
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(items[0].text.contains("open it again"), "{}", items[0].text);
    assert!(
        items[0].text.contains("horch feature abandon"),
        "{}",
        items[0].text
    );
}

#[test]
fn missing_gh_gives_the_cause_in_the_job_result() {
    let mut w = world();
    w.conn
        .execute(
            "UPDATE features SET pr = 'https://example.invalid/pull/7' WHERE id = ?1",
            [FEATURE],
        )
        .unwrap();
    let empty = w.dir.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    let tools = Tools::from_env(&MapEnv::new(w.dir.path()).with("PATH", path(&empty)));
    assert_eq!(tools.gh, None);
    let id = match w.queue(&Remote::default(), 0) {
        Some(Queued::New(id)) => id,
        other => panic!("no new fetch run: {other:?}"),
    };
    let row = jobs_store::get(&w.conn, id).unwrap().unwrap();

    let outcome = w.fetch_with(&row, &tools);

    assert!(outcome.text.contains("gh"), "{}", outcome.text);
    assert_eq!(w.tip_row("main").tip, w.remote_tip("main"));
    let feature = rows::feature(&w.conn, FEATURE).unwrap().unwrap();
    assert_eq!(feature.pr_state, "none");
}

/// The job process of a fetch writes no row: it runs on a read-only
/// connection, and the ledger has no tip until the ingest of its result
/// file. A second ingest of the same file changes nothing.
#[test]
fn fetch_job_process_writes_no_row_and_a_second_ingest_changes_nothing() {
    let mut w = world();
    let id = match w.queue(&Remote::default(), 0) {
        Some(Queued::New(id)) => id,
        other => panic!("no new fetch run: {other:?}"),
    };
    let pid = w.start(id, 0);

    w.run_job(id, pid);

    let tips = |w: &World| -> i64 {
        w.conn
            .query_row("SELECT COUNT(*) FROM remote_tips", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(tips(&w), 0, "the job process wrote a tip");
    let row = jobs_store::get(&w.conn, id).unwrap().unwrap();
    assert_eq!(row.state, State::Running);

    assert_eq!(w.ingest(), vec![(id, State::Passed)]);
    assert_eq!(w.tip_row("main").tip, w.remote_tip("main"));
    let after = snapshot(&w.conn);

    let name = format!("{id}-{}.json", row.run);
    std::fs::copy(
        w.archived().join(&name),
        w.dir.path().join("fleet/run/jobs").join(&name),
    )
    .unwrap();
    assert_eq!(w.ingest(), Vec::new());
    assert_eq!(snapshot(&w.conn), after);
}

/// The rows that an ingest can change: each job, remote tip, event,
/// ingested file and needs-you item.
fn snapshot(conn: &Connection) -> Vec<String> {
    let mut out = Vec::new();
    for sql in [
        "SELECT id || ' ' || state || ' ' || COALESCE(result_json, '') FROM jobs",
        "SELECT branch || ' ' || tip || ' ' || COALESCE(stale_since, '') FROM remote_tips",
        "SELECT COUNT(*) FROM events",
        "SELECT COUNT(*) FROM ingested",
        "SELECT COUNT(*) FROM needs_you",
        "SELECT id || ' ' || pr_state FROM features",
    ] {
        let mut stmt = conn.prepare(sql).unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, rusqlite::types::Value>(0))
            .unwrap();
        out.extend(rows.map(|v| format!("{:?}", v.unwrap())));
    }
    out
}
