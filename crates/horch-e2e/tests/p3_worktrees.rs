#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Output};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use horch_core::taskledger::runs::Run;
use horch_core::taskledger::store::Store;
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{fixtures, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

const PASS_S: i64 = 5;
const LIST_PERIOD_S: i64 = 10;
const CAUGHT_S: i64 = 60;
const PASS_TIME_BUDGET_S: u64 = 20;
const SLOW_ADD_S: u64 = 60;
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

fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn fleet_toml() -> String {
    format!("pass_period_s = 1\nworktree_list_period_s = {LIST_PERIOD_S}\n")
}

fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set("HORCH_LEDGER_DB", ledger(&h).to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    path_without_docs_clis(&mut h);
    let root = h.fleet_root();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("fleet.toml"), fleet_toml()).unwrap();
    h.serve_fleetd();
    h
}

const DOCS_CLIS: [&str; 4] = ["herdr", "claude", "codex", "agy"];

fn path_without_docs_clis(h: &mut Harness) {
    let dir = h.root.join("no-docs-clis");
    std::fs::create_dir_all(&dir).unwrap();
    for entry in std::fs::read_dir(&h.bin).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if !DOCS_CLIS.iter().any(|c| name.to_str() == Some(c)) {
            std::os::unix::fs::symlink(entry.path(), dir.join(&name)).unwrap();
        }
    }
    let mut dirs = vec![dir];
    let host = h.root.join("host-bin");
    if host.is_dir() {
        dirs.push(host);
    }
    let path = std::env::join_paths(dirs).unwrap();
    h.set("PATH", path.to_string_lossy());
}

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

fn seed(h: &Harness, slug: &str, steps: Option<&str>) -> Run {
    let run = seed_run(h, slug, None, "w1:p1");
    if let Some(steps) = steps {
        let words: Vec<String> = steps.split(',').map(|s| format!("{s:?}")).collect();
        exec(
            h,
            &format!(
                "UPDATE runs SET finish_steps = '[{}]' WHERE run_id = '{}'",
                words.join(","),
                run.run_id
            ),
        );
    }
    run
}

fn queue(h: &Harness, run: &Run, name: &str) {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let ticket = dir.join(format!("{name}.md"));
    std::fs::write(&ticket, format!("# {name}\n\n**Check:** none\n")).unwrap();
    let task = seed_task(h, run, &ticket, "opus");
    exec(
        h,
        &format!(
            "UPDATE tasks SET check_none = 1 WHERE task_id = '{}'",
            task.task_id
        ),
    );
}

fn bind_next(h: &Harness) -> (String, String) {
    let b = horch_e2e::bind::bind(h, "opus", None);
    (b.worker_id, b.task_id)
}

struct Agents(Vec<Child>);

impl Drop for Agents {
    fn drop(&mut self) {
        for a in &mut self.0 {
            let _ = a.kill();
            let _ = a.wait();
        }
    }
}

fn live_bind(h: &Harness, agents: &mut Agents) -> (String, String) {
    let split: Value = serde_json::from_str(&herdr(h, &["pane", "split", "w1:p1"])).unwrap();
    let pane = split["result"]["pane"]["pane_id"].as_str().unwrap();
    let agent = std::process::Command::new("/bin/sleep")
        .arg("600")
        .spawn()
        .unwrap();
    let worker = horch_core::taskledger::workers::NewWorker {
        persona: "opus".into(),
        pane_id: Some(pane.to_string()),
        ..Default::default()
    };
    let b = horch_e2e::bind::bind_worker(h, worker, Some(agent.id()));
    agents.0.push(agent);
    (b.worker_id, b.task_id)
}

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

fn stored(h: &Harness, path: &Path) -> String {
    let project = std::fs::canonicalize(&h.project).unwrap();
    match path.strip_prefix(&h.project) {
        Ok(rel) => project.join(rel).display().to_string(),
        Err(_) => path.display().to_string(),
    }
}

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

