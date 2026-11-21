//! `horch feature abandon <feature>`: fleetd writes a verified rescue ref
//! of each branch of the feature that is not landed, moves each such branch
//! to `abandoned`, cancels each task of the run that is not terminal (a
//! live attempt is stopped first), and moves the feature to `abandoned`
//! (docs/specs/fleetd/design.md, "The feature state machine", "A cancel",
//! I10, FM-43; docs/tickets/fleetd-p4/40-horch-feature-abandon.md).
//!
//! Each test has its own temporary ledger, fleet root and git repository.
//! No test reads the load, the limits or the clock of the machine: the fleet
//! clock is the `fleetd` row of the temp ledger, the config is the default,
//! and a recorded process is a number with the fact that the test gives.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{bail, Result};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Request};
use horch_core::fleetd::fleet_runtime::application::handlers_custody::{register, Custody};
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::integration::application::commands::{self, FeatureAbandoned, GitRefTips};
use horch_core::fleetd::integration::application::landing::{
    self, BranchRescues, GitBranchRescues,
};
use horch_core::fleetd::task_execution::application::cancel::{self, Cancel};
use horch_core::fleetd::task_execution::domain::task::{Facts, Recorded, Seen};
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::testing::bind_live;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

const FEATURE: &str = "F-0001";
const RUN_BRANCH: &str = "fleet/r1";
const RESCUE_PREFIX: &str = "refs/horch/rescue/branch/";
const WHY: &str = "the operator abandons the feature";

/// The recorded agent of the worker of the live attempt. No process of
/// this machine is read: the test gives what the pass sees of it.
const AGENT: Recorded = Recorded {
    pid: 4_000_001,
    pid_start: Some(7),
};

struct World {
    _temp: tempfile::TempDir,
    dir: PathBuf,
    repo: PathBuf,
    db: PathBuf,
    store: Store,
    run: Run,
    config: FleetConfig,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A temp repository with `main` and the run branch, a temp ledger with the
/// fleet clock, 1 run and its feature `F-0001` (Q5: the run rowid is 1).
fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = horch_platform::path::canonicalize(temp.path()).unwrap();
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "--quiet", "-m", "base"]);
    git(&repo, &["branch", RUN_BRANCH]);
    let db = dir.join("ledger.sqlite3");
    let mut store = Store::open(&db).unwrap();
    let run = store
        .immediate(|tx| {
            tx.execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, ?1, 0, ?2, 7, ?2)",
                params![std::process::id(), util::now()],
            )?;
            let new_run = NewRun {
                slug: "r1".into(),
                base_branch: Some("main".into()),
                run_branch: Some(RUN_BRANCH.into()),
                ..Default::default()
            };
            let run = runs::create(tx, &repo, &new_run)?;
            let new = NewFeature {
                id: FEATURE,
                name: Some("r1"),
                branch: Some(RUN_BRANCH),
                primary_branch: "main",
            };
            let cause = Cause {
                actor: Actor::Horch,
                text: "feature accepted",
            };
            rows::insert_feature(tx, None, &new, cause)?;
            // FR-12: a task needs an idea in flight of the feature of its run.
            horch_core::taskledger::schema::run_idea(tx, &run.run_id, &horch_core::taskledger::util::now(), true)?;
            Ok(run)
        })
        .unwrap();
    World {
        _temp: temp,
        dir,
        repo,
        db,
        store,
        run,
        config: FleetConfig::default(),
    }
}

/// A rescue port that dies after `left` rescues: fleetd dies in the middle
/// of step 1.
struct DiesAfter<'a> {
    git: GitBranchRescues<'a>,
    left: RefCell<usize>,
}

impl BranchRescues for DiesAfter<'_> {
    fn rescue(&self, repo: &Path, branch: &str) -> Result<Option<String>> {
        let mut left = self.left.borrow_mut();
        if *left == 0 {
            bail!("fleetd died before the rescue of {branch}");
        }
        *left -= 1;
        self.git.rescue(repo, branch)
    }
}

