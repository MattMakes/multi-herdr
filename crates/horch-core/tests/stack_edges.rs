//! A stack edge ends with its lower feature: when the lower feature is
//! `merged`, fleetd writes a rescue ref of the upper feature, rebases it on
//! the fetched primary tip, runs both gates again, clears `stacked_on` and
//! moves the base of its pull request. When the lower feature is
//! `abandoned`, the rebase takes the commits of the lower feature out. A
//! conflict opens 1 resolver task. Continuous rebase moves the bottom of a
//! stack first (docs/specs/fleetd/design.md, "Between features", I10,
//! FM-42; docs/tickets/fleetd-p6/38-stack-edges.md).
//!
//! Each test makes its own ledger, git repository and fake `gh` in a
//! temporary folder. No test touches this repository or the live ledger,
//! and no test reads the clock, the load or the limits of the machine: the
//! fleet clock is a number of the test.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, CheckRunner, JobCtx, JobRunner, Queued, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::integration::adapters::exec_fetch::Tools;
use horch_core::fleetd::integration::adapters::exec_gate::gate_key;
use horch_core::fleetd::integration::adapters::exec_push;
use horch_core::fleetd::integration::adapters::exec_rebase::GitRebaseRefs;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, FeatureFact, NewBranch,
};
use horch_core::fleetd::integration::application::bundles::{self, Bundles};
use horch_core::fleetd::integration::application::features::{self, Features};
use horch_core::fleetd::integration::application::landing;
use horch_core::fleetd::integration::application::new_features::{open_feature, Project};
use horch_core::fleetd::integration::application::rebase::{self, Effect, Rebases};
use horch_core::fleetd::integration::application::remote::{self, Fetched, PrRead};
use horch_core::fleetd::integration::application::stack_edges::{self, Edges};
use horch_core::fleetd::integration::domain::project::Remote;
use horch_core::fleetd::integration::domain::task_branch::LandingState;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::fleetd::worktree_custody::application::create::{self, Observed};
use horch_core::fleetd::worktree_custody::application::gate_tree::{
    self, GateTree, GateTreeRequest,
};
use horch_core::runtime::MapEnv;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs;
use horch_core::taskledger::schema;
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::Worker;
use horch_core::taskledger::Store;
use horch_marketplace::git::scrub_repo_env;
use rusqlite::{params, Connection, TransactionBehavior};

/// The web address of the remote, as `remote.origin.url` names it. A push
/// goes to the local bare repository through `url.<bare>.pushInsteadOf`.
const WEB: &str = "https://github.com/acme/x";

/// The address that the fake `gh` gives to a new pull request.
const NEW_PR: &str = "https://github.com/acme/x/pull/7";

/// The profile of a resolver task in these tests.
const RESOLVER_PROFILE: &str = "opus-hardening";

/// The fake `gh`: `pr view`, `pr create`, `pr edit` and `pr ready` of 1
/// pull request, kept as files in `<bin>/../gh/`, with its base branch.
const FAKE_GH: &str = r#"#!/bin/sh
d="$(cd "$(dirname "$0")/../gh" && pwd)"
echo "$*" >> "$d/log"
case "$1 $2" in
"pr view")
  [ -f "$d/url" ] || { echo "no pull requests found for branch $3" >&2; exit 1; }
  printf '{"url":"%s","state":"OPEN","isDraft":%s,"body":"","baseRefName":"%s"}\n' \
    "$(cat "$d/url")" "$(cat "$d/draft")" "$(cat "$d/base")"
  ;;
"pr create")
  shift 2
  while [ $# -gt 0 ]; do
    case "$1" in
      --base) echo "$2" > "$d/base"; shift ;;
      --head|--title|--body-file) shift ;;
    esac
    shift
  done
  echo "https://github.com/acme/x/pull/7" > "$d/url"
  echo false > "$d/draft"
  cat "$d/url"
  ;;
"pr edit")
  shift 3
  while [ $# -gt 0 ]; do
    case "$1" in
      --base) echo "$2" > "$d/base"; shift ;;
      --body-file) shift ;;
    esac
    shift
  done
  ;;
"pr ready")
  if [ "$4" = "--undo" ]; then echo true > "$d/draft"; else echo false > "$d/draft"; fi
  ;;
*)
  echo "fake gh: unknown call: $*" >&2
  exit 2
  ;;
esac
"#;

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    /// The repository, as its canonical path.
    repo: PathBuf,
    conn: Connection,
    config: FleetConfig,
    tools: Tools,
    tasks: u32,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn ctx(config: &FleetConfig) -> JobCtx<'_> {
    JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    }
}

/// The sandbox profiles of a test: no task check runs in these tests.
struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> anyhow::Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

