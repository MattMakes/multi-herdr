//! The e2e tests of features: FM-39, FM-41 and FM-42, and the gate test of a
//! merge with ideas in flight (`docs/specs/fleetd/design.md`, "Failure
//! modes" FM-39, FM-41, FM-42; "Migration plan" item 8, the gate tests;
//! `docs/tickets/fleetd-p6/61-e2e-features.md`).
//!
//! Each test makes its own temp project with a bare remote `origin`, its own
//! ledger and fleet root. Each feature is the feature of 1 run (Q5) with its
//! idea in flight (FR-12). The test writes the rows that a worker, its task
//! and the operator leave, changes git as you or a worker would (a merge of
//! a pull request is a merge into `main` of the remote by hand), and then
//! runs fleetd only as `horch ledger watch --once` passes. A pass starts its
//! jobs as `horch job run` processes, and the test waits for those processes
//! to end before the next pass.
//!
//! Each process runs with temporary `HORCH_DATA_DIR`, `HORCH_STATE_DIR`,
//! `HORCH_LEDGER_DB` and `FLEET_ROOT`. No test reads the load, the limits or
//! the clock of this machine: the harness gives the machine facts through
//! `HORCH_MACHINE_FILE`, the test `fleet.toml` gives the keys, and each wait
//! is for an event (a number of passes), never for a time. No test starts an
//! agent CLI, and `HORCH_TEST_DOCS_FIXTURE` keeps the docs sync off the
//! network.

#![cfg(unix)]

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::process::Output;

use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, BranchFact, NewBranch};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, NewWorktree};
use horch_core::taskledger::model::{Actor, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util};
use horch_e2e::harness::{wait_for_or, Harness};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, Connection, OptionalExtension};

/// The name of the bare remote of each project.
const REMOTE: &str = "origin";

/// The primary branch of each feature.
const PRIMARY: &str = "main";

/// The persona of each seeded task. No task of a test is dispatched.
const PERSONA: &str = "dev";

/// The `fleet.toml` of the fleet root: the defaults of the phase.
const FLEET_TOML: &str = "pass_period_s = 1\nfetch_period_s = 60\n";

/// After a push to the remote, the fleet clock moves by 1 fetch period
/// before the next pass, in place of a wait for it. A fetch on each pass
/// would hold the 1 git writer of the project and stop each other git job.
const FETCH_PERIOD_S: i64 = 60;

/// The project file: the remote `origin`, no `gh` call, and 2 gates that
/// pass, so no hold of a missing gate stops a feature.
const PROJECT_TOML: &str =
    "remote = \"origin\"\npr = \"manual\"\n\n[gates]\nfast = \"true\"\nfull = \"true\"\n";

/// The most passes that a test waits for 1 fact.
const MAX_PASSES: usize = 30;

/// The pane of the sub-orchestrator of an idea, as fleetd records it.
const PANE: &str = "pane-7";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Orchestrator,
        text,
    }
}

/// Whether the process `pid` runs.
fn alive(pid: i64) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 sends nothing; it only checks that the pid exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// The rescue ref folder of the branch `branch` (Q16 of fleetd-p4): the
/// branch name with `-` in place of `/`.
fn rescue_folder(branch: &str) -> String {
    format!("refs/horch/rescue/branch/{}/", branch.replace('/', "-"))
}

// ─── the world of 1 test ────────────────────────────────────────────────

/// 1 feature of a test: the run under it, its id, its branch and its idea.
struct Feature {
    run: Run,
    id: String,
    branch: String,
    idea: String,
}

/// A temp project on `main` with its bare remote `origin`, its committed
/// project file, a ledger and a fleet root.
struct World {
    h: Harness,
    /// The output of each pass, for the text of a failed wait.
    passes: RefCell<Vec<String>>,
    /// A push to the remote that no fetch period has passed since.
    pushed: Cell<bool>,
}

