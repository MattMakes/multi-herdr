use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, CheckRunner, JobCtx, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::exec_trial_merge::CliTrialMergeGit;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::integration::adapters::trial_merges_store::{self as store, TrialMergeRow};
use horch_core::fleetd::integration::application::trial_merges::{self, Primary};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::workers::Worker;
use horch_core::taskledger::Store;
use horch_marketplace::git::scrub_repo_env;
use horch_platform::channel::User;
use rusqlite::{Connection, TransactionBehavior};

const WIDE: usize = 100;

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    conn: Connection,
    config: FleetConfig,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(dir.path()).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    std::fs::write(repo.join("b.txt"), "b\n").unwrap();
    git(&repo, &["add", "a.txt", "b.txt"]);
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

impl World {
    fn ctx(&self) -> JobCtx<'_> {
        JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }

    fn pass_git(&self) -> CliTrialMergeGit {
        CliTrialMergeGit::new(
            CliWorktreeGit::new("git", self.root.join("fleet"))
                .reading(std::slice::from_ref(&self.repo)),
        )
    }

    fn tip(&self, rev: &str) -> String {
        git(&self.repo, &["rev-parse", rev])
    }

    fn feature(&self, id: &str, files: &[(&str, &str)]) {
        let branch = format!("fleet/{id}");
        git(&self.repo, &["branch", &branch, "main"]);
        self.commit_on(&branch, files);
        let new = NewFeature {
            id,
            name: Some(&id.to_lowercase()),
            branch: Some(&branch),
            primary_branch: "main",
        };
        self.write(|tx| rows::insert_feature(tx, None, &new, cause("feature accepted")).unwrap());
    }

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

    fn pass(&self, budget: usize) -> Vec<i64> {
        let git = self.pass_git();
        let tip = self.tip("main");
        let primary = Primary {
            repo: &self.repo,
            branch: "main",
            tip: &tip,
        };
        self.write(|tx| {
            trial_merges::record_ended(&self.ctx(), tx).unwrap();
            trial_merges::queue_due(&self.ctx(), tx, &git, &primary, budget, FleetSeconds(0))
                .unwrap()
        })
    }

    fn job(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    fn run(&mut self, id: i64) -> State {
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
            jobs_store::set_started(tx, id, me, None, FleetSeconds(1_000)).unwrap();
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
        let written = jobs::run_job(&ctx, &self.conn, id, me, &checks).unwrap();
        assert!(written.is_some(), "the job process wrote its result");
        self.ingest_results();
        self.job(id).state
    }

    fn ingest_results(&self) {
        let owner = User::current().unwrap();
        let reads = jobs::read_results(
            &self.conn,
            &self.root.join("fleet"),
            &owner,
            self.config.job_result_max_bytes,
        )
        .unwrap();
        self.write(|tx| jobs::ingest_results(&self.ctx(), tx, &reads).unwrap());
    }

    fn run_all(&mut self, ids: &[i64]) {
        for id in ids {
            self.run(*id);
        }
    }

    fn trial_jobs(&self) -> Vec<JobRow> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM jobs WHERE kind = 'trial_merge' ORDER BY id")
            .unwrap();
        let ids: Vec<i64> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        ids.into_iter().map(|id| self.job(id)).collect()
    }

    fn verdict(&self, left: &str, right: &str) -> TrialMergeRow {
        let (l, r) = (self.tip(left), self.tip(right));
        store::get(&self.conn, &l, &r)
            .unwrap()
            .unwrap_or_else(|| panic!("a trial_merges row of {left} and {right}"))
    }

    fn checkout_state(&self) -> (String, Vec<u8>, String) {
        (
            self.tip("HEAD"),
            std::fs::read(self.repo.join(".git/index")).unwrap(),
            git(&self.repo, &["status", "--porcelain", "--ignored"]),
        )
    }
}

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

#[test]
fn trial_merge_of_2_tips_runs_once() {
    let mut w = world();
    w.feature("F-0001", &[("a.txt", "a1\n")]);

    let first = w.pass(WIDE);
    assert_eq!(first.len(), 1, "1 job: F-0001 against main");
    let job = w.job(first[0]);
    assert_eq!(job.kind, Kind::TrialMerge);
    assert_eq!(job.repo.as_deref(), Some(w.repo.to_str().unwrap()));
    assert_eq!(job.state, State::Queued);
    let row = w.verdict("main", "fleet/F-0001");
    assert_eq!(row.clean, None, "clean is null while the job runs");
    assert_eq!(row.job_id, job.id);
    assert_eq!(row.left_ref, "refs/heads/main");
    assert_eq!(row.right_ref, "refs/heads/fleet/F-0001");

    assert_eq!(
        w.pass(WIDE),
        Vec::<i64>::new(),
        "a queued run is not queued again"
    );
    assert_eq!(w.run(first[0]), State::Passed);
    assert_eq!(
        w.pass(WIDE),
        Vec::<i64>::new(),
        "the verdict of the 2 tips is used again"
    );
    assert_eq!(w.verdict("main", "fleet/F-0001").clean, Some(true));
    assert_eq!(w.pass(WIDE), Vec::<i64>::new());
    assert_eq!(w.trial_jobs().len(), 1, "the 2 tips had 1 run");

    w.commit_on("fleet/F-0001", &[("a.txt", "a2\n")]);
    assert_eq!(w.pass(WIDE).len(), 1, "a new tip is a new input key");
    assert_eq!(w.trial_jobs().len(), 2);
}

