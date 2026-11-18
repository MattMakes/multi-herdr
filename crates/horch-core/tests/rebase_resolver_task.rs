//! A rebase that conflicts opens 1 resolver task in the run of the
//! feature, with the conflict in `zdiff3` form, the 2 tips and the tests of
//! both sides. A conflict over `resolver_max_lines` or `resolver_max_files`
//! is 1 resolver task for each file. The same pair of tips never opens a
//! second task (docs/specs/fleetd/design.md, "Between features":
//! Continuous rebase and Starting values;
//! docs/tickets/fleetd-p4/42-rebase-conflict-resolver-task.md).
//!
//! Each test makes its own ledger and git repository in a temporary folder.
//! The limits come from a test `fleet.toml`. No test reads the clock, the
//! load or the limits of the machine.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::Kind;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::git::{CliGitGateway, CliStackGit};
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, NewBranch, NewFeature,
};
use horch_core::fleetd::integration::application::branches::{self, Opened, Resolvers};
use horch_core::fleetd::integration::application::rebase::{rebase_key, Rebased};
use horch_core::fleetd::integration::domain::stack::StackPlanner;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::{Actor, FileMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::schema;
use horch_core::taskledger::tasks::{self, NewTask, Task};
use rusqlite::{params, Connection};

const FEATURE: &str = "F-1";
const FEATURE_BRANCH: &str = "fleet/r1";
const PROFILE: &str = "opus";

/// The limits of a resolver task, as a test `fleet.toml` gives them.
const FLEET_TOML: &str = "resolver_max_lines = 100\nresolver_max_files = 2\n";

struct World {
    _dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    run: Run,
    root: PathBuf,
    repo: PathBuf,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// Run git in `dir` for the test itself, with no config of the user.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A ledger of the current schema with the run `r1`, the feature
/// [`FEATURE`] on [`FEATURE_BRANCH`] and its branch row, and 1 task of the
/// run with its check. In git, `main` and the feature branch change each
/// file of `files` in a different way, so a rebase conflicts on each one.
fn world(files: &[&str]) -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut conn = Connection::open(root.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    std::fs::create_dir_all(root.join("fleet")).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    let commit_all = |text: &str, message: &str| {
        for f in files {
            std::fs::write(repo.join(f), format!("{text} {f}\n")).unwrap();
            git(&repo, &["add", f]);
        }
        git(&repo, &["commit", "-q", "-m", message]);
    };
    commit_all("base", "first");
    git(&repo, &["checkout", "-q", "-b", FEATURE_BRANCH]);
    commit_all("feature", "feature");
    git(&repo, &["checkout", "-q", "main"]);
    commit_all("primary", "primary");
    let tx = conn.transaction().unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        run_branch: Some(FEATURE_BRANCH.into()),
        ..Default::default()
    };
    let run = runs::create(&tx, &repo, &new_run).unwrap();
    let feature = NewFeature {
        id: FEATURE,
        name: Some("r1"),
        branch: Some(FEATURE_BRANCH),
        primary_branch: "main",
    };
    rows::insert_feature(&tx, None, &feature, cause("the run starts")).unwrap();
    let branch = NewBranch {
        name: FEATURE_BRANCH,
        feature_id: FEATURE,
        task_id: None,
        owner: "fleetd",
        due_at: 0,
    };
    rows::insert_branch(&tx, None, &branch, cause("feature branch made")).unwrap();
    let mut work = NewTask::work("the feature work", "tickets/01.md", PROFILE);
    work.land_check = Some("cargo test -p feature".into());
    tasks::add(&tx, &run, &work, Actor::Operator).unwrap();
    tx.commit().unwrap();
    let config = config_file::parse(FLEET_TOML).unwrap();
    let w = World {
        _dir: dir,
        conn,
        config,
        run,
        root,
        repo,
    };
    let tip = w.tip(FEATURE_BRANCH);
    let tx = w.conn.unchecked_transaction().unwrap();
    rows::set_branch_fact(
        &tx,
        None,
        FEATURE_BRANCH,
        BranchFact::SeenTip(&tip),
        cause("t"),
    )
    .unwrap();
    tx.commit().unwrap();
    w
}

impl World {
    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    fn fleet_git(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.root.join("fleet"))
            .reading(std::slice::from_ref(&self.repo))
            .writing(&[self.repo.join(".git")])
    }

    /// A rebase run of the feature branch onto `main` ended with a conflict
    /// on `files`, as the rebase job ends it.
    fn conflicted(&self, files: &[&str]) {
        let key = rebase_key(&self.tip(FEATURE_BRANCH), &self.tip("main"));
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        let job = JobKey {
            kind: Kind::Rebase,
            subject: FEATURE_BRANCH,
            input_key: &key,
        };
        let repo = self.repo.to_string_lossy();
        let Queued::New(id) = jobs::queue(&ctx, &tx, &job, Some(&repo), FleetSeconds(0)).unwrap()
        else {
            panic!("a new rebase run is queued");
        };
        let data = Rebased::Conflict {
            files: files.iter().map(|f| f.to_string()).collect(),
        };
        let result = serde_json::json!({
            "passed": false,
            "verdict": true,
            "text": "the rebase conflicts",
            "data": data,
        });
        tx.execute(
            "UPDATE jobs SET state = 'failed', result_json = ?2, ended_at = '2026-01-01' WHERE id = ?1",
            params![id, result.to_string()],
        )
        .unwrap();
        tx.commit().unwrap();
    }

    /// 1 pass of the resolver rule, committed.
    fn pass(&mut self) -> Vec<Opened> {
        let gateway = CliGitGateway::new(self.fleet_git());
        let stack_git = CliStackGit::new(self.fleet_git());
        let r = Resolvers {
            git: &gateway,
            conflicts: &stack_git,
            planner: StackPlanner {
                max_lines: self.config.resolver_max_lines,
                max_files: self.config.resolver_max_files,
            },
            profile: PROFILE,
            guard: None,
        };
        let tx = self.conn.transaction().unwrap();
        let opened =
            branches::open_rebase_resolvers(&tx, &r, cause("the rebase conflicted")).unwrap();
        tx.commit().unwrap();
        opened
    }

    /// The resolver tasks of the run: each task but the feature work.
    fn resolvers(&self) -> Vec<Task> {
        tasks::of_run(&self.conn, &self.run.run_id)
            .unwrap()
            .into_iter()
            .filter(|t| t.title != "the feature work")
            .collect()
    }
}

