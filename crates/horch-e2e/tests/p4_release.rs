//! The release test of Phase 4 (`docs/specs/fleetd/design.md`, "What a
//! release does to a run in flight", row Phase 4; "Migration plan";
//! `docs/tickets/fleetd-p4/MAP.md`, Q4, Q5 and Q-C).
//!
//! A claimed attempt with a detached finish starts on Phase 3. After the
//! update, it ends by the old path, and its task lands 1 time. Each run
//! gets 1 feature row, and each task branch its `branches` row. A run of a
//! project with no `[gates]` gets the dispatch hold and 1 needs-you item,
//! and no task is lost or cancelled.
//!
//! The Phase 3 ledger is built by the real migration steps up to
//! [`schema::PHASE_3_VERSION`], then the rows of a run in flight are
//! written as a Phase 3 binary left them, in the git repository of the
//! harness. The update is the migration to [`schema::PHASE_4_VERSION`]
//! (ticket 02), then `horch fleetd` of this branch.
//!
//! The old processes are stand-ins:
//! - the agent of the claimed attempt and the detached finish are `sleep`
//!   processes. The run file of the finish names its stand-in, as the old
//!   `horch ledger finish` recorded it.
//! - the old path is the code of the old binary, in this test process: the
//!   finish steps (`finish::finish`) that the detached process runs, and
//!   `finish::complete`, the old `horch ledger done`. Each one writes the
//!   ledger directly, as the old process does.
//!
//! No test reads the load, the limits or the clock of this machine: the
//! harness gives the machine facts through `HORCH_MACHINE_FILE`, and the
//! test `fleet.toml` gives the pass period.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};

use horch_core::procid;
use horch_core::taskledger::finish::{self, detach, CodexSandbox, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::runs::root_key;
use horch_core::taskledger::{schema, util, Store};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for_or, Harness};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

/// The fleet clock of the Phase 3 `fleetd` row at the update.
const CLOCK: i64 = 5000;
/// The lease of each live attempt ends 1 day of the fleet clock after the
/// update: no lease ends in a test.
const LEASE_END: i64 = CLOCK + 86_400;

/// The `fleet.toml` of the fleet root: 1 pass each second. The stand-in
/// agent writes no heartbeat file, so no stall rung stops it in a test.
const FLEET_TOML: &str = "pass_period_s = 1\nheartbeat_max_age_s = 300\n";

/// The `fleet.toml` of a project with its 2 gates (Q-C).
const GATES: &str = "[gates]\nfast = \"true\"\nfull = \"true\"\n";

/// The run in flight, its branch and its base.
const RUN: &str = "r1";
const SLUG: &str = "zeta";
const RUN_BRANCH: &str = "fleet/zeta";
const BASE: &str = "main";

/// The claimed task, its attempt, its worker and its tree.
const TASK: &str = "T-1";
const ATTEMPT: &str = "T-1/A1";
const WORKER: &str = "w-claim";
const TREE: &str = "wt-a";
const TREE_BRANCH: &str = "fleet/zeta-T-1";

/// The task that waits for dispatch in the project with no gates.
const WAITING_TASK: &str = "T-2";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A harness with its own ledger, fleet root, data folder and git
/// repository, and no watchdog.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let root = fleet_root(&h);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("fleet.toml"), FLEET_TOML).unwrap();
    h.set("HORCH_LEDGER_DB", db(&h).to_string_lossy());
    h.set("FLEET_ROOT", root.to_string_lossy());
    h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

