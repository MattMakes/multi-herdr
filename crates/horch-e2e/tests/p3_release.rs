//! The release test of Phase 3 (`docs/specs/fleetd/design.md`, "What a
//! release does to a run in flight", row Phase 3;
//! `docs/tickets/fleetd-p3/MAP.md`, Q9 and Q19).
//!
//! A live tree and a dirty orphan tree start on Phase 2. After the update,
//! the live tree is `active`, the orphan is listed, and nothing is removed.
//! The acting reconciler (ticket 18) rescues the orphan: the test waits for
//! that rescue job, and its ref holds the 3 changes of the orphan.
//!
//! The Phase 2 ledger is built by the real migration steps up to
//! [`schema::PHASE_2_VERSION`], then the rows of a run in flight are
//! written as a Phase 2 binary left them, in the git repository of the
//! harness. The live agent is a stand-in: a `sleep` child of the test. The
//! old supervisor is a stand-in too: the test writes
//! `workers.last_checkin_at` each 30 s (`runs.heartbeat_every_s`).
//!
//! The fleet clock counts the awake time of 1 fleetd process, so a
//! `horch ledger watch --once` process adds 0 to it. The test moves the
//! fleet clock by the same step as `HORCH_NOW`.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};

use horch_core::taskledger::runs::root_key;
use horch_core::taskledger::schema;
use horch_core::{clock, procid};
use horch_e2e::harness::{wait_for, Harness};
use rusqlite::{Connection, OptionalExtension};

/// The fleet clock of the Phase 2 `fleetd` row at the update.
const CLOCK: i64 = 5000;
/// The default `liveness.lease_s` of the design.
const LEASE_S: i64 = 120;
/// The checkin period of the old supervisor (`runs.heartbeat_every_s`).
const STEP_S: i64 = 30;
/// The default `worktree_list_period_s`.
const LIST_PERIOD_S: i64 = 60;
/// The passes after the update: 3 periods of the `worktree_list` job.
const PASSES: i64 = 3 * LIST_PERIOD_S / STEP_S;
/// The wall time of the first checkin of the old supervisor.
const T0: i64 = 1_893_456_000; // 2030-01-01T00:00:00Z

/// The live task, its attempt, its worker and its tree.
const TASK_A: &str = "T-1";
const ATTEMPT_A: &str = "T-1/A1";
const WORKER_A: &str = "w-live";
const TREE_A: &str = "wt-a";
/// The task whose worker is dead, its ended attempt, its worker and its
/// dirty tree.
const TASK_B: &str = "T-2";
const ATTEMPT_B: &str = "T-2/A1";
const WORKER_B: &str = "w-dead";
const TREE_B: &str = "wt-b";

/// The 3 changes of the orphan: the content of the modified file
/// `notes.txt` of the harness, and the untracked file and its content.
const MODIFIED: &str = "one\ntwo\nthree, not committed\n";
const UNTRACKED: &str = "draft.txt";
const UNTRACKED_TEXT: &str = "a draft, never added\n";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// `secs` after the Unix epoch as a ledger timestamp, the form of
/// `HORCH_NOW`.
fn rfc3339(secs: i64) -> String {
    clock::stamp(clock::from_epoch_ms(secs * 1000).unwrap())
}

/// A harness with its own ledger file and a git repository, and no
/// watchdog.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set("HORCH_LEDGER_DB", db(&h).to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

