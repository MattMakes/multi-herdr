//! The `bundle_build` job: it builds the candidate of a bundle from the base
//! tip and the tip of each member, writes `refs/horch/bundle/<id>`
//! create-only, reads it back, and gives the candidate tip, or the member
//! that conflicts with its files (docs/specs/fleetd/design.md, "The bundle",
//! "The kinds of job", "fleetd owns the candidate refs";
//! docs/tickets/fleetd-p4/27-bundle-build-job.md).
//!
//! Each test makes its own temporary ledger and git repository. No test
//! touches this repository, and no test reads the clock, the load or the
//! limits of the machine.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::bail;
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow, NewRun as NewJob};
use horch_core::fleetd::fleet_runtime::application::jobs::Ran;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::jobs::Kind;
use horch_core::fleetd::integration::adapters::exec_bundle::{self, Build};
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, NewBranch, NewBundle, NewFeature,
};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::Store;
use horch_marketplace::git::scrub_repo_env;

const FEATURE: &str = "F-0001";

/// The date of each commit of a test: a fixed day in the past.
const COMMIT_DATE: &str = "1700000000 +0000";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _dir: tempfile::TempDir,
    store: Store,
    run: Run,
    tasks: u32,
    root: PathBuf,
    repo: PathBuf,
    /// The first commit of `main`.
    base: String,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut store = Store::open(&root.join("ledger.sqlite3")).unwrap();
    let new = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = store.immediate(|tx| runs::create(tx, &root, &new)).unwrap();
    let new = NewFeature {
        id: FEATURE,
        name: Some("f-0001"),
        branch: Some("fleet/r1"),
        primary_branch: "main",
    };
    store
        .immediate(|tx| rows::insert_feature(tx, None, &new, cause("feature accepted")))
        .unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let base = git(&repo, &["rev-parse", "HEAD"]);
    World {
        _dir: dir,
        store,
        run,
        tasks: 0,
        root,
        repo,
        base,
    }
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

impl World {
    /// Check out `branch` (made from `main` when it is new), write each file
    /// of `files`, and commit them. Gives the new tip.
    fn commit_on(&self, branch: &str, files: &[(&str, &str)]) -> String {
        if git_out(&self.repo, &["rev-parse", "--verify", "-q", branch])
            .status
            .success()
        {
            git(&self.repo, &["checkout", "-q", branch]);
        } else {
            git(&self.repo, &["checkout", "-q", "-b", branch, &self.base]);
        }
        for (name, text) in files {
            std::fs::write(self.repo.join(name), text).unwrap();
            git(&self.repo, &["add", name]);
        }
        git(&self.repo, &["commit", "-q", "-m", branch]);
        let tip = self.tip(branch).unwrap();
        git(&self.repo, &["checkout", "-q", "main"]);
        tip
    }

    /// The tip of `rev`, or `None` when it does not exist.
    fn tip(&self, rev: &str) -> Option<String> {
        let out = git_out(&self.repo, &["rev-parse", "--verify", "-q", rev]);
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn is_ancestor(&self, a: &str, b: &str) -> bool {
        git_out(&self.repo, &["merge-base", "--is-ancestor", a, b])
            .status
            .success()
    }

    /// A new task and its task branch `name` of [`FEATURE`].
    fn task_branch(&mut self, name: &str) {
        self.tasks += 1;
        let n = self.tasks;
        let new = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), "dev");
        let run = self.run.clone();
        let task = self
            .store
            .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id;
        let new = NewBranch {
            name,
            feature_id: FEATURE,
            task_id: Some(&task),
            owner: "worker",
            due_at: 100,
        };
        self.store
            .immediate(|tx| rows::insert_branch(tx, None, &new, cause("task bound")))
            .unwrap();
    }

    /// A new `building` bundle on the base tip with the task branches
    /// `members`, in queue order. Gives its id.
    fn bundle(&mut self, members: &[&str]) -> i64 {
        for m in members {
            self.task_branch(m);
        }
        let new = NewBundle {
            feature_id: FEATURE,
            base_tip: &self.base,
            parent_id: None,
            members,
        };
        self.store
            .immediate(|tx| rows::insert_bundle(tx, None, &new, cause("bundle queued")))
            .unwrap()
    }

    /// Queue run `run` of the `bundle_build` job of `bundle` with the input
    /// key of the base tip and `tips`, and commit it. Gives its row.
    fn queue(&mut self, bundle: i64, tips: &[&str], run: i64) -> JobRow {
        let subject = bundle.to_string();
        let key = exec_bundle::input_key(&self.base, tips);
        let new = new_job(self.repo.to_str().unwrap(), &subject, &key, run);
        let id = jobs_store::insert(self.store.conn(), &new).unwrap();
        jobs_store::get(self.store.conn(), id).unwrap().unwrap()
    }

    /// The process of the run `row` was lost: the next pass ends it
    /// `abandoned`, so a new run can be queued.
    fn lose(&self, row: &JobRow) {
        self.store
            .conn()
            .execute(
                "UPDATE jobs SET state = 'abandoned' WHERE id = ?1",
                [row.id],
            )
            .unwrap();
    }

    fn run(&self, row: &JobRow) -> Ran {
        exec_bundle::run(self.store.conn(), row).unwrap()
    }
}