/// A rescue port that records the state of the branch, of the feature and
/// of the task of the branch at each rescue: nothing moves before its
/// rescue ref exists.
struct SeenRescues<'a> {
    git: GitBranchRescues<'a>,
    db: PathBuf,
    seen: RefCell<Vec<(String, String, String)>>,
}

impl BranchRescues for SeenRescues<'_> {
    fn rescue(&self, repo: &Path, branch: &str) -> Result<Option<String>> {
        let conn = Connection::open(&self.db)?;
        let row = rows::branch(&conn, branch)?.unwrap();
        let feature = rows::feature(&conn, FEATURE)?.unwrap().state;
        let task = tasks::get(&conn, row.task_id.as_deref().unwrap())?.status;
        self.seen
            .borrow_mut()
            .push((row.landing_state.unwrap(), feature, task));
        self.git.rescue(repo, branch)
    }
}

impl World {
    fn git_port(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.dir.join("fleet"))
            .reading(std::slice::from_ref(&self.repo))
            .writing(&[self.repo.join(".git")])
    }

    /// A new task `title` of `persona` with its branch row in `building`,
    /// and the branch in git with 1 commit of its own. Gives (task, branch,
    /// tip).
    fn task(&mut self, title: &str, persona: &str) -> (String, String, String) {
        let branch = format!("{RUN_BRANCH}-{title}");
        let new = NewTask::work(title, &format!("tickets/{title}.md"), persona);
        let (run, config) = (self.run.clone(), self.config.clone());
        let task = self
            .store
            .immediate(|tx| {
                let task = tasks::add(tx, &run, &new, Actor::Orchestrator)?.task_id;
                let ctx = JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                };
                landing::bind_branch(&ctx, tx, &task, &branch, FleetSeconds(0))?;
                Ok(task)
            })
            .unwrap();
        git(
            &self.repo,
            &["checkout", "--quiet", "-b", &branch, RUN_BRANCH],
        );
        std::fs::write(self.repo.join(format!("{title}.txt")), "work\n").unwrap();
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "--quiet", "-m", title]);
        git(&self.repo, &["checkout", "--quiet", "main"]);
        let tip = git(&self.repo, &["rev-parse", &branch]);
        (task, branch, tip)
    }

    /// The branch row moves along `path` through the 1 door.
    fn walk(&mut self, branch: &str, path: &[&str]) {
        let config = self.config.clone();
        self.store
            .immediate(|tx| {
                let ctx = JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                };
                for to in path {
                    let to = landing::state_named(to)?;
                    landing::enter(&ctx, tx, branch, to, FleetSeconds(1), "test move")?;
                }
                Ok(())
            })
            .unwrap();
    }

    fn set_status(&self, task: &str, status: &str) {
        self.store
            .conn()
            .execute(
                "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
                params![task, status],
            )
            .unwrap();
    }

    /// A task whose branch is `landed` and which is `done`.
    fn landed(&mut self, title: &str) -> (String, String, String) {
        let (task, branch, tip) = self.task(title, "dev");
        self.walk(&branch, &["verifying", "ready", "in_bundle", "landed"]);
        self.set_status(&task, "done");
        (task, branch, tip)
    }

    /// The ready task of `persona` is bound to a worker of `persona`: 1
    /// live attempt. Gives the worker id.
    fn claimed(&mut self, persona: &str) -> String {
        let new = NewWorker {
            run_id: self.run.run_id.clone(),
            persona: persona.into(),
            pane_id: Some("w0:p2".into()),
            ..Default::default()
        };
        let conn = self.store.conn();
        let task: String = conn
            .query_row(
                "SELECT task_id FROM tasks WHERE run_id = ?1 AND status = 'ready'
                 AND claimable_by = ?2 ORDER BY seq LIMIT 1",
                [&self.run.run_id, &new.persona],
                |r| r.get(0),
            )
            .unwrap();
        // The test made the branch of the task itself: the bind reserves
        // no second tree and branch.
        conn.execute(
            "UPDATE tasks SET worktree_mode = 'shared' WHERE task_id = ?1",
            [&task],
        )
        .unwrap();
        bind_live(conn, &task, &new, Some(AGENT)).unwrap().worker_id
    }

    fn abandon(&mut self, port: &dyn BranchRescues) -> Result<FeatureAbandoned> {
        let (repo, config) = (self.repo.clone(), self.config.clone());
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        commands::abandon_feature(&mut self.store, &ctx, port, &repo, FEATURE, WHY)
    }

    fn state(&self, branch: &str) -> String {
        rows::branch(self.store.conn(), branch)
            .unwrap()
            .unwrap()
            .landing_state
            .unwrap()
    }

    fn rescue_ref(&self, branch: &str) -> Option<String> {
        rows::branch(self.store.conn(), branch)
            .unwrap()
            .unwrap()
            .rescue_ref
    }

    fn feature_state(&self) -> String {
        rows::feature(self.store.conn(), FEATURE)
            .unwrap()
            .unwrap()
            .state
    }

    fn task_status(&self, task: &str) -> String {
        tasks::get(self.store.conn(), task).unwrap().status
    }

    /// The rescue refs in git.
    fn rescue_refs(&self) -> Vec<String> {
        git(
            &self.repo,
            &["for-each-ref", "--format=%(refname)", RESCUE_PREFIX],
        )
        .lines()
        .map(str::to_string)
        .collect()
    }

    /// `branch` moved to `abandoned` 1 time, and its rescue ref keeps
    /// `tip`.
    fn assert_abandoned_once(&self, branch: &str, tip: &str) {
        assert_eq!(self.state(branch), "abandoned");
        assert_eq!(self.moves("task_branch", branch, "abandoned"), 1);
        let rescue = self.rescue_ref(branch).unwrap();
        assert_eq!(git(&self.repo, &["rev-parse", &rescue]), tip);
    }

    /// The count of moves of the `entity` `id` to `to`.
    fn moves(&self, entity: &str, id: &str, to: &str) -> i64 {
        self.store
            .conn()
            .query_row(
                "SELECT count(*) FROM events WHERE entity = ?1 AND entity_id = ?2
                 AND kind = 'transition' AND to_state = ?3",
                params![entity, id, to],
                |r| r.get(0),
            )
            .unwrap()
    }
}

