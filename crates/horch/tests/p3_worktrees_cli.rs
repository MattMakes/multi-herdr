//! `horch worktrees`, `horch worktree restore` and the disk line of
//! `horch ledger status` (ticket 15 of fleetd-p3).
//!
//! Each test runs the real binary on its own temporary git project, with
//! its own ledger, state and fleet folders, and the watchdog off.

#![cfg(unix)]

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::taskledger::workers::NewWorker;
use support::fleetd::{short_tempdir, TestFleetd};

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worktree_custody::adapters::sqlite as custody;
use horch_core::fleetd::worktree_custody::application::reconcile::{reconcile_worktrees, Facts};
use horch_core::taskledger::map::ticket_facts;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util, Store};

const HEALTHY_MACHINE: &str = r#"{"os":"linux","arch":"x86_64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

const PATH_WITHOUT_AGENT_CLIS: &str = "/usr/bin:/bin";

fn seed_run_and_task(git: impl Fn(&[&str]), proj: &Path, ledger: &Path) {
    git(&["branch", "fleet/r1", "main"]);
    let new = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        run_branch: Some("fleet/r1".into()),
        ..Default::default()
    };
    let ticket = proj.join("t1.md");
    let text = std::fs::read_to_string(&ticket).unwrap();
    let facts = ticket_facts(&text);
    let title = facts.title.clone().unwrap_or_else(|| "task".to_string());
    let task = NewTask {
        land_check: facts.check.clone(),
        ticket_snapshot: Some(text.clone()),
        ..NewTask::work(&title, &ticket.to_string_lossy(), "dev")
    };
    let mut store = Store::open(ledger).unwrap();
    store
        .immediate(|tx| {
            let run = runs::create(tx, proj, &new)?;
            schema::run_idea(tx, &run.run_id, &util::now(), true)?;
            tasks::add(tx, &run, &task, Actor::Orchestrator)
        })
        .unwrap();
}

struct Out {
    code: i32,
    out: String,
    err: String,
}

struct Agent(std::process::Child);

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The fleetd drops first: fields drop in order.
struct World {
    _fleetd: Option<TestFleetd>,
    _agent: Option<Agent>,
    _tmp: tempfile::TempDir,
    /// The temporary folder with no symlink in its path, as git and the
    /// ledger write it.
    root: PathBuf,
    worker: String,
}

impl World {
    fn with_a_tree() -> World {
        World::start("")
    }

    /// A rescue waits for a `worktree_list` result with the HEAD of the
    /// tree, so with the default list period of 60 s, [`World::rescued`]
    /// uses most of its 120 s wait (T-290).
    fn with_a_rescuable_tree() -> World {
        World::start("worktree_list_period_s = 5\n")
    }

    fn start(fleet_toml: &str) -> World {
        let tmp = short_tempdir();
        let root = tmp.path().canonicalize().unwrap();
        let mut w = World {
            _fleetd: None,
            _agent: None,
            _tmp: tmp,
            root,
            worker: String::new(),
        };
        let proj = w.proj();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(w.root().join("home")).unwrap();
        w.git(&proj, &["init", "-q", "-b", "main"]);
        std::fs::write(proj.join("a.txt"), "a\n").unwrap();
        std::fs::write(proj.join("t1.md"), "# Ticket One\n\n**Check:** none\n").unwrap();
        w.git(&proj, &["add", "a.txt", "t1.md"]);
        w.git(&proj, &["commit", "-q", "-m", "init"]);
        std::fs::write(w.root().join("machine.json"), HEALTHY_MACHINE).unwrap();
        std::fs::create_dir_all(w.root().join("fleet")).unwrap();
        std::fs::write(
            w.root().join("fleet/fleet.toml"),
            format!(
                "health_max_process_share = 1.0\nhealth_max_open_file_share = 1.0\n{fleet_toml}"
            ),
        )
        .unwrap();
        seed_run_and_task(|args| drop(w.git(&proj, args)), &proj, &w.ledger());
        w._fleetd = Some(TestFleetd::start(w.command(None), &w.root().join("fleet")));
        let worker = NewWorker {
            persona: "dev".into(),
            role: Some("dev1".into()),
            ..Default::default()
        };
        let agent = Command::new("sleep").arg("3600").spawn().unwrap();
        w.worker = support::bind::bind(&w.ledger(), worker, Some(agent.id())).worker_id;
        w._agent = Some(Agent(agent));
        w.wait_for("fleetd to make the tree of T1", |w| {
            let path = w.tree().to_string_lossy().into_owned();
            let store = w.store();
            custody::by_path(store.conn(), &path)
                .unwrap()
                .is_some_and(|r| r.state.as_str() == "active")
        });
        let tree = w.tree();
        std::fs::write(tree.join("u.txt"), "unsaved\n").unwrap();
        std::fs::write(tree.join("a.txt"), "changed\n").unwrap();
        w
    }