#[test]
fn rebase_conflict_adds_1_resolver_task() {
    let mut w = world(&["f.txt"]);
    let tip = w.tip(FEATURE_BRANCH);
    let onto = w.tip("main");
    w.conflicted(&["f.txt"]);

    let opened = w.pass();

    assert_eq!(opened.len(), 1, "{opened:?}");
    assert_eq!(opened[0].branch, FEATURE_BRANCH);
    assert_eq!(
        (opened[0].tip.as_str(), opened[0].onto.as_str()),
        (tip.as_str(), onto.as_str())
    );
    let resolvers = w.resolvers();
    assert_eq!(resolvers.len(), 1, "1 resolver task in the run");
    let task = &resolvers[0];
    assert_eq!(opened[0].tasks, vec![task.task_id.clone()]);
    assert_eq!(
        task.run_id, w.run.run_id,
        "the task is in the run of the feature"
    );
    assert_eq!(task.work_kind.as_deref(), Some("feature"), "a code kind");
    assert_eq!(task.primary_persona.as_deref(), Some(PROFILE));
    assert_eq!(task.status, "ready");
    assert_eq!(
        tasks::files(&w.conn, &task.task_id, FileMode::Own).unwrap(),
        vec!["f.txt".to_string()]
    );
    let ticket = task
        .ticket_snapshot
        .as_deref()
        .expect("fleetd writes the ticket");
    assert!(ticket.contains("**Kind:** feature"), "{ticket}");
    assert!(
        ticket.contains(&tip) && ticket.contains(&onto),
        "the 2 tips: {ticket}"
    );
    assert!(ticket.contains("f.txt"), "{ticket}");
    for marker in ["<<<<<<<", "|||||||", "=======", ">>>>>>>"] {
        assert!(
            ticket.contains(marker),
            "the zdiff3 form has {marker}: {ticket}"
        );
    }
    assert!(
        ticket.contains("feature f.txt") && ticket.contains("primary f.txt"),
        "{ticket}"
    );
    assert!(
        ticket.contains("cargo test -p feature"),
        "the tests of the feature: {ticket}"
    );
    assert!(
        ticket.contains(FEATURE_BRANCH),
        "it lands on the feature branch: {ticket}"
    );
}

