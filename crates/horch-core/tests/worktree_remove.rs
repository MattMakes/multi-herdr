#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::{Liveness, ProcessFact};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, result_data, CheckRunner, JobCtx, JobKey, JobRunner, Queued, Ran,
    SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::ideas::domain::needs_you::Item;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::fleetd::worktree_custody::adapters::remove_git::{
    self, read_remove_key, remove_key, CliRemoval, Owner, RemovalGateway, Removed,
};
use horch_core::fleetd::worktree_custody::adapters::rescue_git::rescue_key;
use horch_core::fleetd::worktree_custody::adapters::sqlite::{
    self as trees, NewWorktree, WorktreeRow, WorktreeState,
};
use horch_core::fleetd::worktree_custody::application::remove::{self, Observed};
use horch_core::fleetd::worktree_custody::application::rescue;
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::schema;
use horch_core::taskledger::workers::{self, NewWorker, Worker};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    repo: PathBuf,
}

fn world() -> World {
    world_in(tempfile::tempdir().unwrap())
}

fn world_below_home() -> World {
    let home = std::env::var_os("HOME").expect("HOME is set");
    world_in(tempfile::tempdir_in(home).unwrap())
}

fn world_in(dir: tempfile::TempDir) -> World {
    assert!(
        std::env::var_os("FLEET_ROOT").is_none(),
        "these tests use the fleet root of their temp ledger"
    );
    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let repo = repo.canonicalize().unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    std::fs::write(repo.join(".gitignore"), "build/\n*.log\n.worktrees/\n").unwrap();
    git(&repo, &["add", "a.txt", ".gitignore"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    World {
        dir,
        conn,
        config: FleetConfig::default(),
        repo,
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_out(dir, args);
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn git_out(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
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
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap()
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, text).unwrap();
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
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

fn cause() -> Cause<'static> {
    Cause {
        actor: Actor::Horch,
        text: "test",
    }
}

impl World {
    fn tree_path(&self, name: &str) -> PathBuf {
        self.repo.join(".worktrees").join(name)
    }

    fn tree(&mut self, name: &str, worker: Option<&str>) -> PathBuf {
        let path = self.tree_path(name);
        let branch = format!("fleet/{name}");
        let path_s = path.display().to_string();
        git(
            &self.repo,
            &["worktree", "add", "-q", "-b", &branch, &path_s, "main"],
        );
        let new = NewWorktree {
            worktree_id: name.to_string(),
            task_id: None,
            worker_id: worker.map(str::to_string),
            repo_root: self.repo.display().to_string(),
            path: path_s,
            branch,
            base: "main".to_string(),
            base_commit: Some(git(&self.repo, &["rev-parse", "main"])),
            status: WorktreeStatus::Open,
            mode: None,
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        trees::insert_orphan(&tx, None, &new, cause()).unwrap();
        tx.commit().unwrap();
        path
    }

    fn landed(&mut self, name: &str, merged: bool) -> PathBuf {
        let path = self.tree(name, None);
        let file = format!("{name}.txt");
        write(&path.join(&file), "landed\n");
        git(&path, &["add", &file]);
        git(&path, &["commit", "-q", "-m", "work"]);
        let head = git(&path, &["rev-parse", "HEAD"]);
        if merged {
            git(
                &self.repo,
                &["merge", "-q", "--ff-only", &format!("fleet/{name}")],
            );
        }
        self.set_state(name, "landed");
        self.conn
            .execute(
                "UPDATE worktrees SET landed_commit = ?2 WHERE worktree_id = ?1",
                [name, head.as_str()],
            )
            .unwrap();
        path
    }

    fn rescued(&mut self, name: &str) -> PathBuf {
        let path = self.tree(name, None);
        write(&path.join("b.txt"), "b\n");
        git(&path, &["add", "b.txt"]);
        git(&path, &["commit", "-q", "-m", "work"]);
        write(&path.join("a.txt"), "a changed\n");
        write(&path.join("new/u.txt"), "untracked\n");
        write(&path.join("build/out.o"), "ignored\n");
        let key = rescue_key(
            &path.display().to_string(),
            &git(&path, &["rev-parse", "HEAD"]),
        );
        let id = self.queue(Kind::Rescue, name, &key);
        let ended = self.run(id);
        assert_eq!(ended.state, State::Passed, "{}", text(&ended));
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let seen = rescue::observe(&ctx(&self.config), &tx, &ended).unwrap();
        tx.commit().unwrap();
        assert!(matches!(seen, rescue::Observed::Rescued(_)), "{seen:?}");
        assert_eq!(self.row(name).state, WorktreeState::Rescued);
        path
    }

    fn set_state(&self, name: &str, state: &str) {
        self.conn
            .execute(
                "UPDATE worktrees SET state = ?2 WHERE worktree_id = ?1",
                [name, state],
            )
            .unwrap();
    }

    fn row(&self, name: &str) -> WorktreeRow {
        trees::get(&self.conn, name).unwrap().unwrap()
    }

    fn landed_commit(&self, name: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT landed_commit FROM worktrees WHERE worktree_id = ?1",
                [name],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn cleaned_at(&self, name: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT cleaned_at FROM worktrees WHERE worktree_id = ?1",
                [name],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
            .flatten()
    }

    fn job(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    fn queue(&mut self, kind: Kind, name: &str, input_key: &str) -> i64 {
        let key = JobKey {
            kind,
            subject: name,
            input_key,
        };
        let repo = self.repo.display().to_string();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let q = jobs::queue(&ctx(&self.config), &tx, &key, Some(&repo), FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        match q {
            Queued::New(id) => id,
            other => panic!("expected a new run, got {other:?}"),
        }
    }

    fn queue_remove(&mut self, name: &str) -> Result<Queued, String> {
        self.queue_remove_keeping(name, false)
    }

    fn queue_remove_keeping(&mut self, name: &str, keep_branch: bool) -> Result<Queued, String> {
        let row = self.row(name);
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let q = remove::queue_remove(
            &ctx(&config),
            &tx,
            &row,
            "test",
            keep_branch,
            FleetSeconds(0),
        )
        .unwrap();
        tx.commit().unwrap();
        q
    }

    fn queued_remove(&mut self, name: &str) -> i64 {
        match self.queue_remove(name) {
            Ok(Queued::New(id)) => id,
            other => panic!("expected a new run, got {other:?}"),
        }
    }

    fn start(&mut self, id: i64) -> JobRow {
        let spec = LaunchSpec {
            program: "true".into(),
            args: Vec::new(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        let job = self.job(id);
        let config = self.config.clone();
        let mut runner = JobRunner::new(&Launcher);
        let started = runner
            .start(&ctx(&config), &mut self.conn, &job, &spec, FleetSeconds(0))
            .unwrap();
        assert!(started, "the run {id} starts");
        self.job(id)
    }

    fn run(&mut self, id: i64) -> JobRow {
        let pid = self.start(id).pid.unwrap();
        let config = self.config.clone();
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
            sweep: None,
        };
        let written = jobs::run_job(&ctx(&config), &self.conn, id, pid, &checks).unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        jobs::end_from_files(&ctx(&config), &mut self.conn).unwrap();
        self.job(id)
    }

    fn run_with(&mut self, id: i64, name: &str, gw: &dyn RemovalGateway) -> JobRow {
        self.start(id);
        let row = self.row(name);
        let landed = self.landed_commit(name);
        let db = Connection::open(self.dir.path().join("ledger.sqlite3")).unwrap();
        let ledger = |id: &str| trees::get(&db, id);
        let ran = remove_git::remove(
            gw,
            &row,
            landed.as_deref(),
            &Owner::NoAttempt,
            &ledger,
            false,
        );
        jobs_store::set_result(&self.conn, id, &result_json(&ran)).unwrap();
        let facts = [ProcessFact {
            key: id.to_string(),
            liveness: Liveness::Dead,
        }];
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        jobs::observe(&ctx(&config), &tx, &facts, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        self.job(id)
    }

    fn observe(&mut self, id: i64) -> Observed {
        let job = self.job(id);
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let seen = remove::observe(&ctx(&config), &tx, &job).unwrap();
        tx.commit().unwrap();
        seen
    }

    fn remove(&mut self, name: &str) -> (JobRow, Observed) {
        let id = self.queued_remove(name);
        let ended = self.run(id);
        let seen = self.observe(id);
        (ended, seen)
    }

    fn gateway(&self) -> CliRemoval {
        let fleet = self.dir.path().join("fleet");
        let writable = [self.repo.join(".git"), self.repo.join(".worktrees")];
        CliRemoval::new(CliWorktreeGit::new("git", &fleet).writing(&writable), fleet)
    }

    fn attempt(&mut self, id: &str, pid: u32) -> String {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let new = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = runs::create(&tx, self.dir.path(), &new).unwrap();
        let worker = NewWorker {
            run_id: run.run_id,
            persona: "dev".into(),
            ..Default::default()
        };
        let worker_id = workers::insert(&tx, &worker).unwrap();
        let start = horch_core::procid::start_time(pid).map(|s| s as i64);
        tx.execute(
            "INSERT INTO attempts (id, worker_id, state, pid, pid_start) VALUES (?1, ?2, 'live', ?3, ?4)",
            rusqlite::params![id, worker_id, pid, start],
        )
        .unwrap();
        tx.commit().unwrap();
        worker_id
    }

    fn events(&self) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT text FROM events ORDER BY event_id")
            .unwrap();
        let rows = stmt.query_map([], |r| r.get(0)).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    }
}

fn result_json(ran: &Ran) -> String {
    serde_json::json!({
        "passed": ran.outcome.passed,
        "verdict": ran.outcome.verdict,
        "text": ran.outcome.text,
        "data": ran.data,
    })
    .to_string()
}

fn text(job: &JobRow) -> String {
    outcome(job).map(|o| o.text).unwrap_or_default()
}

fn removed(job: &JobRow) -> Removed {
    serde_json::from_value(result_data(job).expect("a passed removal has its data")).unwrap()
}

fn open_items(w: &World) -> Vec<Item> {
    needs_you::open_items(&w.conn).unwrap()
}

fn files(tree: &Path) -> Vec<(String, Vec<u8>)> {
    fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let path = e.unwrap().path();
            let name = path.strip_prefix(root).unwrap().display().to_string();
            if name == ".git" {
                continue;
            }
            if path.is_dir() {
                collect(root, &path, out);
            } else {
                out.push((name, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    collect(tree, tree, &mut out);
    out.sort();
    out
}

fn stash_list(w: &World) -> String {
    git(&w.repo, &["stash", "list"])
}

struct Kept {
    path: PathBuf,
    files: Vec<(String, Vec<u8>)>,
    head: String,
    stashes: String,
}

fn kept(w: &World, path: &Path) -> Kept {
    Kept {
        path: path.to_path_buf(),
        files: files(path),
        head: git(path, &["rev-parse", "HEAD"]),
        stashes: stash_list(w),
    }
}

impl Kept {
    fn assert_kept(&self, w: &World) {
        assert!(
            self.path.is_dir(),
            "the folder {} stays",
            self.path.display()
        );
        assert_eq!(files(&self.path), self.files, "each file stays");
        assert_eq!(git(&self.path, &["rev-parse", "HEAD"]), self.head);
        assert_eq!(stash_list(w), self.stashes, "the stash entries stay");
    }
}

fn assert_refused(w: &mut World, name: &str, why: &str) -> JobRow {
    let path = w.tree_path(name);
    let before = kept(w, &path);
    let state = w.row(name).state;
    let (ended, seen) = w.remove(name);
    assert_eq!(ended.state, State::Failed, "{}", text(&ended));
    assert!(text(&ended).contains(why), "{why:?} in {}", text(&ended));
    assert_eq!(seen, Observed::Failed);
    before.assert_kept(w);
    assert_eq!(w.row(name).state, state, "the row stays");
    ended
}

#[test]
fn rr9_rescue_ref_is_read_back_before_removal() {
    let mut w = world();
    let path = w.rescued("t1");
    let saved = files(&path);
    let rescue_ref = w.row("t1").rescue_ref.unwrap();

    let (ended, seen) = w.remove("t1");
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert_eq!(seen, Observed::Removed);
    let r = removed(&ended);
    assert!(
        r.forced,
        "a verified rescued tree is removed with 1 --force"
    );
    assert_eq!(r.ignored_files, 1, "build/out.o is deleted with the tree");
    assert!(!r.branch_deleted, "the branch is not merged");
    assert!(!path.exists(), "the folder is gone");
    assert!(!git(&w.repo, &["worktree", "list"]).contains("t1"));
    assert_eq!(w.row("t1").state, WorktreeState::Removed);
    assert!(w.cleaned_at("t1").is_some());
    let events = w.events();
    assert!(
        events.iter().any(|e| e.contains("1 ignored file")),
        "the event of the removal counts the ignored files: {events:?}"
    );

    let back = w.dir.path().join("back");
    git(
        &w.repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &back.display().to_string(),
            &rescue_ref,
        ],
    );
    let want: Vec<_> = saved
        .into_iter()
        .filter(|(name, _)| !name.starts_with("build/"))
        .collect();
    assert_eq!(files(&back), want);
    assert!(git(&w.repo, &["branch", "--list", "fleet/t1"]).contains("fleet/t1"));
}

#[test]
fn remove_deletes_the_tree_of_a_repository_below_home() {
    let mut w = world_below_home();
    let path = w.rescued("t1");

    let (ended, seen) = w.remove("t1");

    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert_eq!(seen, Observed::Removed);
    assert!(!path.exists(), "the folder is gone");
    assert_eq!(w.row("t1").state, WorktreeState::Removed);
}

#[test]
fn remove_deletes_an_ignored_file_written_after_the_rescue() {
    let mut w = world();
    let path = w.rescued("t1");
    write(&path.join("app.log"), "ignored\n");
    let (ended, _) = w.remove("t1");
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert_eq!(removed(&ended).ignored_files, 2);
    assert!(!path.exists());
}

#[test]
fn pres_04_w6_drop_refuses_dirty_stash_unique_or_moved_head() {
    let mut w = world();

    let path = w.landed("dirty", true);
    write(&path.join("dirty.txt"), "changed\n");
    assert_refused(&mut w, "dirty", "modified or untracked file");

    let path = w.landed("stash", true);
    write(&path.join("stash.txt"), "stashed\n");
    git(&path, &["stash", "push", "-q", "-m", "keep me"]);
    assert!(stash_list(&w).contains("On fleet/stash: keep me"));
    assert_refused(&mut w, "stash", "a stash entry names the branch");

    let path = w.landed("unique", true);
    write(&path.join("c.txt"), "c\n");
    git(&path, &["add", "c.txt"]);
    git(&path, &["commit", "-q", "-m", "after landing"]);
    assert_refused(&mut w, "unique", "is not the landed commit");
    assert!(git(&w.repo, &["log", "--oneline", "fleet/unique"]).contains("after landing"));

    let path = w.landed("moved", true);
    let before = kept(&w, &path);
    let id = w.queued_remove("moved");
    let gw = MovesHead {
        inner: w.gateway(),
        tree: path.clone(),
    };
    let ended = w.run_with(id, "moved", &gw);
    assert_eq!(ended.state, State::Failed, "{}", text(&ended));
    assert!(
        text(&ended).contains("the HEAD moved during the checks"),
        "{}",
        text(&ended)
    );
    assert!(path.is_dir());
    assert_eq!(files(&path), before.files);
    assert!(git(&path, &["log", "--oneline"]).contains("during the checks"));
    assert_eq!(w.row("moved").state, WorktreeState::Landed);
}

#[test]
fn remove_refuses_a_rescued_tree_that_changed_after_its_rescue() {
    let mut w = world();

    let path = w.rescued("new");
    write(&path.join("late.txt"), "late\n");
    assert_refused(&mut w, "new", "a file changed after the rescue");

    let path = w.rescued("changed");
    write(&path.join("b.txt"), "b changed later\n");
    assert_refused(&mut w, "changed", "a file changed after the rescue");

    let path = w.rescued("untracked");
    write(&path.join("new/u.txt"), "untracked, then changed\n");
    assert_refused(&mut w, "untracked", "a file changed after the rescue");

    let path = w.rescued("deleted");
    std::fs::remove_file(path.join("new/u.txt")).unwrap();
    assert_refused(&mut w, "deleted", "a file changed after the rescue");

    let path = w.rescued("commit");
    git(&path, &["add", "-A"]);
    git(&path, &["commit", "-q", "-m", "after the rescue"]);
    assert_refused(&mut w, "commit", "is not the parent of the rescue commit");

    let path = w.rescued("stash");
    git(&path, &["stash", "push", "-q", "-m", "keep me"]);
    git(&path, &["stash", "apply", "-q"]);
    assert_refused(&mut w, "stash", "a stash entry names the branch");
}

#[test]
fn remove_refuses_a_rescued_tree_that_a_process_uses() {
    let mut w = world();

    let path = w.rescued("cwd");
    let mut child = Command::new("sleep")
        .arg("60")
        .current_dir(path.join("new"))
        .spawn()
        .unwrap();
    let ended = assert_refused(&mut w, "cwd", "have their working folder in the tree");
    assert!(
        text(&ended).contains(&child.id().to_string()),
        "{}",
        text(&ended)
    );
    child.kill().unwrap();
    child.wait().unwrap();

    let mut child = Command::new("sleep")
        .arg("60")
        .current_dir(w.dir.path())
        .spawn()
        .unwrap();
    let worker = w.attempt("A1", child.id());
    w.rescued("owner");
    w.conn
        .execute(
            "UPDATE worktrees SET worker_id = ?2, owner_attempt = 'A1' WHERE worktree_id = ?1",
            ["owner", worker.as_str()],
        )
        .unwrap();
    assert_refused(&mut w, "owner", "a process of the owner attempt is live");
    child.kill().unwrap();
    child.wait().unwrap();

    let (ended, _) = w.remove("owner");
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
}

#[test]
fn remove_of_a_rescued_tree_whose_owner_attempt_ended_with_no_process_passes() {
    let mut w = world();
    let worker = w.attempt("A1", std::process::id());
    w.rescued("t1");
    w.conn
        .execute_batch(&format!(
            "UPDATE attempts SET pid = NULL, pid_start = NULL WHERE id = 'A1';
             UPDATE worktrees SET worker_id = '{worker}', owner_attempt = 'A1'
             WHERE worktree_id = 't1';"
        ))
        .unwrap();
    assert_refused(&mut w, "t1", "has no recorded process");
    w.conn
        .execute("UPDATE attempts SET state = 'ended' WHERE id = 'A1'", [])
        .unwrap();
    let (ended, _) = w.remove("t1");
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
}

#[test]
fn remove_refuses_a_rescued_tree_that_an_attempt_takes_during_the_checks() {
    let mut w = world();
    let path = w.rescued("t1");
    w.attempt("A2", std::process::id());
    let before = kept(&w, &path);
    let id = w.queued_remove("t1");
    let gw = Adopts {
        inner: w.gateway(),
        db: w.dir.path().join("ledger.sqlite3"),
    };
    let ended = w.run_with(id, "t1", &gw);
    assert_eq!(ended.state, State::Failed, "{}", text(&ended));
    assert!(
        text(&ended).contains("the ledger row of the tree changed during the checks"),
        "{}",
        text(&ended)
    );
    before.assert_kept(&w);
    assert_eq!(w.row("t1").state, WorktreeState::Active);
}

#[test]
fn remove_refuses_a_rescued_tree_while_the_owner_group_has_a_process() {
    use std::os::unix::process::CommandExt;
    let mut w = world();
    let mut leader = Command::new("sh")
        .args(["-c", "sleep 60 & wait"])
        .current_dir(w.dir.path())
        .process_group(0)
        .spawn()
        .unwrap();
    let pgid = leader.id();
    let worker = w.attempt("A1", pgid);
    w.rescued("t1");
    w.conn
        .execute(
            "UPDATE worktrees SET worker_id = ?2, owner_attempt = 'A1' WHERE worktree_id = ?1",
            ["t1", worker.as_str()],
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    leader.kill().unwrap();
    leader.wait().unwrap();
    assert!(!horch_core::procid::group_members(pgid).is_empty());
    assert_refused(&mut w, "t1", "a process of the owner attempt is live");
    let killed = Command::new("kill")
        .args(["-9", "--", &format!("-{pgid}")])
        .status()
        .unwrap();
    assert!(killed.success());
    for _ in 0..50 {
        if horch_core::procid::group_members(pgid).is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let (ended, _) = w.remove("t1");
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
}

#[test]
fn remove_refuses_a_landed_tree_with_an_untracked_file() {
    let mut w = world();
    let path = w.landed("t1", true);
    write(&path.join("notes/u.txt"), "untracked\n");
    let ended = assert_refused(&mut w, "t1", "modified or untracked file");
    assert!(!text(&ended).contains("--force"), "{}", text(&ended));
}

#[test]
fn remove_refuses_when_a_fact_is_unknown() {
    let mut w = world();
    let path = w.rescued("t1");
    let before = kept(&w, &path);
    let id = w.queued_remove("t1");
    let gw = NoStashes(w.gateway());
    let ended = w.run_with(id, "t1", &gw);
    assert_eq!(ended.state, State::Failed, "{}", text(&ended));
    assert!(
        text(&ended).contains("the stash entries of the branch")
            && text(&ended).contains("no stash list"),
        "{}",
        text(&ended)
    );
    before.assert_kept(&w);
    assert_eq!(w.row("t1").state, WorktreeState::Rescued);
}

#[test]
fn remove_keeps_a_branch_that_is_not_merged() {
    let mut w = world();
    let path = w.landed("kept", false);
    let tip = git(&w.repo, &["rev-parse", "fleet/kept"]);
    let (ended, seen) = w.remove("kept");
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert_eq!(seen, Observed::Removed);
    let r = removed(&ended);
    assert!(!r.forced, "a landed tree is never removed with --force");
    assert!(!r.branch_deleted);
    assert!(!path.exists());
    assert_eq!(
        git(&w.repo, &["rev-parse", "fleet/kept"]),
        tip,
        "the branch stays"
    );

    let path = w.landed("merged", true);
    let (ended, _) = w.remove("merged");
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert!(removed(&ended).branch_deleted);
    assert!(!path.exists());
    assert_eq!(git(&w.repo, &["branch", "--list", "fleet/merged"]), "");
    assert_eq!(w.row("merged").state, WorktreeState::Removed);
}

#[test]
fn queue_remove_refuses_an_active_orphaned_or_reserved_row() {
    let mut w = world();
    w.tree("t1", None);
    for state in ["active", "orphaned", "reserved", "created", "landing"] {
        w.set_state("t1", state);
        match w.queue_remove("t1") {
            Err(why) => {
                assert!(why.contains(state), "{why}");
                assert!(why.contains("only a landed or rescued tree"), "{why}");
            }
            other => panic!("{state}: expected a refusal, got {other:?}"),
        }
    }
    let runs: i64 = w
        .conn
        .query_row(
            "SELECT COUNT(*) FROM jobs WHERE kind = 'worktree_remove'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(runs, 0, "no run is queued");

    w.set_state("t1", "landed");
    let id = w.queued_remove("t1");
    let job = w.job(id);
    assert_eq!(job.kind, Kind::WorktreeRemove);
    assert_eq!(job.subject, "t1");
    assert_eq!(job.input_key, w.row("t1").path);
    assert_eq!(job.repo.as_deref(), Some(w.row("t1").repo_root.as_str()));
}

#[test]
fn failed_remove_keeps_the_row_and_opens_1_needs_you_item() {
    let mut w = world();
    let path = w.landed("t1", true);
    write(&path.join("t1.txt"), "changed\n");
    let ended = assert_refused(&mut w, "t1", "modified or untracked file");
    let items: Vec<_> = open_items(&w)
        .into_iter()
        .filter(|i| i.entity_id == "t1")
        .collect();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].kind, "remove_failed");
    assert!(items[0].text.contains(&text(&ended)), "{}", items[0].text);

    assert_refused(&mut w, "t1", "modified or untracked file");
    let count = open_items(&w)
        .iter()
        .filter(|i| i.entity_id == "t1")
        .count();
    assert_eq!(count, 1);

    git(&path, &["checkout", "-q", "--", "t1.txt"]);
    let (ended, seen) = w.remove("t1");
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert_eq!(seen, Observed::Removed);
    assert!(open_items(&w).iter().all(|i| i.entity_id != "t1"));
}

#[test]
fn abandoned_remove_keeps_the_row() {
    let mut w = world();
    let path = w.landed("t1", true);
    let id = w.queued_remove("t1");
    w.start(id);
    let facts = [ProcessFact {
        key: id.to_string(),
        liveness: Liveness::Dead,
    }];
    let config = w.config.clone();
    let tx = w
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    jobs::observe(&ctx(&config), &tx, &facts, FleetSeconds(0)).unwrap();
    tx.commit().unwrap();
    assert_eq!(w.job(id).state, State::Abandoned);
    assert_eq!(w.observe(id), Observed::Kept);
    assert_eq!(w.row("t1").state, WorktreeState::Landed);
    assert!(path.is_dir());
}

#[test]
fn remove_runs_no_hook_or_program_of_the_project() {
    let mut w = world();
    let path = w.rescued("t1");
    let landed = w.landed("t2", true);
    let hook_marker = w.dir.path().join("hook-ran");
    let monitor_marker = w.dir.path().join("fsmonitor-ran");
    let hooks = w.repo.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for hook in [
        "post-checkout",
        "reference-transaction",
        "post-index-change",
    ] {
        let hook_path = hooks.join(hook);
        let script = format!("#!/bin/sh\necho {hook} >> '{}'\n", hook_marker.display());
        std::fs::write(&hook_path, script).unwrap();
        make_executable(&hook_path);
    }
    let monitor = w.dir.path().join("monitor.sh");
    let script = format!("#!/bin/sh\necho ran > '{}'\n", monitor_marker.display());
    std::fs::write(&monitor, script).unwrap();
    make_executable(&monitor);
    git(
        &w.repo,
        &["config", "core.fsmonitor", &monitor.display().to_string()],
    );

    let (ended, _) = w.remove("t1");
    assert_eq!(ended.state, State::Failed);
    assert!(text(&ended).contains("core.fsmonitor"), "{}", text(&ended));
    assert!(path.is_dir(), "it changes nothing");
    assert!(!monitor_marker.exists(), "no fsmonitor ran");

    git(&w.repo, &["config", "--unset", "core.fsmonitor"]);
    let (ended, _) = w.remove("t1");
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    let (ended, _) = w.remove("t2");
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert!(removed(&ended).branch_deleted, "the branch delete ran too");
    assert!(!path.exists() && !landed.exists());
    assert!(!hook_marker.exists(), "no hook ran");
    assert!(!monitor_marker.exists(), "no fsmonitor ran");
}

macro_rules! forward_but_stashes {
    () => {
        forward_but_stashes!(base);
        fn move_tree(&self, repo: &Path, from: &Path, to: &Path) -> anyhow::Result<()> {
            self.inner().move_tree(repo, from, to)
        }
        fn remove(&self, repo: &Path, tree: &Path, force: bool) -> anyhow::Result<()> {
            self.inner().remove(repo, tree, force)
        }
    };
    (base) => {
        fn repair(&self, repo: &Path, tree: &Path) -> anyhow::Result<()> {
            self.inner().repair(repo, tree)
        }
        fn head(&self, tree: &Path) -> anyhow::Result<String> {
            self.inner().head(tree)
        }
        fn dirty(&self, tree: &Path) -> anyhow::Result<bool> {
            self.inner().dirty(tree)
        }
        fn rev(&self, repo: &Path, rev: &str) -> anyhow::Result<String> {
            self.inner().rev(repo, rev)
        }
        fn disk_tree(&self, row: &WorktreeRow, head: &str) -> anyhow::Result<String> {
            self.inner().disk_tree(row, head)
        }
        fn ignored_files(&self, tree: &Path) -> anyhow::Result<u64> {
            self.inner().ignored_files(tree)
        }
        fn alive(&self, pid: u32, start: Option<u64>) -> bool {
            self.inner().alive(pid, start)
        }
        fn cwd_pids(&self, tree: &Path) -> anyhow::Result<Vec<u32>> {
            self.inner().cwd_pids(tree)
        }
        fn merged(&self, repo: &Path, branch: &str, base: &str) -> anyhow::Result<bool> {
            self.inner().merged(repo, branch, base)
        }
        fn delete_branch(&self, repo: &Path, branch: &str, base: &str) -> anyhow::Result<()> {
            self.inner().delete_branch(repo, branch, base)
        }
    };
}

struct NoStashes(CliRemoval);

impl NoStashes {
    fn inner(&self) -> &CliRemoval {
        &self.0
    }
}

impl RemovalGateway for NoStashes {
    forward_but_stashes!();
    fn stashes(&self, _repo: &Path) -> anyhow::Result<Vec<String>> {
        anyhow::bail!("no stash list")
    }
}

struct MovesHead {
    inner: CliRemoval,
    tree: PathBuf,
}

impl MovesHead {
    fn inner(&self) -> &CliRemoval {
        &self.inner
    }
}

impl RemovalGateway for MovesHead {
    forward_but_stashes!();
    fn stashes(&self, repo: &Path) -> anyhow::Result<Vec<String>> {
        git(
            &self.tree,
            &["commit", "-q", "--allow-empty", "-m", "during the checks"],
        );
        self.inner.stashes(repo)
    }
}

struct Adopts {
    inner: CliRemoval,
    db: PathBuf,
}

impl Adopts {
    fn inner(&self) -> &CliRemoval {
        &self.inner
    }
}

impl RemovalGateway for Adopts {
    forward_but_stashes!();
    fn stashes(&self, repo: &Path) -> anyhow::Result<Vec<String>> {
        let db = Connection::open(&self.db).unwrap();
        db.execute(
            "UPDATE worktrees SET state = 'active', owner_attempt = 'A2' WHERE worktree_id = 't1'",
            [],
        )
        .unwrap();
        self.inner.stashes(repo)
    }
}

fn quarantine(w: &World, name: &str) -> PathBuf {
    w.repo
        .join(".worktrees")
        .join(format!(".horch-removing-{name}"))
}

#[test]
fn remove_keeps_a_write_through_the_old_path_after_the_decision() {
    let mut w = world();
    let path = w.rescued("t1");
    let id = w.queued_remove("t1");
    let gw = Quarantine {
        inner: w.gateway(),
        on_remove: Some(path.clone()),
        into_quarantine: None,
        fail_move_back: false,
    };
    let ended = w.run_with(id, "t1", &gw);
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert_eq!(
        std::fs::read_to_string(path.join("late/late.txt")).unwrap(),
        "written after the decision\n"
    );
    assert!(!path.join("a.txt").exists() && !path.join(".git").exists());
    assert!(!quarantine(&w, "t1").exists());
    assert!(!git(&w.repo, &["worktree", "list"]).contains("t1"));
}

#[test]
fn remove_moves_back_a_tree_that_changed_in_quarantine() {
    let mut w = world();
    let path = w.rescued("t1");
    let mut want = files(&path);
    want.push(("late.txt".into(), b"late\n".to_vec()));
    want.sort();
    let id = w.queued_remove("t1");
    let gw = Quarantine {
        inner: w.gateway(),
        on_remove: None,
        into_quarantine: Some("late.txt"),
        fail_move_back: false,
    };
    let ended = w.run_with(id, "t1", &gw);
    assert_eq!(ended.state, State::Failed, "{}", text(&ended));
    assert!(
        text(&ended).contains("in quarantine, the folder differs from the rescue ref"),
        "{}",
        text(&ended)
    );
    assert_eq!(files(&path), want, "the tree is back with each file");
    assert!(!quarantine(&w, "t1").exists());
    assert!(git(&path, &["status", "--porcelain"]).contains("late.txt"));
    assert_eq!(w.row("t1").state, WorktreeState::Rescued);
}

#[test]
fn remove_keeps_the_tree_in_quarantine_when_the_move_back_fails() {
    let mut w = world();
    let path = w.rescued("t1");
    let q = quarantine(&w, "t1");
    let id = w.queued_remove("t1");
    let gw = Quarantine {
        inner: w.gateway(),
        on_remove: None,
        into_quarantine: Some("late.txt"),
        fail_move_back: true,
    };
    let ended = w.run_with(id, "t1", &gw);
    assert_eq!(ended.state, State::Failed, "{}", text(&ended));
    let q_s = q.display().to_string();
    assert!(
        text(&ended).contains(&format!("stays in the quarantine folder {q_s}")),
        "{}",
        text(&ended)
    );
    assert!(q.join("late.txt").exists() && q.join("new/u.txt").exists());
    assert!(!path.exists());
    assert_eq!(w.observe(id), Observed::Failed);
    let items = open_items(&w);
    assert!(
        items
            .iter()
            .any(|i| i.entity_id == "t1" && i.text.contains(&q_s)),
        "{items:?}"
    );
    assert_eq!(w.row("t1").state, WorktreeState::Rescued);

    assert_refused_after_restore(&mut w, "t1");
}

fn assert_refused_after_restore(w: &mut World, name: &str) {
    let (ended, _) = w.remove(name);
    assert_eq!(ended.state, State::Failed, "{}", text(&ended));
    assert!(
        text(&ended).contains("a file changed after the rescue"),
        "{}",
        text(&ended)
    );
    let path = w.tree_path(name);
    assert!(path.join("late.txt").exists() && path.join("new/u.txt").exists());
    assert!(!quarantine(w, name).exists());
    assert!(git(&path, &["status", "--porcelain"]).contains("late.txt"));
    assert!(git(&w.repo, &["worktree", "list"]).contains(&path.display().to_string()));
}

#[test]
fn remove_continues_after_a_crash_after_the_rename() {
    let mut w = world();
    let path = w.rescued("t1");
    let q = quarantine(&w, "t1");
    std::fs::rename(&path, &q).unwrap();
    let (ended, seen) = w.remove("t1");
    assert_eq!(ended.state, State::Passed, "{}", text(&ended));
    assert_eq!(seen, Observed::Removed);
    assert!(!path.exists() && !q.exists());
    assert!(!git(&w.repo, &["worktree", "list"]).contains("t1"));

    let path = w.rescued("t2");
    let q = quarantine(&w, "t2");
    std::fs::rename(&path, &q).unwrap();
    write(&q.join("late.txt"), "late\n");
    assert_refused_after_restore(&mut w, "t2");
}

struct Quarantine {
    inner: CliRemoval,
    on_remove: Option<PathBuf>,
    into_quarantine: Option<&'static str>,
    fail_move_back: bool,
}

impl Quarantine {
    fn inner(&self) -> &CliRemoval {
        &self.inner
    }
}

impl RemovalGateway for Quarantine {
    forward_but_stashes!(base);
    fn stashes(&self, repo: &Path) -> anyhow::Result<Vec<String>> {
        self.inner.stashes(repo)
    }
    fn move_tree(&self, repo: &Path, from: &Path, to: &Path) -> anyhow::Result<()> {
        let back = from
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(".horch-removing-");
        if back && self.fail_move_back {
            anyhow::bail!("the move back fails");
        }
        self.inner.move_tree(repo, from, to)?;
        if let (false, Some(file)) = (back, self.into_quarantine) {
            write(&to.join(file), "late\n");
        }
        Ok(())
    }
    fn remove(&self, repo: &Path, tree: &Path, force: bool) -> anyhow::Result<()> {
        if let Some(old) = &self.on_remove {
            assert!(
                std::fs::write(old.join("a.txt"), "lost?\n").is_err(),
                "the old path holds no tree any more"
            );
            write(&old.join("late/late.txt"), "written after the decision\n");
        }
        self.inner.remove(repo, tree, force)
    }
}

#[test]
fn queue_remove_keeps_at_most_1_open_removal_of_a_tree() {
    let mut w = world();
    w.tree("t1", None);
    w.set_state("t1", "landed");
    let path = w.row("t1").path;
    let id = w.queued_remove("t1");
    assert_eq!(w.job(id).input_key, remove_key(&path, false));
    for open in ["queued", "running"] {
        if open == "running" {
            w.start(id);
        }
        match w.queue_remove("t1") {
            Ok(Queued::Active(run)) => assert_eq!(run.id, id),
            other => panic!("{open}: expected the open run, got {other:?}"),
        }
        let why = w.queue_remove_keeping("t1", true).unwrap_err();
        assert!(
            why.contains(&format!("the worktree_remove job {id} in {open}")),
            "{why}"
        );
        assert!(why.contains("with another input"), "{why}");
    }
    let runs: i64 = w
        .conn
        .query_row(
            "SELECT COUNT(*) FROM jobs WHERE kind = 'worktree_remove'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(runs, 1, "no second run is queued");
    w.conn
        .execute(
            "UPDATE jobs SET state = 'failed', ended_at = '2026-01-01T00:00:00Z' WHERE id = ?1",
            [id],
        )
        .unwrap();
    let keep = match w.queue_remove_keeping("t1", true) {
        Ok(Queued::New(keep)) => keep,
        other => panic!("expected a new run, got {other:?}"),
    };
    let key = w.job(keep).input_key;
    assert_eq!(key, remove_key(&path, true));
    assert_eq!(read_remove_key(&key), (path.as_str(), true));
    assert_eq!(read_remove_key(&path), (path.as_str(), false));
}