fn world(name: &str) -> World {
    let mut h = Harness::new(name).with_git();
    let fleet = h.root.join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
    let docs = h.root.join("docs-fixture");
    std::fs::create_dir_all(&docs).unwrap();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
    h.set("FLEET_ROOT", fleet.to_string_lossy());
    h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    let remote = h.root.join("remote.git");
    std::fs::write(h.project.join("fleet.toml"), PROJECT_TOML).unwrap();
    let bare = remote.to_string_lossy().into_owned();
    for args in [
        &["add", "fleet.toml"][..],
        &["commit", "-q", "--no-gpg-sign", "-m", "the project file"],
        &["init", "-q", "--bare", &bare],
        &["remote", "add", REMOTE, &bare],
        &["push", "-q", REMOTE, PRIMARY],
    ] {
        let o = h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
    }
    let mut conn = Connection::open(h.ledger_db()).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    World {
        h,
        passes: RefCell::new(Vec::new()),
        pushed: Cell::new(false),
    }
}

impl World {
    fn conn(&self) -> Connection {
        let conn = Connection::open(self.h.ledger_db()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        conn
    }

    /// The project folder as the ledger names it.
    fn root(&self) -> PathBuf {
        self.h.project.canonicalize().unwrap()
    }

    /// Run git in the project, and give its trimmed stdout.
    fn git(&self, args: &[&str]) -> String {
        let o = self.h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// The commit of `rev` in the project, or `None`.
    fn tip(&self, rev: &str) -> Option<String> {
        let o = self
            .h
            .git_cmd(&["rev-parse", "--verify", "-q", rev])
            .output()
            .unwrap();
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    /// Whether `ancestor` is `tip` or an ancestor of it.
    fn contains(&self, tip: &str, ancestor: &str) -> bool {
        self.h
            .git_cmd(&["merge-base", "--is-ancestor", ancestor, tip])
            .status()
            .unwrap()
            .success()
    }

    /// Whether the commit `rev` has the file `file`.
    fn has_file(&self, rev: &str, file: &str) -> bool {
        self.h
            .git_cmd(&["cat-file", "-e", &format!("{rev}:{file}")])
            .status()
            .unwrap()
            .success()
    }

    /// Commit `file` with `content` on `branch`, made from `from` when it
    /// is new. The project is back on `main` after it. Gives the new tip.
    fn commit_on(&self, branch: &str, from: &str, file: &str, content: &str) -> String {
        if self.tip(&format!("refs/heads/{branch}")).is_some() {
            self.git(&["checkout", "-q", branch]);
        } else {
            self.git(&["checkout", "-q", "-b", branch, from]);
        }
        std::fs::write(self.h.project.join(file), content).unwrap();
        self.git(&["add", file]);
        self.git(&["commit", "-q", "--no-gpg-sign", "-m", file]);
        let tip = self.git(&["rev-parse", "HEAD"]);
        self.git(&["checkout", "-q", PRIMARY]);
        tip
    }

    /// Merge `branch` into `onto`, as a landing does.
    fn merge_into(&self, onto: &str, branch: &str) {
        self.git(&["checkout", "-q", onto]);
        self.git(&[
            "merge",
            "-q",
            "--no-ff",
            "--no-gpg-sign",
            "-m",
            branch,
            branch,
        ]);
        self.git(&["checkout", "-q", PRIMARY]);
    }

    /// You merge the pull request of `branch`: a squash merge into `main`,
    /// pushed to the primary branch of the bare remote.
    fn merge_pull_request(&self, branch: &str) {
        self.git(&["merge", "-q", "--squash", branch]);
        self.git(&[
            "commit",
            "-q",
            "--no-gpg-sign",
            "-m",
            &format!("squash {branch}"),
        ]);
        self.git(&["push", "-q", REMOTE, PRIMARY]);
        self.pushed.set(true);
    }

    /// Record the tip of `branch` as the tip that fleetd saw, as fleetd
    /// records it when it makes or moves the branch.
    fn seen(&self, branch: &str) {
        let tip = self.tip(branch).unwrap();
        let conn = self.conn();
        let tx = conn.unchecked_transaction().unwrap();
        let fact = BranchFact::SeenTip(&tip);
        rows::set_branch_fact(&tx, None, branch, fact, cause("its tip")).unwrap();
        tx.commit().unwrap();
    }

    /// A new feature: the run `slug` on the branch `branch`, made from
    /// `from`, with its idea in flight, as `horch idea add` leaves them.
    fn feature(&self, slug: &str, branch: &str, from: &str) -> Feature {
        self.git(&["branch", branch, from]);
        let conn = self.conn();
        let new = NewRun {
            slug: slug.into(),
            base_branch: Some(PRIMARY.into()),
            run_branch: Some(branch.into()),
            ..Default::default()
        };
        let run = runs::create(&conn, &self.root(), &new).unwrap();
        let idea = schema::run_idea(&conn, &run.run_id, &util::now(), true)
            .unwrap()
            .expect("the idea of the run");
        let id: String = conn
            .query_row("SELECT id FROM features WHERE branch = ?1", [branch], |r| {
                r.get(0)
            })
            .unwrap();
        self.seen(branch);
        Feature {
            run,
            id,
            branch: branch.to_string(),
            idea,
        }
    }

    /// A task of the feature `f` with its task branch `name` in the landing
    /// state `landing` and the task in `status`, as its worker and the bind
    /// leave them. The branch holds 1 commit of `file` on the feature
    /// branch. Gives the task id.
    fn task_branch(
        &self,
        f: &Feature,
        name: &str,
        file: &str,
        status: &str,
        landing: &str,
    ) -> String {
        self.commit_on(name, &f.branch, file, &format!("{file}\n"));
        let conn = self.conn();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        let new = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), PERSONA);
        let task = tasks::add(&conn, &f.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id;
        conn.execute(
            "UPDATE tasks SET status = ?2 WHERE task_id = ?1",
            [&task, status],
        )
        .unwrap();
        let row = NewBranch {
            name,
            feature_id: &f.id,
            task_id: Some(&task),
            owner: "worker",
            due_at: 0,
        };
        let tx = conn.unchecked_transaction().unwrap();
        rows::insert_branch(&tx, None, &row, cause("the bind")).unwrap();
        tx.execute(
            "UPDATE branches SET landing_state = ?2 WHERE name = ?1",
            params![name, landing],
        )
        .unwrap();
        tx.commit().unwrap();
        task
    }

    /// A landed task of `f`: its branch with 1 commit of `file` is merged
    /// into the feature branch, and fleetd saw the new feature tip.
    fn landed_task(&self, f: &Feature, name: &str, file: &str) -> String {
        let task = self.task_branch(f, name, file, "done", "landed");
        self.merge_into(&f.branch, name);
        self.seen(&f.branch);
        task
    }

    /// A task of `f` that fleetd lands: its branch with 1 commit of `file`
    /// is `ready`, and the passes run until its bundle lands it on the
    /// feature branch. The bundle makes the gate tree of the feature.
    fn task_landed_by_fleetd(&self, f: &Feature, name: &str, file: &str) {
        self.task_branch(f, name, file, "done", "ready");
        self.pass_until(&format!("{name} lands on {}", f.branch), |w| {
            let landed = w.value(
                "SELECT 1 FROM branches WHERE name = ?1 AND landing_state = 'landed'",
                [name],
            );
            let tip = w.tip(&f.branch)?;
            (landed.is_some() && w.has_file(&tip, file)).then_some(())
        });
    }

    /// 1 value of the ledger as text, or `None` for no row or a null.
    fn value(&self, sql: &str, args: impl rusqlite::Params) -> Option<String> {
        let v: Option<SqlValue> = self
            .conn()
            .query_row(sql, args, |r| r.get(0))
            .optional()
            .unwrap();
        match v? {
            SqlValue::Null => None,
            SqlValue::Integer(n) => Some(n.to_string()),
            SqlValue::Text(t) => Some(t),
            other => panic!("{sql}: {other:?}"),
        }
    }

    /// Each row of 1 text column of the ledger.
    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = self.conn();
        let mut stmt = conn.prepare(sql).unwrap();
        let out = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        out
    }

    fn feature_state(&self, id: &str) -> Option<String> {
        self.value("SELECT state FROM features WHERE id = ?1", [id])
    }

    fn idea_state(&self, id: &str) -> Option<String> {
        self.value("SELECT state FROM ideas WHERE id = ?1", [id])
    }

    fn stacked_on(&self, id: &str) -> Option<String> {
        self.value("SELECT stacked_on FROM features WHERE id = ?1", [id])
    }

    /// The fetched tip of the primary branch.
    fn fetched_primary(&self) -> Option<String> {
        self.value("SELECT tip FROM remote_tips WHERE branch = ?1", [PRIMARY])
    }

    /// The rescue refs of the branch `branch`, each with its commit.
    fn rescues(&self, branch: &str) -> Vec<(String, String)> {
        let out = self.git(&[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            &rescue_folder(branch),
        ]);
        out.lines()
            .map(|l| {
                let (name, tip) = l.split_once(' ').unwrap();
                (name.to_string(), tip.to_string())
            })
            .collect()
    }

    /// The ids of the jobs of `kind` on `subject`, oldest first.
    fn jobs(&self, kind: &str, subject: &str) -> Vec<i64> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT id FROM jobs WHERE kind = ?1 AND subject = ?2 ORDER BY id")
            .unwrap();
        let out = stmt
            .query_map([kind, subject], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        out
    }

    /// The ids of the passed jobs of `kind` on `subject`, oldest first.
    fn passed_jobs(&self, kind: &str, subject: &str) -> Vec<i64> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id FROM jobs WHERE kind = ?1 AND subject = ?2 AND state = 'passed'
                 ORDER BY id",
            )
            .unwrap();
        let out = stmt
            .query_map([kind, subject], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        out
    }

