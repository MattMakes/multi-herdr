use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use anyhow::{bail, Result};
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Request};
use horch_core::fleetd::fleet_runtime::application::handlers_custody::{register, Custody};
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, BranchRow, NewFeature};
use horch_core::fleetd::integration::application::commands::{self, GitRefTips};
use horch_core::fleetd::integration::application::landing::{
    self, BranchRescues, GitBranchRescues, Landing,
};
use horch_core::fleetd::task_execution::application::cancel;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

const FEATURE: &str = "F-0001";
const RUN_BRANCH: &str = "fleet/r1";
const RESCUE_PREFIX: &str = "refs/horch/rescue/branch/";

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

struct DeadRescues;

impl BranchRescues for DeadRescues {
    fn rescue(&self, _repo: &Path, _branch: &str) -> Result<Option<String>> {
        bail!("fleetd died before the rescue")
    }
}

struct SeenRescues<'a> {
    git: GitBranchRescues<'a>,
    db: PathBuf,
    seen: RefCell<Vec<String>>,
}

impl BranchRescues for SeenRescues<'_> {
    fn rescue(&self, repo: &Path, branch: &str) -> Result<Option<String>> {
        let conn = Connection::open(&self.db)?;
        let row = rows::branch(&conn, branch)?.unwrap();
        self.seen.borrow_mut().push(row.landing_state.unwrap());
        self.git.rescue(repo, branch)
    }
}