/// A ledger of the current schema with the fleet clock at 0, a repository
/// with 1 commit on `main` and the remote `origin` (a local bare
/// repository), the fetched tip of `main`, and a fake `gh` on the PATH of
/// the tools.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mut conn = Connection::open(root.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, 1, 1, 'now', 0, 'now')",
        [],
    )
    .unwrap();
    std::fs::create_dir_all(root.join("fleet")).unwrap();

    let bare = root.join("remote.git");
    git(&root, &["init", "-q", "--bare", "-b", "main", path(&bare)]);
    let repo = root.join("repo");
    git(&root, &["init", "-q", "-b", "main", path(&repo)]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    git(&repo, &["remote", "add", "origin", WEB]);
    let rewrite = format!("url.{}.pushInsteadOf", path(&bare));
    git(&repo, &["config", &rewrite, WEB]);
    // The worktree_add job writes only the tree folder: its parent exists.
    std::fs::create_dir(repo.join(".worktrees")).unwrap();

    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(root.join("gh")).unwrap();
    std::fs::create_dir_all(root.join("tmp")).unwrap();
    let gh = bin.join("gh");
    std::fs::write(&gh, FAKE_GH).unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    let env = MapEnv {
        vars: BTreeMap::from([("PATH".to_string(), path(&bin).to_string())]),
        temp: Some(root.join("tmp")),
        ..MapEnv::default()
    };
    let tools = Tools::from_env(&env);
    assert_eq!(tools.gh.as_deref(), Some(gh.as_path()), "gh is on PATH");

    let w = World {
        _dir: dir,
        root,
        repo,
        conn,
        config: FleetConfig::default(),
        tools,
        tasks: 0,
    };
    let main = w.tip("main");
    w.fetched(&main);
    w
}

impl World {
    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    fn write<T>(&self, f: impl FnOnce(&Connection) -> T) -> T {
        let tx = self.conn.unchecked_transaction().unwrap();
        let out = f(&tx);
        tx.commit().unwrap();
        out
    }

    /// The fetch saw `tip` as the tip of `main`.
    fn fetched(&self, tip: &str) {
        let repo = self.repo.to_string_lossy().into_owned();
        self.write(|tx| rows::put_remote_tip(tx, None, &repo, "main", tip).unwrap());
    }

    /// Write `file` with `text` on `branch` in 1 commit, with no checkout.
    /// Returns the new tip.
    fn commit_on(&self, branch: &str, file: &str, text: &str) -> String {
        let parent = self.tip(&format!("refs/heads/{branch}"));
        let blob = git_in(&self.repo, &["hash-object", "-w", "--stdin"], text);
        let index = self.root.join("index.tmp");
        let env = [("GIT_INDEX_FILE", path(&index))];
        git_env(&self.repo, &env, &["read-tree", &parent]);
        let info = format!("100644,{blob},{file}");
        git_env(
            &self.repo,
            &env,
            &["update-index", "--add", "--cacheinfo", &info],
        );
        let tree = git_env(&self.repo, &env, &["write-tree"]);
        std::fs::remove_file(&index).unwrap();
        let commit = git(
            &self.repo,
            &["commit-tree", &tree, "-p", &parent, "-m", file],
        );
        let full = format!("refs/heads/{branch}");
        git(&self.repo, &["update-ref", &full, &commit, &parent]);
        commit
    }

    /// Merge `branch` into `main` with a merge commit, as you merge a pull
    /// request, and let the fetch see it. Returns the new tip of `main`.
    fn merge_into_main(&self, branch: &str) -> String {
        let main = self.tip("refs/heads/main");
        let other = self.tip(&format!("refs/heads/{branch}"));
        let tree = git(&self.repo, &["merge-tree", "--write-tree", &main, &other]);
        let msg = format!("merge {branch}");
        let commit = git(
            &self.repo,
            &["commit-tree", &tree, "-p", &main, "-p", &other, "-m", &msg],
        );
        git(
            &self.repo,
            &["update-ref", "refs/heads/main", &commit, &main],
        );
        self.fetched(&commit);
        commit
    }

    /// Move `main` by 1 commit of `file`, and let the fetch see it.
    fn move_primary(&self, file: &str, text: &str) -> String {
        let tip = self.commit_on("main", file, text);
        self.fetched(&tip);
        tip
    }

    /// A new `open` feature with its internal run, the branch
    /// `fleet/<id>` made from `from` with 1 commit of `file`, its branch row
    /// with `seen_tip`, and its gate tree. Returns (feature id, branch).
    fn feature(&mut self, from: &str, file: &str) -> (String, String) {
        let project = Project {
            root: &self.repo,
            primary_branch: "main",
        };
        let config = self.config.clone();
        let id = self.write(|tx| open_feature(tx, &ctx(&config), &project, cause("idea")).unwrap());
        let branch = format!("fleet/{}", id.to_lowercase());
        git(&self.repo, &["branch", &branch, from]);
        let tip = self.commit_on(&branch, file, &format!("{file}\n"));
        self.write(|tx| {
            rows::set_feature_fact(tx, None, &id, FeatureFact::Branch(&branch), cause("named"))
                .unwrap();
            let row = NewBranch {
                name: &branch,
                feature_id: &id,
                task_id: None,
                owner: "fleetd",
                due_at: 0,
            };
            rows::insert_branch(tx, None, &row, cause("feature branch")).unwrap();
            rows::set_branch_fact(tx, None, &branch, BranchFact::SeenTip(&tip), cause("seen"))
                .unwrap();
            runs::set_feature_run_branch(tx, &id, &branch).unwrap();
        });
        self.gate_tree(&id, &branch);
        (id, branch)
    }

