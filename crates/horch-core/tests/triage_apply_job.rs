//! The triage decisions `ARCHIVE`, `DELETE`, `LAND` and `PR` as 1 fleetd
//! job kind, `triage_apply` (ai_docs/plans/fleetd-build/tickets/
//! 50-triage-decisions-as-fleetd-jobs.md; RR-5, RR-13, SR-3).
//!
//! Each test sends 1 `worktree_triage` request through `requests::handle`
//! to a temp ledger at generation 1. The handler queues 1 `triage_apply`
//! job. The test starts the job process as `true` through the launcher,
//! runs the executor on a read-only connection, as `horch job run` runs it,
//! and ends the run from its result file, as a pass of fleetd ends it.
//! Git is the host git on a temp repository. `gh` is a fake script. No
//! test calls the real `gh` or a real remote.

#[path = "common/warm.rs"]
mod warm;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Once};

use anyhow::Result;
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Request};
use horch_core::fleetd::fleet_runtime::application::handlers_custody::{register, Custody};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, JobCtx, JobRunner, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind as JobKind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::application::commands::RefTips;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::triage::{self, Measure, Row};
use horch_core::taskledger::workers::Worker;
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

/// The decisions of this file read no ref through the handler.
struct NoRefs;

impl RefTips for NoRefs {
    fn tip(&self, _repo: &Path, _name: &str) -> Result<Option<String>> {
        Ok(None)
    }
}

/// No worker of a test has a sandbox profile.
struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

/// The fake `gh` of this test binary: `HORCH_GH_BIN` names it, so no test
/// can reach the real `gh`. It appends its arguments to `../gh.log` from
/// its folder (the main checkout), and prints the URL of a pull request.
fn fake_gh() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("triage-apply-job-gh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("gh");
        let script = "#!/bin/sh\n\
            [ \"$1\" = --version ] && { echo 'gh version 2.0.0'; exit 0; }\n\
            echo \"$*\" >> ../gh.log\n\
            echo https://example.invalid/pull/7\n";
        let out = warm::write_warmed(&bin, script, &["--version"]);
        assert!(out.status.success());
        std::env::set_var("HORCH_GH_BIN", &bin);
    });
}

/// Run the host git in `dir`. A failure fails the test.
fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
        .status
        .success()
}

