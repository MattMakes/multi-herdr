//! The judge pair in the verifier (docs/tickets/fleetd-p6/41-judge-pair.md):
//! a task of a judgment kind waits in `verifying` after its structural
//! check, and reaches `done` only after its judge task, with no change of
//! the plan. An open CRITICAL item starts a fix round that burns no
//! attempt, and after 2 fix rounds both tasks go to you
//! (docs/specs/fleetd/design.md, "The judge pair", RR-3, FM-9, FM-33).
//!
//! Each test uses a temp git repository, a temp ledger and a temp fleet
//! root. A pass is part 2 (the verifier, in 1 transaction) and part 3 (the
//! structural check and the judge pair, after the commit). A `task_check`
//! job ends through its row, as the job process ends it. A new attempt is
//! written as push dispatch writes it. No test reads the load, the limits
//! or the clock of this machine: the fleet clock is a fact of the test.
#![cfg(unix)]

#[path = "common/test_jobs.rs"]
mod test_jobs;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::State;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::task_execution::application::judge_pair::{
    self, Settled, Step, FIX_ROUND_LIMIT,
};
use horch_core::fleetd::task_execution::application::retry::Policy;
use horch_core::fleetd::task_execution::application::review::Review;
use horch_core::fleetd::task_execution::application::verifier::{self, Structural, Verifier};
use horch_core::fleetd::task_execution::domain::judge_pair::Rank;
use horch_core::taskledger::finish::{self, CodexSandbox, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, VerifyProfile, WorkKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask, Task};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{util, Store};
use horch_platform::channel::User;
use rusqlite::{params, Connection};
use test_jobs::TestJobs;

const RUN_BRANCH: &str = "fleet/r1";
const IDEA: &str = "I-0007";
const FEATURE: &str = "F-0001";
const DOC: &str = "docs/sync.md";
const VERDICT: &str = "docs/sync.review.md";
/// The live sub-orchestrator attempt of the feature, and its pane.
const OWNER: &str = "I-0007/supervising/1";
const PANE: &str = "w1:p1";

const PASS: &str = "# Review of docs/sync.md\n\n## Verdict\n\npass\n\n## Findings\n\n\
- [MEDIUM] Section 2 has no example.\n";

const CRITICAL: &str = "# Review of docs/sync.md\n\n## Verdict\n\ncritical_open\n\n\
## Findings\n\n- [CRITICAL] The sync order loses data on a crash.\n";

const HIGH: &str = "# Review of docs/sync.md\n\n## Verdict\n\npass\n\n## Findings\n\n\
- [HIGH] The intro names 2 commands that do not exist.\n";

/// A second judge: the CRITICAL item is fixed, and it raises 2 new items
/// that are not in its scope.
const SECOND_PASS: &str = "# Review of docs/sync.md\n\n## Verdict\n\npass\n\n## Findings\n\n\
- [MEDIUM] Section 3 is long.\n- [LOW] A typo in the title.\n";

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Write `file` in `dir` and commit it.
fn commit(dir: &Path, file: &str, body: &str) {
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "--quiet", "-m", file]);
}