fn db(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

fn fleet_root(h: &Harness) -> PathBuf {
    h.root.join("fleet")
}

fn read(h: &Harness) -> Connection {
    Connection::open(db(h)).unwrap()
}

/// The evidence root of the old finish: its step logs and its run file.
fn evidence_root(h: &Harness) -> PathBuf {
    h.root.join("evidence")
}

/// A `sleep` process: an old process of the run. Killed and waited on
/// drop, so a failed assertion leaks no process.
struct Stand {
    child: Child,
    started: u64,
}

impl Stand {
    fn start() -> Stand {
        let child = Command::new("/bin/sleep").arg("600").spawn().unwrap();
        let started = procid::start_time(child.id()).expect("the start time of sleep");
        Stand { child, started }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn alive(&self) -> bool {
        procid::alive(self.pid(), Some(self.started))
    }

    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        self.stop();
    }
}

/// `git` in `dir`; it must pass. Gives the trimmed stdout.
fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// The tip of `branch` in the project.
fn tip(h: &Harness, branch: &str) -> String {
    git_in(
        h,
        &h.project,
        &["rev-parse", &format!("refs/heads/{branch}")],
    )
}

/// The commits on `branch` that are not on [`BASE`], newest first.
fn commits_past_base(h: &Harness, branch: &str) -> Vec<String> {
    let range = format!("{BASE}..{branch}");
    let out = git_in(h, &h.project, &["rev-list", &range]);
    out.lines().map(str::to_string).collect()
}

/// Commit `body` as `file` on the checked-out branch of `dir`.
fn commit(h: &Harness, dir: &Path, file: &str, body: &str) -> String {
    std::fs::write(dir.join(file), body).unwrap();
    git_in(h, dir, &["add", file]);
    git_in(h, dir, &["commit", "--quiet", "--no-gpg-sign", "-m", file]);
    git_in(h, dir, &["rev-parse", "HEAD"])
}

/// The project of `h` with `fleet_toml` committed on [`BASE`] (none: no
/// `fleet.toml`), and the run branch [`RUN_BRANCH`] at that commit.
fn project(h: &Harness, fleet_toml: Option<&str>) -> String {
    if let Some(body) = fleet_toml {
        commit(h, &h.project, "fleet.toml", body);
    }
    git_in(h, &h.project, &["branch", RUN_BRANCH, BASE]);
    tip(h, BASE)
}

/// A real tree `<project>/.worktrees/<name>` on the new branch `branch`
/// from `from`, with 1 commit of the file `<name>.txt`. Gives the path, as
/// the ledger writes it, and the commit.
fn tree_with_1_commit(h: &Harness, name: &str, branch: &str, from: &str) -> (PathBuf, String) {
    let project = std::fs::canonicalize(&h.project).unwrap();
    let path = project.join(".worktrees").join(name);
    git_in(
        h,
        &project,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            branch,
            &path.to_string_lossy(),
            from,
        ],
    );
    let head = commit(h, &path, &format!("{name}.txt"), &format!("{name}\n"));
    (path, head)
}

/// The base rows of a Phase 3 ledger: the migration steps up to
/// [`schema::PHASE_3_VERSION`], project `p` of `h`, and the `fleetd` row of
/// the Phase 3 fleetd, stopped for the update: its PID and start time name
/// no live process.
fn phase_3_base(h: &Harness) -> Connection {
    let mut conn = Connection::open(db(h)).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_3_VERSION).unwrap();
    let now = util::now();
    conn.execute(
        "INSERT INTO projects (project_id, root, created_at) VALUES ('p', ?1, ?2)",
        (root_key(&h.project), &now),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
            VALUES (1, 1, 7, 1, ?1, ?2, ?1)",
        (&now, CLOCK),
    )
    .unwrap();
    conn
}

/// Run `run_id` with `slug`, `status`, and the run branch `branch` from
/// [`BASE`] (none: a run of the shared checkout with no run branch).
fn insert_run(conn: &Connection, run_id: &str, slug: &str, status: &str, branch: Option<&str>) {
    conn.execute(
        "INSERT INTO runs (run_id, project_id, slug, status, base_branch, run_branch,
                finish_steps, started_at)
            VALUES (?1, 'p', ?2, ?3, ?4, ?5, '[\"check\",\"land\"]', ?6)",
        params![
            run_id,
            slug,
            status,
            branch.map(|_| BASE),
            branch,
            util::now()
        ],
    )
    .unwrap();
}

/// Task `task_id` of `run_id` in `status`, with the check `true`.
fn insert_task(conn: &Connection, run_id: &str, task_id: &str, seq: i64, status: &str) {
    conn.execute(
        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by,
                land_check, status, attempts, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?1, 'tickets/' || ?1 || '.md', 'opus', 'opus', 'true', ?4, 0, ?5, ?5)",
        params![task_id, run_id, seq, status, util::now()],
    )
    .unwrap();
}

/// The live claimed attempt [`ATTEMPT`] of [`TASK`] in run `run_id`.
fn insert_claimed_attempt(conn: &Connection, run_id: &str, agent: &Stand) {
    insert_claimed(conn, run_id, (TASK, ATTEMPT, WORKER), agent);
}

/// The worktree row `id` of `task_id` at `path` on `branch`, with the
/// status of today and the state of Phase 3.
struct TreeRow<'a> {
    id: &'a str,
    task_id: &'a str,
    worker_id: Option<&'a str>,
    path: &'a Path,
    branch: &'a str,
    status: &'a str,
    state: &'a str,
    owner: Option<&'a str>,
    landed_commit: Option<&'a str>,
}

fn insert_tree(h: &Harness, conn: &Connection, base: &str, t: &TreeRow<'_>) {
    conn.execute(
        "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path, branch, base,
                base_commit, status, landed_commit, created_at, state, owner_attempt)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            t.id,
            t.task_id,
            t.worker_id,
            root_key(&h.project),
            t.path.to_string_lossy(),
            t.branch,
            RUN_BRANCH,
            base,
            t.status,
            t.landed_commit,
            util::now(),
            t.state,
            t.owner,
        ],
    )
    .unwrap();
}