fn write(dir: &Path, file: &str, body: &str) {
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn commit(dir: &Path, file: &str, body: &str) -> String {
    write(dir, file, body);
    git(dir, &["add", file]);
    git(dir, &["commit", "--quiet", "-m", file]);
    git(dir, &["rev-parse", "HEAD"])
}

/// A repository on `main` with 1 commit, a temp ledger at generation 1
/// beside it, and the handlers of the custody kinds.
struct World {
    _tmp: tempfile::TempDir,
    top: PathBuf,
    root: PathBuf,
    conn: Connection,
    handlers: Handlers,
    config: FleetConfig,
}

fn world() -> World {
    fake_gh();
    let tmp = tempfile::tempdir().unwrap();
    let top = tmp.path().canonicalize().unwrap();
    let root = top.join("repo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "--quiet", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&root, &["config", k, v]);
    }
    write(&root, ".gitignore", "target/\n");
    git(&root, &["add", ".gitignore"]);
    commit(&root, "README.md", "base\n");
    let db = top.join("ledger.sqlite3");
    let mut store = Store::open(&db).unwrap();
    store
        .immediate(|tx| {
            tx.execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
                params![std::process::id(), util::now()],
            )?;
            Ok(())
        })
        .unwrap();
    drop(store);
    let conn = Connection::open(&db).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    let mut handlers = Handlers::new();
    register(
        &mut handlers,
        Arc::new(Custody {
            guard: Generation(1),
            config: FleetConfig::default(),
            refs: Box::new(NoRefs),
        }),
    );
    World {
        _tmp: tmp,
        top,
        root,
        conn,
        handlers,
        config: FleetConfig::default(),
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl World {
    /// A linked worktree `.worktrees/<name>` on a new branch `name` from
    /// `main`, with 1 commit of its own.
    fn tree(&self, name: &str) -> PathBuf {
        let path = self.root.join(".worktrees").join(name);
        git(
            &self.root,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                name,
                path.to_str().unwrap(),
                "main",
            ],
        );
        commit(&path, &format!("{name}.txt"), "work\n");
        path
    }

    fn rows(&self) -> Vec<Row> {
        triage::collect(&self.conn, &Git::new("git"), &self.root, Measure::Ids).unwrap()
    }

    /// The `worktree_triage` request of `decision` on the row of `name`,
    /// as the command sends it.
    fn send(&mut self, name: &str, decision: &str, direction: &str) -> Outcome {
        self.send_all(&[(name, decision, direction)])
    }

    fn send_all(&mut self, items: &[(&str, &str, &str)]) -> Outcome {
        let rows = self.rows();
        let decisions: Vec<Value> = items
            .iter()
            .map(|&(name, decision, direction)| {
                let row = rows
                    .iter()
                    .find(|r| r.name == name)
                    .unwrap_or_else(|| panic!("no triage row {name}"));
                json!({
                    "row": row,
                    "decision": decision,
                    "direction": (!direction.is_empty()).then_some(direction),
                    "line": 3,
                    "ticket": null,
                })
            })
            .collect();
        let payload = json!({
            "root": self.root,
            "primary": "opus",
            "decisions": decisions,
        });
        let n = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        let Value::Object(payload) = payload else {
            unreachable!()
        };
        let req = Request {
            request_id: RequestId::parse(&format!("7a2c2a3e-0b9d-4c55-8e7f-{n:012x}")).unwrap(),
            kind: Kind::WorktreeTriage,
            payload,
            token: Some("t".into()),
        };
        let decision = Decision::Authorized {
            auth: "token; channel".into(),
        };
        handle(
            &mut self.conn,
            Generation(1),
            &req,
            Some("me"),
            &decision,
            &self.handlers,
        )
        .unwrap()
        .outcome
    }

    /// Send `decision` on `name`, then run its job to its end. Give the
    /// ended job.
    fn decide(&mut self, name: &str, decision: &str, direction: &str) -> JobRow {
        let result = match self.send(name, decision, direction) {
            Outcome::Applied(v) => v,
            Outcome::Refused(why) => panic!("refused: {why}"),
        };
        let id = result["job"]
            .as_i64()
            .unwrap_or_else(|| panic!("no job in {result}"));
        self.run(id)
    }

    /// Start the job `id` as `true`, run its executor on a read-only
    /// connection, then end it from its result file.
    fn run(&mut self, id: i64) -> JobRow {
        self.run_editing(id, |_| {})
    }

    fn run_editing(&mut self, id: i64, edit: impl FnOnce(&mut Value)) -> JobRow {
        let job = jobs_store::get(&self.conn, id).unwrap().unwrap();
        assert_eq!(job.kind, JobKind::TriageApply);
        let spec = LaunchSpec {
            program: "true".into(),
            args: Vec::new(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        let config = self.config.clone();
        let ctx = JobCtx {
            guard: Some(Generation(1)),
            config: &config,
            needs_you: &NeedsYouList,
        };
        JobRunner::new(&Launcher)
            .start(&ctx, &mut self.conn, &job, &spec, FleetSeconds(0))
            .unwrap();
        let pid = jobs_store::get(&self.conn, id)
            .unwrap()
            .unwrap()
            .pid
            .unwrap();
        let before = counts(&self.conn);
        let read_only = Store::open_read_only(&self.top.join("ledger.sqlite3")).unwrap();
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
            sweep: None,
        };
        jobs::run_job(&ctx, read_only.conn(), id, pid, &checks).unwrap();
        drop(read_only);
        assert_eq!(counts(&self.conn), before, "the job process writes no row");
        let run = jobs_store::get(&self.conn, id).unwrap().unwrap().run;
        let file = self
            .top
            .join("fleet/run/jobs")
            .join(format!("{id}-{run}.json"));
        let mut result: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        edit(&mut result["data"]);
        std::fs::write(&file, serde_json::to_vec(&result).unwrap()).unwrap();
        jobs::end_from_files(&ctx, &mut self.conn).unwrap();
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    /// The columns `cols` of the newest ledger row of the folder `path`.
    fn tree_row(&self, path: &Path, cols: &str) -> Vec<Option<String>> {
        let n = cols.split(',').count();
        self.conn
            .query_row(
                &format!(
                    "SELECT {cols} FROM worktrees WHERE path = ?1 ORDER BY rowid DESC LIMIT 1"
                ),
                [path.to_string_lossy()],
                |r| (0..n).map(|i| r.get::<_, Option<String>>(i)).collect(),
            )
            .unwrap()
    }

    fn has_ref(&self, name: &str) -> bool {
        git_ok(&self.root, &["show-ref", "--verify", "--quiet", name])
    }

    fn refs(&self, prefix: &str) -> Vec<String> {
        let out = git(&self.root, &["for-each-ref", "--format=%(refname)", prefix]);
        out.lines().map(str::to_string).collect()
    }
}

/// The count of rows of each table that a decision changes.
fn counts(conn: &Connection) -> Vec<(i64, i64)> {
    ["worktrees", "events", "jobs", "needs_you"]
        .iter()
        .map(|t| {
            conn.query_row(
                &format!("SELECT COUNT(*), COALESCE(SUM(length(CAST(rowid AS TEXT))), 0) FROM {t}"),
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        })
        .collect()
}

fn text(job: &JobRow) -> String {
    jobs::outcome(job).map(|o| o.text).unwrap_or_default()
}

// ─── the 4 decisions ────────────────────────────────────────────────────

#[test]
fn archive_through_the_request_the_job_and_the_ingest() {
    let mut w = world();
    let tree = w.tree("feat");
    write(&tree, "notes.txt", "untracked\n");
    let job = w.decide("feat", "ARCHIVE", "");
    assert_eq!(job.state, State::Passed, "{}", text(&job));
    assert!(!tree.exists(), "the folder is removed");
    assert!(w.has_ref("refs/horch/archive/feat"));
    assert!(git_ok(
        &w.root,
        &["rev-parse", "--verify", "refs/heads/feat"]
    ));
    let archived = git(&w.root, &["rev-parse", "refs/horch/archive/feat"]);
    assert_eq!(
        git(&w.root, &["show", &format!("{archived}:notes.txt")]),
        "untracked"
    );
    let row = w.tree_row(&tree, "decision,status,state,archive_ref,rescue_ref");
    assert_eq!(row[0].as_deref(), Some("ARCHIVE"));
    assert_eq!(row[1].as_deref(), Some("archived"));
    assert_eq!(row[2].as_deref(), Some("removed"));
    assert_eq!(row[3].as_deref(), Some("refs/horch/archive/feat"));
    let rescue = row[4].clone().expect("a rescue ref before the removal");
    assert!(w.has_ref(&rescue), "{rescue}");
}

#[test]
fn delete_through_the_request_the_job_and_the_ingest() {
    let mut w = world();
    let tree = w.tree("gone");
    let tip = git(&tree, &["rev-parse", "HEAD"]);
    let job = w.decide("gone", "DELETE", "");
    assert_eq!(job.state, State::Passed, "{}", text(&job));
    assert!(!tree.exists());
    assert!(!git_ok(
        &w.root,
        &["rev-parse", "--verify", "refs/heads/gone"]
    ));
    let trash = w.refs("refs/horch/trash/");
    assert_eq!(trash.len(), 1, "{trash:?}");
    assert_eq!(git(&w.root, &["rev-parse", &trash[0]]), tip);
    let row = w.tree_row(&tree, "decision,status,state,archive_ref,rescue_ref");
    assert_eq!(row[0].as_deref(), Some("DELETE"));
    assert_eq!(row[1].as_deref(), Some("trashed"));
    assert_eq!(row[2].as_deref(), Some("removed"));
    assert_eq!(row[3].as_deref(), Some(trash[0].as_str()));
    assert!(row[4].is_some(), "a rescue ref before the removal");
}

#[test]
fn land_through_the_request_the_job_and_the_ingest() {
    let mut w = world();
    let main_before = git(&w.root, &["rev-parse", "main"]);
    let tree = w.tree("lnd");
    commit(&w.root, "other.txt", "main moves\n");
    let main_tip = git(&w.root, &["rev-parse", "main"]);
    let job = w.decide("lnd", "LAND", "");
    assert_eq!(job.state, State::Passed, "{}", text(&job));
    assert_ne!(main_before, main_tip);
    assert_eq!(
        git(&w.root, &["rev-parse", "main"]),
        main_tip,
        "main does not move"
    );
    let landed = git(&w.root, &["rev-parse", "land/lnd"]);
    assert_eq!(git(&w.root, &["rev-parse", "land/lnd^"]), main_tip);
    assert_eq!(
        git(&w.root, &["show", &format!("{landed}:lnd.txt")]),
        "work"
    );
    assert!(!tree.exists());
    let row = w.tree_row(&tree, "decision,state,rescue_ref");
    assert_eq!(row[0].as_deref(), Some("LAND"));
    assert_eq!(row[1].as_deref(), Some("removed"));
    assert!(row[2].is_some(), "a rescue ref before the removal");
}

#[test]
fn pr_through_the_request_the_job_and_the_ingest_with_a_fake_gh() {
    let mut w = world();
    let remote = w.top.join("remote.git");
    git(
        &w.top,
        &[
            "init",
            "--quiet",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    git(
        &w.root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    let tree = w.tree("prb");
    let tip = git(&tree, &["rev-parse", "HEAD"]);
    let job = w.decide("prb", "PR", "Add the prb work");
    assert_eq!(job.state, State::Passed, "{}", text(&job));
    assert_eq!(git(&remote, &["rev-parse", "refs/heads/prb"]), tip);
    let log = std::fs::read_to_string(w.top.join("gh.log")).unwrap();
    assert!(
        log.contains("pr create --head prb --title Add the prb work"),
        "{log}"
    );
    assert!(!tree.exists());
    assert!(w.has_ref("refs/horch/archive/prb"));
    let row = w.tree_row(&tree, "decision,status,state");
    assert_eq!(row[0].as_deref(), Some("PR"));
    assert_eq!(row[1].as_deref(), Some("archived"));
    assert_eq!(row[2].as_deref(), Some("removed"));
}

// ─── the refusals of RR-13 ──────────────────────────────────────────────

/// A refused decision ends its job `passed` with the decision failed: the
/// row is `cleanup_failed` with the reason and no decision, and the folder,
/// the branch and the refs stay.
fn assert_refused(w: &World, tree: &Path, job: &JobRow, why: &str) {
    assert_eq!(job.state, State::Passed, "{}", text(job));
    assert!(text(job).contains(why), "{}", text(job));
    assert!(tree.exists(), "the folder stays");
    assert!(w.refs("refs/horch/rescue/").is_empty(), "no rescue ref");
    assert!(w.refs("refs/horch/archive/").is_empty(), "no archive ref");
    let row = w.tree_row(tree, "decision,status,reason");
    assert_eq!(row[0], None);
    assert_eq!(row[1].as_deref(), Some("cleanup_failed"));
    assert!(row[2].as_deref().unwrap_or("").contains(why), "{row:?}");
}

#[test]
fn a_dirty_tree_is_refused_and_stays() {
    let mut w = world();
    let tree = w.tree("dirty");
    git(&tree, &["checkout", "--quiet", "--detach"]);
    write(&tree, "dirty.txt", "uncommitted\n");
    let job = w.decide("dirty", "ARCHIVE", "");
    assert_refused(&w, &tree, &job, "uncommitted files");
    assert!(tree.join("dirty.txt").exists());
}

#[test]
fn a_branch_that_a_stash_names_is_refused_and_stays() {
    let mut w = world();
    let tree = w.tree("stashed");
    write(&tree, "in-stash.txt", "work\n");
    git(&tree, &["add", "in-stash.txt"]);
    git(&tree, &["stash", "push", "--quiet", "-m", "keep"]);
    assert!(git(&w.root, &["stash", "list"]).contains("On stashed: keep"));
    let job = w.decide("stashed", "DELETE", "");
    assert_eq!(job.state, State::Passed, "{}", text(&job));
    assert!(
        text(&job).contains("a stash names stashed"),
        "{}",
        text(&job)
    );
    assert!(tree.exists(), "the folder stays");
    assert!(git_ok(
        &w.root,
        &["rev-parse", "--verify", "refs/heads/stashed"]
    ));
    assert!(w.refs("refs/horch/rescue/").is_empty(), "no rescue ref");
    let row = w.tree_row(&tree, "decision,status");
    assert_eq!(row[0], None);
    assert_eq!(row[1].as_deref(), Some("cleanup_failed"));
}

#[test]
fn unpublished_unique_commits_are_refused_and_stay() {
    let mut w = world();
    let tree = w.tree("unique");
    git(&tree, &["checkout", "--quiet", "--detach"]);
    commit(&tree, "only-here.txt", "unique\n");
    let job = w.decide("unique", "ARCHIVE", "");
    assert_refused(&w, &tree, &job, "unique commit");
}

#[test]
fn a_moved_head_is_refused_and_stays() {
    let mut w = world();
    let tree = w.tree("moved");
    // The triage row records the branch moved; then the HEAD of the folder
    // moves to another branch before the job runs.
    let row = w.rows().into_iter().find(|r| r.name == "moved").unwrap();
    assert_eq!(row.branch.as_deref(), Some("moved"));
    let result = match w.send("moved", "ARCHIVE", "") {
        Outcome::Applied(v) => v,
        Outcome::Refused(why) => panic!("refused: {why}"),
    };
    git(&tree, &["checkout", "--quiet", "-b", "elsewhere"]);
    let job = w.run(result["job"].as_i64().unwrap());
    assert_refused(&w, &tree, &job, "not its recorded branch moved");
}

/// A removal that fails after the rescue keeps the verified rescue ref:
/// the failed fact names it, and the ingest moves the row to `rescued`
/// with it before it records the failure (T-887). git refuses to remove a
/// locked tree.
#[test]
fn a_failed_removal_after_the_rescue_keeps_the_rescue_ref() {
    let mut w = world();
    let tree = w.tree("locked");
    write(&tree, "notes.txt", "untracked\n");
    git(&w.root, &["worktree", "lock", &tree.to_string_lossy()]);
    let job = w.decide("locked", "DELETE", "");
    assert_eq!(job.state, State::Passed, "{}", text(&job));
    assert!(text(&job).contains("DELETE FAILED"), "{}", text(&job));
    assert!(tree.join("notes.txt").exists(), "the folder stays");
    let rescue = w.refs("refs/horch/rescue/");
    assert_eq!(rescue.len(), 1, "{rescue:?}");
    let row = w.tree_row(&tree, "state,rescue_ref,status,decision");
    assert_eq!(row[0].as_deref(), Some("rescued"));
    assert_eq!(row[1].as_deref(), Some(rescue[0].as_str()));
    assert_eq!(row[2].as_deref(), Some("cleanup_failed"));
    assert_eq!(row[3], None);
}

#[test]
fn refused_second_item_fact_does_not_roll_back_first_item_completed_rescue_and_removal() {
    let mut w = world();
    let tree1 = w.tree("tree1");
    let tree2 = w.tree("tree2");
    let result = match w.send_all(&[("tree1", "ARCHIVE", ""), ("tree2", "ARCHIVE", "")]) {
        Outcome::Applied(v) => v,
        Outcome::Refused(why) => panic!("refused: {why}"),
    };
    let job = w.run_editing(result["job"].as_i64().unwrap(), |facts| {
        assert_eq!(facts["decisions"][0]["ok"], json!(true), "{facts}");
        assert_eq!(facts["decisions"][0]["removed"], json!(true), "{facts}");
        facts["decisions"][1]["archive_ref"] = json!("refs/heads/main");
    });
    assert_eq!(job.state, State::Passed, "{}", text(&job));
    assert!(!tree1.exists(), "the folder of item 1 is removed");
    let row = w.tree_row(&tree1, "state,rescue_ref,status,decision");
    assert_eq!(row[0].as_deref(), Some("removed"));
    let rescue = row[1].clone().expect("the verified rescue ref of item 1");
    assert!(w.has_ref(&rescue), "{rescue}");
    assert_eq!(row[2].as_deref(), Some("archived"));
    assert_eq!(row[3].as_deref(), Some("ARCHIVE"));
    let row = w.tree_row(&tree2, "status,decision,reason");
    assert_eq!(row[0].as_deref(), Some("cleanup_failed"));
    assert_eq!(row[1], None);
    let reason = row[2].as_deref().unwrap_or("");
    assert!(reason.contains("fleetd refused the facts"), "{reason}");
    assert!(reason.contains("refs/heads/main"), "{reason}");
}

// ─── the job input ──────────────────────────────────────────────────────

#[test]
fn a_reply_of_keep_only_queues_no_job() {
    let mut w = world();
    let tree = w.tree("kept");
    let result = match w.send("kept", "KEEP", "") {
        Outcome::Applied(v) => v,
        Outcome::Refused(why) => panic!("refused: {why}"),
    };
    assert!(result["job"].is_null(), "{result}");
    let jobs: i64 = w
        .conn
        .query_row(
            "SELECT COUNT(*) FROM jobs WHERE kind = 'triage_apply'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(jobs, 0);
    assert_eq!(w.tree_row(&tree, "decision")[0].as_deref(), Some("KEEP"));
}

// ─── the schema step ────────────────────────────────────────────────────

/// The step of Phase 5 builds `jobs` again: a ledger before it refuses a
/// `triage_apply` row; after it, each job row keeps its id and columns, the
/// kind is allowed, and the index of 1 open run of 1 kind and subject holds.
#[test]
fn phase_5_step_adds_the_triage_apply_kind_and_keeps_each_job_row() {
    use horch_core::taskledger::schema;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("ledger.sqlite3");
    let mut conn = Connection::open(&path).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_5_VERSION - 1).unwrap();
    let insert = |conn: &Connection, id: i64, kind: &str, subject: &str| {
        conn.execute(
            "INSERT INTO jobs (id, kind, subject, input_key, state, time_limit_s, created_at)
             VALUES (?1, ?2, ?3, 'k' || ?1, 'queued', 60, 'now')",
            params![id, kind, subject],
        )
    };
    insert(&conn, 7, "rescue", "wt1").unwrap();
    assert!(insert(&conn, 8, "triage_apply", "/r").is_err());
    let row = |conn: &Connection| -> Vec<String> {
        conn.query_row("SELECT * FROM jobs WHERE id = 7", [], |r| {
            (0..r.as_ref().column_count())
                .map(|i| {
                    r.get::<_, rusqlite::types::Value>(i)
                        .map(|v| format!("{v:?}"))
                })
                .collect()
        })
        .unwrap()
    };
    let before = row(&conn);
    drop(conn);
    let mut conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_5_VERSION).unwrap();
    assert_eq!(row(&conn), before);
    insert(&conn, 8, "triage_apply", "/r").unwrap();
    assert!(
        insert(&conn, 9, "triage_apply", "/r").is_err(),
        "1 open run of 1 kind and subject"
    );
}