    /// What a failed wait shows: the rows of integration and the output of
    /// each pass.
    fn diagnostics(&self) -> String {
        let jobs = self.rows(
            "SELECT id || ' ' || kind || ' ' || subject || ' ' || state || ' '
                 || COALESCE(result_json, '') FROM jobs ORDER BY id",
        );
        let features = self.rows(
            "SELECT id || ' ' || state || ' ' || COALESCE(branch, '-')
                 || ' stacked_on=' || COALESCE(stacked_on, '-') FROM features",
        );
        let ideas = self.rows(
            "SELECT id || ' ' || feature_id || ' ' || state || ' pane=' || COALESCE(pane, '-')
                 FROM ideas",
        );
        let tasks = self
            .rows("SELECT task_id || ' ' || status || ' ' || COALESCE(idea_id, '-') FROM tasks");
        let branches = self.rows(
            "SELECT name || ' ' || feature_id || ' ' || COALESCE(landing_state, '-')
                 || ' rescue=' || COALESCE(rescue_ref, '-') FROM branches",
        );
        let bundles = self.rows("SELECT id || ' ' || state || ' ' || base_tip FROM bundles");
        let items = self.rows("SELECT key || ' ' || state FROM needs_you");
        let worktrees = self.rows("SELECT path || ' ' || state FROM worktrees");
        format!(
            "jobs:\n{}\nfeatures: {features:?}\nideas: {ideas:?}\ntasks: {tasks:?}\n\
             branches: {branches:?}\nbundles: {bundles:?}\nneeds_you: {items:?}\n\
             worktrees: {worktrees:?}\npasses:\n{}",
            jobs.join("\n"),
            self.passes.borrow().join("\n")
        )
    }

