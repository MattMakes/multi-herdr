//! The gate tests of Phase 3 that the unit tests cannot show end to end
//! (`docs/specs/fleetd/design.md`, FR-11, RR-10, FM-26, FM-27, FM-28 and
//! "Migration plan" item 5; `docs/tickets/fleetd-p3/MAP.md`, Q8, Q9, Q15,
//! Q17 and Q20).
//!
//! Each test runs the real `horch` binary with its own git repository,
//! ledger and state folders in a temp dir. Most tests pass 1 fleetd pass at
//! a time with `horch ledger watch --once`, and move the fleet clock by
//! [`PASS_S`] before each pass, so a time of the design reads as fleet
//! seconds. The jobs that a pass starts run outside it; each step waits
//! until they end, so a job takes 0 fleet seconds. The slow `worktree_add`
//! test runs a fleetd of this build.
//!
//! Each test checks at its end that each modified file, each untracked file
//! and each commit that did not land is still in its tree or on a rescue
//! ref.

#![cfg(unix)]

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use horch_core::taskledger::store::Store;
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{fixtures, Harness};
use serde_json::Value;

/// `pass_period_s` of the default `fleet.toml`: 1 step of the fleet clock.
const PASS_S: i64 = 5;
/// `worktree_list_period_s` of the default `fleet.toml` (RR-10).
const LIST_PERIOD_S: i64 = 60;
/// The "Caught within" time of FM-26 and FM-27.
const CAUGHT_S: i64 = 60;
/// `pass_time_budget_s` of the default `fleet.toml`.
const PASS_TIME_BUDGET_S: u64 = 20;
/// The time of the slow `git worktree add`.
const SLOW_ADD_S: u64 = 60;
/// The size of a big file in a tree: above a disk budget of 1 GB.
const BIG: u64 = 3 << 29;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn ledger(h: &Harness) -> PathBuf {
    h.root.join("ledger.sqlite3")
}

/// A harness with its own ledger file and a git repository, and no
/// watchdog.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set("HORCH_LEDGER_DB", ledger(&h).to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

/// [`world`] in the herdr workspace `w1` (root pane `w1:p1`), for a worker
/// in its own pane and for `horch spawn`.
fn fleet(name: &str) -> Harness {
    let mut h = world(name);
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_WORKSPACE_ID", "w1");
    herdr(&h, &["workspace", "create", "--label", name]);
    h
}