fn needs_you(h: &Harness, kind: &str) -> Vec<String> {
    query(
        h,
        &format!(
            "SELECT key || ' ' || entity_id FROM needs_you
             WHERE kind = '{kind}' AND state = 'open' ORDER BY key"
        ),
    )
}

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
    let attempts = query(
        h,
        "SELECT a.id || ' ' || a.state || ' lease=' || COALESCE(
             (SELECT CAST(expires_at AS TEXT) FROM leases WHERE attempt_id = a.id), '-')
         FROM attempts a ORDER BY a.rowid",
    );
    format!(
        "fleet clock {}\nattempts:\n{}\nworktrees:\n{}\njobs:\n{}\nneeds-you:\n{}",
        fleet_clock(h),
        attempts.join("\n"),
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

fn pass(h: &Harness) -> Output {
    let slug = query(
        h,
        "SELECT slug FROM runs WHERE slug NOT LIKE 'F-%' ORDER BY rowid DESC LIMIT 1",
    )
    .pop()
    .expect("a run of the test");
    let o = h.without_fleetd(|| {
        h.run(&[
            "ledger", "watch", "--once", "--force", "--json", "--run", &slug,
        ])
    });
    assert!(o.status.success(), "{}", text(&o));
    let until = Instant::now() + Duration::from_secs(300);
    while !query(h, "SELECT id FROM jobs WHERE state = 'running'").is_empty() {
        assert!(
            Instant::now() < until,
            "a job runs after 300 s\n{}",
            board(h)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    o
}

fn step(h: &Harness) -> Output {
    h.advance_fleet_clock(PASS_S);
    pass(h)
}

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

fn lists_passed(h: &Harness) -> usize {
    query(
        h,
        "SELECT id FROM jobs WHERE kind = 'worktree_list' AND state = 'passed'",
    )
    .len()
}

struct Work {
    tree: PathBuf,
    files: Vec<(String, Vec<u8>)>,
    commits: Vec<String>,
}

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

fn big_file(path: &Path, bytes: u64) {
    std::fs::File::create(path).unwrap().set_len(bytes).unwrap();
}

#[test]
fn fr11_worker_own_git_worktree_add_is_flagged_on_the_next_pass() {
    let _serial = serial();
    let h = world("p3-fr11");
    let run = seed(&h, "fr11", None);
    let (_, task) = h.without_fleetd(|| {
        queue(&h, &run, "t1");
        bind_next(&h)
    });
    pass(&h);
    let lists = lists_passed(&h);
    let branch = format!("{}-T1-own", run_branch(&h, "fr11"));
    let work = hand_tree(&h, "T1-own", &branch);

    until(
        &h,
        "a worktree_list run after the add",
        LIST_PERIOD_S,
        |h| lists_passed(h) > lists,
    );
    let o = step(&h);
    let id = column(&h, &work.tree, "worktree_id")
        .unwrap_or_else(|| panic!("no row flags the tree\n{}\n{}", board(&h), text(&o)));
    let s = state(&h, &work.tree).unwrap();
    assert!(s == "orphaned" || s == "rescued", "{s}\n{}", board(&h));
    assert_eq!(
        column(&h, &work.tree, "task_id"),
        Some(task),
        "{}",
        board(&h)
    );
    assert_eq!(
        moves(&h, &id).first().map(String::as_str),
        Some(">orphaned"),
        "{}",
        board(&h)
    );
    assert_kept(&h, &[&work]);
}

#[test]
fn rr10_hand_made_worktree_is_adopted_within_1_list_period() {
    let _serial = serial();
    let h = world("p3-rr10");
    seed(&h, "rr10", None);
    pass(&h);
    let work = hand_tree(&h, "by-hand", "by-hand");
    let mut steps = 0;
    while state(&h, &work.tree).is_none() {
        assert!(
            steps * PASS_S < LIST_PERIOD_S + PASS_S,
            "the adoption: not within {} steps\n{}",
            steps,
            board(&h)
        );
        step(&h);
        steps += 1;
    }
    let s = state(&h, &work.tree).unwrap();
    assert!(s == "orphaned" || s == "rescued", "{s}\n{}", board(&h));
    assert_eq!(column(&h, &work.tree, "task_id"), Some(String::new()));
    assert!(steps * PASS_S <= LIST_PERIOD_S + PASS_S, "{steps} steps");
    assert_kept(&h, &[&work]);
}

#[test]
fn fm26_tree_made_outside_fleetd_is_rescued_and_its_files_are_on_the_ref() {
    let _serial = serial();
    let h = world("p3-fm26");
    seed(&h, "fm26", None);
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
    for _ in 0..(LIST_PERIOD_S / PASS_S) {
        step(&h);
    }
    assert_eq!(state(&h, &work.tree).as_deref(), Some("rescued"));
    assert!(in_tree(&h, &work), "{}", board(&h));
    assert_kept(&h, &[&work]);
}

#[test]
fn fm27_vanished_tree_opens_1_needs_you_item_within_60_s() {
    let _serial = serial();
    let h = world("p3-fm27-vanished");
    seed(&h, "fm27", None);
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

fn in_attempt(h: &Harness, w: &str, args: &[&str]) -> Output {
    let mut cmd = std::process::Command::new("/bin/sh");
    cmd.args(["-c", "read go; exec \"$@\"", "sh"])
        .arg(Harness::horch_bin())
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    h.seal(&mut cmd);
    cmd.env("HORCH_WORKER_ID", w);
    let mut child = cmd.spawn().unwrap();
    let wrapper = child.id();
    Store::open(&ledger(h))
        .unwrap()
        .conn()
        .execute(
            "UPDATE attempts SET pid = ?1, pid_start = ?2 WHERE worker_id = ?3",
            (wrapper, horch_core::procid::start_time(wrapper).unwrap(), w),
        )
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), b"go\n").unwrap();
    child.wait_with_output().unwrap()
}

fn active_tree(h: &Harness, task: &str) -> PathBuf {
    until(h, "the active tree of the task", CAUGHT_S, |h| {
        !query(
            h,
            &format!("SELECT path FROM worktrees WHERE task_id = '{task}' AND state = 'active'"),
        )
        .is_empty()
    });
    PathBuf::from(
        query(
            h,
            &format!("SELECT path FROM worktrees WHERE task_id = '{task}' AND state = 'active'"),
        )
        .pop()
        .unwrap(),
    )
}

fn commit_gates(h: &Harness) {
    std::fs::write(
        h.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    git_in(h, &h.project, &["add", "fleet.toml"]);
    git_in(
        h,
        &h.project,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
    );
}

#[test]
fn fm27_trees_that_are_not_landed_are_not_removed() {
    let _serial = serial();
    let h = fleet("p3-fm27-keep");
    let run = seed(&h, "fm27", Some("land"));
    let mut agents = Agents(Vec::new());
    let (_, task) = h.without_fleetd(|| {
        queue(&h, &run, "t1");
        live_bind(&h, &mut agents)
    });
    exec(
        &h,
        &format!("UPDATE leases SET expires_at = expires_at + 86400 WHERE task_id = '{task}'"),
    );
    let open = active_tree(&h, &task);
    let open_work = work_in(&h, &open, "open");
    let hand = hand_tree(&h, "hand", "hand-branch");

    pass(&h);
    for _ in 0..(2 * LIST_PERIOD_S / PASS_S) {
        step(&h);
    }
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
fn fm27_only_landed_or_rescued_trees_are_removed() {
    let _serial = serial();
    let h = world("p3-fm27-only");
    commit_gates(&h);
    let run = seed(&h, "fm27", Some("land"));
    let ((w1, t1), (_, t2)) = h.without_fleetd(|| {
        queue(&h, &run, "t1");
        queue(&h, &run, "t2");
        (bind_next(&h), bind_next(&h))
    });
    let landed = active_tree(&h, &t1);
    let w = work_in(&h, &landed, "lands");
    std::fs::write(landed.join("README.md"), "# alpha\n").unwrap();
    std::fs::remove_file(landed.join("lands-untracked.txt")).unwrap();
    let landed_head = w.commits[0].clone();
    let o = in_attempt(&h, &w1, &["ledger", "finish", "--json"]);
    assert!(o.status.success(), "{}", text(&o));

    let open = active_tree(&h, &t2);
    let open_work = work_in(&h, &open, "open");
    let hand = hand_tree(&h, "hand", "hand-branch");

    pass(&h);
    until(
        &h,
        "the removal of the landed tree",
        2 * LIST_PERIOD_S,
        |h| state(h, &landed).as_deref() == Some("removed"),
    );
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
    let _serial = serial();
    let h = fleet("p3-fm28");
    let project = h.project.clone();
    std::fs::write(project.join(".gitignore"), "*.big\n").unwrap();
    git_in(&h, &project, &["add", ".gitignore"]);
    git_in(
        &h,
        &project,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "ignore"],
    );
    std::fs::write(
        h.fleet_root().join("fleet.toml"),
        format!("{}worktree_disk_budget_gb = 1\n", fleet_toml()),
    )
    .unwrap();

    let old_run = seed(&h, "old", None);
    queue(&h, &old_run, "old");
    let branch = format!("{}-T1-old", run_branch(&h, "old"));
    let old = hand_tree(&h, "T1-old", &branch);
    big_file(&old.tree.join("old.big"), BIG);
    pass(&h);
    until(&h, "the rescue of the old tree", CAUGHT_S, |h| {
        state(h, &old.tree).as_deref() == Some("rescued")
    });
    assert_ne!(column(&h, &old.tree, "task_id"), Some(String::new()));
    exec(&h, "UPDATE runs SET status = 'done' WHERE slug = 'old'");
    exec(&h, "UPDATE tasks SET status = 'done' WHERE title = 'old'");
    exec(
        &h,
        "UPDATE ideas SET state = 'merged'
         WHERE id = (SELECT idea_id FROM tasks WHERE title = 'old')",
    );

    let live_run = seed(&h, "live", None);
    let mut agents = Agents(Vec::new());
    let (_, task) = h.without_fleetd(|| {
        queue(&h, &live_run, "live");
        live_bind(&h, &mut agents)
    });
    exec(
        &h,
        &format!("UPDATE leases SET expires_at = expires_at + 86400 WHERE task_id = '{task}'"),
    );
    until(&h, "the active tree of the live task", CAUGHT_S, |h| {
        !query(
            h,
            &format!("SELECT path FROM worktrees WHERE task_id = '{task}' AND state = 'active'"),
        )
        .is_empty()
    });
    let active = PathBuf::from(
        query(
            &h,
            &format!("SELECT path FROM worktrees WHERE task_id = '{task}' AND state = 'active'"),
        )
        .pop()
        .unwrap(),
    );
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
    queue(&h, &live_run, "held");
    for _ in 0..(LIST_PERIOD_S / PASS_S) {
        step(&h);
    }
    let attempts = query(
        &h,
        "SELECT a.id FROM attempts a JOIN tasks t ON t.task_id = a.task_id WHERE t.title = 'held'",
    );
    assert!(attempts.is_empty(), "{attempts:?}\n{}", board(&h));
    let status = query(&h, "SELECT status FROM tasks WHERE title = 'held'");
    assert_eq!(status, ["ready"], "{}", board(&h));
    let item = query(
        &h,
        "SELECT text FROM needs_you WHERE kind = 'worktree_disk' AND state = 'open'",
    );
    assert!(item[0].contains("dispatch"), "{item:?}");
    assert_kept(&h, &[&old, &live]);
}

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

fn wait_for_heartbeat(h: &Harness, pid: u32) {
    let until = Instant::now() + Duration::from_secs(60);
    while heartbeat(h, pid).is_none() {
        assert!(Instant::now() < until, "no heartbeat of fleetd {pid}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

struct Watched {
    out: String,
    took: Duration,
    longest_still: Duration,
    exited_ms: i64,
}

fn add_watching_heartbeat(h: &Harness, task: &str, pid: u32) -> Watched {
    let began = Instant::now();
    let mut last = heartbeat(h, pid);
    let mut moved_at = Instant::now();
    let mut longest_still = Duration::ZERO;
    let path = loop {
        let found = query(
            h,
            &format!("SELECT path FROM worktrees WHERE task_id = '{task}' AND state = 'active'"),
        );
        if let Some(path) = found.into_iter().next() {
            break path;
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
    Watched {
        out: format!("path: {path}\n"),
        took: began.elapsed(),
        longest_still,
        exited_ms,
    }
}

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
    let _serial = serial();
    let mut h = fleet("p3-slow-add");
    slow_git(&mut h);
    let mut agents = Agents(Vec::new());
    let (_, task) = h.without_fleetd(|| {
        let run = seed(&h, "slow", None);
        queue(&h, &run, "t1");
        live_bind(&h, &mut agents)
    });
    let fleetds = Fleetds::new(&h);
    let pid = fleetds.row().expect("the row of the test fleetd").0;
    wait_for_heartbeat(&h, pid);

    let add = add_watching_heartbeat(&h, &task, pid);

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
