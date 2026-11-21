use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use anyhow::Result;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, BundleFact, NewBranch, NewBundle, NewFeature,
};
use horch_core::fleetd::integration::application::candidates::{self, Candidates, Deletion, Ran};
use horch_core::fleetd::integration::application::ports::{
    BranchTip, Candidate, IntegrationGitGateway, Member, RefMove,
};
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

const FEATURE: &str = "F-0001";
const FEATURE_BRANCH: &str = "fleet/r1";
const FEATURE_REF: &str = "refs/heads/fleet/r1";
const A: &str = "fleet/r1-a";
const B: &str = "fleet/r1-b";
const C: &str = "fleet/r1-c";

struct World {
    _temp: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn git_out(dir: &Path, args: &[&str]) -> Output {
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
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = git_out(dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(temp.path()).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["branch", FEATURE_BRANCH]);
    let mut conn = Connection::open(root.join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        base_branch: Some("main".into()),
        run_branch: Some(FEATURE_BRANCH.into()),
        ..Default::default()
    };
    let run = runs::create(&conn, &repo, &new_run).unwrap();
    let tx = conn.transaction().unwrap();
    let new = NewFeature {
        id: FEATURE,
        name: Some("r1"),
        branch: Some(FEATURE_BRANCH),
        primary_branch: "main",
    };
    rows::insert_feature(&tx, None, &new, cause("feature accepted")).unwrap();
    horch_core::taskledger::schema::run_idea(
        &tx,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    let row = NewBranch {
        name: FEATURE_BRANCH,
        feature_id: FEATURE,
        task_id: None,
        owner: "fleetd",
        due_at: 0,
    };
    rows::insert_branch(&tx, None, &row, cause("feature branch")).unwrap();
    tx.commit().unwrap();
    let w = World {
        _temp: temp,
        root,
        repo,
        conn,
        run,
        config: FleetConfig::default(),
    };
    for (n, branch) in [A, B, C].into_iter().enumerate() {
        w.task_branch(n + 1, branch);
    }
    w
}

impl World {
    fn tip(&self, rev: &str) -> Option<String> {
        let out = git_out(&self.repo, &["rev-parse", "--verify", "-q", rev]);
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn gateway(&self) -> CliGitGateway {
        CliGitGateway::new(
            CliWorktreeGit::new("git", self.root.join("fleet")).writing(&[self.repo.join(".git")]),
        )
    }

    fn task_branch(&self, n: usize, name: &str) {
        git(&self.repo, &["checkout", "-q", "-b", name, FEATURE_BRANCH]);
        let file = format!("t{n}.txt");
        std::fs::write(self.repo.join(&file), format!("{n}\n")).unwrap();
        git(&self.repo, &["add", &file]);
        git(&self.repo, &["commit", "-q", "-m", &file]);
        git(&self.repo, &["checkout", "-q", "main"]);
        let new = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), "dev");
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        let row = NewBranch {
            name,
            feature_id: FEATURE,
            task_id: Some(&task),
            owner: "worker",
            due_at: 0,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        rows::insert_branch(&tx, None, &row, cause("task bound")).unwrap();
        tx.commit().unwrap();
    }

    fn bundle(&self, members: &[&str], parent: Option<i64>, built: bool, path: &[&str]) -> i64 {
        let base = self.tip(FEATURE_BRANCH).unwrap();
        let tx = self.conn.unchecked_transaction().unwrap();
        let new = NewBundle {
            feature_id: FEATURE,
            base_tip: &base,
            parent_id: parent,
            members,
            due_at: 0,
        };
        let id = rows::insert_bundle(&tx, None, &new, cause("ready branches")).unwrap();
        if built {
            let list: Vec<Member> = members
                .iter()
                .map(|m| Member {
                    name: m.to_string(),
                    tip: self.tip(m).unwrap(),
                })
                .collect();
            let gw = self.gateway();
            let tip = match gw.build_candidate(&self.repo, &base, &list).unwrap() {
                Candidate::Built { tip } => tip,
                other => panic!("the candidate does not build: {other:?}"),
            };
            let name = format!("refs/horch/bundle/{id}");
            assert_eq!(
                gw.update_ref(&self.repo, &name, &tip, None).unwrap(),
                RefMove::Moved
            );
            for fact in [
                BundleFact::CandidateRef(Some(&name)),
                BundleFact::CandidateTip(&tip),
            ] {
                rows::set_bundle_fact(&tx, None, id, fact, cause("its build job passed")).unwrap();
            }
        }
        for to in path {
            transition(
                &tx,
                None,
                Entity::Bundle,
                &id.to_string(),
                to,
                cause("the test moves it"),
                &NeedsYouList,
            )
            .unwrap();
        }
        tx.commit().unwrap();
        id
    }

    fn enter(&self, id: i64, to: &str) {
        let tx = self.conn.unchecked_transaction().unwrap();
        transition(
            &tx,
            None,
            Entity::Bundle,
            &id.to_string(),
            to,
            cause("the test moves it"),
            &NeedsYouList,
        )
        .unwrap();
        tx.commit().unwrap();
    }

    fn pass_with(&mut self, git: &dyn IntegrationGitGateway) -> Vec<Deletion> {
        let config = self.config.clone();
        let ctx = ctx(&config);
        let c = Candidates { jobs: &ctx, git };
        let tx = self.conn.transaction().unwrap();
        let deletions = candidates::advance(&c, &tx).unwrap();
        tx.commit().unwrap();
        deletions
    }

    fn pass(&mut self) -> Vec<Deletion> {
        let gw = self.gateway();
        self.pass_with(&gw)
    }

    fn pass_that_crashes_before_the_commit(&mut self) -> Vec<Deletion> {
        let config = self.config.clone();
        let ctx = ctx(&config);
        let gw = self.gateway();
        let c = Candidates {
            jobs: &ctx,
            git: &gw,
        };
        let tx = self.conn.transaction().unwrap();
        let deletions = candidates::advance(&c, &tx).unwrap();
        drop(tx);
        deletions
    }

    fn candidate_ref(&self, id: i64) -> Option<String> {
        rows::bundle(&self.conn, id).unwrap().unwrap().candidate_ref
    }

    fn candidate_tip(&self, id: i64) -> String {
        rows::bundle(&self.conn, id)
            .unwrap()
            .unwrap()
            .candidate_tip
            .unwrap()
    }

    fn events(&self) -> i64 {
        self.conn
            .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
            .unwrap()
    }

    fn branches(&self) -> String {
        git(
            &self.repo,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
            ],
        )
    }
}

fn bundle_ref(id: i64) -> String {
    format!("refs/horch/bundle/{id}")
}

fn ids(deletions: &[Deletion]) -> Vec<i64> {
    deletions.iter().map(|d| d.bundle).collect()
}

fn landed_a(w: &World) -> i64 {
    let id = w.bundle(&[A], None, true, &["gating", "passed"]);
    let tip = w.candidate_tip(id);
    git(&w.repo, &["update-ref", FEATURE_REF, &tip]);
    w.enter(id, "landed");
    id
}

struct Counting {
    inner: CliGitGateway,
    deleted: Cell<u32>,
}

impl IntegrationGitGateway for Counting {
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>> {
        self.inner.tip(repo, name)
    }
    fn update_ref(
        &self,
        repo: &Path,
        name: &str,
        new: &str,
        expected_old: Option<&str>,
    ) -> Result<RefMove> {
        self.inner.update_ref(repo, name, new, expected_old)
    }
    fn delete_ref(&self, repo: &Path, name: &str, expected_old: &str) -> Result<RefMove> {
        let out = self.inner.delete_ref(repo, name, expected_old)?;
        if out == RefMove::Moved {
            self.deleted.set(self.deleted.get() + 1);
        }
        Ok(out)
    }
    fn contains(&self, repo: &Path, tip: &str, ancestor: &str) -> Result<bool> {
        self.inner.contains(repo, tip, ancestor)
    }
    fn changes_in(&self, repo: &Path, branch_tip: &str, base: &str, target: &str) -> Result<bool> {
        self.inner.changes_in(repo, branch_tip, base, target)
    }
    fn is_ancestor(&self, repo: &Path, a: &str, b: &str) -> Result<bool> {
        self.inner.is_ancestor(repo, a, b)
    }
    fn build_candidate(&self, repo: &Path, base: &str, members: &[Member]) -> Result<Candidate> {
        self.inner.build_candidate(repo, base, members)
    }
    fn fleet_branches(&self, repo: &Path) -> Result<Vec<BranchTip>> {
        self.inner.fleet_branches(repo)
    }
    fn task_branches(&self, repo: &Path, run_branch: &str) -> Result<Vec<BranchTip>> {
        self.inner.task_branches(repo, run_branch)
    }