impl World {
    fn git_port(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.dir.join("fleet"))
            .reading(std::slice::from_ref(&self.repo))
            .writing(&[self.repo.join(".git")])
    }

    fn task(&mut self, title: &str) -> (String, String, String) {
        let branch = format!("{RUN_BRANCH}-{title}");
        let new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
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

    fn conflicted(&mut self, title: &str) -> (String, String, String) {
        let (task, branch, tip) = self.task(title);
        self.walk(&branch, &["verifying", "ready", "in_bundle", "conflicted"]);
        (task, branch, tip)
    }

    fn row(&self, branch: &str) -> BranchRow {
        rows::branch(self.store.conn(), branch).unwrap().unwrap()
    }

    fn state(&self, branch: &str) -> String {
        self.row(branch).landing_state.unwrap()
    }

    fn task_status(&self, task: &str) -> String {
        tasks::get(self.store.conn(), task).unwrap().status
    }

    fn rescue_refs(&self) -> Vec<String> {
        git(
            &self.repo,
            &["for-each-ref", "--format=%(refname)", RESCUE_PREFIX],
        )
        .lines()
        .map(str::to_string)
        .collect()
    }

    fn abandon_events(&self, branch: &str) -> i64 {
        self.store
            .conn()
            .query_row(
                "SELECT count(*) FROM events WHERE entity = 'task_branch' AND entity_id = ?1
                 AND kind = 'transition' AND to_state = 'abandoned'",
                [branch],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn abandon(&mut self, port: &dyn BranchRescues, branch: &str) -> Result<commands::Abandoned> {
        let config = self.config.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let repo = self.repo.clone();
        commands::abandon(
            &mut self.store,
            &ctx,
            port,
            &repo,
            branch,
            "the operator abandons it",
        )
    }
}

#[test]
fn abandon_writes_a_rescue_ref_then_abandoned() {
    let mut w = world();
    let (task, branch, tip) = w.conflicted("a");
    let before = w.task_status(&task);
    let git_port = w.git_port();
    let port = SeenRescues {
        git: GitBranchRescues { git: &git_port },
        db: w.db.clone(),
        seen: RefCell::default(),
    };

    let out = w.abandon(&port, &branch).unwrap();

    assert_eq!(
        port.seen.into_inner(),
        ["conflicted"],
        "the rescue ref is written while the branch is still conflicted"
    );
    let refs = w.rescue_refs();
    assert_eq!(refs.len(), 1, "1 rescue ref: {refs:?}");
    let rescue = &refs[0];
    assert_eq!(git(&w.repo, &["rev-parse", rescue]), tip);
    assert_eq!(out.rescue_ref.as_deref(), Some(rescue.as_str()));
    assert!(!out.already);
    let row = w.row(&branch);
    assert_eq!(row.landing_state.as_deref(), Some("abandoned"));
    assert_eq!(row.rescue_ref.as_deref(), Some(rescue.as_str()));
    assert_eq!(row.owner, "fleetd");
    assert_eq!(w.task_status(&task), before, "the task stays as it is");
    assert_eq!(git(&w.repo, &["rev-parse", &branch]), tip, "the tip stays");
    assert_eq!(w.abandon_events(&branch), 1);

    let again = w
        .abandon(&GitBranchRescues { git: &git_port }, &branch)
        .unwrap();
    assert!(again.already);
    assert_eq!(again.rescue_ref.as_deref(), Some(rescue.as_str()));
    assert_eq!(w.rescue_refs().len(), 1);
    assert_eq!(w.abandon_events(&branch), 1);
}

#[test]
fn gate_failed_branch_is_abandoned_too() {
    let mut w = world();
    let (_, branch, tip) = w.task("a");
    w.walk(&branch, &["verifying", "ready", "in_bundle", "gate_failed"]);
    let git_port = w.git_port();
    let out = w
        .abandon(&GitBranchRescues { git: &git_port }, &branch)
        .unwrap();
    let rescue = out.rescue_ref.unwrap();
    assert_eq!(git(&w.repo, &["rev-parse", &rescue]), tip);
    assert_eq!(w.state(&branch), "abandoned");
}

#[test]
fn landed_branch_is_refused() {
    let mut w = world();
    let (_, branch, _) = w.task("a");
    w.walk(&branch, &["verifying", "ready", "in_bundle", "landed"]);
    let git_port = w.git_port();

    let err = w
        .abandon(&GitBranchRescues { git: &git_port }, &branch)
        .unwrap_err();

    assert!(err.to_string().contains("is landed"), "{err:#}");
    assert_eq!(w.state(&branch), "landed");
    assert!(
        w.rescue_refs().is_empty(),
        "no rescue ref of a landed branch"
    );
    assert_eq!(w.abandon_events(&branch), 0);
}

#[test]
fn unknown_branch_is_refused() {
    let mut w = world();
    let git_port = w.git_port();
    let err = w
        .abandon(&GitBranchRescues { git: &git_port }, "fleet/r1-none")
        .unwrap_err();
    assert!(err.to_string().contains("not a task branch"), "{err:#}");
    assert!(w.rescue_refs().is_empty());
}

#[test]
fn crash_before_the_rescue_changes_nothing() {
    let mut w = world();
    let (_, branch, tip) = w.conflicted("a");

    assert!(w.abandon(&DeadRescues, &branch).is_err());
    assert_eq!(w.state(&branch), "conflicted");
    assert!(w.rescue_refs().is_empty());
    assert_eq!(w.row(&branch).rescue_ref, None);

    let git_port = w.git_port();
    let out = w
        .abandon(&GitBranchRescues { git: &git_port }, &branch)
        .unwrap();
    assert_eq!(w.rescue_refs().len(), 1);
    assert_eq!(git(&w.repo, &["rev-parse", &out.rescue_ref.unwrap()]), tip);
    assert_eq!(w.state(&branch), "abandoned");
    assert_eq!(w.abandon_events(&branch), 1);
}

#[test]
fn crash_after_the_rescue_completes_the_abandon() {
    let mut w = world();
    let (_, branch, tip) = w.conflicted("a");
    let git_port = w.git_port();
    let port = GitBranchRescues { git: &git_port };

    let rescued = commands::rescue(w.store.conn(), &port, &w.repo, &branch).unwrap();
    let first = rescued.rescue_ref.clone().unwrap();
    assert_eq!(git(&w.repo, &["rev-parse", &first]), tip);
    assert_eq!(w.state(&branch), "conflicted", "nothing moved");
    assert_eq!(w.row(&branch).rescue_ref, None);

    let out = w.abandon(&port, &branch).unwrap();
    assert_eq!(out.rescue_ref.as_deref(), Some(first.as_str()));
    assert_eq!(w.rescue_refs(), [first.as_str()], "no second rescue ref");
    let row = w.row(&branch);
    assert_eq!(row.landing_state.as_deref(), Some("abandoned"));
    assert_eq!(row.rescue_ref.as_deref(), Some(first.as_str()));
    assert_eq!(w.abandon_events(&branch), 1);
    assert_eq!(git(&w.repo, &["rev-parse", &branch]), tip, "the tip stays");
}

#[test]
fn cancel_that_dies_before_the_abandon_is_completed_by_the_pass() {
    let mut w = world();
    let (task, branch, tip) = w.task("a");

    w.store
        .immediate(|tx| {
            let t = tasks::get(tx, &task)?;
            cancel::request(tx, &t, "the operator cancelled it")
        })
        .unwrap();
    assert_eq!(w.task_status(&task), "cancelled");
    assert_eq!(w.state(&branch), "building");
    assert!(w.rescue_refs().is_empty());

    let git_port = w.git_port();
    let port = GitBranchRescues { git: &git_port };
    for clock in [10, 20] {
        let config = w.config.clone();
        let effects = w
            .store
            .immediate(|tx| {
                let ctx = JobCtx {
                    guard: None,
                    config: &config,
                    needs_you: &NeedsYouList,
                };
                let l = Landing { jobs: &ctx };
                landing::follow(&l, tx, FleetSeconds(clock), 50)
            })
            .unwrap();
        for e in &effects {
            let done = landing::run(e, &port).unwrap();
            let ctx = JobCtx {
                guard: None,
                config: &config,
                needs_you: &NeedsYouList,
            };
            w.store
                .immediate(|tx| landing::observe(&ctx, tx, FleetSeconds(clock), &done))
                .unwrap();
        }
    }
    let refs = w.rescue_refs();
    assert_eq!(refs.len(), 1, "1 rescue ref: {refs:?}");
    assert_eq!(git(&w.repo, &["rev-parse", &refs[0]]), tip);
    let row = w.row(&branch);
    assert_eq!(row.landing_state.as_deref(), Some("abandoned"));
    assert_eq!(row.rescue_ref.as_deref(), Some(refs[0].as_str()));
    assert_eq!(w.abandon_events(&branch), 1);
}

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
        kind: Kind::BranchAbandon,
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
fn branch_abandon_request_verifies_the_rescue_ref_before_the_move() {
    let mut w = world();
    let (_, branch, tip) = w.conflicted("a");
    let (_, other, other_tip) = w.conflicted("b");
    let git_port = w.git_port();
    let port = GitBranchRescues { git: &git_port };
    let rescue = port.rescue(&w.repo, &branch).unwrap().unwrap();
    let other_rescue = port.rescue(&w.repo, &other).unwrap().unwrap();
    let h = handlers(&w);

    let bad = [
        json!({"branch": branch, "rescue_ref": format!("{rescue}9"), "tip": tip}),
        json!({"branch": branch, "rescue_ref": rescue, "tip": other_tip}),
        json!({"branch": branch, "rescue_ref": other_rescue, "tip": other_tip}),
        json!({"branch": branch, "rescue_ref": null, "tip": null}),
    ];
    for (n, payload) in bad.into_iter().enumerate() {
        match send(&w, &h, &format!("bad-{n}"), payload.clone()) {
            Outcome::Refused(_) => {}
            Outcome::Applied(v) => panic!("{payload} applied: {v}"),
        }
        assert_eq!(w.state(&branch), "conflicted", "{payload}");
        assert_eq!(w.row(&branch).rescue_ref, None, "{payload}");
    }

    let ok = json!({"branch": branch, "rescue_ref": rescue, "tip": tip});
    let Outcome::Applied(result) = send(&w, &h, "good", ok) else {
        panic!("refused");
    };
    assert_eq!(result["rescue_ref"], json!(rescue));
    assert_eq!(result["already"], json!(false));
    let row = w.row(&branch);
    assert_eq!(row.landing_state.as_deref(), Some("abandoned"));
    assert_eq!(row.rescue_ref.as_deref(), Some(rescue.as_str()));
    assert_eq!(w.state(&other), "conflicted");
}