/// The detached finish of [`WORKER`] at [`TASK`], as the old
/// `horch ledger finish` left it: its run file names the stand-in
/// `finish`, and the worker waits on it in the background.
fn insert_detached_finish(h: &Harness, conn: &Connection, finish: &Stand) -> PathBuf {
    let run = detach::run_path(&evidence_root(h), TASK, WORKER);
    let recorded = detach::record(&run, finish.pid()).unwrap();
    assert_eq!(recorded.started, finish.started);
    let now = util::now();
    conn.execute(
        "INSERT INTO worker_waits (worker_id, kind, reason, started_at, deadline)
            VALUES (?1, ?2, 'the detached finish runs', ?3, ?3)",
        params![WORKER, detach::FINISH_WAIT, &now],
    )
    .unwrap();
    run
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

/// The update, part 1: the migration of ticket 02.
fn migrate_to_phase_4(h: &Harness) {
    let mut conn = read(h);
    assert_eq!(user_version(&conn), schema::PHASE_3_VERSION);
    schema::migrate_to(&mut conn, schema::PHASE_4_VERSION).unwrap();
}

/// The update, part 2: `horch fleetd` of this branch, and its first pass.
fn start_fleetd(h: &Harness, fleetds: &mut Fleetds) {
    fleetds.start(h, "fleetd");
    wait_passes(h, fleetds, 1);
}

/// The `at` of the heartbeat of fleetd: it changes at the end of each
/// pass.
fn heartbeat_at(h: &Harness) -> Option<String> {
    let path = fleet_root(h).join("run").join("heartbeat.json");
    let hb: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    hb["at"].as_str().map(str::to_string)
}

/// Wait for the end of `n` passes of fleetd after this call.
fn wait_passes(h: &Harness, fleetds: &Fleetds, n: usize) {
    for _ in 0..n {
        let before = heartbeat_at(h);
        wait_for_or(
            "1 more pass of fleetd",
            || {
                let now = heartbeat_at(h);
                (now.is_some() && now != before).then_some(())
            },
            || fleetds.output("fleetd", "stderr"),
        );
    }
}

/// Each row of `sql`, its columns joined by ` `.
fn rows(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(sql).unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| {
        let mut cols = Vec::with_capacity(n);
        for i in 0..n {
            let v: rusqlite::types::Value = r.get(i)?;
            cols.push(match v {
                rusqlite::types::Value::Null => "-".to_string(),
                rusqlite::types::Value::Integer(i) => i.to_string(),
                rusqlite::types::Value::Text(t) => t,
                other => format!("{other:?}"),
            });
        }
        Ok(cols.join(" "))
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

/// The rows of the run in flight, for the message of a failed assertion.
fn seen(h: &Harness, fleetds: &Fleetds) -> String {
    let conn = read(h);
    let parts = [
        (
            "tasks",
            "SELECT task_id, status FROM tasks ORDER BY task_id",
        ),
        (
            "attempts",
            "SELECT id, state, via, outcome FROM attempts ORDER BY id",
        ),
        (
            "worktrees",
            "SELECT worktree_id, status, state, landed_commit FROM worktrees ORDER BY worktree_id",
        ),
        (
            "features",
            "SELECT id, name, branch, primary_branch, state FROM features ORDER BY id",
        ),
        (
            "branches",
            "SELECT name, feature_id, task_id, landing_state, owner FROM branches ORDER BY name",
        ),
        (
            "bundles",
            "SELECT id, feature_id, state FROM bundles ORDER BY id",
        ),
        (
            "jobs",
            "SELECT id, kind, subject, state,
                CASE WHEN state = 'failed' THEN substr(result_json, 1, 300) END
             FROM jobs ORDER BY id",
        ),
        (
            "needs_you",
            "SELECT key, kind, state FROM needs_you ORDER BY key",
        ),
    ];
    let mut out = String::new();
    for (name, sql) in parts {
        out.push_str(&format!("{name}:\n"));
        for row in rows(&conn, sql) {
            out.push_str(&format!("  {row}\n"));
        }
    }
    out.push_str("fleetd stderr:\n");
    out.push_str(&fleetds.output("fleetd", "stderr"));
    out
}

fn scalar(conn: &Connection, sql: &str, key: &str) -> Option<String> {
    conn.query_row(sql, [key], |r| r.get(0)).optional().unwrap()
}

/// The landing state of the task branch `branch`.
fn landing_state(conn: &Connection, branch: &str) -> Option<String> {
    scalar(
        conn,
        "SELECT landing_state FROM branches WHERE name = ?1",
        branch,
    )
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// The tools of the old finish: the git of the harness, `/bin/sh` for the
/// check, and the evidence root of [`evidence_root`]. No pr step and no
/// cleanup step runs, so no `gh`, no analyser and no custody job.
fn old_tools(h: &Harness) -> Tools {
    Tools {
        git: Git::new(h.git_bin().expect("git").to_path_buf()),
        gh: h.root.join("no-gh"),
        node: "/bin/sh".into(),
        complexity_cli: None,
        code_sweep_dir: None,
        evidence_root: evidence_root(h),
        shell: "/bin/sh".into(),
        reviewers_down: Vec::new(),
        codex: CodexSandbox::default(),
        jobs: None,
    }
}

/// The old path of the detached finish, in this process: the finish steps
/// of [`WORKER`] as the old binary runs them, then the old
/// `horch ledger done`. Each step must pass.
fn old_finish_and_done(h: &Harness, fleetds: &Fleetds) {
    let mut store = Store::open(&db(h)).unwrap();
    let tools = old_tools(h);
    let report = finish::finish(&mut store, &tools, WORKER).unwrap();
    assert!(
        report.complete,
        "the old finish steps pass: {report:?}\n{}",
        seen(h, fleetds)
    );
    finish::complete(&mut store, WORKER, "landed by the old finish", None).unwrap();
}

/// The Phase 3 run of [`claimed_attempt_with_detached_finish_lands_once_after_the_update`]
/// and of the crash test: run [`RUN`] of a project with its gates, and
/// [`TASK`] in its claimed attempt with 1 commit in its tree and a detached
/// finish that runs.
struct Flight {
    // fleetd stops first: the teardown of the harness fails on a live
    // process.
    fleetds: Fleetds,
    h: Harness,
    agent: Stand,
    finish: Stand,
    run_file: PathBuf,
    base: String,
    head: String,
}

impl Flight {
    fn on_phase_3(name: &str) -> Flight {
        let h = world(name);
        let base = project(&h, Some(GATES));
        let (path, head) = tree_with_1_commit(&h, TREE, TREE_BRANCH, RUN_BRANCH);
        let agent = Stand::start();
        let finish = Stand::start();
        let conn = phase_3_base(&h);
        insert_run(&conn, RUN, SLUG, "active", Some(RUN_BRANCH));
        insert_task(&conn, RUN, TASK, 1, "ready");
        insert_claimed_attempt(&conn, RUN, &agent);
        insert_tree(
            &h,
            &conn,
            &base,
            &TreeRow {
                id: TREE,
                task_id: TASK,
                worker_id: Some(WORKER),
                path: &path,
                branch: TREE_BRANCH,
                status: "open",
                state: "active",
                owner: Some(ATTEMPT),
                landed_commit: None,
            },
        );
        let run_file = insert_detached_finish(&h, &conn, &finish);
        drop(conn);
        let fleetds = Fleetds::new(&h);
        Flight {
            h,
            agent,
            finish,
            run_file,
            base,
            head,
            fleetds,
        }
    }

    /// The update: the migration of ticket 02, then fleetd of this branch.
    fn update(&mut self) {
        migrate_to_phase_4(&self.h);
        let conn = read(&self.h);
        let via = scalar(&conn, "SELECT via FROM attempts WHERE id = ?1", ATTEMPT);
        assert_eq!(
            via.as_deref(),
            Some("claim"),
            "the attempt began with a claim (Q4)"
        );
        assert_eq!(
            landing_state(&conn, TREE_BRANCH).as_deref(),
            Some("building"),
            "the backfill gives the task branch its row"
        );
        drop(conn);
        start_fleetd(&self.h, &mut self.fleetds);
    }

    fn seen(&self) -> String {
        seen(&self.h, &self.fleetds)
    }

    /// While the detached finish runs, fleetd starts no landing of
    /// [`TASK`]: its branch is `building`, no bundle exists, the run branch
    /// does not move, and the task keeps its 1 attempt.
    fn assert_no_landing_while_the_finish_runs(&self) {
        wait_passes(&self.h, &self.fleetds, 3);
        let conn = read(&self.h);
        let seen = self.seen();
        assert!(self.finish.alive(), "the detached finish runs\n{seen}");
        assert!(self.agent.alive(), "the agent goes on\n{seen}");
        assert_eq!(
            landing_state(&conn, TREE_BRANCH).as_deref(),
            Some("building"),
            "{seen}"
        );
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM bundles"), 0, "{seen}");
        assert_eq!(tip(&self.h, RUN_BRANCH), self.base, "{seen}");
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM attempts WHERE task_id = 'T-1'"),
            1,
            "{seen}"
        );
        let attempt = scalar(&conn, "SELECT state FROM attempts WHERE id = ?1", ATTEMPT);
        assert_eq!(attempt.as_deref(), Some("live"), "{seen}");
        let task = scalar(&conn, "SELECT status FROM tasks WHERE task_id = ?1", TASK);
        assert_eq!(task.as_deref(), Some("running"), "{seen}");
    }

    /// The detached finish ends by the old path: its steps land the task,
    /// its run file goes, and its process ends. Then the worker runs the
    /// old `horch ledger done`.
    fn finish_by_the_old_path(&mut self) {
        old_finish_and_done(&self.h, &self.fleetds);
        detach::clear(&self.run_file);
        self.finish.stop();
    }

    /// After the old path, [`TASK`] is landed 1 time: the run branch holds
    /// the 1 commit of the task, its task branch is `landed` through 1
    /// bundle with no candidate commit, and 5 more passes change nothing.
    fn assert_landed_once(&self) {
        wait_for_or(
            "the task branch of T-1 to be landed",
            || {
                let conn = read(&self.h);
                (landing_state(&conn, TREE_BRANCH).as_deref() == Some("landed")).then_some(())
            },
            || self.seen(),
        );
        wait_passes(&self.h, &self.fleetds, 5);
        let conn = read(&self.h);
        let seen = self.seen();
        let task = scalar(&conn, "SELECT status FROM tasks WHERE task_id = ?1", TASK);
        assert_eq!(task.as_deref(), Some("done"), "{seen}");
        assert_eq!(
            landing_state(&conn, TREE_BRANCH).as_deref(),
            Some("landed"),
            "{seen}"
        );
        // T-871: a branch lands only through a bundle. Its work is already
        // on the run branch, so the bundle lands it with no candidate commit.
        let bundles = format!(
            "SELECT COUNT(*) FROM bundles b JOIN bundle_members m ON m.bundle_id = b.id
             WHERE m.branch = '{TREE_BRANCH}' AND b.state = 'landed'"
        );
        assert_eq!(count(&conn, &bundles), 1, "1 landed bundle\n{seen}");
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM attempts WHERE task_id = 'T-1'"),
            1,
            "the task has 1 attempt\n{seen}"
        );
        let landed = scalar(
            &conn,
            "SELECT landed_commit FROM worktrees WHERE worktree_id = ?1",
            TREE,
        )
        .unwrap_or_else(|| panic!("the tree has a landed commit\n{seen}"));
        assert_eq!(tip(&self.h, RUN_BRANCH), landed, "{seen}");
        let past = commits_past_base(&self.h, RUN_BRANCH);
        assert_eq!(past.len(), 1, "the task lands 1 time: {past:?}\n{seen}");
        let file = git_in(
            &self.h,
            &self.h.project,
            &["show", &format!("{RUN_BRANCH}:{TREE}.txt")],
        );
        assert_eq!(file, TREE, "{seen}");
        assert_eq!(
            git_in(
                &self.h,
                &self.h.project,
                &["rev-parse", &format!("{landed}^{{tree}}")]
            ),
            git_in(
                &self.h,
                &self.h.project,
                &["rev-parse", &format!("{}^{{tree}}", self.head)]
            ),
            "the landed commit holds the work of the task\n{seen}"
        );
    }
}

#[test]
fn claimed_attempt_with_detached_finish_lands_once_after_the_update() {
    let mut f = Flight::on_phase_3("p4-release-land");
    f.update();
    f.assert_no_landing_while_the_finish_runs();
    f.finish_by_the_old_path();
    f.assert_landed_once();
}

/// The fault points of the old land, after the update. Each one leaves the
/// rows and the refs as a detached finish that dies there leaves them, and
/// the stand-in of the finish ends with no result:
/// - `BeforeLandingRow`: before the commit of the `landing` row of the tree.
/// - `AfterLandingRow`: after that commit, and before the fast-forward of
///   the run branch.
/// - `AfterFastForward`: after the fast-forward, and before the commit of
///   the `landed` row.
#[derive(Clone, Copy, Debug)]
enum Fault {
    BeforeLandingRow,
    AfterLandingRow,
    AfterFastForward,
}

impl Fault {
    /// Leave the rows and refs of a finish that died at this point.
    fn leave(self, f: &Flight) {
        let conn = read(&f.h);
        if !matches!(self, Fault::BeforeLandingRow) {
            conn.execute(
                "UPDATE worktrees SET status = 'landing', state = 'landing'
                    WHERE worktree_id = ?1",
                [TREE],
            )
            .unwrap();
        }
        if matches!(self, Fault::AfterFastForward) {
            git_in(
                &f.h,
                &f.h.project,
                &[
                    "update-ref",
                    &format!("refs/heads/{RUN_BRANCH}"),
                    &f.head,
                    &f.base,
                ],
            );
        }
    }
}

/// The crash test: a detached finish dies at each fault point of its land
/// after the update. fleetd starts no landing of the task, and the next
/// finish by the old path completes the land 1 time. No tip and no work is
/// lost.
#[test]
fn old_finish_that_dies_around_its_land_lands_once_after_the_update() {
    for fault in [
        Fault::BeforeLandingRow,
        Fault::AfterLandingRow,
        Fault::AfterFastForward,
    ] {
        let mut f = Flight::on_phase_3(&format!("p4-release-{fault:?}"));
        f.update();
        fault.leave(&f);
        f.finish.stop();
        assert!(
            matches!(detach::state(&f.run_file), detach::Detached::Lost(_)),
            "{fault:?}: the dead finish left no result"
        );
        wait_passes(&f.h, &f.fleetds, 3);
        let conn = read(&f.h);
        let seen = f.seen();
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM bundles"),
            0,
            "{fault:?}\n{seen}"
        );
        let expected = match fault {
            Fault::AfterFastForward => &f.head,
            _ => &f.base,
        };
        assert_eq!(&tip(&f.h, RUN_BRANCH), expected, "{fault:?}\n{seen}");
        assert_eq!(
            tip(&f.h, TREE_BRANCH),
            f.head,
            "{fault:?}: the work stays\n{seen}"
        );
        drop(conn);
        // The next finish by the old path.
        f.finish_by_the_old_path();
        f.assert_landed_once();
    }
}

/// The rows of the Phase 3 ledger of [`each_run_gets_1_feature_row_and_each_task_branch_a_row`]:
/// - `r1`, active on `fleet/zeta`: `T-1` done with its tree landed, `T-2`
///   in its claimed attempt with an open tree, `T-3` with an old abandoned
///   tree and a newer open tree.
/// - `r2`, done on `fleet/eta`: `T-4` done with its tree landed.
/// - `r3`, abandoned on `fleet/theta`: `T-5` with an abandoned tree.
/// - `r4`, active with no run branch: `T-6` in the shared checkout.
#[test]
fn each_run_gets_1_feature_row_and_each_task_branch_a_row() {
    let h = world("p4-release-rows");
    let base = project(&h, Some(GATES));
    for branch in ["fleet/eta", "fleet/theta"] {
        git_in(&h, &h.project, &["branch", branch, BASE]);
    }
    let agent = Stand::start();
    // Each tree: (id, task, branch, run branch, status, state, owner). A
    // landed tree's commit is on its run branch, as the old land left it.
    let trees = [
        (
            "wt-1",
            "T-1",
            "fleet/zeta-T-1",
            "fleet/zeta",
            "landed",
            "landed",
            None,
        ),
        (
            "wt-2",
            TASK_2,
            "fleet/zeta-T-2",
            "fleet/zeta",
            "open",
            "active",
            Some(ATTEMPT_2),
        ),
        (
            "wt-3-old",
            "T-3",
            "fleet/zeta-T-3-old",
            "fleet/zeta",
            "abandoned",
            "orphaned",
            None,
        ),
        (
            "wt-3",
            "T-3",
            "fleet/zeta-T-3",
            "fleet/zeta",
            "open",
            "orphaned",
            None,
        ),
        (
            "wt-4",
            "T-4",
            "fleet/eta-T-4",
            "fleet/eta",
            "landed",
            "landed",
            None,
        ),
        (
            "wt-5",
            "T-5",
            "fleet/theta-T-5",
            "fleet/theta",
            "abandoned",
            "orphaned",
            None,
        ),
    ];
    let mut heads = Vec::new();
    for (id, _, branch, run_branch, status, _, _) in trees {
        let (path, head) = tree_with_1_commit(&h, id, branch, run_branch);
        if status == "landed" {
            git_in(&h, &h.project, &["branch", "-f", run_branch, &head]);
        }
        heads.push((path, head));
    }
    let conn = phase_3_base(&h);
    insert_run(&conn, "r1", "zeta", "active", Some("fleet/zeta"));
    insert_run(&conn, "r2", "eta", "done", Some("fleet/eta"));
    insert_run(&conn, "r3", "theta", "abandoned", Some("fleet/theta"));
    insert_run(&conn, "r4", "iota", "active", None);
    insert_task(&conn, "r1", "T-1", 1, "done");
    insert_task(&conn, "r1", TASK_2, 2, "ready");
    insert_task(&conn, "r1", "T-3", 3, "ready");
    insert_task(&conn, "r2", "T-4", 1, "done");
    insert_task(&conn, "r3", "T-5", 1, "cancelled");
    insert_task(&conn, "r4", "T-6", 1, "ready");
    insert_claimed(&conn, "r1", (TASK_2, ATTEMPT_2, WORKER_2), &agent);
    for (i, (id, task, branch, _, status, state, owner)) in trees.into_iter().enumerate() {
        let (path, head) = &heads[i];
        insert_tree(
            &h,
            &conn,
            &base,
            &TreeRow {
                id,
                task_id: task,
                worker_id: (task == TASK_2).then_some(WORKER_2),
                path,
                branch,
                status,
                state,
                owner,
                landed_commit: (status == "landed").then_some(head.as_str()),
            },
        );
    }
    // The older tree of T-3 is older by its time.
    conn.execute(
        "UPDATE worktrees SET created_at = '2020-01-01T00:00:00Z' WHERE worktree_id = 'wt-3-old'",
        [],
    )
    .unwrap();
    drop(conn);

    // The backfill of Phase 4: 1 feature row for each run with a run
    // branch, and none for r4 (Q5). The feature id is `F-<run rowid>`.
    migrate_to_phase_4(&h);
    // The ideas of docs sync get features of their own, after F-0004.
    let features_sql = "SELECT id, name, branch, primary_branch, state FROM features
        WHERE id IN ('F-0001', 'F-0002', 'F-0003', 'F-0004') ORDER BY id";
    let backfilled = rows(&read(&h), features_sql);
    assert_eq!(
        backfilled,
        vec![
            "F-0001 zeta fleet/zeta main open",
            "F-0002 eta fleet/eta main merged",
            "F-0003 theta fleet/theta main abandoned",
        ],
    );
    let mut fleetds = Fleetds::new(&h);
    start_fleetd(&h, &mut fleetds);
    wait_passes(&h, &fleetds, 3);

    // After the update each run has 1 feature row: the release of Phase 6
    // gives r4 its feature with no branch, and the rows of Phase 4 stay.
    let conn = read(&h);
    let seen = seen(&h, &fleetds);
    assert_eq!(
        rows(&conn, features_sql),
        vec![
            "F-0001 zeta fleet/zeta main open",
            "F-0002 eta fleet/eta main merged",
            "F-0003 theta fleet/theta main abandoned",
            "F-0004 iota - main open",
        ],
        "1 feature row for each run\n{seen}"
    );
    let feature_branches = rows(
        &conn,
        "SELECT name, feature_id, owner FROM branches WHERE task_id IS NULL
            AND feature_id IN ('F-0001', 'F-0002', 'F-0003') ORDER BY name",
    );
    assert_eq!(
        feature_branches,
        vec![
            "fleet/eta F-0002 fleetd",
            "fleet/theta F-0003 fleetd",
            "fleet/zeta F-0001 fleetd",
        ],
        "each feature branch has its row\n{seen}"
    );
    let task_branches = rows(
        &conn,
        "SELECT name, feature_id, task_id FROM branches WHERE task_id IS NOT NULL ORDER BY name",
    );
    assert_eq!(
        task_branches,
        vec![
            "fleet/eta-T-4 F-0002 T-4",
            "fleet/theta-T-5 F-0003 T-5",
            "fleet/zeta-T-1 F-0001 T-1",
            "fleet/zeta-T-2 F-0001 T-2",
            "fleet/zeta-T-3 F-0001 T-3",
        ],
        "each task branch has 1 row: the branch of the newest tree of its task\n{seen}"
    );
    for (branch, state) in [
        ("fleet/zeta-T-1", "landed"),
        ("fleet/eta-T-4", "landed"),
        ("fleet/zeta-T-2", "building"),
    ] {
        assert_eq!(
            landing_state(&conn, branch).as_deref(),
            Some(state),
            "{branch}\n{seen}"
        );
    }
    let shared = count(&conn, "SELECT COUNT(*) FROM branches WHERE task_id = 'T-6'");
    assert_eq!(
        shared, 0,
        "a run with no run branch keeps the old path (Q5)\n{seen}"
    );
    // No tip moved, and no work is lost: each tree that did not land keeps
    // its commit, and each run branch holds the commit that landed on it.
    for (i, (_, _, branch, _, status, _, _)) in trees.into_iter().enumerate() {
        if status != "landed" {
            assert_eq!(tip(&h, branch), heads[i].1, "{branch}\n{seen}");
        }
    }
    assert_eq!(tip(&h, "fleet/zeta"), heads[0].1, "{seen}");
    assert_eq!(tip(&h, "fleet/eta"), heads[4].1, "{seen}");
    assert_eq!(tip(&h, "fleet/theta"), base, "{seen}");
    assert!(agent.alive(), "the agent of T-2 goes on\n{seen}");
}

/// The claimed task of [`each_run_gets_1_feature_row_and_each_task_branch_a_row`].
const TASK_2: &str = "T-2";
const ATTEMPT_2: &str = "T-2/A1";
const WORKER_2: &str = "w-claim-2";

/// The live claimed attempt `attempt` of `task` in run `run_id`: its
/// worker `worker` with the agent `agent`, its lease and its task in
/// `running`, as the claim of a Phase 3 binary left them.
fn insert_claimed(
    conn: &Connection,
    run_id: &str,
    (task, attempt, worker): (&str, &str, &str),
    agent: &Stand,
) {
    let now = util::now();
    conn.execute(
        "UPDATE tasks SET status = 'running', attempts = 1, current_worker_id = ?2
            WHERE task_id = ?1",
        (task, worker),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO workers (worker_id, run_id, kind, persona, role, host, agent_pid,
                agent_started, last_checkin_at, task_id, attempt, status, spawned_at, started_at,
                claimed_at)
            VALUES (?1, ?2, 'worker', 'opus', ?1, ?3, ?4, ?5, ?6, ?7, 1, 'working', ?6, ?6, ?6)",
        params![
            worker,
            run_id,
            util::host_name(),
            agent.pid(),
            agent.started,
            &now,
            task
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at, active_at)
            VALUES (?1, ?2, ?3, ?4, ?5, 'live', ?6, ?7)",
        params![
            attempt,
            task,
            worker,
            agent.pid(),
            agent.started,
            &now,
            CLOCK
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO leases (task_id, attempt_id, expires_at, renewed_at, renew_basis)
            VALUES (?1, ?2, ?3, ?4, 'claim')",
        params![task, attempt, LEASE_END, &now],
    )
    .unwrap();
}

/// Q-C: a run that started before the update in a project with no
/// `[gates]` gets the dispatch hold and 1 needs-you item. Its task in
/// flight goes on, its waiting task gets no attempt, and no task is lost or
/// cancelled.
#[test]
fn run_without_gates_holds_dispatch_and_loses_no_task() {
    let h = world("p4-release-no-gates");
    let base = project(&h, None);
    let (path, head) = tree_with_1_commit(&h, TREE, TREE_BRANCH, RUN_BRANCH);
    let agent = Stand::start();
    let conn = phase_3_base(&h);
    insert_run(&conn, RUN, SLUG, "active", Some(RUN_BRANCH));
    insert_task(&conn, RUN, TASK, 1, "ready");
    insert_task(&conn, RUN, WAITING_TASK, 2, "ready");
    insert_claimed_attempt(&conn, RUN, &agent);
    insert_tree(
        &h,
        &conn,
        &base,
        &TreeRow {
            id: TREE,
            task_id: TASK,
            worker_id: Some(WORKER),
            path: &path,
            branch: TREE_BRANCH,
            status: "open",
            state: "active",
            owner: Some(ATTEMPT),
            landed_commit: None,
        },
    );
    drop(conn);

    migrate_to_phase_4(&h);
    let mut fleetds = Fleetds::new(&h);
    start_fleetd(&h, &mut fleetds);
    let key = "missing_fast_gate:p";
    wait_for_or(
        "the needs-you item of the missing fast gate",
        || {
            let conn = read(&h);
            (scalar(&conn, "SELECT state FROM needs_you WHERE key = ?1", key).as_deref()
                == Some("open"))
            .then_some(())
        },
        || seen(&h, &fleetds),
    );
    wait_passes(&h, &fleetds, 3);

    let conn = read(&h);
    let seen = seen(&h, &fleetds);
    let items = rows(
        &conn,
        "SELECT key, state FROM needs_you WHERE kind = 'missing_fast_gate'",
    );
    assert_eq!(
        items,
        vec![format!("{key} open")],
        "1 needs-you item\n{seen}"
    );
    let tasks = rows(&conn, "SELECT task_id, status FROM tasks ORDER BY task_id");
    assert_eq!(
        tasks,
        vec![format!("{TASK} running"), format!("{WAITING_TASK} ready")],
        "no task is lost or cancelled\n{seen}"
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM attempts WHERE task_id = 'T-2'"),
        0,
        "the hold holds the dispatch of the waiting task\n{seen}"
    );
    let attempt = scalar(&conn, "SELECT state FROM attempts WHERE id = ?1", ATTEMPT);
    assert_eq!(
        attempt.as_deref(),
        Some("live"),
        "the task in flight goes on\n{seen}"
    );
    assert!(agent.alive(), "the agent goes on\n{seen}");
    let o = h.horch(&["ledger", "status"]).output().unwrap();
    assert!(
        text(&o).contains("missing fast gate"),
        "horch ledger status names the hold\n{}\n{seen}",
        text(&o)
    );
    assert_eq!(tip(&h, TREE_BRANCH), head, "no work is lost\n{seen}");
    assert_eq!(tip(&h, RUN_BRANCH), base, "{seen}");
}