fn policy(_: &Task) -> Policy {
    Policy::default()
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// The counters of 1 task.
#[derive(Debug, PartialEq, Eq)]
struct Counts {
    attempts: i64,
    attempt_floor: i64,
    fix_rounds: i64,
}

struct World {
    tmp: tempfile::TempDir,
    store: Store,
    run: Run,
    tools: Tools,
    config: FleetConfig,
}

impl World {
    /// A git repository with the run branch, 1 run with the steps of the
    /// profile `minimal`, and 1 feature with 1 idea.
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().canonicalize().unwrap();
        let root = top.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "--quiet", "-b", "main"]);
        for (k, v) in [
            ("user.name", "t"),
            ("user.email", "t@example.com"),
            ("commit.gpgsign", "false"),
        ] {
            git(&root, &["config", k, v]);
        }
        commit(&root, "README.md", "base\n");
        git(&root, &["branch", RUN_BRANCH]);
        let mut store = Store::open(&top.join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some(RUN_BRANCH.into()),
                        finish_steps: VerifyProfile::Minimal.steps(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        store
            .immediate(|tx| {
                let feature = NewFeature {
                    id: FEATURE,
                    name: Some("retry-sync"),
                    branch: Some("fleet/retry-sync"),
                    primary_branch: "main",
                };
                rows::insert_feature(tx, None, &feature, cause("feature opened"))?;
                let idea = NewIdea {
                    id: IDEA,
                    feature_id: FEATURE,
                    priority: 0,
                };
                ideas_store::insert_idea(tx, None, &idea, cause("idea added"))?;
                Ok(())
            })
            .unwrap();
        let tools = Tools {
            git: Git::new("git"),
            gh: top.join("gh"),
            node: "/bin/sh".into(),
            complexity_cli: None,
            code_sweep_dir: None,
            evidence_root: top.join("evidence"),
            shell: "/bin/sh".into(),
            reviewers_down: Vec::new(),
            codex: CodexSandbox::default(),
            jobs: Some(std::sync::Arc::new(TestJobs)),
        };
        World {
            tmp,
            store,
            run,
            tools,
            config: FleetConfig::default(),
        }
    }

    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    fn root(&self) -> PathBuf {
        self.tmp.path().canonicalize().unwrap()
    }

    /// A new worker of the run, as push dispatch records it.
    fn worker(&mut self) -> String {
        let run = self.run.run_id.clone();
        self.store
            .immediate(|tx| {
                let id = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run,
                        persona: "opus".into(),
                        role: Some("opus-1".into()),
                        ..Default::default()
                    },
                )?;
                workers::supervisor_started(tx, &id, 4242, Some(1))?;
                workers::agent_started(tx, &id, 4243, Some(2))?;
                Ok(id)
            })
            .unwrap()
    }

    /// Add the task `title` with the `Own` list `own` and claim it with a
    /// new worker, in a new worktree. Gives the task and its worker.
    fn claimed(&mut self, title: &str, kind: WorkKind, own: &[&str]) -> (String, String) {
        let mut t = NewTask::work(title, "t.md", "opus");
        t.land_check = Some("true".into());
        t.work_kind = Some(kind);
        t.own = own.iter().map(|s| s.to_string()).collect();
        let run = self.run.clone();
        self.store
            .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator).map(|_| ()))
            .unwrap();
        let worker = self.worker();
        self.store
            .immediate(|tx| tasks::claim(tx, &worker).map(|_| ()))
            .unwrap();
        let wt =
            finish::worktree_new_with(&mut self.store, &self.tools.git, &worker, title, &TestJobs)
                .unwrap();
        let task = wt.task_id.unwrap();
        self.conn()
            .execute(
                "UPDATE tasks SET idea_id = ?2 WHERE task_id = ?1",
                params![task, IDEA],
            )
            .unwrap();
        (task, worker)
    }

    /// The task moves to `verifying` with a live push attempt of `worker`,
    /// as push dispatch and the ingest of its result leave it.
    fn verifying(&self, task: &str, worker: &str) {
        let n: i64 = self
            .conn()
            .query_row(
                "SELECT attempts FROM tasks WHERE task_id = ?1",
                [task],
                |r| r.get(0),
            )
            .unwrap();
        self.conn()
            .execute(
                "UPDATE tasks SET status = 'verifying', current_worker_id = ?2 WHERE task_id = ?1",
                params![task, worker],
            )
            .unwrap();
        self.conn()
            .execute(
                "INSERT INTO attempts (id, task_id, worker_id, state, started_at, via)
                 VALUES (?1, ?2, ?3, 'live', ?4, 'push')",
                params![format!("{task}-a{n}"), task, worker, util::now()],
            )
            .unwrap();
    }

    /// A docs task that wrote its document, in `verifying`.
    fn judged(&mut self) -> String {
        let (task, worker) = self.claimed("judged", WorkKind::Docs, &[DOC]);
        commit(
            &self.tree(&task),
            DOC,
            "# Sync\n\n## Retries\nThe fetch runs again.\n",
        );
        self.verifying(&task, &worker);
        task
    }

    /// The judge task of `judged`: its worktree starts from the tip of the
    /// judged branch, and it writes the verdict file `verdict`.
    fn judge(&mut self, judged: &str, verdict: &str) -> String {
        let (task, worker) = self.claimed("judge", WorkKind::Docs, &[VERDICT]);
        self.conn()
            .execute(
                "UPDATE tasks SET work_kind = NULL, spec_json = '{\"kind\":\"judge\"}',
                    parent_task_id = ?2
                 WHERE task_id = ?1",
                params![task, judged],
            )
            .unwrap();
        self.onto_judged(&task, judged);
        commit(&self.tree(&task), VERDICT, verdict);
        self.verifying(&task, &worker);
        task
    }

    /// The judge tree of `judge` sits on the tip of the judged branch, as
    /// the judge branch move of a fix round leaves it (ticket 42).
    fn onto_judged(&self, judge: &str, judged: &str) {
        let tip = git(&self.tree(judged), &["rev-parse", "HEAD"]);
        git(&self.tree(judge), &["reset", "--quiet", "--hard", &tip]);
        self.conn()
            .execute(
                "UPDATE worktrees SET base_commit = ?2 WHERE task_id = ?1",
                params![judge, tip],
            )
            .unwrap();
    }

    /// The next attempt of `task` in `verifying`, with a new worker in the
    /// same worktree, as push dispatch starts it. `work` makes its
    /// commits.
    fn next_attempt(&mut self, task: &str, work: impl FnOnce(&Path)) {
        let worker = self.worker();
        self.conn()
            .execute(
                "UPDATE tasks SET attempts = attempts + 1 WHERE task_id = ?1",
                [task],
            )
            .unwrap();
        work(&self.tree(task));
        self.verifying(task, &worker);
    }

    /// The fix attempt of `judged` changes its document.
    fn fix(&mut self, judged: &str, round: i64) {
        self.next_attempt(judged, |wt| {
            commit(
                wt,
                DOC,
                &format!("# Sync\n\n## Retries\nThe fetch runs again, fix {round}.\n"),
            );
        });
    }

    /// The next attempt of the judge `judge` of `judged` writes `verdict`
    /// on the new judged tip.
    fn judge_again(&mut self, judge: &str, judged: &str, verdict: &str) {
        self.onto_judged(judge, judged);
        let verdict = verdict.to_string();
        self.next_attempt(judge, move |wt| commit(wt, VERDICT, &verdict));
    }

    fn tree(&self, task: &str) -> PathBuf {
        PathBuf::from(
            finish::worktree_of(self.conn(), task)
                .unwrap()
                .unwrap()
                .path,
        )
    }

    /// 1 pass: part 2, the verifier in 1 transaction; part 3, the
    /// structural check of each judgment kind and judge kind after the
    /// commit. The tips are the HEADs of the worktrees.
    fn pass(&mut self) -> Vec<(String, Structural)> {
        let mut tips = BTreeMap::new();
        for t in tasks::of_run(self.conn(), &self.run.run_id).unwrap() {
            if let Some(wt) = finish::worktree_of(self.conn(), &t.task_id).unwrap() {
                tips.insert(t.task_id, git(Path::new(&wt.path), &["rev-parse", "HEAD"]));
            }
        }
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let v = Verifier {
            jobs: &ctx,
            clock: FleetSeconds(0),
            tips: &tips,
            policy: &policy,
        };
        let report = self.store.immediate(|tx| verifier::verify(&v, tx)).unwrap();
        let r = Review {
            jobs: &ctx,
            clock: FleetSeconds(0),
            tools: &self.tools,
            policy: &policy,
        };
        verifier::structural(&r, &mut self.store, &report.passed).unwrap()
    }

    /// The job process of each queued check ends `passed`.
    fn end_checks(&self) {
        let result = r#"{"passed":true,"verdict":true,"text":"`true` in wt: exit 0"}"#;
        self.conn()
            .execute(
                "UPDATE jobs SET state = 'passed', result_json = ?2, ended_at = ?3
                 WHERE kind = 'task_check' AND state = ?1",
                params![State::Queued.as_str(), result, util::now()],
            )
            .unwrap();
    }

    /// Pass 1 queues each check, each job passes, and pass 2 gives the
    /// result of part 3.
    fn checked(&mut self) -> Vec<(String, Structural)> {
        self.pass();
        self.end_checks();
        self.pass()
    }

    fn status(&self, task: &str) -> String {
        tasks::get(self.conn(), task).unwrap().status
    }

    fn counts(&self, task: &str) -> Counts {
        self.conn()
            .query_row(
                "SELECT attempts, attempt_floor, fix_rounds FROM tasks WHERE task_id = ?1",
                [task],
                |r| {
                    Ok(Counts {
                        attempts: r.get(0)?,
                        attempt_floor: r.get(1)?,
                        fix_rounds: r.get(2)?,
                    })
                },
            )
            .unwrap()
    }

    /// The outcome of the newest attempt of `task`.
    fn outcome(&self, task: &str) -> Option<String> {
        self.conn()
            .query_row(
                "SELECT outcome FROM attempts WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 1",
                [task],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn task_count(&self) -> usize {
        tasks::of_run(self.conn(), &self.run.run_id).unwrap().len()
    }

    /// The rowid of the event that moved `task` to `to`.
    fn moved_at(&self, task: &str, to: &str) -> i64 {
        self.conn()
            .query_row(
                "SELECT max(rowid) FROM events
                 WHERE kind = 'transition' AND entity = 'task' AND entity_id = ?1 AND to_state = ?2",
                params![task, to],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// The live sub-orchestrator process of the feature, in its pane.
    fn sub_orchestrator(&self) {
        let worker = "w-idea-lead";
        self.conn()
            .execute(
                "INSERT INTO workers (worker_id, run_id, persona, status, spawned_at, host)
                 VALUES (?1, ?2, 'idea-lead', 'working', 'then', ?3)",
                params![worker, self.run.run_id, util::host_name()],
            )
            .unwrap();
        self.conn()
            .execute(
                "INSERT INTO attempts (id, worker_id, idea_id, mux_handle, pid, pid_start, state, started_at)
                 VALUES (?1, ?2, ?3, ?4, 4101, 11, 'live', 'then')",
                params![OWNER, worker, IDEA, PANE],
            )
            .unwrap();
    }
}

/// What part 3 did with `task` in `pass`.
fn of<'a>(pass: &'a [(String, Structural)], task: &str) -> &'a Structural {
    &pass
        .iter()
        .find(|(id, _)| id == task)
        .unwrap_or_else(|| panic!("no result of {task}: {pass:?}"))
        .1
}

/// What the judge pair did in `pass` for the verdict of `judge`.
fn settled<'a>(pass: &'a [(String, Structural)], judge: &str) -> &'a Settled {
    match of(pass, judge) {
        Structural::Judged(s) => s,
        other => panic!("{judge} is not judged: {other:?}"),
    }
}