    /// 1 pass of fleetd, `horch ledger watch --once`. Then wait until each
    /// job process that the pass started has ended, so the next pass reads
    /// its result. A project with 2 runs needs `--run`; the pass covers the
    /// whole ledger, and `--run r1` names only the run of its printed checks.
    ///
    /// The `PATH` of the pass is an empty folder (and the host tools on
    /// Linux, see `pass_path`): the scan of the CLI
    /// binaries of Phase 7 finds no CLI and queues no docs or conformance
    /// job. Git, herdr and the agents have their absolute paths in the
    /// sealed environment.
    fn pass(&self) {
        if self.pushed.get()
            && self
                .value("SELECT 1 FROM fleetd WHERE id = 1", [])
                .is_some()
        {
            self.h.advance_fleet_clock(FETCH_PERIOD_S);
            self.pushed.set(false);
        }
        let no_clis = self.h.root.join("no-clis");
        std::fs::create_dir_all(&no_clis).unwrap();
        let mut cmd = self.h.horch(&[
            "ledger", "watch", "--once", "--force", "--json", "--run", "r1",
        ]);
        cmd.env("PATH", pass_path(&self.h.root, no_clis));
        let o = cmd.output().unwrap();
        self.passes.borrow_mut().push(text(&o));
        wait_for_or(
            "the end of each job process",
            || {
                let pids = self.rows(
                    "SELECT CAST(pid AS TEXT) FROM jobs WHERE state = 'running' AND pid IS NOT NULL",
                );
                pids.iter()
                    .all(|p| !alive(p.parse().unwrap()))
                    .then_some(())
            },
            || self.diagnostics(),
        );
        assert!(o.status.success(), "{}", self.diagnostics());
    }

