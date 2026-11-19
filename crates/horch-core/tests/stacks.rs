//! Stacks: a trial merge of 2 features that conflicts puts the younger
//! feature on the older one, 1 time, with 1 resolver task that lands on the
//! younger feature (docs/specs/fleetd/design.md, "Between features", "Stack
//! on evidence" and "Starting values"; ddd/08-code/integration/design.md,
//! `StackPlanner`, `StackFeature` and `OpenResolverTask`;
//! docs/tickets/fleetd-p6/37-stacks.md).
//!
//! Each test makes its own temporary ledger and git repository. No test
//! touches this repository or the live ledger. No test starts a process:
//! a test plays the job process of a trial merge with `run_job` of this
//! process.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, JobCtx, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea, Requirement};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::exec_trial_merge::CliTrialMergeGit;
use horch_core::fleetd::integration::adapters::git::CliStackGit;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, FeatureFact};
use horch_core::fleetd::integration::application::new_features::{open_feature, Project};
use horch_core::fleetd::integration::application::stacks::{self, Outcome, Stacking};
use horch_core::fleetd::integration::application::trial_merges::{self, Primary};
use horch_core::fleetd::integration::domain::stack::StackPlanner;
use horch_core::fleetd::task_execution::application::planned_tasks::{
    add_planned_tasks, PlannedTask,
};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs;
use horch_core::taskledger::workers::Worker;
use horch_core::taskledger::Store;
use horch_marketplace::git::scrub_repo_env;
use rusqlite::{Connection, TransactionBehavior};

/// A work budget of trial merges that no test reaches.
const WIDE: usize = 100;