/// 1 round of the judge with an open CRITICAL item.
fn critical_round(w: &mut World, judged: &str, judge: &str, round: i64) -> Settled {
    w.fix(judged, round);
    let pass = w.checked();
    assert_eq!(of(&pass, judged), &Structural::WaitsForJudge);
    w.judge_again(judge, judged, CRITICAL);
    let pass = w.checked();
    settled(&pass, judge).clone()
}

#[test]
fn judged_task_stays_verifying_until_its_judge_task_is_done() {
    let mut w = World::new();
    let judged = w.judged();

    // The structural check passes, and the task waits for its judge.
    let pass = w.checked();
    assert_eq!(of(&pass, &judged), &Structural::WaitsForJudge);
    assert_eq!(w.status(&judged), "verifying");
    for _ in 0..2 {
        let pass = w.pass();
        assert_eq!(of(&pass, &judged), &Structural::WaitsForJudge);
        assert_eq!(w.status(&judged), "verifying");
    }

    // The judge task is in verifying, and its check did not end yet.
    let judge = w.judge(&judged, PASS);
    w.pass();
    assert_eq!(w.status(&judged), "verifying");
    assert_eq!(w.status(&judge), "verifying");
    assert!(!judge_pair::judge_done(w.conn(), &judged).unwrap());

    w.end_checks();
    let pass = w.pass();
    assert_eq!(settled(&pass, &judge).step, Step::Done);
    assert_eq!(w.status(&judge), "done");
    assert_eq!(w.status(&judged), "done");
    assert!(judge_pair::judge_done(w.conn(), &judged).unwrap());
}

