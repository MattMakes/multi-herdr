use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use horch_core::fleetd::fleet_runtime::adapters::jobs_store;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::Kind;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::exec_rebase::GitRebaseRefs;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, NewBranch, NewFeature,
};
use horch_core::fleetd::integration::application::features::{self, Features};
use horch_core::fleetd::integration::application::landing;
use horch_core::fleetd::integration::application::ports::{
    BranchTip, Candidate, IntegrationGitGateway, Member, RefMove,
};
use horch_core::fleetd::integration::application::rebase::{self, Effect, Rebases};
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::fleetd::worktree_custody::application::create::{self, Observed};
use horch_core::fleetd::worktree_custody::application::gate_tree::{
    self, GateTree, GateTreeRequest,
};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection};

const FEATURE: &str = "F-0001";
const RUN_BRANCH: &str = "fleet/r1";

struct World {
    _temp: tempfile::TempDir,
    repo: PathBuf,
    conn: Connection,
    run: Run,
    config: FleetConfig,
    git: CliGitGateway,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
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
        ])
        .args(args)
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
    let w = bare_world();
    w.gate_tree();
    w
}

fn bare_world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "--quiet", "-m", "base"]);
    git(&repo, &["branch", RUN_BRANCH]);
    let mut conn = Connection::open(dir.join("ledger.sqlite3")).unwrap();
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
        run_branch: Some(RUN_BRANCH.into()),
        ..Default::default()
    };
    let run = runs::create(&conn, &repo, &new_run).unwrap();
    let new = NewFeature {
        id: FEATURE,
        name: Some("r1"),
        branch: Some(RUN_BRANCH),
        primary_branch: "main",
    };
    let tx = conn.transaction().unwrap();
    rows::insert_feature(&tx, None, &new, cause("feature accepted")).unwrap();
    horch_core::taskledger::schema::run_idea(
        &tx,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    tx.commit().unwrap();
    let git = CliGitGateway::new(
        CliWorktreeGit::new("git", dir.join("fleet")).writing(&[repo.join(".git")]),
    );
    World {
        _temp: temp,
        repo,
        conn,
        run,
        config: FleetConfig::default(),
        git,
    }
}