    /// Passes until `f` gives a value, at most [`MAX_PASSES`].
    fn pass_until<T>(&self, what: &str, mut f: impl FnMut(&World) -> Option<T>) -> T {
        for _ in 0..MAX_PASSES {
            self.pass();
            if let Some(v) = f(self) {
                return v;
            }
        }
        panic!(
            "not after {MAX_PASSES} passes: {what}\n{}",
            self.diagnostics()
        );
    }
}

// ─── FM-41 and the gate test ───────────────────────────────────────────

/// FM-41, the gate test of Phase 6: you merge the feature branch into the
/// primary branch of the bare remote by hand while 1 idea is `in_progress`.
/// After 1 fetch period the feature is `merged`, the idea is in a new
/// feature with the same pane, and its task branch has a rescue ref and
/// lands on the new feature.
#[test]
fn fm41_merge_with_an_idea_in_flight_moves_it_to_a_new_feature() {
    let w = world("p6-fm41");
    let f = w.feature("r1", "fleet/r1", PRIMARY);
    w.landed_task(&f, "fleet/r1-T1-a", "a.txt");
    // The second task is done and verified; its branch waits for its bundle.
    let flight = "fleet/r1-T2-b";
    w.task_branch(&f, flight, "b.txt", "done", "ready");
    let flight_tip = w.tip(flight).unwrap();
    w.conn()
        .execute("UPDATE ideas SET pane = ?2 WHERE id = ?1", [&f.idea, PANE])
        .unwrap();
    assert_eq!(w.idea_state(&f.idea).as_deref(), Some("in_progress"));

    // You merge the pull request of the feature at once.
    w.merge_pull_request(&f.branch);
    let old_tip = w.tip(&f.branch).unwrap();

    let new = w.pass_until("the idea is in a new feature", |w| {
        let at = w.value("SELECT feature_id FROM ideas WHERE id = ?1", [&f.idea])?;
        (at != f.id).then_some(at)
    });
    assert_eq!(w.feature_state(&f.id).as_deref(), Some("merged"));
    assert_eq!(
        w.value("SELECT pane FROM ideas WHERE id = ?1", [&f.idea])
            .as_deref(),
        Some(PANE),
        "the idea keeps its pane"
    );
    let new_branch = w
        .value("SELECT branch FROM features WHERE id = ?1", [&new])
        .expect("the new feature has its branch");
    assert_ne!(new_branch, f.branch);

    // The task branch lands on the new feature.
    w.pass_until("the task branch lands on the new feature", |w| {
        let landed = w.value(
            "SELECT 1 FROM branches WHERE name = ?1 AND feature_id = ?2 AND landing_state = 'landed'",
            params![flight, new],
        );
        let tip = w.tip(&format!("refs/heads/{new_branch}"))?;
        (landed.is_some() && w.has_file(&tip, "b.txt")).then_some(())
    });
    let new_tip = w.tip(&new_branch).unwrap();
    let primary = w.fetched_primary().expect("the fetched primary tip");
    assert!(
        w.contains(&new_tip, &primary),
        "the new feature starts from the fetched primary tip\n{}",
        w.diagnostics()
    );
    let rescues = w.rescues(flight);
    assert!(
        rescues.iter().any(|(_, tip)| *tip == flight_tip),
        "a rescue ref holds the old tip of the task branch: {rescues:?}"
    );
    assert_eq!(
        w.tip(&f.branch).as_deref(),
        Some(old_tip.as_str()),
        "no branch lands on the merged feature"
    );
    assert_eq!(w.feature_state(&f.id).as_deref(), Some("merged"));
}

// ─── FM-42 ─────────────────────────────────────────────────────────────

/// A stack of 2 features: the upper feature `fleet/r2` is made on the lower
/// feature `fleet/r1`, as a trial merge that conflicts leaves it. Each
/// feature has 1 landed task. Gives (lower, upper).
fn stack(w: &World) -> (Feature, Feature) {
    let lower = w.feature("r1", "fleet/r1", PRIMARY);
    w.task_landed_by_fleetd(&lower, "fleet/r1-T1-a", "a.txt");
    let upper = w.feature("r2", "fleet/r2", &lower.branch);
    w.task_landed_by_fleetd(&upper, "fleet/r2-T1-b", "b.txt");
    w.conn()
        .execute(
            "UPDATE features SET stacked_on = ?2 WHERE id = ?1",
            [&upper.id, &lower.id],
        )
        .unwrap();
    (lower, upper)
}