#[test]
fn rr3_judgment_task_and_its_judge_reach_done_with_no_plan_change() {
    let mut w = World::new();
    let judged = w.judged();
    w.checked();
    let judge = w.judge(&judged, PASS);
    let tasks_before = w.task_count();

    let pass = w.checked();

    let s = settled(&pass, &judge);
    assert_eq!(s.judged, judged);
    assert_eq!(s.step, Step::Done);
    // A first judge keeps its MEDIUM item: nothing is dropped.
    assert!(s.dropped.is_empty(), "{s:?}");
    assert_eq!(s.decision, None);
    assert_eq!(
        (w.status(&judge), w.status(&judged)),
        ("done".into(), "done".into())
    );
    // The judge task moves to done first, then the judged task.
    assert!(w.moved_at(&judge, "done") < w.moved_at(&judged, "done"));
    // No change of the plan: no task is added, and no fix round ran.
    assert_eq!(w.task_count(), tasks_before);
    assert_eq!(w.counts(&judged).fix_rounds, 0);
    // The next pass finds both done and does nothing more.
    assert!(w.pass().is_empty());
}

#[test]
fn critical_item_gives_1_fix_round_not_counted_in_attempts() {
    let mut w = World::new();
    let judged = w.judged();
    w.checked();
    let judge = w.judge(&judged, CRITICAL);
    let judged_before = w.counts(&judged);
    let judge_before = w.counts(&judge);
    let tasks_before = w.task_count();

    let pass = w.checked();

    assert_eq!(settled(&pass, &judge).step, Step::FixRound(1));
    // The judge attempt ends critical_open, which burns no attempt, and
    // the judge waits in pending for the fix attempt.
    assert_eq!(w.outcome(&judge).as_deref(), Some("critical_open"));
    assert_eq!(w.status(&judge), "pending");
    assert_eq!(
        w.counts(&judge),
        Counts {
            attempt_floor: judge_before.attempt_floor + 1,
            ..judge_before
        }
    );
    // The judged task is ready for its fix attempt: 1 fix round, and the
    // attempt count does not change.
    assert_eq!(w.status(&judged), "ready");
    assert_eq!(w.outcome(&judged).as_deref(), Some("critical_open"));
    assert_eq!(
        w.counts(&judged),
        Counts {
            attempts: judged_before.attempts,
            attempt_floor: judged_before.attempt_floor + 1,
            fix_rounds: 1,
        }
    );
    assert_eq!(w.task_count(), tasks_before);

    // The fix attempt takes no attempt of max_attempts.
    w.fix(&judged, 1);
    let c = w.counts(&judged);
    assert_eq!(
        c.attempts - c.attempt_floor,
        judged_before.attempts - judged_before.attempt_floor
    );
    let pass = w.checked();
    assert_eq!(of(&pass, &judged), &Structural::WaitsForJudge);

    // The fresh judge finds the CRITICAL item fixed: both reach done.
    w.judge_again(&judge, &judged, PASS);
    let pass = w.checked();
    assert_eq!(settled(&pass, &judge).step, Step::Done);
    assert_eq!(
        (w.status(&judge), w.status(&judged)),
        ("done".into(), "done".into())
    );
    assert_eq!(w.counts(&judged).fix_rounds, 1);
}

