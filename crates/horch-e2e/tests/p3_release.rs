#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};

use horch_core::taskledger::runs::root_key;
use horch_core::taskledger::schema;
use horch_core::taskledger::Store;
use horch_core::{clock, procid};
use horch_e2e::harness::{wait_for_or, Harness};
use rusqlite::Connection;

const CLOCK: i64 = 5000;
const LEASE_S: i64 = 120;
const STEP_S: i64 = 30;
const LIST_PERIOD_S: i64 = 60;
const PASSES: i64 = 3 * LIST_PERIOD_S / STEP_S;
const T0: i64 = 1_893_456_000;

const RUN: &str = "zeta";

const TASK_A: &str = "T-1";
const ATTEMPT_A: &str = "T-1/A1";
const WORKER_A: &str = "w-live";
const TREE_A: &str = "wt-a";
const TASK_B: &str = "T-2";
const ATTEMPT_B: &str = "T-2/A1";
const WORKER_B: &str = "w-dead";
const TREE_B: &str = "wt-b";

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

fn rfc3339(secs: i64) -> String {
    clock::stamp(clock::from_epoch_ms(secs * 1000).unwrap())
}

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

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

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

struct Wrapper {
    child: Child,
    stdin: std::process::ChildStdin,
    replies: std::io::BufReader<std::process::ChildStdout>,
    dir: PathBuf,
    runs: u32,
}

impl Wrapper {
    fn start(h: &Harness) -> Wrapper {
        let mut child = Command::new("/bin/sh")
            .args([
                "-c",
                r#"while read -r s; do /bin/sh "$s" >"$s.out" 2>"$s.err"; echo $?; done"#,
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let replies = std::io::BufReader::new(child.stdout.take().unwrap());
        Wrapper {
            child,
            stdin,
            replies,
            dir: h.root.clone(),
            runs: 0,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn run(&mut self, cmd: &Command) -> Output {
        use std::io::{BufRead, Write};
        use std::os::unix::process::ExitStatusExt;
        self.runs += 1;
        let script = self.dir.join(format!("worker-{}.sh", self.runs));
        let quote =
            |s: &std::ffi::OsStr| format!("'{}'", s.to_string_lossy().replace('\'', r"'\''"));
        let mut text = String::new();
        if let Some(dir) = cmd.get_current_dir() {
            text += &format!("cd {}\n", quote(dir.as_os_str()));
        }
        text += "exec /usr/bin/env -i";
        for (key, value) in cmd.get_envs() {
            if let Some(value) = value {
                text += &format!(" {}={}", quote(key), quote(value));
            }
        }
        text += &format!(" {}", quote(cmd.get_program()));
        for arg in cmd.get_args() {
            text += &format!(" {}", quote(arg));
        }
        std::fs::write(&script, text + "\n").unwrap();
        writeln!(self.stdin, "{}", script.display()).unwrap();
        let mut reply = String::new();
        self.replies.read_line(&mut reply).unwrap();
        let code: i32 = reply.trim().parse().unwrap();
        let read = |ext: &str| std::fs::read(format!("{}.{ext}", script.display())).unwrap();
        Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: read("out"),
            stderr: read("err"),
        }
    }
}

impl Drop for Wrapper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn phase_2_ledger(h: &Harness, agent: &Agent, wrapper: &Wrapper, (tree_a, tree_b): (&Path, &Path)) {
    let project = &h.project;
    let base = git_in(h, project, &["rev-parse", "main"]);
    let mut conn = Connection::open(db(h)).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_2_VERSION).unwrap();
    let wrapper = wrapper.pid();
    let t0 = rfc3339(T0);
    let host = horch_core::taskledger::util::host_name();
    conn.execute(
        "INSERT INTO projects (project_id, root, created_at) VALUES ('p', ?1, ?2)",
        (root_key(project), &t0),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO runs (run_id, project_id, slug, status, started_at)
            VALUES ('r1', 'p', ?1, 'active', ?2)",
        (RUN, &t0),
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
            wrapper,
            procid::start_time(wrapper).expect("the start time of the wrapper"),
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

fn checkin(h: &Harness, at: i64) {
    let conn = Connection::open(db(h)).unwrap();
    conn.execute(
        "UPDATE workers SET last_checkin_at = ?2 WHERE worker_id = ?1",
        (WORKER_A, rfc3339(at)),
    )
    .unwrap();
}

fn horch_at(h: &Harness, at: i64, args: &[&str]) -> Output {
    run_at(h.horch(args), at, args)
}

fn worker_at(h: &Harness, wrapper: &mut Wrapper, at: i64, args: &[&str]) -> Output {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", WORKER_A);
    cmd.env("HORCH_NOW", rfc3339(at));
    let o = wrapper.run(&cmd);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    o
}

fn run_at(mut cmd: Command, at: i64, args: &[&str]) -> Output {
    cmd.env("HORCH_NOW", rfc3339(at));
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    o
}

fn tree_row(conn: &Connection, id: &str) -> (String, Option<String>, Option<String>) {
    conn.query_row(
        "SELECT state, owner_attempt, rescue_ref FROM worktrees WHERE worktree_id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

fn jobs(h: &Harness) -> String {
    let conn = Connection::open(db(h)).unwrap();
    let mut stmt = conn
        .prepare("SELECT kind || ' ' || subject || ' ' || state FROM jobs ORDER BY id")
        .unwrap();
    let rows: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    rows.join("\n")
}

fn scalar_text(conn: &Connection, sql: &str, key: &str) -> String {
    conn.query_row(sql, [key], |r| r.get(0)).unwrap()
}

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
    let mut wrapper = Wrapper::start(&h);
    phase_2_ledger(&h, &agent, &wrapper, (&tree_a, &tree_b));

    let mut now = T0 + STEP_S;
    checkin(&h, now);
    drop(Store::open(&db(&h)).unwrap());
    let conn = Connection::open(db(&h)).unwrap();
    assert_eq!(user_version(&conn), schema::VERSION);
    let (state, owner, _) = tree_row(&conn, TREE_A);
    assert_eq!(state, "active", "the backfill");
    assert_eq!(owner.as_deref(), Some(ATTEMPT_A), "the backfill");
    let (state, owner, _) = tree_row(&conn, TREE_B);
    assert_eq!(state, "orphaned", "the backfill");
    assert_eq!(owner, None, "the backfill");
    drop(conn);
    h.serve_fleetd();
    h.stop_fleetd();

    let mut seen = Vec::new();
    for _ in 0..PASSES {
        now += STEP_S;
        h.advance_fleet_clock(STEP_S);
        checkin(&h, now);
        worker_at(&h, &mut wrapper, now, &["ledger", "progress", "building"]);
        let o = horch_at(
            &h,
            now,
            &[
                "ledger", "watch", "--once", "--force", "--json", "--run", RUN,
            ],
        );
        seen.push(text(&o));
    }
    now += STEP_S;
    h.advance_fleet_clock(STEP_S);
    checkin(&h, now);
    worker_at(&h, &mut wrapper, now, &["ledger", "progress", "building"]);
    let mut last = String::new();
    wait_for_or(
        "the rescue of B, queued and observed",
        || {
            last = text(&horch_at(
                &h,
                now,
                &[
                    "ledger", "watch", "--once", "--force", "--json", "--run", RUN,
                ],
            ));
            let conn = Connection::open(db(&h)).unwrap();
            tree_row(&conn, TREE_B).2
        },
        || jobs(&h),
    );
    seen.push(last);
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