/// The profile of a resolver task in these tests.
const RESOLVER_PROFILE: &str = "opus-hardening";

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    conn: Connection,
    config: FleetConfig,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    std::fs::write(repo.join("b.txt"), "b\n").unwrap();
    std::fs::write(repo.join("c.txt"), "c\n").unwrap();
    git(&repo, &["add", "a.txt", "b.txt", "c.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let db = root.join("ledger.sqlite3");
    drop(Store::open(&db).unwrap());
    let conn = Connection::open(&db).unwrap();
    World {
        _dir: dir,
        root,
        repo,
        conn,
        config: FleetConfig::default(),
    }
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> anyhow::Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

/// 1 task of a feature, as a test plans it.
struct Work<'a> {
    key: &'a str,
    check: &'a str,
}

impl World {
    fn ctx(&self) -> JobCtx<'_> {
        JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }

    /// The git of a pass: it only reads the repository.
    fn reader(&self) -> CliWorktreeGit {
        CliWorktreeGit::new("git", self.root.join("fleet"))
            .reading(std::slice::from_ref(&self.repo))
    }

    fn stacking(&self) -> Stacking<'_> {
        Stacking {
            repo: &self.repo,
            ideas: Path::new("/fleet/ideas"),
            profile: RESOLVER_PROFILE,
            planner: StackPlanner {
                max_lines: self.config.resolver_max_lines,
                max_files: self.config.resolver_max_files,
            },
            guard: None,
        }
    }

    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    /// A new `open` feature with its internal run and the branch
    /// `fleet/<id>` made from `main`, and 1 commit on it that writes
    /// `files`. Returns the feature id.
    fn feature(&self, files: &[(&str, &str)]) -> String {
        let project = Project {
            root: &self.repo,
            primary_branch: "main",
        };
        let id = self.write(|tx| open_feature(tx, &self.ctx(), &project, cause("idea")).unwrap());
        let branch = format!("fleet/{}", id.to_lowercase());
        git(&self.repo, &["branch", &branch, "main"]);
        self.commit_on(&branch, files);
        self.write(|tx| {
            rows::set_feature_fact(tx, None, &id, FeatureFact::Branch(&branch), cause("named"))
                .unwrap()
        });
        id
    }

    /// Set `created_at` of the feature `id`.
    fn feature_created(&self, id: &str, at: &str) {
        self.conn
            .execute(
                "UPDATE features SET created_at = ?2 WHERE id = ?1",
                [id, at],
            )
            .unwrap();
    }

    /// The idea `id` of `feature` in `state`, made at `at`, with the
    /// requirements `reqs`.
    fn idea(&self, feature: &str, id: &str, state: &str, at: &str, reqs: &[(&str, &str)]) {
        self.write(|tx| {
            let new = NewIdea {
                id,
                feature_id: feature,
                priority: 0,
            };
            ideas_store::insert_idea(tx, None, &new, cause("idea added")).unwrap();
            let reqs: Vec<Requirement<'_>> = reqs
                .iter()
                .map(|(req_id, text)| Requirement { req_id, text })
                .collect();
            if !reqs.is_empty() {
                ideas_store::insert_requirements(tx, None, id, &reqs, cause("plan")).unwrap();
            }
        });
        self.conn
            .execute(
                "UPDATE ideas SET state = ?2, created_at = ?3 WHERE id = ?1",
                [id, state, at],
            )
            .unwrap();
    }

    /// The tasks `work` of the idea `idea` in the run of `feature`.
    fn tasks(&self, feature: &str, idea: &str, work: &[Work<'_>]) {
        let run = runs::of_feature(&self.conn, feature).unwrap().unwrap();
        let planned: Vec<PlannedTask> = work
            .iter()
            .map(|w| PlannedTask {
                key: w.key.into(),
                title: w.key.into(),
                task_file: "/fleet/ideas/plan.yaml".into(),
                ticket_snapshot: None,
                work_kind: None,
                profile: "opus".into(),
                land_check: Some(w.check.into()),
                spec_json: "{}".into(),
                after: Vec::new(),
                judges: None,
                own: Vec::new(),
            })
            .collect();
        self.write(|tx| {
            add_planned_tasks(tx, None, &run, idea, &planned, cause("plan")).unwrap();
        });
    }

    /// Write `files` on `branch` in 1 commit, with no checkout. Returns the
    /// new tip.
    fn commit_on(&self, branch: &str, files: &[(&str, &str)]) -> String {
        let index = self.root.join("scratch-index");
        let env = [("GIT_INDEX_FILE", index.to_str().unwrap())];
        git_env(&self.repo, &env, &["read-tree", branch]);
        for (name, text) in files {
            let blob = git_in(&self.repo, &["hash-object", "-w", "--stdin"], text);
            let entry = format!("100644,{blob},{name}");
            git_env(
                &self.repo,
                &env,
                &["update-index", "--add", "--cacheinfo", &entry],
            );
        }
        let tree = git_env(&self.repo, &env, &["write-tree"]);
        let parent = self.tip(branch);
        let commit = git(
            &self.repo,
            &["commit-tree", &tree, "-p", &parent, "-m", "work"],
        );
        git(
            &self.repo,
            &["update-ref", &format!("refs/heads/{branch}"), &commit],
        );
        std::fs::remove_file(index).unwrap();
        commit
    }

    fn write<T>(&self, f: impl FnOnce(&Connection) -> T) -> T {
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
            .unwrap();
        let out = f(&tx);
        tx.commit().unwrap();
        out
    }

    /// The trial merges of today's tips: queue them, play each job, and
    /// record each verdict.
    fn trial_merge(&mut self) {
        let git = CliTrialMergeGit::new(self.reader());
        let tip = self.tip("main");
        let repo = self.repo.clone();
        let primary = Primary {
            repo: &repo,
            branch: "main",
            tip: &tip,
        };
        let ids = self.write(|tx| {
            trial_merges::queue_due(&self.ctx(), tx, &git, &primary, WIDE, FleetSeconds(0)).unwrap()
        });
        for id in ids {
            self.run_job(id);
        }
        self.write(|tx| trial_merges::record_ended(&self.ctx(), tx).unwrap());
    }

    /// Play the job process of the run `id`: it starts, runs and writes
    /// its result.
    fn run_job(&mut self, id: i64) {
        let me = std::process::id();
        self.write(|tx| {
            transition(
                tx,
                None,
                Entity::Job,
                &id.to_string(),
                "running",
                cause("started"),
                &NeedsYouList,
            )
            .unwrap();
            horch_core::fleetd::fleet_runtime::adapters::jobs_store::set_started(
                tx,
                id,
                me,
                None,
                FleetSeconds(1_000),
            )
            .unwrap();
        });
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
            sweep: None,
        };
        let config = self.config.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let written = jobs::run_job(&ctx, &mut self.conn, id, me, &checks).unwrap();
        assert!(written.is_some(), "the job process wrote its result");
        jobs::end_from_files(&ctx, &mut self.conn).unwrap();
    }

    /// 1 pass of the stacks.
    fn stack(&self) -> Vec<Outcome> {
        let git = CliStackGit::new(self.reader());
        let s = self.stacking();
        self.write(|tx| stacks::stack_conflicts(tx, &s, &git, cause("trial merge conflicted")))
            .unwrap()
    }

    fn stacked_on(&self, feature: &str) -> Option<String> {
        rows::feature(&self.conn, feature)
            .unwrap()
            .unwrap()
            .stacked_on
    }

    /// The tasks of the run of `feature` that are not `fixtures`, as
    /// (task id, idea id, ticket snapshot), in order.
    fn resolvers(&self, feature: &str) -> Vec<Resolver> {
        let run = runs::of_feature(&self.conn, feature).unwrap().unwrap();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT task_id, idea_id, ticket_snapshot, work_kind, primary_persona, status,
                        task_file, spec_json
                 FROM tasks WHERE run_id = ?1 AND title LIKE 'Resolve %' ORDER BY seq",
            )
            .unwrap();
        stmt.query_map([&run.run_id], |r| {
            Ok(Resolver {
                id: r.get(0)?,
                idea: r.get(1)?,
                ticket: r.get(2)?,
                work_kind: r.get(3)?,
                profile: r.get(4)?,
                status: r.get(5)?,
                task_file: r.get(6)?,
                spec: r.get(7)?,
            })
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }
}