impl World {
    fn jobs(&self) -> JobCtx<'_> {
        JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }

    fn follow_with(&self, gateway: &dyn IntegrationGitGateway) {
        let ctx = self.jobs();
        let f = Features {
            jobs: &ctx,
            git: gateway,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        features::follow(&f, &tx, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
    }

    fn rebase_refs(&self) -> GitRebaseRefs {
        let dir = self.repo.parent().unwrap();
        GitRebaseRefs::new(
            CliWorktreeGit::new("git", dir.join("fleet")).writing(&[self.repo.join(".git")]),
        )
    }

    fn rescue(&self, effect: &Effect) {
        let done = rebase::run(effect, &self.rebase_refs()).unwrap();
        let ctx = self.jobs();
        let tx = self.conn.unchecked_transaction().unwrap();
        rebase::observe(&ctx, &tx, &done).unwrap();
        tx.commit().unwrap();
    }

    fn seen_run_branch(&self) {
        let tip = self.tip(RUN_BRANCH);
        let tx = self.conn.unchecked_transaction().unwrap();
        let row = NewBranch {
            name: RUN_BRANCH,
            feature_id: FEATURE,
            task_id: None,
            owner: "fleetd",
            due_at: 0,
        };
        rows::insert_branch(&tx, None, &row, cause("feature branch")).unwrap();
        rows::set_branch_fact(
            &tx,
            None,
            RUN_BRANCH,
            BranchFact::SeenTip(&tip),
            cause("seen"),
        )
        .unwrap();
        tx.commit().unwrap();
    }

    fn follow_rebase(&self) -> Vec<Effect> {
        let ctx = self.jobs();
        let refs = self.rebase_refs();
        let r = Rebases {
            jobs: &ctx,
            git: &self.git,
            refs: &refs,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        let effects = rebase::follow(&r, &tx, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        effects
    }

    fn follow(&self) {
        self.follow_with(&self.git);
    }

    fn project_root(&self) -> PathBuf {
        PathBuf::from(&self.run.project_root)
    }

    fn gate_tree(&self) {
        let job = self.reserve_gate_tree();
        self.make_gate_tree(job);
    }

    fn reserve_gate_tree(&self) -> i64 {
        let tip = self.tip(RUN_BRANCH);
        let repo = self.project_root();
        let req = GateTreeRequest {
            feature_id: FEATURE,
            run_id: &self.run.run_id,
            repo: &repo,
            base: RUN_BRANCH,
            tip: &tip,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        let reserved = gate_tree::reserve(&self.jobs(), &tx, &req, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        match reserved {
            GateTree::Reserved { job, .. } => job,
            other => panic!("expected a reserved gate tree, got {other:?}"),
        }
    }

    fn make_gate_tree(&self, job: i64) {
        let path = gate_tree::path(&self.project_root(), FEATURE);
        let tip = self.tip(RUN_BRANCH);
        git(
            &self.repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "--detach",
                &path.to_string_lossy(),
                &tip,
            ],
        );
        self.conn
            .execute(
                "UPDATE jobs SET state = 'passed', result_json = ?2 WHERE id = ?1",
                params![job, r#"{"passed":true,"verdict":true,"text":"made"}"#],
            )
            .unwrap();
        let ended = jobs_store::get(&self.conn, job).unwrap().unwrap();
        let tx = self.conn.unchecked_transaction().unwrap();
        let seen = create::observe(&self.jobs(), &tx, &ended).unwrap();
        tx.commit().unwrap();
        assert!(matches!(seen, Observed::Created(_)), "{seen:?}");
    }

    fn state(&self) -> String {
        rows::feature(&self.conn, FEATURE).unwrap().unwrap().state
    }

    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    fn commit_on(&self, branch: &str, file: &str, text: &str) -> String {
        git(&self.repo, &["checkout", "--quiet", branch]);
        std::fs::write(self.repo.join(file), text).unwrap();
        git(&self.repo, &["add", file]);
        git(&self.repo, &["commit", "--quiet", "-m", file]);
        let tip = self.tip("HEAD");
        git(&self.repo, &["checkout", "--quiet", "main"]);
        tip
    }

    fn fetched_main(&self, tip: &str) {
        let tx = self.conn.unchecked_transaction().unwrap();
        rows::put_remote_tip(&tx, None, &self.run.project_root, "main", tip).unwrap();
        tx.commit().unwrap();
    }

    fn task(&self, title: &str) -> (String, String) {
        let new = NewTask::work(title, &format!("tickets/{title}.md"), "dev");
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        let branch = format!("{RUN_BRANCH}-{title}");
        let ctx = self.jobs();
        let tx = self.conn.unchecked_transaction().unwrap();
        landing::bind_branch(&ctx, &tx, &task, &branch, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        (task, branch)
    }

    fn settle(&self, task: &str, status: &str, branch: &str, landing: &str) {
        self.conn
            .execute(
                "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
                params![task, status],
            )
            .unwrap();
        self.conn
            .execute(
                "UPDATE branches SET landing_state = ?2 WHERE name = ?1",
                params![branch, landing],
            )
            .unwrap();
    }

    fn integrating(&self) {
        self.conn
            .execute(
                "UPDATE ideas SET state = 'integrating' WHERE feature_id = ?1",
                [FEATURE],
            )
            .unwrap();
    }

    fn full_gates(&self) -> Vec<(String, String, Option<String>)> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT tip, base_tip, result FROM gate_runs
                 WHERE feature_id = ?1 AND gate = 'full' ORDER BY id",
            )
            .unwrap();
        let rows = stmt
            .query_map([FEATURE], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    }

    fn pass_full_gate(&self) {
        let (id, job): (i64, i64) = self
            .conn
            .query_row(
                "SELECT id, job_id FROM gate_runs
                 WHERE feature_id = ?1 AND gate = 'full' AND result IS NULL
                 ORDER BY id DESC LIMIT 1",
                [FEATURE],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let tx = self.conn.unchecked_transaction().unwrap();
        rows::set_gate_result(&tx, None, id, job, "pass", cause("the gate ran")).unwrap();
        tx.execute("UPDATE jobs SET state = 'passed' WHERE id = ?1", [job])
            .unwrap();
        tx.commit().unwrap();
    }

    fn feature_events(&self) -> i64 {
        self.conn
            .query_row(
                "SELECT count(*) FROM events WHERE entity = 'feature' AND entity_id = ?1",
                [FEATURE],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn ready(&self) {
        let (t1, b1) = self.task("one");
        let (t2, b2) = self.task("two");
        self.commit_on(RUN_BRANCH, "b.txt", "b\n");
        self.fetched_main(&self.tip("main"));
        self.settle(&t1, "done", &b1, "landed");
        self.settle(&t2, "cancelled", &b2, "abandoned");
        self.integrating();
        self.follow();
        self.pass_full_gate();
        self.follow();
        assert_eq!(self.state(), "ready");
    }

    fn squash_merge(&self) -> String {
        git(&self.repo, &["merge", "--quiet", "--squash", RUN_BRANCH]);
        git(&self.repo, &["commit", "--quiet", "-m", "squash"]);
        self.commit_on("main", "c.txt", "c\n")
    }
}

struct Counting<'a> {
    inner: &'a CliGitGateway,
    calls: Cell<usize>,
}

impl Counting<'_> {
    fn call(&self) {
        self.calls.set(self.calls.get() + 1);
    }
}

impl IntegrationGitGateway for Counting<'_> {
    fn tip(&self, repo: &Path, name: &str) -> Result<Option<String>> {
        self.call();
        self.inner.tip(repo, name)
    }
    fn update_ref(
        &self,
        repo: &Path,
        name: &str,
        new: &str,
        expected_old: Option<&str>,
    ) -> Result<RefMove> {
        self.call();
        self.inner.update_ref(repo, name, new, expected_old)
    }
    fn delete_ref(&self, repo: &Path, name: &str, expected_old: &str) -> Result<RefMove> {
        self.call();
        self.inner.delete_ref(repo, name, expected_old)
    }
    fn contains(&self, repo: &Path, tip: &str, ancestor: &str) -> Result<bool> {
        self.call();
        self.inner.contains(repo, tip, ancestor)
    }
    fn changes_in(&self, repo: &Path, tip: &str, base: &str, target: &str) -> Result<bool> {
        self.call();
        self.inner.changes_in(repo, tip, base, target)
    }
    fn is_ancestor(&self, repo: &Path, a: &str, b: &str) -> Result<bool> {
        self.call();
        self.inner.is_ancestor(repo, a, b)
    }
    fn build_candidate(&self, repo: &Path, base: &str, members: &[Member]) -> Result<Candidate> {
        self.call();
        self.inner.build_candidate(repo, base, members)
    }
    fn fleet_branches(&self, repo: &Path) -> Result<Vec<BranchTip>> {
        self.call();
        self.inner.fleet_branches(repo)
    }
    fn task_branches(&self, repo: &Path, run_branch: &str) -> Result<Vec<BranchTip>> {
        self.call();
        self.inner.task_branches(repo, run_branch)
    }

    fn stash_branches(&self, repo: &Path) -> Result<Vec<String>> {
        self.call();
        self.inner.stash_branches(repo)
    }
    fn fleet_toml(&self, repo: &Path, tip: &str) -> Result<Option<String>> {
        self.call();
        self.inner.fleet_toml(repo, tip)
    }
    fn ensure_settings(&self, dir: &Path) -> Result<()> {
        self.call();
        self.inner.ensure_settings(dir)
    }
}

#[test]
fn feature_is_ready_after_its_branches_landed_and_the_full_gate_passed() {
    let w = world();
    w.fetched_main(&w.tip("main"));
    w.follow();
    assert_eq!(w.state(), "open");
    assert!(w.full_gates().is_empty());

    let (t1, b1) = w.task("one");
    let (t2, b2) = w.task("two");
    let tip = w.commit_on(RUN_BRANCH, "b.txt", "b\n");
    let main = w.tip("main");
    w.settle(&t1, "done", &b1, "landed");
    w.follow();
    assert_eq!(w.state(), "open");
    assert!(w.full_gates().is_empty());

    w.settle(&t2, "cancelled", &b2, "building");
    w.follow();
    assert!(w.full_gates().is_empty());

    w.settle(&t2, "cancelled", &b2, "abandoned");
    w.integrating();
    w.follow();
    assert_eq!(w.state(), "open");
    assert_eq!(w.full_gates(), vec![(tip.clone(), main.clone(), None)]);
    let job: (String, String) = w
        .conn
        .query_row(
            "SELECT kind, subject FROM jobs ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(job, ("full_gate".to_string(), FEATURE.to_string()));

    w.follow();
    assert_eq!(w.full_gates().len(), 1);
    assert_eq!(w.state(), "open");

    w.pass_full_gate();
    w.follow();
    assert_eq!(w.state(), "ready");
    w.follow();
    assert_eq!(w.state(), "ready");
}

#[test]
fn full_gate_waits_for_the_gate_tree_of_the_feature() {
    let w = bare_world();
    w.fetched_main(&w.tip("main"));
    let (t1, b1) = w.task("one");
    let tip = w.commit_on(RUN_BRANCH, "b.txt", "b\n");
    let main = w.tip("main");
    w.settle(&t1, "done", &b1, "landed");
    w.integrating();

    w.follow();
    assert!(w.full_gates().is_empty(), "no gate tree, no gate");
    let tree = gate_tree::of_feature(&w.conn, &w.project_root(), FEATURE)
        .unwrap()
        .expect("the reserved row of the gate tree");
    let adds: Vec<(i64, String)> = {
        let mut stmt = w
            .conn
            .prepare("SELECT id, subject FROM jobs WHERE kind = 'worktree_add'")
            .unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    };
    assert_eq!(adds.len(), 1, "{adds:?}");
    assert_eq!(adds[0].1, tree.worktree_id);

    w.make_gate_tree(adds[0].0);
    w.follow();
    assert_eq!(w.full_gates(), vec![(tip, main, None)]);
}

#[test]
fn moved_primary_tip_makes_the_feature_open_and_the_rebase_comes_before_the_full_gate() {
    let w = world();
    w.ready();
    let moved = w.commit_on("main", "c.txt", "c\n");
    w.follow();
    assert_eq!(w.state(), "ready");

    w.fetched_main(&moved);
    w.follow();
    assert_eq!(w.state(), "open");
    w.follow();
    assert_eq!(w.state(), "open");
    assert_eq!(
        w.full_gates().len(),
        1,
        "no full gate on a feature tip that does not contain the fetched primary tip"
    );
    assert!(
        !rebase::gate_tree_busy(&w.conn, FEATURE).unwrap(),
        "no full gate holds the gate tree"
    );
    w.seen_run_branch();
    let old = w.tip(RUN_BRANCH);
    let effects = w.follow_rebase();
    assert!(
        matches!(&effects[..], [Effect::Rescue { branch, tip, .. }]
            if branch == RUN_BRANCH && *tip == old),
        "the rescue of the old feature tip: {effects:?}"
    );
    w.rescue(&effects[0]);
    assert!(w.follow_rebase().is_empty());
    assert!(
        jobs_store::active(&w.conn, Kind::Rebase, RUN_BRANCH)
            .unwrap()
            .is_some(),
        "the rebase job of {RUN_BRANCH} is queued"
    );

    git(&w.repo, &["rebase", "--quiet", "main", RUN_BRANCH]);
    git(&w.repo, &["checkout", "--quiet", "main"]);
    let rebased = w.tip(RUN_BRANCH);
    w.follow();
    let gates = w.full_gates();
    assert_eq!(gates.len(), 2);
    assert_eq!(gates[1], (rebased, moved, None));
    w.pass_full_gate();
    w.follow();
    assert_eq!(w.state(), "ready");
}

#[test]
fn moved_feature_tip_makes_the_feature_open() {
    let w = world();
    w.ready();
    w.commit_on(RUN_BRANCH, "d.txt", "d\n");
    w.follow();
    assert_eq!(w.state(), "open");
}

#[test]
fn squash_merged_feature_is_merged() {
    let w = world();
    w.ready();
    let merged = w.squash_merge();
    w.follow();
    assert_eq!(w.state(), "ready");

    w.fetched_main(&merged);
    w.follow();
    assert_eq!(w.state(), "merged");
}

#[test]
fn merged_is_decided_once() {
    let w = world();
    w.ready();
    let base = w.tip("main");
    let merged = w.squash_merge();
    w.fetched_main(&merged);
    w.follow();
    assert_eq!(w.state(), "merged");
    let events = w.feature_events();

    w.fetched_main(&base);
    let counting = Counting {
        inner: &w.git,
        calls: Cell::new(0),
    };
    w.follow_with(&counting);
    assert_eq!(w.state(), "merged");
    assert_eq!(counting.calls.get(), 0);
    assert_eq!(w.feature_events(), events);
}

#[test]
fn remote_none_reads_the_local_primary_branch() {
    let w = world();
    let (t1, b1) = w.task("one");
    w.commit_on(RUN_BRANCH, "b.txt", "b\n");
    let tip = w.commit_on(RUN_BRANCH, "fleet.toml", "remote = \"none\"\n");
    w.settle(&t1, "done", &b1, "landed");
    w.integrating();
    let side = w.commit_on("main", "c.txt", "c\n");
    git(&w.repo, &["reset", "--quiet", "--hard", "HEAD~1"]);
    w.fetched_main(&side);
    let main = w.tip("main");

    w.follow();
    assert_eq!(w.full_gates(), vec![(tip, main, None)]);
    w.pass_full_gate();
    w.follow();
    assert_eq!(w.state(), "ready");

    git(&w.repo, &["branch", "elsewhere"]);
    git(&w.repo, &["checkout", "--quiet", "elsewhere"]);
    git(&w.repo, &["merge", "--quiet", "--squash", RUN_BRANCH]);
    git(&w.repo, &["commit", "--quiet", "-m", "squash"]);
    let squashed = w.tip("HEAD");
    git(&w.repo, &["checkout", "--quiet", "main"]);
    w.fetched_main(&squashed);
    w.follow();
    assert_eq!(w.state(), "ready");

    git(&w.repo, &["merge", "--quiet", "--ff-only", "elsewhere"]);
    w.follow();
    assert_eq!(w.state(), "merged");
}

#[test]
fn invalid_feature_row_between_two_valid_rows_fails_only_its_row_and_advances_both_valid_rows() {
    use horch_core::fleetd::ideas::application::needs_you::open_items;
    use horch_core::fleetd::integration::application::features::FOLLOW_FAILED_KIND;

    let w = bare_world();
    for n in [2, 3] {
        let branch = format!("fleet/r{n}");
        git(&w.repo, &["branch", &branch]);
        let new_run = NewRun {
            slug: format!("r{n}"),
            base_branch: Some("main".into()),
            run_branch: Some(branch.clone()),
            ..Default::default()
        };
        let run = runs::create(&w.conn, &w.repo, &new_run).unwrap();
        let id = format!("F-{n:04}");
        let name = format!("r{n}");
        let new = NewFeature {
            id: &id,
            name: Some(&name),
            branch: Some(&branch),
            primary_branch: "main",
        };
        let tx = w.conn.unchecked_transaction().unwrap();
        rows::insert_feature(&tx, None, &new, cause("feature accepted")).unwrap();
        schema::run_idea(&tx, &run.run_id, &util::now(), true).unwrap();
        tx.commit().unwrap();
    }
    w.fetched_main(&w.tip("main"));
    w.conn
        .execute("UPDATE features SET state = 'ready'", [])
        .unwrap();
    w.conn
        .execute(
            "UPDATE features SET branch = 'fleet/r2 bad' WHERE id = 'F-0002'",
            [],
        )
        .unwrap();
    let state = |id: &str| rows::feature(&w.conn, id).unwrap().unwrap().state;
    let failed = || -> Vec<(String, String)> {
        open_items(&w.conn)
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == FOLLOW_FAILED_KIND)
            .map(|i| (i.key, i.entity_id))
            .collect()
    };

    let ctx = w.jobs();
    let f = Features {
        jobs: &ctx,
        git: &w.git,
    };
    let tx = w.conn.unchecked_transaction().unwrap();
    let steps = features::follow(&f, &tx, FleetSeconds(0)).unwrap();
    tx.commit().unwrap();
    let ids: Vec<&str> = steps.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["F-0001", "F-0002", "F-0003"]);
    assert!(matches!(steps[1].1, features::Step::Waits(_)), "{steps:?}");
    assert_eq!(state("F-0001"), "open");
    assert_eq!(state("F-0002"), "ready");
    assert_eq!(state("F-0003"), "open");
    let item = (format!("{FOLLOW_FAILED_KIND}:F-0002"), "F-0002".to_string());
    assert_eq!(failed(), vec![item.clone()]);

    w.follow();
    assert_eq!(failed(), vec![item]);

    w.conn
        .execute(
            "UPDATE features SET branch = 'fleet/r2' WHERE id = 'F-0002'",
            [],
        )
        .unwrap();
    w.follow();
    assert_eq!(state("F-0002"), "open");
    assert!(failed().is_empty());
}

fn gate_tree_end(w: &World) -> (String, bool) {
    use horch_core::fleetd::worktree_custody::application::remove;
    let tree = gate_tree::of_feature(&w.conn, &w.project_root(), FEATURE)
        .unwrap()
        .expect("the gate tree row");
    let ended = remove::owners_ended(&w.conn).unwrap();
    let given_up = ended
        .get(&tree.worktree_id)
        .is_some_and(|e| !e.keep_branch && e.rescue_job.is_none());
    (tree.state.as_str().to_string(), given_up)
}

#[test]
fn merged_feature_releases_its_gate_tree() {
    let w = world();
    w.ready();
    assert_eq!(gate_tree_end(&w), ("created".to_string(), false));
    let merged = w.squash_merge();
    w.fetched_main(&merged);

    w.follow();

    assert_eq!(w.state(), "merged");
    assert_eq!(gate_tree_end(&w), ("orphaned".to_string(), true));
}

#[test]
fn abandoned_feature_releases_its_gate_tree() {
    use horch_core::fleetd::integration::application::commands;
    let w = world();
    let ctx = w.jobs();
    let tx = w.conn.unchecked_transaction().unwrap();

    commands::record_feature(&ctx, &tx, FEATURE, &[], "the operator stops it").unwrap();

    tx.commit().unwrap();
    assert_eq!(w.state(), "abandoned");
    assert_eq!(gate_tree_end(&w), ("orphaned".to_string(), true));
}