/// The rebase of the upper feature after its lower edge ended: it is not
/// stacked, its old tip has a rescue ref, its branch is on the fetched
/// primary tip with its own work, and its full gate runs again after the
/// rebase. Gives the new tip.
fn assert_rebased_on_primary(w: &World, upper: &Feature, old_tip: &str) -> String {
    let tip = w.pass_until("the upper feature is on the primary tip", |w| {
        let primary = w.fetched_primary()?;
        let tip = w.tip(&upper.branch)?;
        (w.stacked_on(&upper.id).is_none() && tip != old_tip && w.contains(&tip, &primary))
            .then_some(tip)
    });
    assert!(w.has_file(&tip, "b.txt"), "the upper work stays");
    let rescues = w.rescues(&upper.branch);
    assert!(
        rescues.iter().any(|(_, t)| t == old_tip),
        "a rescue ref holds the old tip of the upper feature: {rescues:?}"
    );
    let rebase = *w
        .jobs("rebase", &upper.branch)
        .last()
        .expect("a rebase job of the upper feature");
    w.pass_until("the full gate of the upper feature passes again", |w| {
        w.passed_jobs("full_gate", &upper.id)
            .iter()
            .any(|id| *id > rebase)
            .then_some(())
    });
    tip
}

/// FM-42, merged: you merge the lower feature of a stack. The upper feature
/// is rebased on the primary tip and its gates run again.
#[test]
fn fm42_lower_feature_merged_rebases_the_upper() {
    let w = world("p6-fm42-merged");
    let (lower, upper) = stack(&w);
    let old_tip = w.tip(&upper.branch).unwrap();

    w.merge_pull_request(&lower.branch);
    w.pass_until("the lower feature is merged", |w| {
        (w.feature_state(&lower.id).as_deref() == Some("merged")).then_some(())
    });
    let tip = assert_rebased_on_primary(&w, &upper, &old_tip);
    assert!(
        w.has_file(&tip, "a.txt"),
        "the merged lower work is in main"
    );
}

/// FM-42, abandoned: you abandon the lower feature of a stack. The upper
/// feature is rebased on the primary tip without the commits of the lower
/// one, and its gates run again.
#[test]
fn fm42_lower_feature_abandoned_rebases_the_upper() {
    let w = world("p6-fm42-abandoned");
    let (lower, upper) = stack(&w);
    let old_tip = w.tip(&upper.branch).unwrap();
    w.pass();

    // The state that `horch feature abandon` leaves.
    w.conn()
        .execute(
            "UPDATE features SET state = 'abandoned' WHERE id = ?1",
            [&lower.id],
        )
        .unwrap();
    let tip = assert_rebased_on_primary(&w, &upper, &old_tip);
    assert!(
        !w.has_file(&tip, "a.txt"),
        "the work of the abandoned lower feature is gone from the upper"
    );
}

// ─── FM-39 ─────────────────────────────────────────────────────────────