    /// The upper feature `upper` is on the lower feature `lower`, as a
    /// trial merge that conflicts leaves it.
    fn stack(&self, upper: &str, lower: &str) {
        self.conn
            .execute(
                "UPDATE features SET stacked_on = ?2 WHERE id = ?1",
                [upper, lower],
            )
            .unwrap();
    }

    /// The feature `id` is in `state`, as the feature state machine or
    /// `horch feature abandon` leaves it.
    fn set_state(&self, id: &str, state: &str) {
        self.conn
            .execute("UPDATE features SET state = ?2 WHERE id = ?1", [id, state])
            .unwrap();
    }

    fn stacked_on(&self, id: &str) -> Option<String> {
        rows::feature(&self.conn, id).unwrap().unwrap().stacked_on
    }

    /// The idea `idea` of `feature` in `state`.
    fn idea(&self, feature: &str, idea: &str, state: &str) {
        self.write(|tx| {
            let new = NewIdea {
                id: idea,
                feature_id: feature,
                priority: 0,
            };
            ideas_store::insert_idea(tx, None, &new, cause("idea added")).unwrap();
            tx.execute("UPDATE ideas SET state = ?2 WHERE id = ?1", [idea, state])
                .unwrap();
        });
    }

    fn fleet_git(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.root.join("fleet"))
            .reading(std::slice::from_ref(&self.repo))
            .writing(&[self.repo.join(".git")])
    }

    /// The gate tree of `feature`, made by its `worktree_add` job and
    /// observed, as a pass observes it.
    fn gate_tree(&mut self, feature: &str, branch: &str) {
        let repo = self.repo.clone();
        let tip = self.tip(&format!("refs/heads/{branch}"));
        let run = runs::feature_run(&self.conn, feature).unwrap().unwrap();
        let req = GateTreeRequest {
            feature_id: feature,
            run_id: &run.run_id,
            repo: &repo,
            base: branch,
            tip: &tip,
        };
        let config = self.config.clone();
        let reserved =
            self.write(|tx| gate_tree::reserve(&ctx(&config), tx, &req, FleetSeconds(0)));
        let job = match reserved {
            Ok(GateTree::Reserved { job, .. }) => job,
            other => panic!("expected a reserved gate tree, got {other:?}"),
        };
        let ended = self.run_job(job);
        assert_eq!(ended.state, State::Passed, "{:?}", outcome(&ended));
        let seen = self.write(|tx| create::observe(&ctx(&config), tx, &ended).unwrap());
        assert!(matches!(seen, Observed::Created(_)), "{seen:?}");
    }

    /// 1 pass of the stack edges and of the rebase rule at the fleet clock
    /// `clock`, in that order and in 1 transaction, committed. Gives the
    /// effects of both.
    fn pass(&mut self, clock: i64) -> Vec<Effect> {
        let config = self.config.clone();
        let gw = CliGitGateway::new(self.fleet_git());
        let refs = GitRebaseRefs::new(self.fleet_git());
        let ctx = ctx(&config);
        let r = Rebases {
            jobs: &ctx,
            git: &gw,
            refs: &refs,
        };
        let e = Edges {
            rebases: &r,
            ideas: Path::new("/fleet/ideas"),
            profile: RESOLVER_PROFILE,
        };
        let tx = self.conn.unchecked_transaction().unwrap();
        let mut effects = stack_edges::follow(&e, &tx, FleetSeconds(clock)).unwrap();
        effects.extend(rebase::follow(&r, &tx, FleetSeconds(clock)).unwrap());
        tx.commit().unwrap();
        effects
    }

    /// Run each effect after the commit of its pass. With `observe`, record
    /// each result, as the next part of the pass does; without it, the pass
    /// crashed before it recorded them.
    fn effects(&mut self, effects: &[Effect], observe: bool) {
        let refs = GitRebaseRefs::new(self.fleet_git());
        let config = self.config.clone();
        for effect in effects {
            let done = rebase::run(effect, &refs).unwrap();
            if observe {
                self.write(|tx| rebase::observe(&ctx(&config), tx, &done).unwrap());
            }
        }
    }

    /// Passes, effects and rebase runs at the fleet clock from `clock`, until
    /// a pass changes nothing. Gives the next clock.
    fn settle(&mut self, mut clock: i64) -> i64 {
        for _ in 0..10 {
            let effects = self.pass(clock);
            clock += 1;
            let active = self.active_rebases();
            if effects.is_empty() && active.is_empty() {
                return clock;
            }
            self.effects(&effects, true);
            for job in active {
                self.run_job(job.id);
            }
        }
        panic!("the stack edge did not settle in 10 passes");
    }

    fn job(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    /// Each rebase run, oldest first.
    fn rebases(&self) -> Vec<JobRow> {
        self.jobs_of(Kind::Rebase)
    }

    fn active_rebases(&self) -> Vec<JobRow> {
        self.rebases()
            .into_iter()
            .filter(|j| matches!(j.state, State::Queued | State::Running))
            .collect()
    }

    fn jobs_of(&self, kind: Kind) -> Vec<JobRow> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM jobs WHERE kind = ?1 ORDER BY id")
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([kind.as_str()], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        ids.into_iter().map(|id| self.job(id)).collect()
    }

    /// Run the job `id` as `horch job run` runs it, and end it from its
    /// result file, as a pass ends it. Gives the ended row.
    fn run_job(&mut self, id: i64) -> JobRow {
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
        let ctx = ctx(&config);
        let mut runner = JobRunner::new(&Launcher);
        assert!(runner
            .start(&ctx, &mut self.conn, &job, &spec, FleetSeconds(0))
            .unwrap());
        let pid = self.job(id).pid.unwrap();
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
            sweep: None,
        };
        let written = jobs::run_job(&ctx, &mut self.conn, id, pid, &checks).unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        jobs::end_from_files(&ctx, &mut self.conn).unwrap();
        self.job(id)
    }

    /// Each rescue ref of the branch `branch`, as (name, tip).
    fn rescues(&self, branch: &str) -> Vec<(String, String)> {
        let folder = format!("refs/horch/rescue/branch/{}/", branch.replace('/', "-"));
        let out = git(
            &self.repo,
            &["for-each-ref", "--format=%(refname) %(objectname)", &folder],
        );
        out.lines()
            .filter_map(|l| l.split_once(' '))
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect()
    }

    /// The commits of `branch` that `main` does not have, newest first, by
    /// subject.
    fn own_commits(&self, branch: &str) -> Vec<String> {
        let range = format!("refs/heads/main..refs/heads/{branch}");
        let out = git(&self.repo, &["log", "--format=%s", &range]);
        out.lines().map(str::to_string).collect()
    }

    /// Each task of the internal run of `feature` whose title starts with
    /// "Resolve", as (task id, idea id, spec).
    fn resolvers(&self, feature: &str) -> Vec<(String, String, serde_json::Value)> {
        let run = runs::of_feature(&self.conn, feature).unwrap().unwrap();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT task_id, idea_id, spec_json FROM tasks
                 WHERE run_id = ?1 AND title LIKE 'Resolve %' ORDER BY seq",
            )
            .unwrap();
        stmt.query_map([&run.run_id], |r| {
            let spec: String = r.get(2)?;
            Ok((r.get(0)?, r.get(1)?, serde_json::from_str(&spec).unwrap()))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    /// The text of each open needs-you item.
    fn needs_you(&self) -> Vec<String> {
        needs_you::open_items(&self.conn)
            .unwrap()
            .into_iter()
            .map(|i| i.text)
            .collect()
    }

    /// A task of `feature`, its git branch `name` from the feature branch
    /// with 1 commit of `file`, and its task branch row in `to`.
    fn task_branch(&mut self, feature: &str, branch: &str, name: &str, file: &str, to: &str) {
        git(&self.repo, &["branch", name, branch]);
        self.commit_on(name, file, &format!("{name}\n"));
        self.tasks += 1;
        let n = self.tasks;
        let run = runs::feature_run(&self.conn, feature).unwrap().unwrap();
        let new = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), "dev");
        let config = self.config.clone();
        self.write(|tx| {
            let task = tasks::add(tx, &run, &new, Actor::Orchestrator)
                .unwrap()
                .task_id;
            let row = NewBranch {
                name,
                feature_id: feature,
                task_id: Some(&task),
                owner: "worker",
                due_at: 0,
            };
            rows::insert_branch(tx, None, &row, cause("task bound")).unwrap();
            let ctx = ctx(&config);
            for state in [LandingState::Verifying, LandingState::Ready] {
                landing::enter(&ctx, tx, name, state, FleetSeconds(0), "its task is done").unwrap();
            }
        });
        if to == "landed" {
            self.conn
                .execute(
                    "UPDATE branches SET landing_state = 'landed' WHERE name = ?1",
                    [name],
                )
                .unwrap();
        }
    }

    /// 1 pass of the feature state machine at `clock`, committed.
    fn features_pass(&mut self, clock: i64) -> Vec<(String, features::Step)> {
        let config = self.config.clone();
        let gw = CliGitGateway::new(self.fleet_git());
        let ctx = ctx(&config);
        let f = Features {
            jobs: &ctx,
            git: &gw,
        };
        self.write(|tx| features::follow(&f, tx, FleetSeconds(clock)).unwrap())
    }

    /// 1 pass of the bundles at `clock`, committed.
    fn bundles_pass(&mut self, clock: i64) {
        let config = self.config.clone();
        let gw = CliGitGateway::new(self.fleet_git());
        let ctx = ctx(&config);
        let b = Bundles {
            jobs: &ctx,
            git: &gw,
        };
        self.write(|tx| bundles::advance(&b, tx, FleetSeconds(clock)).unwrap());
    }

    /// The full gate run `id` passed: its job and its `gate_runs` row.
    fn gate_passed(&self, id: i64) {
        self.conn
            .execute("UPDATE jobs SET state = 'passed' WHERE id = ?1", [id])
            .unwrap();
        self.conn
            .execute(
                "UPDATE gate_runs SET result = 'pass' WHERE job_id = ?1",
                params![id],
            )
            .unwrap();
    }

    fn gh_dir(&self) -> PathBuf {
        self.root.join("gh")
    }

    fn gh_log(&self) -> Vec<String> {
        std::fs::read_to_string(self.gh_dir().join("log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The base branch of the pull request of the fake `gh`.
    fn pr_base(&self) -> String {
        std::fs::read_to_string(self.gh_dir().join("base"))
            .unwrap()
            .trim()
            .to_string()
    }

    /// Queue the push of `feature` at `clock` when the push rule wants it
    /// (`rebased`: its branch was rebased), run it, and end it passed.
    fn push(&mut self, feature: &str, rebased: bool, clock: i64) {
        self.conn
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [clock])
            .unwrap();
        let config = self.config.clone();
        let repo = self.repo.to_string_lossy().into_owned();
        let queued = self.write(|tx| {
            remote::queue_push(
                &ctx(&config),
                tx,
                &repo,
                feature,
                &Remote::default(),
                rebased,
                FleetSeconds(clock),
            )
            .unwrap()
        });
        let id = match queued {
            Some(Queued::New(id)) => id,
            Some(Queued::Active(row)) => row.id,
            other => panic!("no push run of {feature} at {clock} s: {other:?}"),
        };
        self.run_push(id);
    }

    /// Run the push run `id`, and end it passed.
    fn run_push(&mut self, id: i64) {
        let row = self.job(id);
        let job = Store::open_read_only(Path::new(self.conn.path().unwrap())).unwrap();
        let ran = exec_push::run_with(job.conn(), &row, &self.tools, &Launcher).unwrap();
        assert!(ran.outcome.passed, "{}", ran.outcome.text);
        let config = self.config.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        exec_push::record(&ctx(&config), &tx, &row, &ran).unwrap();
        tx.commit().unwrap();
        self.conn
            .execute(
                "UPDATE jobs SET state = 'passed', ended_at = 'now' WHERE id = ?1",
                [id],
            )
            .unwrap();
    }
}

/// A stack: the lower feature with 1 commit of `l.txt`, and the upper
/// feature on it with 1 more commit of `u.txt`. Returns (lower, lower
/// branch, upper, upper branch).
fn stack(w: &mut World) -> (String, String, String, String) {
    let (lower, lower_branch) = w.feature("main", "l.txt");
    let (upper, upper_branch) = w.feature(&lower_branch, "u.txt");
    w.stack(&upper, &lower);
    (lower, lower_branch, upper, upper_branch)
}

fn is_ancestor(repo: &Path, a: &str, b: &str) -> bool {
    git_cmd(repo, &["merge-base", "--is-ancestor", a, b])
        .status()
        .unwrap()
        .success()
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn lower_feature_merged_rebases_the_upper_on_the_primary_tip() {
    let mut w = world();
    let (lower, lower_branch, upper, upper_branch) = stack(&mut w);
    let old = w.tip(&upper_branch);
    w.move_primary("p.txt", "p\n");
    let primary = w.merge_into_main(&lower_branch);
    w.set_state(&lower, "merged");

    w.settle(1);

    let new = w.tip(&upper_branch);
    assert_ne!(new, old);
    assert!(
        is_ancestor(&w.repo, &primary, &new),
        "the upper is on the primary tip"
    );
    assert_eq!(w.own_commits(&upper_branch), vec!["u.txt".to_string()]);
    assert_eq!(w.stacked_on(&upper), None, "the stack edge is cleared");
    let rebases = w.rebases();
    assert_eq!(rebases.len(), 1, "1 rebase of the upper feature");
    assert_eq!(rebases[0].subject, upper_branch);
    assert_eq!(rebases[0].input_key, rebase::rebase_key(&old, &primary));
    assert_eq!(rebases[0].state, State::Passed);
    // The next pass records the new tip of the upper feature.
    w.pass(100);
    let seen = rows::branch(&w.conn, &upper_branch)
        .unwrap()
        .unwrap()
        .seen_tip;
    assert_eq!(seen.as_deref(), Some(new.as_str()));
    assert!(w.active_rebases().is_empty());
}

#[test]
fn lower_feature_abandoned_rebases_the_upper_without_its_commits() {
    let mut w = world();
    let (lower, lower_branch, upper, upper_branch) = stack(&mut w);
    let old = w.tip(&upper_branch);
    let lower_tip = w.tip(&lower_branch);
    let primary = w.move_primary("p.txt", "p\n");
    w.set_state(&lower, "abandoned");

    w.settle(1);

    let new = w.tip(&upper_branch);
    assert!(
        is_ancestor(&w.repo, &primary, &new),
        "the upper is on the primary tip"
    );
    assert!(
        !is_ancestor(&w.repo, &lower_tip, &new),
        "the upper has no commit of the abandoned feature"
    );
    assert_eq!(w.own_commits(&upper_branch), vec!["u.txt".to_string()]);
    let files = git(&w.repo, &["ls-tree", "--name-only", &new]);
    assert!(!files.lines().any(|f| f == "l.txt"), "{files}");
    assert_eq!(w.stacked_on(&upper), None);
    let rebases = w.rebases();
    assert_eq!(rebases.len(), 1, "1 rebase of the upper feature");
    assert_eq!(
        rebases[0].input_key,
        rebase::rebase_key_cut(&old, &primary, &lower_tip)
    );
}

#[test]
fn rescue_ref_exists_before_the_rebase() {
    let mut w = world();
    let (lower, lower_branch, upper, upper_branch) = stack(&mut w);
    let old = w.tip(&upper_branch);
    let primary = w.merge_into_main(&lower_branch);
    w.set_state(&lower, "merged");

    // The first pass gives the rescue and queues nothing.
    let effects = w.pass(1);
    assert_eq!(
        effects,
        vec![Effect::Rescue {
            repo: w.repo.clone(),
            branch: upper_branch.clone(),
            tip: old.clone(),
        }]
    );
    assert!(w.rebases().is_empty(), "no rebase before the rescue ref");
    assert!(
        w.rescues(&upper_branch).is_empty(),
        "the pass writes no ref"
    );
    assert_eq!(w.stacked_on(&upper).as_deref(), Some(lower.as_str()));

    // A crash after the rescue ref and before the commit of its row.
    w.effects(&effects, false);
    assert_eq!(w.rescues(&upper_branch).len(), 1);
    let effects = w.pass(2);
    assert_eq!(effects.len(), 1, "the next pass writes the rescue again");
    w.effects(&effects, true);
    assert_eq!(w.rescues(&upper_branch).len(), 1, "the same ref");
    assert_eq!(w.rescues(&upper_branch)[0].1, old, "it names the old tip");

    assert!(w.pass(3).is_empty());
    let rebases = w.rebases();
    assert_eq!(rebases.len(), 1, "the rebase is queued after the rescue");
    assert_eq!(rebases[0].input_key, rebase::rebase_key(&old, &primary));
    let row = rows::branch(&w.conn, &upper_branch).unwrap().unwrap();
    let rescue = row.rescue_ref.expect("the branch row names the rescue ref");
    assert_eq!(w.tip(&rescue), old);
}

#[test]
fn both_gates_run_again_after_the_rebase() {
    let mut w = world();
    let (lower, lower_branch, upper, upper_branch) = stack(&mut w);
    // FR-12: the task of the upper feature needs an idea in flight.
    w.idea(&upper, "I-1", "in_progress");
    w.task_branch(&upper, &upper_branch, "fleet/t1", "t1.txt", "landed");
    // Each idea of the upper feature is integrating.
    w.conn
        .execute(
            "UPDATE ideas SET state = 'integrating' WHERE feature_id = ?1",
            [&upper],
        )
        .unwrap();
    w.commit_on(&upper_branch, "t1.txt", "fleet/t1\n");
    let old = w.tip(&upper_branch);
    w.write(|tx| {
        rows::set_branch_fact(
            tx,
            None,
            &upper_branch,
            BranchFact::SeenTip(&old),
            cause("t"),
        )
        .unwrap()
    });
    let main = w.tip("main");
    // The full gate of the upper feature passed on its old tips.
    let steps = w.features_pass(1);
    let full = w.jobs_of(Kind::FullGate);
    assert_eq!(full.len(), 1, "{steps:?}");
    assert_eq!(full[0].input_key, gate_key(&old, &main));
    w.gate_passed(full[0].id);
    w.features_pass(2);
    assert_eq!(
        rows::feature(&w.conn, &upper).unwrap().unwrap().state,
        "ready"
    );

    let primary = w.merge_into_main(&lower_branch);
    w.set_state(&lower, "merged");
    let clock = w.settle(3);
    let new = w.tip(&upper_branch);
    assert_ne!(new, old);
    assert_eq!(w.stacked_on(&upper), None);

    // The full gate: the old pass is stale, and a new run judges the new
    // tips.
    w.features_pass(clock);
    w.features_pass(clock + 1);
    let full = w.jobs_of(Kind::FullGate);
    assert_eq!(full.len(), 2, "{full:?}");
    assert_eq!(full[1].input_key, gate_key(&new, &primary));
    assert_eq!(
        rows::feature(&w.conn, &upper).unwrap().unwrap().state,
        "open"
    );

    // The fast gate: a ready task branch is bundled on the new tip, and its
    // fast gate judges the bundle over the new tip.
    w.gate_passed(full[1].id);
    // FR-12: the idea I-1 is integrating, so the task t3 needs an idea in flight.
    w.idea(&upper, "I-2", "in_progress");
    w.task_branch(&upper, &upper_branch, "fleet/t3", "t3.txt", "ready");
    w.bundles_pass(clock + 2);
    let builds = w.jobs_of(Kind::BundleBuild);
    assert_eq!(builds.len(), 1, "{builds:?}");
    let built = w.run_job(builds[0].id);
    assert_eq!(built.state, State::Passed, "{:?}", outcome(&built));
    w.bundles_pass(clock + 3);
    let fast = w.jobs_of(Kind::FastGate);
    assert_eq!(fast.len(), 1, "{fast:?}");
    assert!(
        fast[0].input_key.ends_with(&format!("\"{new}\"]")),
        "the fast gate judges over the new tip: {}",
        fast[0].input_key
    );
}

#[test]
fn pr_base_moves_to_the_primary_branch() {
    let mut w = world();
    let (lower, lower_branch, upper, _) = stack(&mut w);
    // The stacked upper feature is ready: its pull request has the branch
    // of the lower feature as its base.
    w.set_state(&upper, "ready");
    w.push(&upper, false, 1);
    let create = w
        .gh_log()
        .into_iter()
        .find(|c| c.starts_with("pr create"))
        .expect("a pull request is made");
    assert!(
        create.contains(&format!("--base {lower_branch}")),
        "{create}"
    );
    assert_eq!(w.pr_base(), lower_branch);

    w.merge_into_main(&lower_branch);
    w.set_state(&lower, "merged");
    let clock = w.settle(2);
    assert_eq!(w.stacked_on(&upper), None);
    // The pass after the rebase queues the push of the new tip.
    w.pass(clock);
    let push = jobs_store::active(&w.conn, Kind::Push, &upper)
        .unwrap()
        .expect("a push after the rebase");
    w.run_push(push.id);
    assert_eq!(w.pr_base(), "main", "{:?}", w.gh_log());
    assert!(
        w.gh_log()
            .iter()
            .any(|c| c == &format!("pr edit {NEW_PR} --base main")),
        "{:?}",
        w.gh_log()
    );
    assert_eq!(
        w.gh_log()
            .iter()
            .filter(|c| c.starts_with("pr create"))
            .count(),
        1,
        "the pull request is moved, not made again"
    );
}

#[test]
fn pr_manual_item_names_the_primary_branch_after_the_edge() {
    let mut w = world();
    let (lower, lower_branch) = w.feature("main", "l.txt");
    git(&w.repo, &["branch", "fleet/up", &lower_branch]);
    w.commit_on("fleet/up", "fleet.toml", "pr = \"manual\"\n");
    let (upper, upper_branch) = w.feature("fleet/up", "u.txt");
    w.stack(&upper, &lower);
    w.set_state(&upper, "ready");
    w.push(&upper, false, 1);
    let stacked = format!("{WEB}/compare/{lower_branch}...{upper_branch}");
    assert!(
        w.needs_you().iter().any(|t| t.contains(&stacked)),
        "{:?}",
        w.needs_you()
    );

    w.merge_into_main(&lower_branch);
    w.set_state(&lower, "merged");
    let clock = w.settle(2);
    w.pass(clock);
    let push = jobs_store::active(&w.conn, Kind::Push, &upper)
        .unwrap()
        .expect("a push after the rebase");
    w.run_push(push.id);
    let unstacked = format!("{WEB}/compare/main...{upper_branch}");
    let items = w.needs_you();
    assert!(items.iter().any(|t| t.contains(&unstacked)), "{items:?}");
    assert!(!items.iter().any(|t| t.contains(&stacked)), "{items:?}");
    assert!(w.gh_log().is_empty(), "manual makes no gh call");
}

#[test]
fn stack_is_rebased_bottom_first() {
    let mut w = world();
    let (lower, lower_branch, upper, upper_branch) = stack(&mut w);
    let primary = w.move_primary("p.txt", "p\n");

    // The first passes rescue and rebase only the lower feature.
    let effects = w.pass(1);
    let branches: Vec<&str> = effects
        .iter()
        .map(|Effect::Rescue { branch, .. }| branch.as_str())
        .collect();
    assert_eq!(branches, vec![lower_branch.as_str()]);
    w.effects(&effects, true);
    assert!(w.pass(2).is_empty());
    let rebases = w.rebases();
    assert_eq!(rebases.len(), 1);
    assert_eq!(rebases[0].subject, lower_branch, "the bottom first");
    assert!(
        w.pass(3).is_empty(),
        "the upper waits for its lower feature"
    );
    assert_eq!(w.rebases().len(), 1);

    let ended = w.run_job(rebases[0].id);
    assert_eq!(ended.state, State::Passed, "{:?}", outcome(&ended));
    assert!(is_ancestor(&w.repo, &primary, &w.tip(&lower_branch)));

    // Then the upper feature.
    let effects = w.pass(4);
    let branches: Vec<&str> = effects
        .iter()
        .map(|Effect::Rescue { branch, .. }| branch.as_str())
        .collect();
    assert_eq!(branches, vec![upper_branch.as_str()]);
    w.effects(&effects, true);
    w.pass(5);
    let rebases = w.rebases();
    assert_eq!(rebases.len(), 2);
    assert_eq!(rebases[1].subject, upper_branch);
    let ended = w.run_job(rebases[1].id);
    assert_eq!(ended.state, State::Passed, "{:?}", outcome(&ended));
    assert!(is_ancestor(&w.repo, &primary, &w.tip(&upper_branch)));
    assert_eq!(w.stacked_on(&upper).as_deref(), Some(lower.as_str()));
}

#[test]
fn idempotent_pull_request_closed() {
    let mut w = world();
    let (lower, _, upper, upper_branch) = stack(&mut w);
    let old = w.tip(&upper_branch);
    w.conn
        .execute(
            "UPDATE features SET pr = ?2 WHERE id = ?1",
            [lower.as_str(), NEW_PR],
        )
        .unwrap();
    let repo = w.repo.to_string_lossy().into_owned();
    let fetched = Fetched {
        tips: Vec::new(),
        prs: vec![(lower.clone(), PrRead::Closed)],
    };
    // 2 fetches read the closed pull request of the lower feature.
    for clock in [1, 2] {
        w.write(|tx| remote::record_passed(tx, None, &NeedsYouList, &repo, &fetched).unwrap());
        assert!(w.pass(clock).is_empty());
    }
    let items: Vec<String> = w
        .needs_you()
        .into_iter()
        .filter(|t| t.contains("closed"))
        .collect();
    assert_eq!(items.len(), 1, "1 needs-you item: {items:?}");
    assert!(items[0].contains(&lower), "{}", items[0]);
    assert!(w.rebases().is_empty(), "no rebase");
    assert!(w.rescues(&upper_branch).is_empty(), "no rescue ref");
    assert_eq!(w.stacked_on(&upper).as_deref(), Some(lower.as_str()));
    assert_eq!(w.tip(&upper_branch), old);
}

#[test]
fn rebase_conflict_opens_1_resolver_task() {
    let mut w = world();
    let (lower, _, upper, upper_branch) = stack(&mut w);
    w.idea(&upper, "I-1", "captured");
    // The upper feature changes a.txt, and so does the primary branch.
    let old = w.commit_on(&upper_branch, "a.txt", "upper\n");
    w.write(|tx| {
        rows::set_branch_fact(
            tx,
            None,
            &upper_branch,
            BranchFact::SeenTip(&old),
            cause("t"),
        )
        .unwrap()
    });
    w.move_primary("a.txt", "primary\n");
    w.set_state(&lower, "abandoned");

    let clock = w.settle(1);
    for c in [clock, clock + 1] {
        assert!(w.pass(c).is_empty());
    }
    assert_eq!(w.tip(&upper_branch), old, "the branch did not move");
    let rebases = w.rebases();
    assert_eq!(rebases.len(), 1);
    assert_eq!(rebases[0].state, State::Failed);
    let resolvers = w.resolvers(&upper);
    assert_eq!(resolvers.len(), 1, "1 resolver task: {resolvers:?}");
    let (_, idea, spec) = &resolvers[0];
    assert_eq!(idea, "I-1");
    assert_eq!(spec["outputs"], serde_json::json!(["a.txt"]));
    assert_eq!(w.stacked_on(&upper).as_deref(), Some(lower.as_str()));
}

// ─── git ────────────────────────────────────────────────────────────────

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn git_cmd(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    scrub_repo_env(&mut cmd);
    cmd.current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args);
    cmd
}

fn checked(cmd: &mut Command, args: &[&str]) -> String {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn git(dir: &Path, args: &[&str]) -> String {
    checked(&mut git_cmd(dir, args), args)
}

fn git_env(dir: &Path, env: &[(&str, &str)], args: &[&str]) -> String {
    let mut cmd = git_cmd(dir, args);
    cmd.envs(env.iter().copied());
    checked(&mut cmd, args)
}

fn git_in(dir: &Path, args: &[&str], input: &str) -> String {
    use std::io::Write;
    let mut child = git_cmd(dir, args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}