/// What the pass sees of the agent of the live attempt.
fn facts(seen: Seen) -> Facts {
    let mut f = Facts::default();
    f.processes.insert(AGENT, seen);
    f
}

#[test]
fn abandon_rescues_each_unlanded_branch_first() {
    let mut w = world();
    let (a_task, a, a_tip) = w.task("a", "dev");
    let (_, b, b_tip) = w.task("b", "dev");
    w.walk(&b, &["verifying", "ready", "in_bundle", "conflicted"]);
    let git_port = w.git_port();
    let port = SeenRescues {
        git: GitBranchRescues { git: &git_port },
        db: w.db.clone(),
        seen: RefCell::default(),
    };

    let out = w.abandon(&port).unwrap();

    let open = || ("open".to_string(), "ready".to_string());
    let seen = port.seen.into_inner();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!(
        (seen[0].0.as_str(), seen[1].0.as_str()),
        ("building", "conflicted")
    );
    for (_, feature, task) in &seen {
        assert_eq!(
            (feature.clone(), task.clone()),
            open(),
            "nothing moved before a rescue"
        );
    }
    let refs = w.rescue_refs();
    assert_eq!(refs.len(), 2, "{refs:?}");
    for (branch, tip) in [(&a, &a_tip), (&b, &b_tip)] {
        assert_eq!(w.state(branch), "abandoned");
        let rescue = w.rescue_ref(branch).unwrap();
        assert_eq!(git(&w.repo, &["rev-parse", &rescue]), *tip, "{branch}");
        assert_eq!(git(&w.repo, &["rev-parse", branch]), *tip, "the tip stays");
    }
    assert_eq!(w.feature_state(), "abandoned");
    assert_eq!(w.task_status(&a_task), "cancelled");
    assert!(!out.already);
    let mut names: Vec<&str> = out.branches.iter().map(|b| b.branch.as_str()).collect();
    names.sort();
    assert_eq!(names, [a.as_str(), b.as_str()]);

    // A second abandon moves nothing and writes no second ref.
    let again = w.abandon(&GitBranchRescues { git: &git_port }).unwrap();
    assert!(again.already);
    assert!(again.branches.is_empty());
    assert_eq!(w.rescue_refs(), refs);
    assert_eq!(w.moves("feature", FEATURE, "abandoned"), 1);
    assert_eq!(w.moves("task_branch", &a, "abandoned"), 1);
}