#[derive(Debug)]
struct Resolver {
    id: String,
    idea: Option<String>,
    ticket: Option<String>,
    work_kind: Option<String>,
    profile: Option<String>,
    status: String,
    task_file: Option<String>,
    spec: Option<String>,
}

impl Resolver {
    fn ticket(&self) -> &str {
        self.ticket.as_deref().unwrap_or_default()
    }

    fn spec(&self) -> serde_json::Value {
        serde_json::from_str(self.spec.as_deref().unwrap()).unwrap()
    }
}

/// `lines` lines of the text `word`.
fn lines(word: &str, lines: usize) -> String {
    (0..lines).map(|i| format!("{word} {i}\n")).collect()
}

/// 2 features that change `a.txt` in 2 ways, each with 1 idea in flight,
/// 1 requirement and 1 task with its unit tests. The first is the older.
fn two_conflicting(w: &World) -> (String, String) {
    let older = w.feature(&[("a.txt", "older\n")]);
    let younger = w.feature(&[("a.txt", "younger\n")]);
    w.feature_created(&older, "2026-11-01T10:00:00Z");
    w.feature_created(&younger, "2026-11-02T10:00:00Z");
    w.idea(
        &older,
        "I-older",
        "in_progress",
        "2026-11-01T10:00:00Z",
        &[("R1", "Older writes a.")],
    );
    w.idea(
        &younger,
        "I-younger",
        "in_progress",
        "2026-11-02T10:00:00Z",
        &[("R1", "Younger writes a.")],
    );
    w.tasks(
        &older,
        "I-older",
        &[Work {
            key: "older-a",
            check: "cargo test -p older",
        }],
    );
    w.tasks(
        &younger,
        "I-younger",
        &[Work {
            key: "younger-a",
            check: "cargo test -p younger",
        }],
    );
    (older, younger)
}

// ─── tests ──────────────────────────────────────────────────────────────