    fn stash_branches(&self, repo: &Path) -> Result<Vec<String>> {
        self.inner.stash_branches(repo)
    }
    fn fleet_toml(&self, repo: &Path, tip: &str) -> Result<Option<String>> {
        self.inner.fleet_toml(repo, tip)
    }
    fn ensure_settings(&self, dir: &Path) -> Result<()> {
        self.inner.ensure_settings(dir)
    }
}

#[test]
fn candidate_ref_of_each_ended_bundle_is_deleted_and_verified() {
    let mut w = world();
    let landed = landed_a(&w);
    let failed = w.bundle(&[B], None, true, &["gating", "failed"]);
    let dropped = w.bundle(&[A, B], None, true, &["gating", "dropped"]);
    let gating = w.bundle(&[C], None, true, &["gating"]);
    let never_built = w.bundle(&[A], None, false, &["dropped"]);
    let ended = [landed, failed, dropped];
    let feature_tip = w.tip(FEATURE_BRANCH);

    let deletions = w.pass();
    assert_eq!(ids(&deletions), ended);
    for d in &deletions {
        assert_eq!(d.repo, w.repo);
        assert_eq!(d.name, bundle_ref(d.bundle));
        assert_eq!(d.tip, w.candidate_tip(d.bundle));
        assert!(w.tip(&d.name).is_some(), "the pass deleted {}", d.name);
        assert_eq!(w.candidate_ref(d.bundle), Some(bundle_ref(d.bundle)));
    }

    let gw = w.gateway();
    for d in &deletions {
        assert_eq!(candidates::run(&gw, d).unwrap(), Ran::Deleted);
        assert_eq!(w.tip(&d.name), None);
        assert_eq!(
            w.candidate_ref(d.bundle),
            Some(bundle_ref(d.bundle)),
            "the effect records nothing"
        );
    }

    assert!(w.pass().is_empty());
    for id in ended {
        assert_eq!(w.candidate_ref(id), None, "bundle {id}");
    }

    assert!(w.tip(&bundle_ref(gating)).is_some());
    assert_eq!(w.candidate_ref(gating), Some(bundle_ref(gating)));
    assert_eq!(w.candidate_ref(never_built), None);
    assert_eq!(w.tip(FEATURE_BRANCH), feature_tip);

    let events = w.events();
    assert!(w.pass().is_empty());
    assert_eq!(w.events(), events);
    let left = git(&w.repo, &["for-each-ref", "refs/horch/bundle"]);
    assert_eq!(
        left.lines().count(),
        1,
        "only the open bundle ref stays: {left}"
    );
}

#[test]
fn ref_of_a_failed_bundle_with_a_live_child_stays() {
    let mut w = world();
    let parent = w.bundle(&[A, B], None, true, &["gating", "failed"]);
    let child = w.bundle(&[A], Some(parent), true, &[]);

    for next in ["", "gating", "passed"] {
        if !next.is_empty() {
            w.enter(child, next);
        }
        assert!(w.pass().is_empty(), "child in {next:?}");
        assert!(w.tip(&bundle_ref(parent)).is_some());
        assert_eq!(w.candidate_ref(parent), Some(bundle_ref(parent)));
    }

    w.enter(child, "dropped");
    let deletions = w.pass();
    assert_eq!(ids(&deletions), [parent, child]);
    let gw = w.gateway();
    for d in &deletions {
        assert_eq!(candidates::run(&gw, d).unwrap(), Ran::Deleted);
    }
    assert!(w.pass().is_empty());
    for id in [parent, child] {
        assert_eq!(w.tip(&bundle_ref(id)), None);
        assert_eq!(w.candidate_ref(id), None);
    }
}

#[test]
fn failed_deletion_is_done_again() {
    let mut w = world();
    let id = landed_a(&w);
    let name = bundle_ref(id);

    let lock = w.repo.join(".git").join(format!("{name}.lock"));
    std::fs::write(&lock, "").unwrap();
    let deletions = w.pass();
    assert_eq!(ids(&deletions), [id]);
    let gw = w.gateway();
    assert!(candidates::run(&gw, &deletions[0]).is_err());
    assert!(w.tip(&name).is_some());

    let again = w.pass();
    assert_eq!(again, deletions);
    assert_eq!(w.candidate_ref(id), Some(name.clone()));

    std::fs::remove_file(&lock).unwrap();
    assert_eq!(candidates::run(&gw, &again[0]).unwrap(), Ran::Deleted);
    assert!(w.pass().is_empty());
    assert_eq!(w.tip(&name), None);
    assert_eq!(w.candidate_ref(id), None);
}

#[test]
fn crash_before_the_commit_or_before_the_deletion_deletes_the_ref_1_time() {
    let mut w = world();
    let landed = landed_a(&w);
    let failed = w.bundle(&[B], None, true, &["gating", "failed"]);
    let branches = w.branches();
    let candidate = w.candidate_tip(landed);
    let counting = Counting {
        inner: w.gateway(),
        deleted: Cell::new(0),
    };

    let lost = w.pass_that_crashes_before_the_commit();
    assert_eq!(ids(&lost), [landed, failed]);
    let lost = w.pass_with(&counting);
    assert_eq!(ids(&lost), [landed, failed]);
    assert_eq!(counting.deleted.get(), 0);

    let deletions = w.pass_with(&counting);
    assert_eq!(deletions, lost);
    for d in &deletions {
        assert_eq!(candidates::run(&counting, d).unwrap(), Ran::Deleted);
    }
    assert_eq!(counting.deleted.get(), 2);

    assert!(w.pass_that_crashes_before_the_commit().is_empty());
    assert_eq!(w.candidate_ref(landed), Some(bundle_ref(landed)));

    assert!(w.pass_with(&counting).is_empty());
    assert!(w.pass_with(&counting).is_empty());
    assert_eq!(counting.deleted.get(), 2);
    for id in [landed, failed] {
        assert_eq!(w.candidate_ref(id), None);
        assert_eq!(w.tip(&bundle_ref(id)), None);
    }

    assert_eq!(
        candidates::run(&counting, &lost[0]).unwrap(),
        Ran::Mismatch { actual: None }
    );
    assert_eq!(counting.deleted.get(), 2);

    assert_eq!(w.branches(), branches);
    assert_eq!(w.tip(FEATURE_BRANCH), Some(candidate));
}

#[test]
fn ref_at_another_tip_is_not_deleted() {
    let mut w = world();
    let id = landed_a(&w);
    let name = bundle_ref(id);
    let other = w.tip(A).unwrap();
    git(&w.repo, &["update-ref", &name, &other]);

    let deletions = w.pass();
    assert_eq!(ids(&deletions), [id]);
    let gw = w.gateway();
    assert_eq!(
        candidates::run(&gw, &deletions[0]).unwrap(),
        Ran::Mismatch {
            actual: Some(other.clone())
        }
    );
    assert_eq!(w.tip(&name), Some(other));
    assert_eq!(w.pass(), deletions);
    assert_eq!(w.candidate_ref(id), Some(name));
}