#[test]
fn abandon_cancels_each_open_task_after_its_stop() {
    let mut w = world();
    let (live, live_branch, live_tip) = w.task("live", "live");
    let worker = w.claimed("live");
    let (idle, _, _) = w.task("idle", "dev");
    let (done, _, _) = w.landed("done");
    let git_port = w.git_port();

    let out = w.abandon(&GitBranchRescues { git: &git_port }).unwrap();

    // The task with a live attempt waits for the stop of its process; the
    // other open task is cancelled at once; a done task stays done.
    assert_eq!(w.task_status(&live), "running", "no cancel before the stop");
    assert!(tasks::cancel_requested(w.store.conn(), &live)
        .unwrap()
        .is_some());
    assert_eq!(w.task_status(&idle), "cancelled");
    assert_eq!(w.task_status(&done), "done");
    assert_eq!(out.to_stop(), std::slice::from_ref(&worker));
    let stopping: Vec<&str> = out
        .cancels
        .iter()
        .filter(|c| matches!(c, Cancel::Stopping { .. }))
        .map(|c| c.task().task_id.as_str())
        .collect();
    assert_eq!(stopping, [live.as_str()]);
    assert_eq!(w.feature_state(), "abandoned");
    let rescue = w.rescue_ref(&live_branch).unwrap();
    assert_eq!(git(&w.repo, &["rev-parse", &rescue]), live_tip);

    // The process still runs: the pass does not cancel the task.
    let moved = w
        .store
        .immediate(|tx| cancel::complete_cancels(tx, &facts(Seen::Alive)))
        .unwrap();
    assert!(moved.is_empty());
    assert_eq!(w.task_status(&live), "running");

    // The process is gone: the pass cancels the task 1 time.
    let moved = w
        .store
        .immediate(|tx| cancel::complete_cancels(tx, &facts(Seen::Dead)))
        .unwrap();
    assert_eq!(moved, std::slice::from_ref(&live));
    assert_eq!(w.task_status(&live), "cancelled");
    assert_eq!(w.moves("task", &live, "cancelled"), 1);
}

#[test]
fn landed_branches_stay_landed() {
    let mut w = world();
    let (done, landed, landed_tip) = w.landed("done");
    let (_, open, _) = w.task("open", "dev");
    let git_port = w.git_port();

    let out = w.abandon(&GitBranchRescues { git: &git_port }).unwrap();

    assert_eq!(w.state(&landed), "landed");
    assert_eq!(
        w.rescue_ref(&landed),
        None,
        "a landed branch needs no rescue"
    );
    assert_eq!(w.task_status(&done), "done");
    assert_eq!(git(&w.repo, &["rev-parse", &landed]), landed_tip);
    assert_eq!(w.state(&open), "abandoned");
    assert_eq!(
        w.rescue_refs().len(),
        1,
        "only the open branch has a rescue ref"
    );
    let names: Vec<&str> = out.branches.iter().map(|b| b.branch.as_str()).collect();
    assert_eq!(names, [open.as_str()]);
    assert_eq!(w.moves("task_branch", &landed, "abandoned"), 0);
}

#[test]
fn merged_feature_is_refused() {
    let mut w = world();
    let (task, open, _) = w.task("open", "dev");
    w.store
        .immediate(|tx| {
            let cause = Cause {
                actor: Actor::Horch,
                text: "git shows its changes in the primary tip",
            };
            transition(
                tx,
                None,
                Entity::Feature,
                FEATURE,
                "merged",
                cause,
                &NeedsYouList,
            )?;
            Ok(())
        })
        .unwrap();
    let git_port = w.git_port();

    let err = w.abandon(&GitBranchRescues { git: &git_port }).unwrap_err();

    assert!(format!("{err:#}").contains("merged"), "{err:#}");
    assert!(w.rescue_refs().is_empty());
    assert_eq!(w.state(&open), "building");
    assert_eq!(w.task_status(&task), "ready");
    assert_eq!(w.feature_state(), "merged");
}