fn db(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

/// The stand-in agent: a `sleep` process, killed and waited on drop, so a
/// failed assertion leaks no process.
struct Agent {
    child: Child,
    started: u64,
}

impl Agent {
    fn start() -> Agent {
        let child = Command::new("/bin/sleep").arg("600").spawn().unwrap();
        let started = procid::start_time(child.id()).expect("the start time of sleep");
        Agent { child, started }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn alive(&self) -> bool {
        procid::alive(self.pid(), Some(self.started))
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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

/// A real tree `<project>/.worktrees/<name>` on a new branch, with 1
/// commit of the file `<name>.txt`. Gives the path, as the ledger writes
/// it, and the commit.
fn tree_with_1_commit(h: &Harness, name: &str) -> (PathBuf, String) {
    let project = std::fs::canonicalize(&h.project).unwrap();
    let path = project.join(".worktrees").join(name);
    let branch = format!("zeta-{name}");
    git_in(
        h,
        &project,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            &branch,
            &path.to_string_lossy(),
            "main",
        ],
    );
    let file = format!("{name}.txt");
    std::fs::write(path.join(&file), format!("{name}\n")).unwrap();
    git_in(h, &path, &["add", &file]);
    git_in(
        h,
        &path,
        &["commit", "--quiet", "--no-gpg-sign", "-m", name],
    );
    let head = git_in(h, &path, &["rev-parse", "HEAD"]);
    (path, head)
}

/// The Phase 2 ledger of `h`: run `r1` of its project.
/// - [`TASK_A`] is `running` with the live attempt [`ATTEMPT_A`] and its
///   lease. Its worker [`WORKER_A`] has the agent `agent`, and its tree
///   [`TREE_A`] at `tree_a` is `open`.
/// - [`TASK_B`] is held. Its attempt [`ATTEMPT_B`] ended, its worker
///   [`WORKER_B`] is dead, and its tree [`TREE_B`] at `tree_b` is `open`.
/// - The `fleetd` row of the Phase 2 fleetd has the fleet clock at
///   [`CLOCK`].
fn phase_2_ledger(h: &Harness, agent: &Agent, (tree_a, tree_b): (&Path, &Path)) {
    let project = &h.project;
    let base = git_in(h, project, &["rev-parse", "main"]);
    let mut conn = Connection::open(db(h)).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_2_VERSION).unwrap();
    let t0 = rfc3339(T0);
    let host = horch_core::taskledger::util::host_name();
    conn.execute(
        "INSERT INTO projects (project_id, root, created_at) VALUES ('p', ?1, ?2)",
        (root_key(project), &t0),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO runs (run_id, project_id, slug, status, started_at)
            VALUES ('r1', 'p', 'zeta', 'active', ?1)",
        [&t0],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by,
                status, held_reason, attempts, current_worker_id, created_at, updated_at)
            VALUES (?1, 'r1', 1, 'T1', 'tickets/t1.md', 'opus', 'opus', 'running', NULL, 1, ?2, ?5, ?5),
                   (?3, 'r1', 2, 'T2', 'tickets/t2.md', 'opus', 'opus', 'escalated', 'its worker died', 1, ?4, ?5, ?5)",
        (TASK_A, WORKER_A, TASK_B, WORKER_B, &t0),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO workers (worker_id, run_id, kind, persona, host, agent_pid, agent_started,
                last_checkin_at, task_id, attempt, status, spawned_at, started_at, claimed_at)
            VALUES (?1, 'r1', 'worker', 'opus', ?2, ?3, ?4, ?5, ?6, 1, 'working', ?5, ?5, ?5),
                   (?7, 'r1', 'worker', 'opus', ?2, NULL, NULL, ?5, ?8, 1, 'stopped', ?5, ?5, ?5)",
        (
            WORKER_A,
            &host,
            agent.pid(),
            agent.started,
            &t0,
            TASK_A,
            WORKER_B,
            TASK_B,
        ),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at, ended_at,
                outcome, active_at)
            VALUES (?1, ?2, ?3, ?4, ?5, 'live', ?6, NULL, NULL, ?7),
                   (?8, ?9, ?10, NULL, NULL, 'ended', ?6, ?6, 'crashed', NULL)",
        rusqlite::params![
            ATTEMPT_A,
            TASK_A,
            WORKER_A,
            agent.pid(),
            agent.started,
            &t0,
            CLOCK,
            ATTEMPT_B,
            TASK_B,
            WORKER_B,
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO leases (task_id, attempt_id, expires_at, renewed_at, renew_basis)
            VALUES (?1, ?2, ?3, ?4, 'claim')",
        (TASK_A, ATTEMPT_A, CLOCK + LEASE_S, &t0),
    )
    .unwrap();
    for (id, task, worker, tree) in [
        (TREE_A, TASK_A, WORKER_A, tree_a),
        (TREE_B, TASK_B, WORKER_B, tree_b),
    ] {
        conn.execute(
            "INSERT INTO worktrees (worktree_id, task_id, worker_id, repo_root, path, branch, base,
                    base_commit, status, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, 'zeta-' || ?6, 'main', ?7, 'open', ?8)",
            rusqlite::params![
                id,
                task,
                worker,
                root_key(project),
                tree.to_string_lossy(),
                tree.file_name().unwrap().to_string_lossy(),
                base,
                &t0,
            ],
        )
        .unwrap();
    }
    // The Phase 2 fleetd stopped for the update: its PID and start time
    // name no live process.
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
            VALUES (1, 1, 7, 1, ?1, ?2, ?1)",
        (&t0, CLOCK),
    )
    .unwrap();
    assert_eq!(user_version(&conn), schema::PHASE_2_VERSION);
}