/// FM-39: a cancelled task does not block its idea. fleetd rescues the
/// branch of the cancelled task and its tree, the idea reaches
/// `integrating`, then `merged` after the merge of its feature, and fleetd
/// removes its rescued tree. The rescue refs stay.
#[test]
fn fm39_cancelled_task_does_not_block_its_idea() {
    let w = world("p6-fm39");
    let f = w.feature("r1", "fleet/r1", PRIMARY);
    w.landed_task(&f, "fleet/r1-T1-a", "a.txt");
    let cancelled = "fleet/r1-T2-b";
    let task = w.task_branch(&f, cancelled, "b.txt", "cancelled", "building");
    let cancelled_tip = w.tip(cancelled).unwrap();

    // The tree of the cancelled task, with work that is not committed.
    let path = w.h.project.join(".worktrees").join("t2");
    let path_s = path.to_string_lossy().into_owned();
    w.git(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "fleet/r1-T2-tree",
        &path_s,
        PRIMARY,
    ]);
    std::fs::write(path.join("u.txt"), "untracked\n").unwrap();
    let new = NewWorktree {
        worktree_id: "t2".into(),
        task_id: Some(task),
        worker_id: None,
        repo_root: w.root().display().to_string(),
        path: path.canonicalize().unwrap().display().to_string(),
        branch: "fleet/r1-T2-tree".into(),
        base: PRIMARY.into(),
        base_commit: Some(w.git(&["rev-parse", PRIMARY])),
        status: WorktreeStatus::Open,
        mode: None,
    };
    {
        let conn = w.conn();
        let tx = conn.unchecked_transaction().unwrap();
        trees::insert_orphan(&tx, None, &new, cause("a test")).unwrap();
        tx.execute(
            "UPDATE worktrees SET state = 'orphaned' WHERE worktree_id = 't2'",
            [],
        )
        .unwrap();
        tx.commit().unwrap();
    }

    w.pass_until("the idea is integrating", |w| {
        (w.idea_state(&f.idea).as_deref() == Some("integrating")).then_some(())
    });
    assert_eq!(
        w.value(
            "SELECT landing_state FROM branches WHERE name = ?1",
            [cancelled]
        )
        .as_deref(),
        Some("abandoned")
    );
    let rescues = w.rescues(cancelled);
    assert!(
        rescues.iter().any(|(_, t)| *t == cancelled_tip),
        "a rescue ref holds the tip of the cancelled branch: {rescues:?}"
    );
    w.pass_until("the tree of the cancelled task is rescued", |w| {
        (w.value("SELECT state FROM worktrees WHERE worktree_id = 't2'", [])
            .as_deref()
            == Some("rescued"))
        .then_some(())
    });
    let refs = w.git(&["for-each-ref", "--format=%(refname)", "refs/horch/rescue/"]);

    w.merge_pull_request(&f.branch);
    w.pass_until("the idea is merged and its tree is removed", |w| {
        let merged = w.idea_state(&f.idea).as_deref() == Some("merged");
        let removed = w
            .value("SELECT state FROM worktrees WHERE worktree_id = 't2'", [])
            .as_deref()
            == Some("removed");
        (merged && removed).then_some(())
    });
    assert_eq!(w.feature_state(&f.id).as_deref(), Some("merged"));
    assert!(!path.exists(), "the folder of the rescued tree is gone");
    let after = w.git(&["for-each-ref", "--format=%(refname)", "refs/horch/rescue/"]);
    for r in refs.lines() {
        assert!(after.lines().any(|a| a == r), "the rescue ref {r} stays");
    }
}

// ─── several features ──────────────────────────────────────────────────

/// 2 features run at the same time in 1 project: each has its own branch
/// and idea, neither stacks on the other, and both reach `ready`.
#[test]
fn several_features_run_at_the_same_time() {
    let w = world("p6-several");
    let one = w.feature("r1", "fleet/r1", PRIMARY);
    let two = w.feature("r2", "fleet/r2", PRIMARY);
    assert_ne!(one.id, two.id);
    assert_ne!(one.idea, two.idea);
    w.task_branch(&one, "fleet/r1-T1-a", "a.txt", "done", "ready");
    w.task_branch(&two, "fleet/r2-T1-b", "b.txt", "done", "ready");

    w.pass_until("both features are ready", |w| {
        let ready = |id: &str| w.feature_state(id).as_deref() == Some("ready");
        (ready(&one.id) && ready(&two.id)).then_some(())
    });
    assert_eq!(w.stacked_on(&one.id), None);
    assert_eq!(w.stacked_on(&two.id), None);
    for (f, file, other) in [(&one, "a.txt", "b.txt"), (&two, "b.txt", "a.txt")] {
        assert_eq!(w.idea_state(&f.idea).as_deref(), Some("integrating"));
        let tip = w.tip(&f.branch).unwrap();
        assert!(w.has_file(&tip, file), "{} keeps its work", f.id);
        assert!(
            !w.has_file(&tip, other),
            "{} has no work of the other",
            f.id
        );
    }
}

/// The `PATH` of a pass: the empty `no_clis` folder, and on Linux also the
/// host tool folder `host-bin` of the harness. fleetd finds `bwrap` and
/// `socat` on the `PATH`; without them each job runs no git.
fn pass_path(root: &std::path::Path, no_clis: std::path::PathBuf) -> std::ffi::OsString {
    #[cfg(target_os = "linux")]
    let dirs = vec![no_clis, root.join("host-bin")];
    #[cfg(not(target_os = "linux"))]
    let dirs = {
        let _ = root;
        vec![no_clis]
    };
    std::env::join_paths(dirs).expect("the harness folders hold no PATH separator")
}