fn herdr(h: &Harness, args: &[&str]) -> String {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let o = cmd.args(args).output().unwrap();
    assert!(o.status.success(), "herdr {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn ok_as(h: &Harness, worker: &str, args: &[&str]) -> String {
    let o = h
        .horch(args)
        .env("HORCH_WORKER_ID", worker)
        .output()
        .unwrap();
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git_ok(h: &Harness, dir: &Path, args: &[&str]) -> bool {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    c.output().unwrap().status.success()
}

fn tree_path(said: &str) -> PathBuf {
    PathBuf::from(
        said.lines()
            .find_map(|l| l.strip_prefix("path: "))
            .unwrap_or_else(|| panic!("no path line: {said}")),
    )
}

/// Queue the task `name` with the ticket `tickets/<name>.md`.
fn queue(h: &Harness, name: &str) {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = dir.join(format!("{name}.md"));
    std::fs::write(&ticket, format!("# {name}\n\n**Check:** none\n")).unwrap();
    let ticket = ticket.to_string_lossy().into_owned();
    ok(
        h,
        &["ledger", "add", "--file", &ticket, "--primary", "opus"],
    );
}

/// A new worker claims the next task. Gives the worker and the task id.
fn claim(h: &Harness) -> (String, String) {
    let w = ok(h, &["ledger", "worker", "new", "--persona", "opus"])
        .trim()
        .to_string();
    let c: Value = serde_json::from_str(&ok_as(h, &w, &["ledger", "claim", "--json"])).unwrap();
    let task = c["task"]["task_id"].as_str().unwrap().to_string();
    (w, task)
}

/// The stand-in agents of a test: each is killed when dropped, so a failed
/// assertion leaks none.
struct Agents(Vec<Child>);

impl Drop for Agents {
    fn drop(&mut self) {
        for a in &mut self.0 {
            let _ = a.kill();
            let _ = a.wait();
        }
    }
}

/// A worker in its own pane with a `sleep` process as its live agent
/// claims the next task. Gives the worker and the task id.
fn live_claim(h: &Harness, agents: &mut Agents) -> (String, String) {
    let split: Value = serde_json::from_str(&herdr(h, &["pane", "split", "w1:p1"])).unwrap();
    let pane = split["result"]["pane"]["pane_id"].as_str().unwrap();
    let w = ok(
        h,
        &[
            "ledger",
            "worker",
            "new",
            "--persona",
            "opus",
            "--pane",
            pane,
        ],
    )
    .trim()
    .to_string();
    let agent = std::process::Command::new("/bin/sleep")
        .arg("600")
        .spawn()
        .unwrap();
    horch_core::taskledger::Store::open(&ledger(h))
        .and_then(|mut store| {
            let pid = agent.id();
            // The agent PID row that `horch worker` wrote before Phase 5.
            store.immediate(|tx| {
                horch_core::taskledger::workers::agent_started(
                    tx,
                    &w,
                    pid,
                    horch_core::procid::start_time(pid),
                )
            })
        })
        .unwrap();
    agents.0.push(agent);
    let c: Value = serde_json::from_str(&ok_as(h, &w, &["ledger", "claim", "--json"])).unwrap();
    (w, c["task"]["task_id"].as_str().unwrap().to_string())
}

/// The values of the first column of `sql`, as text; `NULL` is "".
fn query(h: &Harness, sql: &str) -> Vec<String> {
    let store = Store::open(&ledger(h)).unwrap();
    let mut stmt = store.conn().prepare(sql).unwrap();
    let rows = stmt
        .query_map([], |r| r.get::<_, rusqlite::types::Value>(0))
        .unwrap()
        .map(|v| match v.unwrap() {
            rusqlite::types::Value::Null => String::new(),
            rusqlite::types::Value::Integer(i) => i.to_string(),
            rusqlite::types::Value::Text(t) => t,
            other => format!("{other:?}"),
        })
        .collect();
    rows
}

fn exec(h: &Harness, sql: &str) {
    Store::open(&ledger(h))
        .unwrap()
        .conn()
        .execute_batch(sql)
        .unwrap();
}

/// `path` as the ledger writes it: with each link of the project folder
/// resolved (`/tmp` is `/private/tmp` on macOS).
fn stored(h: &Harness, path: &Path) -> String {
    let project = std::fs::canonicalize(&h.project).unwrap();
    match path.strip_prefix(&h.project) {
        Ok(rel) => project.join(rel).display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

/// 1 column of the newest row of the folder `path`, or `None` with no row.
fn column(h: &Harness, path: &Path, name: &str) -> Option<String> {
    query(
        h,
        &format!(
            "SELECT {name} FROM worktrees WHERE path = '{}' ORDER BY rowid DESC LIMIT 1",
            stored(h, path)
        ),
    )
    .pop()
}

fn state(h: &Harness, path: &Path) -> Option<String> {
    column(h, path, "state")
}

/// The moves of the row `id` through the door, as `<from>><to>`.
fn moves(h: &Harness, id: &str) -> Vec<String> {
    query(
        h,
        &format!(
            "SELECT COALESCE(from_state, '') || '>' || to_state FROM events
             WHERE entity = 'worktree' AND entity_id = '{id}' AND to_state IS NOT NULL
             ORDER BY rowid"
        ),
    )
}

/// The open needs-you items of `kind`, as `<key> <entity_id>`.
fn needs_you(h: &Harness, kind: &str) -> Vec<String> {
    query(
        h,
        &format!(
            "SELECT key || ' ' || entity_id FROM needs_you
             WHERE kind = '{kind}' AND state = 'open' ORDER BY key"
        ),
    )
}

/// The worktree rows, the jobs and the open needs-you items, for a failure.
fn board(h: &Harness) -> String {
    let rows = query(
        h,
        "SELECT state || ' ' || path || ' task=' || COALESCE(task_id, '-')
         || ' bytes=' || COALESCE(bytes_on_disk, '-') || ' ref=' || COALESCE(rescue_ref, '-')
         FROM worktrees ORDER BY rowid",
    );
    let jobs = query(
        h,
        "SELECT id || ' ' || kind || ' ' || state || ' ' || subject || ' '
         || COALESCE(substr(result_json, 1, 300), '') FROM jobs ORDER BY id",
    );
    let items = query(
        h,
        "SELECT kind || ' ' || entity_id || ' ' || text FROM needs_you WHERE state = 'open'",
    );
    format!(
        "fleet clock {}\nworktrees:\n{}\njobs:\n{}\nneeds-you:\n{}",
        fleet_clock(h),
        rows.join("\n"),
        jobs.join("\n"),
        items.join("\n")
    )
}

fn fleet_clock(h: &Harness) -> i64 {
    query(
        h,
        "SELECT CAST(fleet_clock_s AS TEXT) FROM fleetd WHERE id = 1",
    )
    .pop()
    .map_or(0, |c| c.parse().unwrap())
}

/// 1 fleetd pass, then a wait until each job that it started has ended.
fn pass(h: &Harness) -> Output {
    let o = h.run(&["ledger", "watch", "--once", "--force", "--json"]);
    assert!(o.status.success(), "{}", text(&o));
    let until = Instant::now() + Duration::from_secs(120);
    while !query(h, "SELECT id FROM jobs WHERE state = 'running'").is_empty() {
        assert!(
            Instant::now() < until,
            "a job runs after 120 s\n{}",
            board(h)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    o
}

/// Move the fleet clock by 1 pass period, and run 1 pass.
fn step(h: &Harness) -> Output {
    h.advance_fleet_clock(PASS_S);
    pass(h)
}

/// Run steps until `done` holds, for at most `limit_s` fleet seconds.
/// Gives the fleet seconds that it took.
fn until(h: &Harness, what: &str, limit_s: i64, done: impl Fn(&Harness) -> bool) -> i64 {
    let start = fleet_clock(h);
    let mut last = String::new();
    loop {
        let took = fleet_clock(h) - start;
        if done(h) {
            return took;
        }
        assert!(
            took < limit_s,
            "{what}: not within {limit_s} fleet seconds\n{}\nlast pass:\n{last}",
            board(h)
        );
        last = text(&step(h));
    }
}

/// The passed `worktree_list` runs.
fn lists_passed(h: &Harness) -> usize {
    query(
        h,
        "SELECT id FROM jobs WHERE kind = 'worktree_list' AND state = 'passed'",
    )
    .len()
}

/// The work of 1 tree that must not be lost: its files with their bytes,
/// and its commits that did not land.
struct Work {
    tree: PathBuf,
    files: Vec<(String, Vec<u8>)>,
    commits: Vec<String>,
}

/// 1 commit of `<name>.txt`, then a modified tracked file and an untracked
/// file, in the tree `tree`.
fn work_in(h: &Harness, tree: &Path, name: &str) -> Work {
    let committed = format!("{name}.txt");
    std::fs::write(tree.join(&committed), format!("{name} committed\n")).unwrap();
    git_in(h, tree, &["add", &committed]);
    git_in(
        h,
        tree,
        &["commit", "--quiet", "--no-gpg-sign", "-m", &committed],
    );
    let head = git_in(h, tree, &["rev-parse", "HEAD"]);
    let modified = format!("# alpha\n{name} modified\n");
    std::fs::write(tree.join("README.md"), &modified).unwrap();
    let untracked = format!("{name}-untracked.txt");
    std::fs::write(tree.join(&untracked), format!("{name} untracked\n")).unwrap();
    Work {
        tree: tree.to_path_buf(),
        files: vec![
            (committed, format!("{name} committed\n").into_bytes()),
            ("README.md".into(), modified.into_bytes()),
            (untracked, format!("{name} untracked\n").into_bytes()),
        ],
        commits: vec![head],
    }
}

/// A tree that `git worktree add` makes by hand at
/// `<project>/.worktrees/<name>` on the new branch `branch`, with work.
fn hand_tree(h: &Harness, name: &str, branch: &str) -> Work {
    let project = h.project.clone();
    let tree = project.join(".worktrees").join(name);
    git_in(
        h,
        &project,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            branch,
            &tree.to_string_lossy(),
        ],
    );
    work_in(h, &tree, name)
}

/// Whether `rev` holds each file of `work` with its bytes, and each of its
/// commits.
fn holds(h: &Harness, dir: &Path, rev: &str, work: &Work) -> bool {
    let files = work.files.iter().all(|(file, bytes)| {
        let o = h
            .git_cmd(&["cat-file", "blob", &format!("{rev}:{file}")])
            .current_dir(dir)
            .output()
            .unwrap();
        o.status.success() && &o.stdout == bytes
    });
    files
        && work
            .commits
            .iter()
            .all(|c| git_ok(h, dir, &["merge-base", "--is-ancestor", c, rev]))
}

/// Whether the folder of `work` holds each of its files and commits.
fn in_tree(h: &Harness, work: &Work) -> bool {
    work.tree.is_dir()
        && work
            .files
            .iter()
            .all(|(file, bytes)| std::fs::read(work.tree.join(file)).ok().as_ref() == Some(bytes))
        && work
            .commits
            .iter()
            .all(|c| git_ok(h, &work.tree, &["merge-base", "--is-ancestor", c, "HEAD"]))
}

fn rescue_refs(h: &Harness) -> Vec<String> {
    let project = h.project.clone();
    let refs = git_in(
        h,
        &project,
        &["for-each-ref", "--format=%(refname)", "refs/horch/rescue/"],
    );
    refs.lines().map(str::to_string).collect()
}

/// Each work of `works` is still in its tree or on a rescue ref.
fn assert_kept(h: &Harness, works: &[&Work]) {
    let project = h.project.clone();
    let refs = rescue_refs(h);
    for work in works {
        let kept = in_tree(h, work) || refs.iter().any(|r| holds(h, &project, r, work));
        assert!(
            kept,
            "the work of {} is in no tree and on no rescue ref {refs:?}\n{}",
            work.tree.display(),
            board(h)
        );
    }
}

fn run_branch(h: &Harness, slug: &str) -> String {
    query(
        h,
        &format!("SELECT run_branch FROM runs WHERE slug = '{slug}'"),
    )
    .pop()
    .unwrap()
}

/// A file of `bytes` bytes with no data on disk (a sparse file): the size
/// of a folder counts it, and git ignores it by `.gitignore`.
fn big_file(path: &Path, bytes: u64) {
    std::fs::File::create(path).unwrap().set_len(bytes).unwrap();
}

#[test]
fn fr11_worker_own_git_worktree_add_is_flagged_on_the_next_pass() {
    let h = world("p3-fr11");
    ok(&h, &["ledger", "run", "new", "fr11"]);
    queue(&h, "t1");
    let (_, task) = claim(&h);
    pass(&h);
    let lists = lists_passed(&h);
    let branch = format!("{}-T1-own", run_branch(&h, "fr11"));
    let work = hand_tree(&h, "T1-own", &branch);

    // The first `worktree_list` run after the add, then the next pass.
    until(
        &h,
        "a worktree_list run after the add",
        LIST_PERIOD_S,
        |h| lists_passed(h) > lists,
    );
    let o = step(&h);
    let id = column(&h, &work.tree, "worktree_id")
        .unwrap_or_else(|| panic!("no row flags the tree\n{}\n{}", board(&h), text(&o)));
    assert_eq!(
        state(&h, &work.tree).as_deref(),
        Some("orphaned"),
        "{}",
        board(&h)
    );
    assert_eq!(
        column(&h, &work.tree, "task_id"),
        Some(task),
        "{}",
        board(&h)
    );
    assert_eq!(moves(&h, &id), [">orphaned"], "{}", board(&h));
    assert_kept(&h, &[&work]);
}

#[test]
fn rr10_hand_made_worktree_is_adopted_within_1_list_period() {
    let h = world("p3-rr10");
    ok(&h, &["ledger", "run", "new", "rr10"]);
    pass(&h);
    // The tree comes after the `worktree_list` run of this period: the
    // longest wait.
    let work = hand_tree(&h, "by-hand", "by-hand");
    // 1 period, and the pass that reads the run of the next period.
    let took = until(&h, "the adoption", LIST_PERIOD_S + PASS_S, |h| {
        state(h, &work.tree).is_some()
    });
    let s = state(&h, &work.tree).unwrap();
    assert!(s == "orphaned" || s == "rescued", "{s}\n{}", board(&h));
    assert_eq!(column(&h, &work.tree, "task_id"), Some(String::new()));
    assert!(took <= LIST_PERIOD_S + PASS_S, "{took}");
    assert_kept(&h, &[&work]);
}

#[test]
fn fm26_tree_made_outside_fleetd_is_rescued_and_its_files_are_on_the_ref() {
    let h = world("p3-fm26");
    ok(&h, &["ledger", "run", "new", "fm26"]);
    let work = hand_tree(&h, "script", "script-branch");
    pass(&h);
    until(&h, "the rescue", CAUGHT_S, |h| {
        state(h, &work.tree).as_deref() == Some("rescued")
    });
    let rescue = column(&h, &work.tree, "rescue_ref").unwrap();
    let project = h.project.clone();
    assert!(
        holds(&h, &project, &rescue, &work),
        "{rescue} does not hold the files and the commit\n{}",
        board(&h)
    );
    // A rescued tree with no task is never removed (Q8, Q15).
    for _ in 0..(LIST_PERIOD_S / PASS_S) {
        step(&h);
    }
    assert_eq!(state(&h, &work.tree).as_deref(), Some("rescued"));
    assert!(in_tree(&h, &work), "{}", board(&h));
    assert_kept(&h, &[&work]);
}

#[test]
fn fm27_vanished_tree_opens_1_needs_you_item_within_60_s() {
    let h = world("p3-fm27-vanished");
    ok(&h, &["ledger", "run", "new", "fm27"]);
    let work = hand_tree(&h, "gone", "gone-branch");
    pass(&h);
    until(&h, "the rescue", CAUGHT_S, |h| {
        state(h, &work.tree).as_deref() == Some("rescued")
    });
    let id = column(&h, &work.tree, "worktree_id").unwrap();

    std::fs::remove_dir_all(&work.tree).unwrap();
    let item = |h: &Harness| -> Vec<String> {
        needs_you(h, "worktree_vanished")
            .into_iter()
            .filter(|i| i.ends_with(&id))
            .collect()
    };
    until(&h, "the needs-you item", CAUGHT_S, |h| !item(h).is_empty());
    for _ in 0..(LIST_PERIOD_S / PASS_S) {
        step(&h);
    }
    assert_eq!(item(&h).len(), 1, "{}", board(&h));
    let s = state(&h, &work.tree).unwrap();
    assert_ne!(s, "removed", "the row stays\n{}", board(&h));
    assert_kept(&h, &[&work]);
}

#[test]
fn fm27_only_landed_or_rescued_trees_are_removed() {
    let h = world("p3-fm27-only");
    ok(&h, &["ledger", "run", "new", "fm27", "--steps", "land"]);
    queue(&h, "t1");
    queue(&h, "t2");
    let (w1, _) = claim(&h);
    let landed = tree_path(&ok_as(&h, &w1, &["ledger", "worktree", "new", "lands"]));
    let w = work_in(&h, &landed, "lands");
    std::fs::write(landed.join("README.md"), "# alpha\n").unwrap();
    std::fs::remove_file(landed.join("lands-untracked.txt")).unwrap();
    let landed_head = w.commits[0].clone();
    let o = h
        .horch(&["ledger", "finish", "--json"])
        .env("HORCH_WORKER_ID", &w1)
        .output()
        .unwrap();
    let r: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
    assert!(o.status.success(), "{r}");
    assert_eq!(state(&h, &landed).as_deref(), Some("landed"), "{r}");

    let (w2, _) = claim(&h);
    let open = tree_path(&ok_as(&h, &w2, &["ledger", "worktree", "new", "open"]));
    let open_work = work_in(&h, &open, "open");
    let hand = hand_tree(&h, "hand", "hand-branch");

    pass(&h);
    until(
        &h,
        "the removal of the landed tree",
        2 * LIST_PERIOD_S,
        |h| state(h, &landed).as_deref() == Some("removed"),
    );
    // 1 more period: no other tree goes.
    for _ in 0..(LIST_PERIOD_S / PASS_S) {
        step(&h);
    }
    assert!(!landed.exists(), "{}", board(&h));
    let id = column(&h, &landed, "worktree_id").unwrap();
    let ms = moves(&h, &id);
    assert!(ms.ends_with(&["landed>removed".to_string()]), "{ms:?}");
    let run_branch = run_branch(&h, "fm27");
    let project = h.project.clone();
    git_in(
        &h,
        &project,
        &["merge-base", "--is-ancestor", &landed_head, &run_branch],
    );
    for tree in [&open, &hand.tree] {
        let s = state(&h, tree).unwrap();
        assert!(
            ["active", "orphaned", "rescued"].contains(&s.as_str()),
            "{}: {s}\n{}",
            tree.display(),
            board(&h)
        );
        assert!(tree.is_dir(), "{}", board(&h));
    }
    assert_kept(&h, &[&open_work, &hand]);
}

#[test]
fn fm28_over_budget_removes_finished_trees_and_holds_dispatch_before_an_active_tree() {
    let h = fleet("p3-fm28");
    let project = h.project.clone();
    std::fs::write(project.join(".gitignore"), "*.big\n").unwrap();
    git_in(&h, &project, &["add", ".gitignore"]);
    git_in(
        &h,
        &project,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "ignore"],
    );
    let fleet_root = h.root.join("fleet");
    std::fs::create_dir_all(&fleet_root).unwrap();
    std::fs::write(
        fleet_root.join("fleet.toml"),
        "worktree_disk_budget_gb = 1\n",
    )
    .unwrap();

    // A run that is done, with a rescued tree of its task above the budget.
    ok(&h, &["ledger", "run", "new", "old"]);
    queue(&h, "old");
    let branch = format!("{}-T1-old", run_branch(&h, "old"));
    let old = hand_tree(&h, "T1-old", &branch);
    big_file(&old.tree.join("old.big"), BIG);
    pass(&h);
    until(&h, "the rescue of the old tree", CAUGHT_S, |h| {
        state(h, &old.tree).as_deref() == Some("rescued")
    });
    assert_ne!(column(&h, &old.tree, "task_id"), Some(String::new()));
    exec(&h, "UPDATE runs SET status = 'done' WHERE slug = 'old'");

    // A live run with an active tree.
    ok(&h, &["ledger", "run", "new", "live"]);
    queue(&h, "live");
    let mut agents = Agents(Vec::new());
    let (w, _) = live_claim(&h, &mut agents);
    let active = tree_path(&ok_as(&h, &w, &["ledger", "worktree", "new", "live"]));
    let live = work_in(&h, &active, "live");

    until(&h, "the removal of the old tree", 2 * LIST_PERIOD_S, |h| {
        state(h, &old.tree).as_deref() == Some("removed")
    });
    assert!(!old.tree.exists(), "{}", board(&h));
    assert_eq!(
        state(&h, &active).as_deref(),
        Some("active"),
        "{}",
        board(&h)
    );
    assert!(needs_you(&h, "worktree_disk").is_empty(), "{}", board(&h));

    // Over the budget with only an active tree: dispatch holds, the tree
    // stays.
    big_file(&active.join("live.big"), BIG);
    until(&h, "the disk hold", 2 * LIST_PERIOD_S, |h| {
        !needs_you(h, "worktree_disk").is_empty()
    });
    for _ in 0..(LIST_PERIOD_S / PASS_S) {
        step(&h);
    }
    assert_eq!(needs_you(&h, "worktree_disk").len(), 1, "{}", board(&h));
    assert_eq!(
        state(&h, &active).as_deref(),
        Some("active"),
        "{}",
        board(&h)
    );
    let o = h.run(&["spawn", "opus", "--from-pane", "w1:p1", "--no-tile"]);
    assert!(!o.status.success(), "the spawn was not held: {}", text(&o));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("on hold") && err.contains("worktree"), "{err}");
    assert_kept(&h, &[&old, &live]);
}

/// A `git` that sleeps [`SLOW_ADD_S`] before `git worktree add`, and runs
/// the real `git` at once for each other command.
fn slow_git(h: &mut Harness) {
    let path = std::env::var_os("PATH").expect("a PATH");
    let real = std::env::split_paths(&path)
        .map(|d| d.join("git"))
        .find(|g| g.is_absolute() && g.is_file())
        .expect("git on the PATH");
    let dir = h.root.join("slow-git");
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("git");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprev=\nfor a in \"$@\"; do\n  if [ \"$prev\" = worktree ] && [ \"$a\" = add ]; then\n    /bin/sleep {SLOW_ADD_S}\n    break\n  fi\n  prev=$a\ndone\nexec '{}' \"$@\"\n",
            real.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    h.set("HORCH_GIT_BIN", script.to_string_lossy());
}

/// The awake time of the last heartbeat of the fleetd `pid`, in ms.
fn heartbeat(h: &Harness, pid: u32) -> Option<u64> {
    let path = h.root.join("fleet").join("run").join("heartbeat.json");
    let hb: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (hb["pid"].as_u64() == Some(u64::from(pid)))
        .then(|| hb["awake"]["awake_ms"].as_u64())
        .flatten()
}

fn epoch_ms(at: &str) -> i64 {
    horch_core::clock::parse(at)
        .unwrap_or_else(|| panic!("a time: {at}"))
        .timestamp_millis()
}

fn now_ms() -> i64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    i64::try_from(since.as_millis()).unwrap()
}

/// Wait until the fleetd `pid` wrote its first heartbeat.
fn wait_for_heartbeat(h: &Harness, pid: u32) {
    let until = Instant::now() + Duration::from_secs(60);
    while heartbeat(h, pid).is_none() {
        assert!(Instant::now() < until, "no heartbeat of fleetd {pid}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// What a command did while the test watched the heartbeat.
struct Watched {
    out: String,
    took: Duration,
    /// The longest time with no new heartbeat.
    longest_still: Duration,
    /// When the command exited, in ms after the Unix epoch.
    exited_ms: i64,
}

/// Run `horch ledger worktree new <name>` as `worker`, and watch the
/// heartbeat of the fleetd `pid` until the command exits.
fn add_watching_heartbeat(h: &Harness, worker: &str, name: &str, pid: u32) -> Watched {
    let mut add = h
        .horch(&["ledger", "worktree", "new", name])
        .env("HORCH_WORKER_ID", worker)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let began = Instant::now();
    let mut last = heartbeat(h, pid);
    let mut moved_at = Instant::now();
    let mut longest_still = Duration::ZERO;
    let status = loop {
        if let Some(s) = add.try_wait().unwrap() {
            break s;
        }
        assert!(
            began.elapsed() < Duration::from_secs(SLOW_ADD_S + 120),
            "the add did not end\n{}",
            board(h)
        );
        let now = heartbeat(h, pid);
        if now != last {
            last = now;
            moved_at = Instant::now();
        }
        longest_still = longest_still.max(moved_at.elapsed());
        std::thread::sleep(Duration::from_millis(200));
    };
    let exited_ms = now_ms();
    let mut out = String::new();
    add.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    let mut err = String::new();
    add.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    assert!(status.success(), "{out}\n{err}");
    Watched {
        out,
        took: began.elapsed(),
        longest_still,
        exited_ms,
    }
}

/// The 1 `worktree_add` run of the tree at `path` passed, took the slow
/// add, and ended before `exited_ms`: the command gave the path only after
/// the job passed (Q17).
fn assert_add_job_passed_before(h: &Harness, path: &Path, exited_ms: i64) {
    let id = column(h, path, "worktree_id").unwrap();
    let job = query(
        h,
        &format!(
            "SELECT state || ' ' || started_at || ' ' || ended_at FROM jobs
             WHERE kind = 'worktree_add' AND subject = '{id}'"
        ),
    );
    assert_eq!(job.len(), 1, "{}", board(h));
    let parts: Vec<&str> = job[0].split(' ').collect();
    assert_eq!(parts[0], "passed", "{}", board(h));
    let (started, ended) = (epoch_ms(parts[1]), epoch_ms(parts[2]));
    let slow_ms = i64::try_from(SLOW_ADD_S * 1000).unwrap();
    assert!(
        ended - started >= slow_ms,
        "the job is not the slow add: {}",
        job[0]
    );
    assert!(
        ended <= exited_ms,
        "the path came before the job passed: {}",
        job[0]
    );
}

#[test]
fn slow_worktree_add_of_60_s_keeps_the_heartbeat_and_the_tree_is_used_only_after_its_job_passed() {
    let mut h = fleet("p3-slow-add");
    slow_git(&mut h);
    ok(&h, &["ledger", "run", "new", "slow"]);
    queue(&h, "t1");
    let mut agents = Agents(Vec::new());
    let (w, _) = live_claim(&h, &mut agents);
    let mut fleetds = Fleetds::new(&h);
    let pid = fleetds.start(&h, "fleetd");
    wait_for_heartbeat(&h, pid);

    let add = add_watching_heartbeat(&h, &w, "feat", pid);

    assert!(
        add.took >= Duration::from_secs(SLOW_ADD_S),
        "the add was not slow: {:?}",
        add.took
    );
    assert!(
        add.longest_still <= Duration::from_secs(PASS_TIME_BUDGET_S),
        "the heartbeat stood still {:?} during the add\n{}",
        add.longest_still,
        board(&h)
    );
    assert!(
        fleetds
            .row()
            .is_some_and(|(p, started)| horch_core::procid::alive(p, Some(started))),
        "fleetd stopped during the add"
    );
    let path = tree_path(&add.out);
    assert!(path.is_dir(), "{}", add.out);
    assert_add_job_passed_before(&h, &path, add.exited_ms);
    assert_eq!(state(&h, &path).as_deref(), Some("active"), "{}", board(&h));
    let work = work_in(&h, &path, "feat");
    assert_kept(&h, &[&work]);
}