#[test]
fn given_2_features_touching_1_file_when_trial_merge_then_younger_stacked_and_1_resolver_task() {
    let mut w = world();
    let (older, younger) = two_conflicting(&w);
    let older_tip = w.tip(&format!("fleet/{}", older.to_lowercase()));
    let younger_tip = w.tip(&format!("fleet/{}", younger.to_lowercase()));

    w.trial_merge();
    let outcomes = w.stack();

    let [Outcome::Stacked {
        upper,
        lower,
        idea,
        tasks,
    }] = outcomes.as_slice()
    else {
        panic!("1 conflict stacks 1 feature: {outcomes:?}");
    };
    assert_eq!((upper, lower), (&younger, &older));
    assert_eq!(idea, "I-younger");
    assert_eq!(w.stacked_on(&younger).as_deref(), Some(older.as_str()));
    assert_eq!(
        w.stacked_on(&older),
        None,
        "the older feature stays at the bottom"
    );

    let resolvers = w.resolvers(&younger);
    assert_eq!(resolvers.len(), 1, "1 resolver task: {resolvers:?}");
    let r = &resolvers[0];
    assert_eq!(tasks, &vec![r.id.clone()]);
    assert!(
        w.resolvers(&older).is_empty(),
        "the task lands on the younger feature"
    );
    assert_eq!(
        r.work_kind.as_deref(),
        Some("feature"),
        "a resolver task has a code kind"
    );
    assert_eq!(r.profile.as_deref(), Some(RESOLVER_PROFILE));
    assert_eq!(r.status, "ready");
    assert_eq!(
        r.task_file.as_deref(),
        Some("/fleet/ideas/I-younger/plan.yaml")
    );

    let ticket = r.ticket();
    // The conflict in zdiff3 form: the 2 sides and the base.
    for part in [
        "<<<<<<<", "|||||||", "=======", ">>>>>>>", "older", "younger", "`a.txt`",
    ] {
        assert!(
            ticket.contains(part),
            "the ticket holds {part:?}:\n{ticket}"
        );
    }
    // The requirements and the unit tests of both features.
    for part in [
        "Older writes a.",
        "Younger writes a.",
        "cargo test -p older",
        "cargo test -p younger",
        older_tip.as_str(),
        younger_tip.as_str(),
        "1 commit for each requirement",
    ] {
        assert!(
            ticket.contains(part),
            "the ticket holds {part:?}:\n{ticket}"
        );
    }

    let spec = r.spec();
    assert_eq!(spec["kind"], "feature");
    assert_eq!(spec["profile"], RESOLVER_PROFILE);
    assert_eq!(spec["resolves"]["lower"], older.as_str());
    assert_eq!(spec["resolves"]["upper"], younger.as_str());
    assert_eq!(spec["resolves"]["files"], serde_json::json!(["a.txt"]));

    let events = w.count(&format!(
        "SELECT COUNT(*) FROM events WHERE entity = 'feature' AND entity_id = '{younger}'
           AND data_json LIKE '%resolver_task%'"
    ));
    assert_eq!(events, 1, "1 resolver-task-opened event");
}

#[test]
fn conflicting_features_stack_younger_on_older_once() {
    let mut w = world();
    let (first, second) = two_conflicting(&w);
    // The feature with the lower id is the younger one: the order is the
    // time each feature was made.
    w.feature_created(&first, "2026-11-03T10:00:00Z");
    let (older, younger) = (second, first);

    w.trial_merge();
    let outcomes = w.stack();
    assert!(
        matches!(outcomes.as_slice(), [Outcome::Stacked { upper, lower, .. }]
            if *upper == younger && *lower == older),
        "{outcomes:?}"
    );

    // The same conflict in the next pass: no 2nd edge, no 2nd task.
    let again = w.stack();
    assert!(
        again
            .iter()
            .all(|o| matches!(o, Outcome::AlreadyStacked { .. })),
        "{again:?}"
    );

    // New tips that still conflict: the edge stays, and no 2nd task.
    w.commit_on(
        &format!("fleet/{}", younger.to_lowercase()),
        &[("a.txt", "younger 2\n")],
    );
    w.trial_merge();
    let later = w.stack();
    assert!(
        !later.iter().any(|o| matches!(o, Outcome::Stacked { .. })),
        "{later:?}"
    );

    assert_eq!(w.stacked_on(&younger).as_deref(), Some(older.as_str()));
    assert_eq!(w.stacked_on(&older), None);
    assert_eq!(w.resolvers(&younger).len(), 1);
    assert!(w.resolvers(&older).is_empty());
    let edges = w.count(&format!(
        "SELECT COUNT(*) FROM events WHERE entity = 'feature' AND entity_id = '{younger}'
           AND data_json LIKE '%stacked_on%'"
    ));
    assert_eq!(edges, 1, "1 stack edge event");
}

#[test]
fn resolver_task_belongs_to_an_idea_of_the_younger_feature() {
    let mut w = world();
    let older = w.feature(&[("a.txt", "older\n")]);
    let younger = w.feature(&[("a.txt", "younger\n")]);
    w.feature_created(&older, "2026-11-01T10:00:00Z");
    w.feature_created(&younger, "2026-11-02T10:00:00Z");
    // The older feature has the youngest idea of all: it is not a choice.
    w.idea(
        &older,
        "I-older",
        "in_progress",
        "2026-11-09T10:00:00Z",
        &[],
    );
    w.idea(
        &younger,
        "I-first",
        "in_progress",
        "2026-11-02T10:00:00Z",
        &[],
    );
    w.idea(
        &younger,
        "I-second",
        "ticketing",
        "2026-11-03T10:00:00Z",
        &[],
    );
    // Not in flight: an idea that waits for its gate, a parked idea and a
    // cancelled idea.
    w.idea(
        &younger,
        "I-integrating",
        "integrating",
        "2026-11-04T10:00:00Z",
        &[],
    );
    w.idea(&younger, "I-parked", "parked", "2026-11-05T10:00:00Z", &[]);
    w.idea(
        &younger,
        "I-cancelled",
        "cancelled",
        "2026-11-06T10:00:00Z",
        &[],
    );

    w.trial_merge();
    let outcomes = w.stack();

    assert!(
        matches!(outcomes.as_slice(), [Outcome::Stacked { idea, .. }] if idea == "I-second"),
        "{outcomes:?}"
    );
    let resolvers = w.resolvers(&younger);
    assert_eq!(resolvers.len(), 1);
    assert_eq!(resolvers[0].idea.as_deref(), Some("I-second"));
    assert_eq!(
        w.count(&format!(
            "SELECT COUNT(*) FROM tasks WHERE task_id = '{}' AND idea_id = 'I-second'",
            resolvers[0].id
        )),
        1,
        "FR-12: the task row names its idea"
    );
}