#[test]
fn same_conflict_twice_adds_no_second_task() {
    let mut w = world(&["f.txt"]);
    w.conflicted(&["f.txt"]);
    assert_eq!(w.pass().len(), 1);

    assert!(w.pass().is_empty(), "a second pass opens nothing");
    assert_eq!(w.resolvers().len(), 1);

    // A cancelled resolver task is a decision of the operator: the same
    // pair of tips does not open it again.
    let task = w.resolvers().remove(0);
    tasks::cancel(&w.conn, &task, "the operator resolves it").unwrap();
    assert!(w.pass().is_empty(), "a cancelled task is not replaced");

    // Another rebase run of the same 2 tips ends with the same conflict.
    w.conflicted(&["f.txt"]);
    assert!(w.pass().is_empty(), "the same tips open no second task");
    assert_eq!(w.resolvers().len(), 1);
}

#[test]
fn conflict_over_2_files_is_split_by_file() {
    let files = ["a.txt", "b.txt", "c.txt"];
    let mut w = world(&files);
    w.conflicted(&files);

    let opened = w.pass();

    assert_eq!(opened.len(), 1, "1 conflict");
    assert_eq!(opened[0].tasks.len(), 3, "1 task for each file");
    let resolvers = w.resolvers();
    assert_eq!(resolvers.len(), 3);
    for (task, file) in resolvers.iter().zip(files) {
        assert_eq!(
            tasks::files(&w.conn, &task.task_id, FileMode::Own).unwrap(),
            vec![file.to_string()],
            "{}",
            task.title
        );
        let ticket = task.ticket_snapshot.as_deref().unwrap();
        assert!(ticket.contains(&format!("feature {file}")), "{ticket}");
        for other in files.iter().filter(|f| **f != file) {
            assert!(!ticket.contains(&format!("feature {other}")), "{ticket}");
        }
    }
    assert!(
        w.pass().is_empty(),
        "the split conflict opens no second set"
    );
}

#[test]
fn conflict_in_the_limits_is_1_task_with_each_file() {
    let files = ["a.txt", "b.txt"];
    let mut w = world(&files);
    w.conflicted(&files);

    let opened = w.pass();

    assert_eq!(opened[0].tasks.len(), 1);
    let task = &w.resolvers()[0];
    assert_eq!(
        tasks::files(&w.conn, &task.task_id, FileMode::Own).unwrap(),
        vec!["a.txt".to_string(), "b.txt".to_string()]
    );
}

#[test]
fn conflict_of_a_tip_that_moved_opens_nothing() {
    let mut w = world(&["f.txt"]);
    w.conflicted(&["f.txt"]);
    git(&w.repo, &["checkout", "-q", FEATURE_BRANCH]);
    std::fs::write(w.repo.join("g.txt"), "g\n").unwrap();
    git(&w.repo, &["add", "g.txt"]);
    git(&w.repo, &["commit", "-q", "-m", "more"]);
    git(&w.repo, &["checkout", "-q", "main"]);

    assert!(w.pass().is_empty(), "the conflict is of an old tip");
    assert!(w.resolvers().is_empty());
}

#[test]
fn conflict_of_a_stacked_feature_is_left_to_its_stack_edge() {
    let mut w = world(&["f.txt"]);
    let tx = w.conn.unchecked_transaction().unwrap();
    let lower = NewFeature {
        id: "F-0",
        name: Some("r0"),
        branch: Some("fleet/r0"),
        primary_branch: "main",
    };
    rows::insert_feature(&tx, None, &lower, cause("the lower run starts")).unwrap();
    tx.execute(
        "UPDATE features SET stacked_on = 'F-0' WHERE id = ?1",
        params![FEATURE],
    )
    .unwrap();
    tx.commit().unwrap();
    w.conflicted(&["f.txt"]);

    assert!(w.pass().is_empty(), "stack_edges opens this resolver task");
    assert!(w.resolvers().is_empty());
}