#[test]
fn pair_with_no_shared_file_gets_no_job() {
    let w = world();
    w.feature("F-0001", &[("a.txt", "a1\n")]);
    w.feature("F-0002", &[("b.txt", "b2\n")]);

    let queued = w.pass(WIDE);
    let subjects: Vec<String> = w.trial_jobs().into_iter().map(|j| j.subject).collect();
    assert_eq!(queued.len(), 2, "{subjects:?}");
    assert_eq!(subjects, ["main+F-0001", "main+F-0002"]);
}

#[test]
fn conflict_is_recorded_with_its_files() {
    let mut w = world();
    w.feature("F-0001", &[("a.txt", "a1\n"), ("b.txt", "b1\n")]);
    w.feature("F-0002", &[("a.txt", "a2\n"), ("b.txt", "b2\n")]);

    let queued = w.pass(WIDE);
    let subjects: Vec<String> = w.trial_jobs().into_iter().map(|j| j.subject).collect();
    assert_eq!(subjects, ["main+F-0001", "main+F-0002", "F-0001+F-0002"]);
    w.run_all(&queued);
    let pair = w.trial_jobs()[2].clone();
    assert_eq!(pair.state, State::Failed, "a conflict is a failed run");
    assert!(
        jobs::outcome(&pair).unwrap().verdict,
        "a conflict is a verdict"
    );

    assert_eq!(
        w.pass(WIDE),
        Vec::<i64>::new(),
        "a conflict verdict is used again"
    );
    let row = w.verdict("fleet/F-0001", "fleet/F-0002");
    assert_eq!(row.clean, Some(false));
    assert_eq!(row.conflict_files, ["a.txt", "b.txt"]);
    assert_eq!(row.job_id, pair.id);
    let clean = w.verdict("main", "fleet/F-0002");
    assert_eq!(clean.clean, Some(true));
    assert!(clean.conflict_files.is_empty());
}

#[test]
fn trial_merge_job_does_not_check_out() {
    let mut w = world();
    w.feature("F-0001", &[("a.txt", "a1\n")]);
    w.feature("F-0002", &[("a.txt", "a2\n")]);
    let before = w.checkout_state();

    let queued = w.pass(WIDE);
    assert_eq!(queued.len(), 3);
    w.run_all(&queued);

    assert_eq!(
        w.checkout_state(),
        before,
        "HEAD, index and tree did not move"
    );
    assert_eq!(
        std::fs::read_to_string(w.repo.join("a.txt")).unwrap(),
        "a\n"
    );
    assert_eq!(
        w.pass(WIDE),
        Vec::<i64>::new(),
        "the next pass records the verdicts"
    );
    assert_eq!(w.verdict("fleet/F-0001", "fleet/F-0002").clean, Some(false));
    let heads = git(&w.repo, &["for-each-ref", "--format=%(refname)"]);
    assert_eq!(
        heads.lines().collect::<Vec<_>>(),
        [
            "refs/heads/fleet/F-0001",
            "refs/heads/fleet/F-0002",
            "refs/heads/main"
        ],
        "no ref names a trial merge"
    );
}

#[test]
fn n_features_do_not_make_n_times_n_jobs_in_1_pass() {
    let w = world();
    let n = 6;
    for i in 1..=n {
        w.feature(&format!("F-000{i}"), &[("a.txt", &format!("a{i}\n"))]);
    }
    let due = n + n * (n - 1) / 2;
    let budget = 4;

    let mut passes = 0;
    let mut total = 0;
    loop {
        let queued = w.pass(budget).len();
        assert!(queued <= budget, "pass {passes} queued {queued} runs");
        if queued == 0 {
            break;
        }
        total += queued;
        passes += 1;
        assert!(passes <= due, "the passes end");
    }
    assert_eq!(total, due, "each due trial merge is queued 1 time");
    assert_eq!(w.trial_jobs().len(), due);
    assert!(passes > 1, "the budget split the work over passes");
}