#[test]
fn resolver_task_waits_when_the_younger_feature_has_no_idea_in_flight() {
    let mut w = world();
    let older = w.feature(&[("a.txt", "older\n")]);
    let younger = w.feature(&[("a.txt", "younger\n")]);
    w.feature_created(&older, "2026-11-01T10:00:00Z");
    w.feature_created(&younger, "2026-11-02T10:00:00Z");
    w.idea(&younger, "I-parked", "parked", "2026-11-05T10:00:00Z", &[]);

    w.trial_merge();
    let outcomes = w.stack();

    assert!(
        matches!(outcomes.as_slice(), [Outcome::NoIdea { upper }] if *upper == younger),
        "{outcomes:?}"
    );
    assert_eq!(
        w.stacked_on(&younger),
        None,
        "no edge without its resolver task"
    );
    assert!(w.resolvers(&younger).is_empty());
}

#[test]
fn large_conflict_is_split_by_file() {
    let mut w = world();
    // 2 files are in the limit of files, but their conflict has more than
    // 100 changed lines.
    let older = w.feature(&[
        ("a.txt", &lines("older a", 30)),
        ("b.txt", &lines("older b", 30)),
    ]);
    let younger = w.feature(&[
        ("a.txt", &lines("younger a", 30)),
        ("b.txt", &lines("younger b", 30)),
    ]);
    w.feature_created(&older, "2026-11-01T10:00:00Z");
    w.feature_created(&younger, "2026-11-02T10:00:00Z");
    w.idea(
        &younger,
        "I-younger",
        "in_progress",
        "2026-11-02T10:00:00Z",
        &[],
    );

    w.trial_merge();
    let outcomes = w.stack();

    let [Outcome::Stacked { tasks, .. }] = outcomes.as_slice() else {
        panic!("{outcomes:?}");
    };
    assert_eq!(tasks.len(), 2, "1 resolver task for each file");
    let resolvers = w.resolvers(&younger);
    let files: Vec<serde_json::Value> = resolvers
        .iter()
        .map(|r| r.spec()["resolves"]["files"].clone())
        .collect();
    assert_eq!(
        files,
        [serde_json::json!(["a.txt"]), serde_json::json!(["b.txt"])]
    );
    assert!(resolvers[0].ticket().contains("`a.txt`"));
    assert!(!resolvers[0].ticket().contains("`b.txt`"));
    assert_eq!(
        w.stacked_on(&younger).as_deref(),
        Some(older.as_str()),
        "1 edge"
    );
}

#[test]
fn small_conflict_of_2_files_is_1_resolver_task() {
    let mut w = world();
    let older = w.feature(&[("a.txt", "older a\n"), ("b.txt", "older b\n")]);
    let younger = w.feature(&[("a.txt", "younger a\n"), ("b.txt", "younger b\n")]);
    w.feature_created(&older, "2026-11-01T10:00:00Z");
    w.feature_created(&younger, "2026-11-02T10:00:00Z");
    w.idea(
        &younger,
        "I-younger",
        "in_progress",
        "2026-11-02T10:00:00Z",
        &[],
    );

    w.trial_merge();
    w.stack();

    let resolvers = w.resolvers(&younger);
    assert_eq!(resolvers.len(), 1);
    assert_eq!(
        resolvers[0].spec()["resolves"]["files"],
        serde_json::json!(["a.txt", "b.txt"])
    );
}

// ─── git ────────────────────────────────────────────────────────────────

fn git_cmd(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    scrub_repo_env(&mut cmd);
    cmd.current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
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