#[test]
fn branch_with_no_rescue_ref_is_not_moved() {
    let mut w = world();
    let (task, a, _) = w.task("a", "dev");
    let git_port = w.git_port();
    let port = GitBranchRescues { git: &git_port };
    let rescued = commands::rescue_feature(w.store.conn(), &port, &w.repo, FEATURE).unwrap();
    // A branch is bound after step 1: it has no rescue ref yet.
    let (_, late, _) = w.task("late", "dev");

    let config = w.config.clone();
    let jobs = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let err = w
        .store
        .immediate(|tx| commands::record_feature(&jobs, tx, FEATURE, &rescued, WHY))
        .unwrap_err();
    assert!(format!("{err:#}").contains(&late), "{err:#}");
    assert_eq!(
        (w.state(&a), w.state(&late)),
        ("building".into(), "building".into())
    );
    assert_eq!(w.task_status(&task), "ready");
    assert_eq!(w.feature_state(), "open");

    // The next abandon rescues both, then moves both.
    w.abandon(&port).unwrap();
    assert_eq!(w.rescue_refs().len(), 2);
    assert_eq!(
        (w.state(&a), w.state(&late)),
        ("abandoned".into(), "abandoned".into())
    );
}

// ─── the feature_abandon request of fleetd (Phase 5) ────────────────────

fn handlers(w: &World) -> Handlers {
    let mut h = Handlers::new();
    register(
        &mut h,
        Arc::new(Custody {
            guard: Generation(1),
            config: w.config.clone(),
            refs: Box::new(GitRefTips { git: w.git_port() }),
        }),
    );
    h
}

fn send(w: &World, h: &Handlers, id: &str, payload: Value) -> Outcome {
    let Value::Object(payload) = payload else {
        panic!("a payload is an object");
    };
    let request = Request {
        request_id: RequestId::parse(id).unwrap(),
        kind: Kind::FeatureAbandon,
        payload,
        token: Some("t".into()),
    };
    let decision = Decision::Authorized {
        auth: "token; channel".into(),
    };
    let mut conn = Connection::open(&w.db).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    handle(
        &mut conn,
        Generation(1),
        &request,
        Some("operator"),
        &decision,
        h,
    )
    .unwrap()
    .outcome
}

#[test]
fn feature_abandon_request_verifies_each_rescue_ref_before_the_moves() {
    let mut w = world();
    let (a_task, a, a_tip) = w.task("a", "dev");
    let (_, b, b_tip) = w.task("b", "dev");
    let (_, landed, _) = w.landed("done");
    let git_port = w.git_port();
    let port = GitBranchRescues { git: &git_port };
    let a_ref = port.rescue(&w.repo, &a).unwrap().unwrap();
    let b_ref = port.rescue(&w.repo, &b).unwrap().unwrap();
    let h = handlers(&w);
    let entry = |branch: &str, rescue: &str, tip: &str| json!({"branch": branch, "rescue_ref": rescue, "tip": tip});

    // Each payload that git or the ledger does not confirm is refused, and
    // nothing moves.
    let bad = [
        json!({"feature": FEATURE, "branches": [entry(&a, &a_ref, &a_tip)]}),
        json!({"feature": FEATURE, "branches": [entry(&a, &a_ref, &b_tip), entry(&b, &b_ref, &b_tip)]}),
        json!({"feature": FEATURE, "branches": [entry(&a, &b_ref, &b_tip), entry(&b, &b_ref, &b_tip)]}),
        json!({"feature": FEATURE, "branches": [entry(&a, &format!("{a_ref}9"), &a_tip), entry(&b, &b_ref, &b_tip)]}),
    ];
    for (n, payload) in bad.into_iter().enumerate() {
        match send(&w, &h, &format!("bad-{n}"), payload.clone()) {
            Outcome::Refused(_) => {}
            Outcome::Applied(v) => panic!("{payload} applied: {v}"),
        }
        assert_eq!(
            (w.state(&a), w.state(&b)),
            ("building".into(), "building".into())
        );
        assert_eq!(w.feature_state(), "open", "{payload}");
        assert_eq!(w.task_status(&a_task), "ready", "{payload}");
    }

    let ok = json!({"feature": FEATURE, "branches": [entry(&a, &a_ref, &a_tip), entry(&b, &b_ref, &b_tip)]});
    let Outcome::Applied(result) = send(&w, &h, "good", ok) else {
        panic!("refused");
    };
    assert_eq!(result["already"], json!(false));
    assert_eq!(w.feature_state(), "abandoned");
    assert_eq!(w.rescue_ref(&a).as_deref(), Some(a_ref.as_str()));
    assert_eq!(w.rescue_ref(&b).as_deref(), Some(b_ref.as_str()));
    assert_eq!(w.state(&landed), "landed");
    assert_eq!(w.task_status(&a_task), "cancelled");
}