fn user_version(conn: &Connection) -> i64 {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

/// The checkin of the old supervisor at the wall time `at`.
fn checkin(h: &Harness, at: i64) {
    let conn = Connection::open(db(h)).unwrap();
    conn.execute(
        "UPDATE workers SET last_checkin_at = ?2 WHERE worker_id = ?1",
        (WORKER_A, rfc3339(at)),
    )
    .unwrap();
}

/// Run `horch` at the wall time `at`.
fn horch_at(h: &Harness, at: i64, args: &[&str]) -> Output {
    run_at(h.horch(args), at, args)
}

/// Run `horch` at the wall time `at` as the worker [`WORKER_A`].
fn worker_at(h: &Harness, at: i64, args: &[&str]) -> Output {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", WORKER_A);
    run_at(cmd, at, args)
}

fn run_at(mut cmd: Command, at: i64, args: &[&str]) -> Output {
    cmd.env("HORCH_NOW", rfc3339(at));
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    o
}

/// `(state, owner_attempt, rescue_ref)` of the tree `id`.
fn tree_row(conn: &Connection, id: &str) -> (String, Option<String>, Option<String>) {
    conn.query_row(
        "SELECT state, owner_attempt, rescue_ref FROM worktrees WHERE worktree_id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

fn scalar_text(conn: &Connection, sql: &str, key: &str) -> String {
    conn.query_row(sql, [key], |r| r.get(0)).unwrap()
}

/// Each file and each commit of both trees is in place: A at `head_a`, B
/// at `head_b` with its modified and its untracked file.
fn assert_trees_kept(
    h: &Harness,
    (a, head_a): (&Path, &str),
    (b, head_b): (&Path, &str),
    seen: &str,
) {
    assert!(a.join("wt-a.txt").is_file(), "A's file\n{seen}");
    assert_eq!(
        git_in(h, a, &["rev-parse", "HEAD"]),
        head_a,
        "A's commit\n{seen}"
    );
    assert!(b.join("wt-b.txt").is_file(), "B's committed file\n{seen}");
    assert_eq!(
        git_in(h, b, &["rev-parse", "HEAD"]),
        head_b,
        "B's commit\n{seen}"
    );
    assert_eq!(
        std::fs::read_to_string(b.join("notes.txt")).unwrap(),
        MODIFIED,
        "B's modified file\n{seen}"
    );
    assert_eq!(
        std::fs::read_to_string(b.join(UNTRACKED)).unwrap(),
        UNTRACKED_TEXT,
        "B's untracked file\n{seen}"
    );
    let status = git_in(h, b, &["status", "--porcelain"]);
    let lines: Vec<&str> = status.lines().map(str::trim).collect();
    assert!(lines.contains(&"M notes.txt"), "{status}");
    assert!(
        lines.contains(&format!("?? {UNTRACKED}").as_str()),
        "{status}"
    );
}

/// The rescue ref `r` of B holds the 3 changes: its parent is B's commit,
/// and its files hold the modified and the untracked file.
fn assert_rescue_holds_3_changes(h: &Harness, r: &str, head_b: &str) {
    assert!(r.starts_with("refs/horch/rescue/"), "{r}");
    let project = &h.project;
    assert_eq!(git_in(h, project, &["rev-parse", &format!("{r}^")]), head_b);
    let show = |file: &str| git_in(h, project, &["show", &format!("{r}:{file}")]);
    assert_eq!(show("notes.txt"), MODIFIED.trim_end());
    assert_eq!(show(UNTRACKED), UNTRACKED_TEXT.trim_end());
    assert_eq!(show("wt-b.txt"), "wt-b");
}

#[test]
fn live_tree_and_dirty_orphan_on_phase_2_are_active_and_listed_and_nothing_is_removed_after_the_update(
) {
    let h = world("p3-release");
    let agent = Agent::start();
    let (tree_a, head_a) = tree_with_1_commit(&h, "wt-a");
    let (tree_b, head_b) = tree_with_1_commit(&h, "wt-b");
    std::fs::write(tree_b.join("notes.txt"), MODIFIED).unwrap();
    std::fs::write(tree_b.join(UNTRACKED), UNTRACKED_TEXT).unwrap();
    phase_2_ledger(&h, &agent, (&tree_a, &tree_b));

    // The update: the binary of this branch opens the ledger, and the
    // migration runs.
    let mut now = T0 + STEP_S;
    checkin(&h, now);
    let o = horch_at(&h, now, &["ledger", "show", TASK_A, "--json"]);
    let conn = Connection::open(db(&h)).unwrap();
    assert_eq!(user_version(&conn), schema::VERSION, "{}", text(&o));
    let (state, owner, _) = tree_row(&conn, TREE_A);
    assert_eq!(state, "active", "the backfill");
    assert_eq!(owner.as_deref(), Some(ATTEMPT_A), "the backfill");
    let (state, owner, _) = tree_row(&conn, TREE_B);
    assert_eq!(state, "orphaned", "the backfill");
    assert_eq!(owner, None, "the backfill");
    drop(conn);

    // 3 periods of the worktree_list job: the old supervisor checks in,
    // the agent records progress, and fleetd runs 1 pass, each 30 s.
    let mut seen = Vec::new();
    for _ in 0..PASSES {
        now += STEP_S;
        h.advance_fleet_clock(STEP_S);
        checkin(&h, now);
        worker_at(&h, now, &["ledger", "progress", "building"]);
        let o = horch_at(&h, now, &["ledger", "watch", "--once", "--force", "--json"]);
        seen.push(text(&o));
    }
    // The last pass reads the end of the rescue job of B.
    let rescued = wait_for("the end of the rescue job of B", || {
        let conn = Connection::open(db(&h)).unwrap();
        let state: Option<String> = conn
            .query_row(
                "SELECT state FROM jobs WHERE kind = 'rescue' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        state.filter(|s| s != "queued" && s != "running")
    });
    now += STEP_S;
    h.advance_fleet_clock(STEP_S);
    checkin(&h, now);
    worker_at(&h, now, &["ledger", "progress", "building"]);
    let o = horch_at(&h, now, &["ledger", "watch", "--once", "--force", "--json"]);
    seen.push(format!("rescue job: {rescued}\n{}", text(&o)));
    let listed = text(&horch_at(&h, now, &["worktrees"]));
    seen.push(listed.clone());
    let seen = seen.join("\n");

    let conn = Connection::open(db(&h)).unwrap();
    let worker = scalar_text(
        &conn,
        "SELECT status FROM workers WHERE worker_id = ?1",
        WORKER_A,
    );
    assert_eq!(worker, "working", "A's worker is not reaped\n{seen}");
    let attempt = scalar_text(&conn, "SELECT state FROM attempts WHERE id = ?1", ATTEMPT_A);
    assert_eq!(attempt, "live", "{seen}");
    assert!(agent.alive(), "A's agent goes on\n{seen}");
    let (state, owner, _) = tree_row(&conn, TREE_A);
    assert_eq!(state, "active", "{seen}");
    assert_eq!(owner.as_deref(), Some(ATTEMPT_A), "{seen}");

    let b_line = listed
        .lines()
        .find(|l| l.ends_with(&format!("path={}", tree_b.display())))
        .unwrap_or_else(|| panic!("B's tree is listed\n{seen}"));
    assert!(
        b_line.contains("task=") && !b_line.contains("state=removed"),
        "{b_line}"
    );
    let (_, _, rescue_ref) = tree_row(&conn, TREE_B);
    let r = rescue_ref.unwrap_or_else(|| panic!("B has a rescue ref\n{seen}"));
    assert_rescue_holds_3_changes(&h, &r, &head_b);

    let removed: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM worktrees WHERE state = 'removed'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(removed, 0, "no row is removed\n{seen}");
    assert_trees_kept(&h, (&tree_a, &head_a), (&tree_b, &head_b), &seen);
}