#[test]
fn third_critical_round_escalates_to_the_operator() {
    let mut w = World::new();
    let judged = w.judged();
    w.checked();
    let judge = w.judge(&judged, CRITICAL);
    let pass = w.checked();
    assert_eq!(settled(&pass, &judge).step, Step::FixRound(1));
    let s = critical_round(&mut w, &judged, &judge, 1);
    assert_eq!(s.step, Step::FixRound(2));
    assert_eq!(w.counts(&judged).fix_rounds, FIX_ROUND_LIMIT);

    let s = critical_round(&mut w, &judged, &judge, 2);

    assert_eq!(s.step, Step::Escalated);
    assert_eq!(w.status(&judged), "escalated");
    assert_eq!(w.status(&judge), "escalated");
    assert_eq!(w.outcome(&judge).as_deref(), Some("critical_open"));
    assert_eq!(w.counts(&judged).fix_rounds, FIX_ROUND_LIMIT);
    let items: Vec<_> = needs_you::open_items(w.conn())
        .unwrap()
        .into_iter()
        .filter(|i| i.kind == judge_pair::ESCALATED_KIND)
        .collect();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].key, judge_pair::escalated_key(&judged));
    // Neither task leaves done: neither reached it.
    let done: i64 = w
        .conn()
        .query_row(
            "SELECT count(*) FROM events WHERE kind = 'transition' AND to_state = 'done'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(done, 0);
    assert!(w.pass().is_empty());
}