#[test]
fn crash_in_the_middle_completes_on_the_next_pass() {
    let mut w = world();
    let (a_task, a, a_tip) = w.task("a", "dev");
    let (_, b, b_tip) = w.task("b", "dev");
    let (live, _, _) = w.task("live", "live");
    let worker = w.claimed("live");
    let git_port = w.git_port();
    let port = GitBranchRescues { git: &git_port };

    // Fault point 1: fleetd dies in step 1, after the rescue of 1 branch.
    let dies = DiesAfter {
        git: GitBranchRescues { git: &git_port },
        left: RefCell::new(1),
    };
    assert!(w.abandon(&dies).is_err());
    let first = w.rescue_refs();
    assert_eq!(first.len(), 1, "{first:?}");
    assert_eq!(
        (w.state(&a), w.state(&b)),
        ("building".into(), "building".into())
    );
    assert_eq!(w.feature_state(), "open");
    assert_eq!(w.task_status(&a_task), "ready");

    // Fault point 2: step 1 completes, and fleetd dies before the commit
    // of the intended rows.
    let rescued = commands::rescue_feature(w.store.conn(), &port, &w.repo, FEATURE).unwrap();
    assert_eq!(rescued.len(), 3);
    let second = w.rescue_refs();
    assert_eq!(second.len(), 3, "1 ref for each open branch: {second:?}");
    assert!(second.contains(&first[0]), "the same tip gets the same ref");
    assert_eq!(w.feature_state(), "open", "nothing moved");
    assert_eq!(w.state(&a), "building");

    // Fault point 3: the commit, and fleetd dies before the stop of the
    // live attempt (the use case commits; the caller stops after it).
    let done = w.abandon(&port).unwrap();
    assert_eq!(done.to_stop(), std::slice::from_ref(&worker));
    assert_eq!(w.rescue_refs(), second, "no second ref for the same tip");
    assert_eq!(w.feature_state(), "abandoned");
    assert_eq!(w.task_status(&live), "running", "it waits for its stop");

    // The next pass: a new abandon gives the worker to stop again and moves
    // nothing again; the pass of fleetd cancels the task when its process
    // is gone.
    let again = w.abandon(&port).unwrap();
    assert!(again.already);
    assert_eq!(again.to_stop(), [worker]);
    let moved = w
        .store
        .immediate(|tx| cancel::complete_cancels(tx, &facts(Seen::Dead)))
        .unwrap();
    assert_eq!(moved, std::slice::from_ref(&live));

    // Each effect happened 1 time, and no tip is lost.
    assert_eq!(w.rescue_refs(), second);
    w.assert_abandoned_once(&a, &a_tip);
    w.assert_abandoned_once(&b, &b_tip);
    assert_eq!(w.moves("feature", FEATURE, "abandoned"), 1);
    assert_eq!(w.moves("task", &live, "cancelled"), 1);
    assert_eq!(w.task_status(&live), "cancelled");
}