    fn wait_for(&self, what: &str, done: impl Fn(&World) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while !done(self) {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for {what}"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    fn wait_restored(&self, tree: &Path) {
        let path = tree.to_string_lossy().into_owned();
        self.wait_for("fleetd to make the restored tree", |w| {
            let store = w.store();
            custody::by_path(store.conn(), &path)
                .unwrap()
                .is_some_and(|r| r.state.as_str() == "created")
        });
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn proj(&self) -> PathBuf {
        self.root().join("proj")
    }

    /// The tree of T1.
    fn tree(&self) -> PathBuf {
        self.proj().join(".worktrees/r1-T1-ticket-one")
    }

    fn ledger(&self) -> PathBuf {
        self.root().join("ledger.sqlite3")
    }

    /// `git <args>` in `dir`, with no config of the operator's.
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_clear()
            .env("PATH", PATH_WITHOUT_AGENT_CLIS)
            .env("HOME", self.root().join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "n")
            .env("GIT_AUTHOR_EMAIL", "a@b")
            .env("GIT_COMMITTER_NAME", "n")
            .env("GIT_COMMITTER_EMAIL", "a@b")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// The `horch` binary with the environment of the test; `worker` sets
    /// `HORCH_WORKER_ID`.
    fn command(&self, worker: Option<&str>) -> Command {
        let root = self.root();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH_WITHOUT_AGENT_CLIS)
            .env("HOME", root.join("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.ledger())
            .env("HORCH_PROJECT_DIR", self.proj())
            .env("HORCH_MACHINE_FILE", root.join("machine.json"))
            .env("FLEET_ROOT", root.join("fleet"))
            .env("HORCH_NO_WATCHDOG", "1");
        if let Some(w) = worker {
            cmd.env("HORCH_WORKER_ID", w);
        }
        cmd
    }

    fn horch(&self, worker: Option<&str>, args: &[&str]) -> Out {
        let o = self.command(worker).args(args).output().unwrap();
        Out {
            code: o.status.code().unwrap(),
            out: String::from_utf8(o.stdout).unwrap(),
            // A host that cannot read a count (a sandbox) gets a guard note.
            err: String::from_utf8(o.stderr)
                .unwrap()
                .lines()
                .filter(|l| !l.starts_with("horch: NOTE: the health guard "))
                .map(|l| format!("{l}\n"))
                .collect(),
        }
    }

    /// Run `args` as the operator, require exit 0, return stdout.
    fn ok(&self, args: &[&str]) -> String {
        let o = self.horch(None, args);
        assert_eq!(o.code, 0, "{args:?}: {}{}", o.out, o.err);
        o.out
    }

    fn store(&self) -> Store {
        Store::open(&self.ledger()).unwrap()
    }

    /// The row of the tree of T1.
    fn row(&self) -> custody::WorktreeRow {
        let store = self.store();
        let path = self.tree().to_string_lossy().into_owned();
        custody::by_path(store.conn(), &path).unwrap().unwrap()
    }

    fn rows(&self) -> i64 {
        self.store()
            .conn()
            .query_row("SELECT COUNT(*) FROM worktrees", [], |r| r.get(0))
            .unwrap()
    }

    /// The facts that the `worktree_list` job of the pass stores.
    fn measure(&self, bytes: i64) {
        let id = self.row().worktree_id;
        self.store()
            .immediate(|tx| custody::set_facts(tx, &id, "head", true, Some(bytes), "now"))
            .unwrap();
    }

    /// The tree of T1 loses its owner, as the reconciler finds it.
    fn orphan(&self) {
        let id = self.row().worktree_id;
        self.store()
            .immediate(|tx| {
                let cause = Cause {
                    actor: Actor::Horch,
                    text: "its attempt crashed",
                };
                transition(
                    tx,
                    None,
                    Entity::Worktree,
                    &id,
                    "orphaned",
                    cause,
                    &NeedsYouList,
                )?;
                Ok(())
            })
            .unwrap();
    }

    /// 1 call of the reconciler with no new fact, as part 2 of a pass:
    /// the id of the rescue job that it queues for the orphan of T1.
    fn reconcile(&self) -> i64 {
        let id = self.row().worktree_id;
        let config = FleetConfig::default();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let report = self
            .store()
            .immediate(|tx| reconcile_worktrees(&ctx, tx, &Facts::default(), FleetSeconds(0)))
            .unwrap();
        assert_eq!(report.rescues, std::slice::from_ref(&id), "{report:?}");
        self.store()
            .conn()
            .query_row(
                "SELECT id FROM jobs WHERE kind = 'rescue' AND subject = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// Orphan the tree of T1, then let the worker take it again: its
    /// rescue job runs first, and the row keeps the rescue ref.
    fn rescued(&self) -> String {
        self.orphan();
        self.wait_for("fleetd to rescue the tree of T1", |w| {
            w.row().rescue_ref.is_some()
        });
        self.row().rescue_ref.expect("the rescue ref of the tree")
    }

    fn rescue_refs(&self) -> String {
        self.git(
            &self.proj(),
            &["for-each-ref", "--format=%(refname)", "refs/horch/rescue/"],
        )
    }

    fn branch_exists(&self, branch: &str) -> bool {
        Command::new("git")
            .args([
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ])
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH_WITHOUT_AGENT_CLIS)
            .env("HOME", self.root().join("home"))
            .status()
            .unwrap()
            .success()
    }
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

/// 1 line for each tree, with a word for each fact, then the total.
#[test]
fn worktrees_lists_each_tree_with_state_task_owner_age_and_size() {
    let w = World::with_a_tree();
    w.measure(2048);
    let attempt = w.row().owner_attempt.expect("the owner attempt");

    let out = w.ok(&["worktrees"]);

    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    let line = lines[0];
    for word in [
        "state=active",
        "status=open",
        "task=T1",
        "run=r1",
        "worker=dev1",
        &format!("owner={attempt}"),
        "size=2.0KB",
        "dirty=1",
    ] {
        assert!(
            line.contains(&format!(" {word} ")) || line.starts_with(&format!("{word} ")),
            "{word}: {line}"
        );
    }
    assert!(line.contains(" age="), "{line}");
    assert!(
        line.ends_with(&format!(" path={}", w.tree().display())),
        "{line}"
    );
    assert_eq!(lines[1], "total: 1 worktree, 2.0 KB on disk");
}

/// RR-10: each orphan on disk is listed with the rescue job that the
/// reconciler queued for it; the listing changes nothing.
#[test]
fn worktrees_shows_the_rescue_job_that_the_reconciler_queued_for_an_orphan() {
    let w = World::with_a_tree();
    w.measure(2048);
    w.orphan();
    let job = w.reconcile();

    let out = w.ok(&["worktrees"]);

    let line = out.lines().next().unwrap();
    assert!(line.starts_with("state=orphaned "), "{line}");
    assert!(
        line.contains(&format!(" jobs=rescue#{job}:queued ")),
        "{line}"
    );
    assert!(!line.contains(" would="), "{line}");
    assert!(
        line.ends_with(&format!(" path={}", w.tree().display())),
        "{line}"
    );
    assert!(
        out.ends_with("total: 1 worktree, 2.0 KB on disk\n"),
        "{out}"
    );
    assert_eq!(w.rescue_refs(), "");
    assert!(w.tree().join("u.txt").is_file());
    assert_eq!(w.row().state, custody::WorktreeState::Orphaned);
    let v = json(&w.ok(&["worktrees", "--json"]));
    assert_eq!(
        v["worktrees"][0]["jobs"],
        serde_json::json!([{"id": job, "kind": "rescue", "state": "queued"}])
    );
}

#[test]
fn worktrees_json_has_each_field() {
    let w = World::with_a_tree();
    w.measure(4096);

    let v = json(&w.ok(&["worktrees", "--json"]));

    let rows = v["worktrees"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{v}");
    let r = &rows[0];
    for key in [
        "worktree_id",
        "path",
        "state",
        "status",
        "task",
        "run",
        "worker",
        "owner_attempt",
        "age_s",
        "bytes_on_disk",
        "dirty",
        "rescue_ref",
        "jobs",
    ] {
        assert!(r.get(key).is_some(), "{key}: {r}");
    }
    assert_eq!(r["state"], "active");
    assert_eq!(r["task"], "T1");
    assert_eq!(r["run"], "r1");
    assert_eq!(r["worker"], "dev1");
    assert_eq!(r["bytes_on_disk"], 4096);
    assert_eq!(r["dirty"], true);
    assert_eq!(r["jobs"], serde_json::json!([]));
    assert_eq!(v["total_bytes"], 4096);
    assert_eq!(v["unmeasured"], 0);
}

#[test]
fn restore_of_a_task_makes_branch_rescue_t_seq_n_and_its_tree_with_each_rescued_file() {
    let w = World::with_a_rescuable_tree();
    let rescue_ref = w.rescued();
    let n = rescue_ref.rsplit('/').next().unwrap();
    let branch = format!("rescue/T1-{n}");
    let tree = w.proj().join(format!(".worktrees/rescue-T1-{n}"));

    let out = w.ok(&["worktree", "restore", "T1"]);

    assert_eq!(
        out,
        format!("restored {rescue_ref} on {branch} at {}\n", tree.display())
    );
    w.wait_restored(&tree);
    assert!(w.branch_exists(&branch));
    let rescued = w.git(
        &w.proj(),
        &["rev-parse", &format!("{rescue_ref}^{{commit}}")],
    );
    let tip = w.git(&w.proj(), &["rev-parse", &branch]);
    assert_eq!(tip, rescued);
    assert_eq!(
        std::fs::read_to_string(tree.join("u.txt")).unwrap(),
        "unsaved\n"
    );
    assert_eq!(
        std::fs::read_to_string(tree.join("a.txt")).unwrap(),
        "changed\n"
    );
    // F2 of review round 1: the tree is for the operator. It is `created`
    // with no owner, which the reconciler leaves as it is, not `active`
    // with no owner, which it orphans.
    let listed = w.ok(&["worktrees"]);
    let line = listed
        .lines()
        .find(|l| l.ends_with(&format!(" path={}", tree.display())))
        .unwrap_or_else(|| panic!("{listed}"));
    assert!(line.starts_with("state=created "), "{line}");
    assert!(
        line.contains(" task=- ") && line.contains(" owner=- "),
        "{line}"
    );
}

/// F1 of review round 1: Phase 3 deletes no rescue ref, so a tree that
/// custody removed still gives its rescue ref back.
#[test]
fn restore_of_a_task_whose_tree_is_removed_uses_its_rescue_ref() {
    let w = World::with_a_rescuable_tree();
    let rescue_ref = w.rescued();
    let id = w.row().worktree_id;
    w.store()
        .conn()
        .execute(
            "UPDATE worktrees SET state = 'removed' WHERE worktree_id = ?1",
            [&id],
        )
        .unwrap();

    let out = w.ok(&["worktree", "restore", "T1"]);

    let tree = w.proj().join(".worktrees/rescue-T1-1");
    assert_eq!(
        out,
        format!(
            "restored {rescue_ref} on rescue/T1-1 at {}\n",
            tree.display()
        )
    );
    w.wait_restored(&tree);
    assert_eq!(
        std::fs::read_to_string(tree.join("u.txt")).unwrap(),
        "unsaved\n"
    );
}

/// The security rule of the orchestrator: no hook of the project runs
/// when restore makes the tree.
#[test]
fn restore_runs_no_hook_of_the_project() {
    let w = World::with_a_rescuable_tree();
    w.rescued();
    let marker = w.root().join("hook-ran");
    let hook = w.proj().join(".git/hooks/post-checkout");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();

    w.ok(&["worktree", "restore", "T1"]);

    w.wait_restored(&w.proj().join(".worktrees/rescue-T1-1"));
    assert!(w.proj().join(".worktrees/rescue-T1-1/u.txt").is_file());
    assert!(
        !marker.exists(),
        "the post-checkout hook of the project ran"
    );
}

#[test]
fn restore_refuses_when_the_branch_or_the_folder_exists_and_changes_nothing() {
    let w = World::with_a_rescuable_tree();
    w.rescued();
    let rows = w.rows();
    let tree = w.proj().join(".worktrees/rescue-T1-1");

    w.git(&w.proj(), &["branch", "rescue/T1-1", "main"]);
    let refused = w.horch(None, &["worktree", "restore", "T1"]);
    assert_eq!(refused.code, 1, "{}", refused.out);
    assert!(
        refused.err.contains("rescue/T1-1 exists"),
        "{}",
        refused.err
    );
    assert!(!tree.exists());
    assert_eq!(w.rows(), rows);
    let main = w.git(&w.proj(), &["rev-parse", "main"]);
    assert_eq!(w.git(&w.proj(), &["rev-parse", "rescue/T1-1"]), main);

    w.git(&w.proj(), &["branch", "-D", "rescue/T1-1"]);
    std::fs::create_dir_all(&tree).unwrap();
    let refused = w.horch(None, &["worktree", "restore", "T1"]);
    assert_eq!(refused.code, 1, "{}", refused.out);
    assert!(
        refused.err.contains(&format!("{} exists", tree.display())),
        "{}",
        refused.err
    );
    assert!(!w.branch_exists("rescue/T1-1"));
    assert_eq!(std::fs::read_dir(&tree).unwrap().count(), 0);
    assert_eq!(w.rows(), rows);
}

#[test]
fn restore_of_an_archive_name_keeps_today_s_behavior() {
    let w = World::with_a_tree();
    let proj = w.proj();
    w.git(&proj, &["branch", "side", "main"]);
    let tip = w.git(&proj, &["rev-parse", "side"]).trim().to_string();
    w.git(&proj, &["update-ref", "refs/horch/archive/old", &tip]);

    let out = w.ok(&["worktree", "restore", "old"]);

    let path = proj.join(".worktrees/old");
    assert_eq!(
        out,
        format!(
            "restored {} on restored/old at {} from refs/horch/archive/old\n",
            path.display(),
            &tip[..7]
        )
    );
    w.wait_restored(&path);
    assert!(path.join("a.txt").is_file());
    assert!(w.branch_exists("restored/old"));
}

/// FM-28: the board shows the total size of all trees, as a line and as a
/// field of `--json`.
#[test]
fn ledger_status_shows_the_total_size_of_all_trees() {
    let w = World::with_a_tree();
    w.measure(3 << 20);

    let board = w.horch(None, &["ledger", "status"]);
    assert!(
        board
            .out
            .contains("\nWorktree disk: 3.0 MB in 1 worktree.\n"),
        "{}",
        board.out
    );
    assert!(
        board.out.ends_with(
            "1 tasks: 0 done, 1 running, 0 landing, 0 queued, 0 cancelled. Not complete.\n"
        ),
        "{}",
        board.out
    );
    let v = json(&w.horch(None, &["ledger", "status", "--json"]).out);
    assert_eq!(v["worktree_disk"]["bytes"], 3 << 20);
    assert_eq!(v["worktree_disk"]["count"], 1);
    assert_eq!(v["worktree_disk"]["unmeasured"], 0);
}