#[test]
fn second_judge_new_medium_items_are_dropped() {
    let mut w = World::new();
    let judged = w.judged();
    w.checked();
    let judge = w.judge(&judged, CRITICAL);
    w.checked();
    w.fix(&judged, 1);
    w.checked();
    w.judge_again(&judge, &judged, SECOND_PASS);

    let pass = w.checked();

    let s = settled(&pass, &judge);
    assert_eq!(s.step, Step::Done);
    let dropped: Vec<Rank> = s.dropped.iter().map(|f| f.rank).collect();
    assert_eq!(dropped, [Rank::Medium, Rank::Low]);
    // The verifier logs the drop in 1 note event of the judge task.
    let logged: String = w
        .conn()
        .query_row(
            "SELECT text FROM events WHERE kind = 'note' AND task_id = ?1",
            [&judge],
            |r| r.get(0),
        )
        .unwrap();
    assert!(logged.contains("Section 3 is long."), "{logged}");
    assert!(logged.contains("A typo in the title."), "{logged}");
    assert_eq!(
        (w.status(&judge), w.status(&judged)),
        ("done".into(), "done".into())
    );
}

#[test]
fn high_item_does_not_block() {
    let mut w = World::new();
    w.sub_orchestrator();
    let judged = w.judged();
    w.checked();
    let judge = w.judge(&judged, HIGH);

    let pass = w.checked();

    let s = settled(&pass, &judge).clone();
    assert_eq!(s.step, Step::Done);
    assert_eq!(
        (w.status(&judge), w.status(&judged)),
        ("done".into(), "done".into())
    );
    // The HIGH item goes to the decisions file of the feature.
    let decision = s.decision.expect("1 entry of the decision log");
    assert_eq!(decision.feature, FEATURE);
    let root = w.root();
    decision.write(&root, &User::current().unwrap()).unwrap();
    let log = std::fs::read_to_string(decision.path(&root)).unwrap();
    assert!(
        log.contains("[HIGH] The intro names 2 commands that do not exist."),
        "{log}"
    );
    assert!(log.contains(&format!("{judge}-a1:H1")), "{log}");
    // It rings the sub-orchestrator of the feature: 1 doorbell event of
    // its live attempt, typed after the commit.
    let ring = s.ring.expect("1 ring");
    assert_eq!((ring.attempt.as_str(), ring.pane.as_str()), (OWNER, PANE));
    let bells: i64 = w
        .conn()
        .query_row(
            "SELECT count(*) FROM events WHERE kind = 'doorbell' AND entity_id = ?1",
            [OWNER],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bells, 1);
}