/// Run `run` of the `bundle_build` job of the bundle `bundle` of the
/// repository `repo`, with the input key `key`.
fn new_job<'a>(repo: &'a str, bundle: &'a str, key: &'a str, run: i64) -> NewJob<'a> {
    NewJob {
        kind: Kind::BundleBuild,
        subject: bundle,
        repo: Some(repo),
        input_key: key,
        run,
        abandoned_runs: 0,
        not_before: None,
        time_limit_s: 600,
    }
}

/// The build that the data of `ran` gives.
fn build(ran: &Ran) -> Build {
    serde_json::from_value(ran.data.clone().expect("the run gives its build")).unwrap()
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
    let mut cmd = Command::new("git");
    cmd.arg("-C")
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
        .env("GIT_AUTHOR_DATE", COMMIT_DATE)
        .env("GIT_COMMITTER_DATE", COMMIT_DATE);
    scrub_repo_env(&mut cmd);
    cmd.output().unwrap()
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn bundle_build_writes_the_candidate_ref_and_its_tip() {
    let mut w = world();
    let one = w.commit_on("fleet/r1/t1", &[("t1.txt", "1\n")]);
    let two = w.commit_on("fleet/r1/t2", &[("t2.txt", "2\n")]);
    let id = w.bundle(&["fleet/r1/t1", "fleet/r1/t2"]);
    let job = w.queue(id, &[&one, &two], 1);

    let ran = w.run(&job);

    assert!(ran.outcome.passed, "{}", ran.outcome.text);
    let name = format!("refs/horch/bundle/{id}");
    assert_eq!(name, exec_bundle::candidate_ref(id));
    let Build::Candidate {
        candidate_ref,
        candidate_tip,
    } = build(&ran)
    else {
        panic!("a candidate: {:?}", build(&ran));
    };
    assert_eq!(candidate_ref, name);
    assert_eq!(w.tip(&name).as_deref(), Some(candidate_tip.as_str()));
    // Each task's commits are kept (Q15), and no other ref moved.
    for tip in [&one, &two, &w.base] {
        assert!(w.is_ancestor(tip, &candidate_tip), "{tip} is kept");
    }
    assert_eq!(w.tip("fleet/r1/t1"), Some(one));
    assert_eq!(w.tip("fleet/r1/t2"), Some(two));
    assert_eq!(w.tip("main"), Some(w.base.clone()));
}

#[test]
fn conflicting_member_is_named_with_its_files() {
    let mut w = world();
    let one = w.commit_on("fleet/r1/t1", &[("x.txt", "one\n")]);
    let two = w.commit_on("fleet/r1/t2", &[("x.txt", "two\n"), ("y.txt", "y\n")]);
    let id = w.bundle(&["fleet/r1/t1", "fleet/r1/t2"]);
    let job = w.queue(id, &[&one, &two], 1);

    let ran = w.run(&job);

    assert!(!ran.outcome.passed);
    assert!(ran.outcome.verdict, "the build gave its verdict");
    assert_eq!(
        build(&ran),
        Build::Conflict {
            member: "fleet/r1/t2".into(),
            files: vec!["x.txt".into()],
        }
    );
    assert!(
        ran.outcome.text.contains("fleet/r1/t2"),
        "{}",
        ran.outcome.text
    );
    assert_eq!(w.tip(&exec_bundle::candidate_ref(id)), None);
}

#[test]
fn repeated_run_with_the_same_ref_passes() {
    let mut w = world();
    let one = w.commit_on("fleet/r1/t1", &[("t1.txt", "1\n")]);
    let two = w.commit_on("fleet/r1/t2", &[("t2.txt", "2\n")]);
    let id = w.bundle(&["fleet/r1/t1", "fleet/r1/t2"]);
    let first = w.queue(id, &[&one, &two], 1);
    let ran = w.run(&first);
    assert!(ran.outcome.passed, "{}", ran.outcome.text);
    w.lose(&first);
    let second = w.queue(id, &[&one, &two], 2);

    let again = w.run(&second);

    assert!(again.outcome.passed, "{}", again.outcome.text);
    assert_eq!(build(&again), build(&ran));
    let Build::Candidate { candidate_tip, .. } = build(&ran) else {
        panic!("a candidate: {:?}", build(&ran));
    };
    // The tip depends only on the input key: each merge commit has the
    // newest date of its parents, not the date of the run.
    let dates = git(&w.repo, &["show", "-s", "--format=%at %ct", &candidate_tip]);
    assert_eq!(dates, "1700000000 1700000000");
    assert_eq!(w.tip(&exec_bundle::candidate_ref(id)), Some(candidate_tip));
}

#[test]
fn existing_ref_with_another_tip_fails() {
    let mut w = world();
    let one = w.commit_on("fleet/r1/t1", &[("t1.txt", "1\n")]);
    let id = w.bundle(&["fleet/r1/t1"]);
    let name = exec_bundle::candidate_ref(id);
    git(&w.repo, &["update-ref", &name, &w.base]);
    let job = w.queue(id, &[&one], 1);

    let ran = w.run(&job);

    assert!(!ran.outcome.passed);
    assert!(!ran.outcome.verdict);
    assert!(ran.outcome.text.contains(&w.base), "{}", ran.outcome.text);
    assert_eq!(w.tip(&name), Some(w.base.clone()));
}

#[test]
fn input_key_of_other_tips_fails_and_runs_no_git() {
    let mut w = world();
    let one = w.commit_on("fleet/r1/t1", &[("t1.txt", "1\n")]);
    let id = w.bundle(&["fleet/r1/t1", "fleet/r1/t2"]);
    let job = w.queue(id, &[&one], 1);

    let ran = w.run(&job);

    assert!(!ran.outcome.passed);
    assert!(
        ran.outcome.text.contains("input key"),
        "{}",
        ran.outcome.text
    );
    assert_eq!(w.tip(&exec_bundle::candidate_ref(id)), None);
}

/// The fleetd git rule (Q26): a merge driver of the project is code of the
/// project, and it never runs with the rights of fleetd.
#[test]
fn candidate_build_runs_no_merge_driver_of_the_project() {
    let mut w = world();
    let marker = w.root.join("driver-ran");
    let driver = format!("touch {} && false", marker.display());
    git(&w.repo, &["config", "merge.evil.driver", &driver]);
    std::fs::write(w.repo.join(".git/info/attributes"), "* merge=evil\n").unwrap();
    let one = w.commit_on(
        "fleet/r1/t1",
        &[("x.txt", "one\n"), (".gitattributes", "* merge=evil\n")],
    );
    let two = w.commit_on(
        "fleet/r1/t2",
        &[("x.txt", "two\n"), (".gitattributes", "* merge=evil\n")],
    );
    let id = w.bundle(&["fleet/r1/t1", "fleet/r1/t2"]);
    let job = w.queue(id, &[&one, &two], 1);

    let ran = w.run(&job);

    assert!(!ran.outcome.passed);
    assert!(
        ran.outcome.text.contains("merge.evil.driver"),
        "{}",
        ran.outcome.text
    );
    assert!(!marker.exists(), "the merge driver of the project ran");
    assert_eq!(w.tip(&exec_bundle::candidate_ref(id)), None);
}

/// A crash at each fault point of the effect: before the commit of the
/// intended row, after that commit and before the ref write, and after the
/// ref write and before the result. The next run completes the effect 1
/// time, and no tip and no work is lost.
#[test]
fn crash_at_each_fault_point_writes_the_candidate_ref_1_time() {
    let mut w = world();
    let one = w.commit_on("fleet/r1/t1", &[("t1.txt", "1\n")]);
    let two = w.commit_on("fleet/r1/t2", &[("t2.txt", "2\n")]);
    let id = w.bundle(&["fleet/r1/t1", "fleet/r1/t2"]);
    let name = exec_bundle::candidate_ref(id);
    let subject = id.to_string();
    let key = exec_bundle::input_key(&w.base, &[&one, &two]);

    // 1. A fault before the commit of the intended row: no row, no ref.
    let repo = w.repo.to_str().unwrap().to_string();
    let new = new_job(&repo, &subject, &key, 1);
    let faulted = w.store.immediate(|tx| -> anyhow::Result<()> {
        jobs_store::insert(tx, &new)?;
        bail!("fault before the commit");
    });
    assert!(faulted.is_err());
    let count: i64 = w
        .store
        .conn()
        .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "a fault before the commit leaves no row");
    assert_eq!(w.tip(&name), None);

    // 2. A fault after the commit and before the effect: the row is
    // committed, and its process is lost before the ref write.
    let first = w.queue(id, &[&one, &two], 1);
    assert_eq!(w.tip(&name), None);
    w.lose(&first);

    // The next run does the effect.
    let second = w.queue(id, &[&one, &two], 2);
    let done = w.run(&second);
    assert!(done.outcome.passed, "{}", done.outcome.text);
    let Build::Candidate { candidate_tip, .. } = build(&done) else {
        panic!("a candidate: {:?}", build(&done));
    };

    // 3. A fault after the ref write and before the result: the run is
    // lost, and the next run passes with the same tip and no new move.
    w.lose(&second);
    let third = w.queue(id, &[&one, &two], 3);
    let again = w.run(&third);
    assert!(again.outcome.passed, "{}", again.outcome.text);
    assert_eq!(build(&again), build(&done));

    assert_eq!(w.tip(&name), Some(candidate_tip.clone()));
    for tip in [&one, &two, &w.base] {
        assert!(w.is_ancestor(tip, &candidate_tip), "{tip} is kept");
    }
    assert_eq!(w.tip("fleet/r1/t1"), Some(one));
    assert_eq!(w.tip("fleet/r1/t2"), Some(two));
    assert_eq!(w.tip("main"), Some(w.base.clone()));
}
