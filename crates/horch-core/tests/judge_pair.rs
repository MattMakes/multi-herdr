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
use horch_core::fleetd::task_execution::adapters::verdict_file;
use horch_core::fleetd::task_execution::application::judge_pair::{
    self, Settled, Step, FIX_ROUND_LIMIT,
};
use horch_core::fleetd::task_execution::application::retry::Policy;
use horch_core::fleetd::task_execution::application::review::Review;
use horch_core::fleetd::task_execution::application::verifier::{self, Structural, Verifier};
use horch_core::fleetd::task_execution::domain::judge_pair::Rank;
use horch_core::fleetd::task_execution::domain::verification;
use horch_core::taskledger::finish::{self, CodexSandbox, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, VerifyProfile, WorkKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask, Task};
use horch_core::taskledger::testing::bind_live;
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
const OWNER: &str = "I-0007/supervising/1";
const PANE: &str = "w1:p1";

const PASS: &str = "# Review of docs/sync.md\n\n## Verdict\n\npass\n\n## Findings\n\n\
- [MEDIUM] Section 2 has no example.\n";

const CRITICAL: &str = "# Review of docs/sync.md\n\n## Verdict\n\ncritical_open\n\n\
## Findings\n\n- [CRITICAL] The sync order loses data on a crash.\n";

const HIGH: &str = "# Review of docs/sync.md\n\n## Verdict\n\npass\n\n## Findings\n\n\
- [HIGH] The intro names 2 commands that do not exist.\n";

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
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = horch_platform::path::canonicalize(tmp.path()).unwrap();
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
        horch_platform::path::canonicalize(self.tmp.path()).unwrap()
    }

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

    fn bind(&mut self, task: &str) -> String {
        let new = NewWorker {
            run_id: self.run.run_id.clone(),
            persona: "opus".into(),
            role: Some("opus-1".into()),
            ..Default::default()
        };
        let agent = horch_core::fleetd::task_execution::domain::task::Recorded {
            pid: 4243,
            pid_start: Some(2),
        };
        let id = bind_live(self.store.conn(), task, &new, Some(agent))
            .unwrap()
            .worker_id;
        self.conn()
            .execute(
                "UPDATE workers SET supervisor_pid = 4242, supervisor_started = 1
                 WHERE worker_id = ?1",
                [&id],
            )
            .unwrap();
        id
    }

    fn claimed(&mut self, title: &str, kind: WorkKind, own: &[&str]) -> (String, String) {
        let mut t = NewTask::work(title, "t.md", "opus");
        t.land_check = Some("true".into());
        t.work_kind = Some(kind);
        t.own = own.iter().map(|s| s.to_string()).collect();
        let run = self.run.clone();
        let task = self
            .store
            .immediate(|tx| tasks::add(tx, &run, &t, Actor::Orchestrator))
            .unwrap()
            .task_id;
        let worker = self.bind(&task);
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

    fn fix(&mut self, judged: &str, round: i64) {
        self.next_attempt(judged, |wt| {
            commit(
                wt,
                DOC,
                &format!("# Sync\n\n## Retries\nThe fetch runs again, fix {round}.\n"),
            );
        });
    }

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

fn of<'a>(pass: &'a [(String, Structural)], task: &str) -> &'a Structural {
    &pass
        .iter()
        .find(|(id, _)| id == task)
        .unwrap_or_else(|| panic!("no result of {task}: {pass:?}"))
        .1
}

fn settled<'a>(pass: &'a [(String, Structural)], judge: &str) -> &'a Settled {
    match of(pass, judge) {
        Structural::Judged(s) => s,
        other => panic!("{judge} is not judged: {other:?}"),
    }
}

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

    let pass = w.checked();
    assert_eq!(of(&pass, &judged), &Structural::WaitsForJudge);
    assert_eq!(w.status(&judged), "verifying");
    for _ in 0..2 {
        let pass = w.pass();
        assert_eq!(of(&pass, &judged), &Structural::WaitsForJudge);
        assert_eq!(w.status(&judged), "verifying");
    }

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
    assert!(s.dropped.is_empty(), "{s:?}");
    assert_eq!(s.decision, None);
    assert_eq!(
        (w.status(&judge), w.status(&judged)),
        ("done".into(), "done".into())
    );
    assert!(w.moved_at(&judge, "done") < w.moved_at(&judged, "done"));
    assert_eq!(w.task_count(), tasks_before);
    assert_eq!(w.counts(&judged).fix_rounds, 0);
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
    assert_eq!(w.outcome(&judge).as_deref(), Some("critical_open"));
    assert_eq!(w.status(&judge), "pending");
    assert_eq!(
        w.counts(&judge),
        Counts {
            attempt_floor: judge_before.attempt_floor + 1,
            ..judge_before
        }
    );
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

    w.fix(&judged, 1);
    let c = w.counts(&judged);
    assert_eq!(
        c.attempts - c.attempt_floor,
        judged_before.attempts - judged_before.attempt_floor
    );
    let pass = w.checked();
    assert_eq!(of(&pass, &judged), &Structural::WaitsForJudge);

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

#[test]
fn planned_judge_task_with_no_check_passes_on_its_verdict() {
    let mut w = World::new();
    let judged = w.judged();
    w.checked();
    let judge = w.judge(&judged, PASS);
    w.conn()
        .execute(
            "UPDATE tasks SET land_check = NULL WHERE task_id = ?1",
            [&judge],
        )
        .unwrap();
    assert_eq!(
        finish::default_check(w.conn(), &w.run.run_id).unwrap(),
        None
    );

    let pass = w.pass();

    assert_eq!(settled(&pass, &judge).step, Step::Done);
    assert_eq!(
        (w.status(&judge), w.status(&judged)),
        ("done".into(), "done".into())
    );
    let reasons: Vec<String> = verdict_file::recorded(w.conn(), &judge)
        .unwrap()
        .into_iter()
        .map(|v| v.reason)
        .collect();
    assert_eq!(
        reasons.first().map(String::as_str),
        Some(verification::JUDGE_NO_CHECK)
    );
}

#[test]
fn task_with_no_check_that_is_not_a_judge_is_still_refused() {
    let mut w = World::new();
    let judged = w.judged();
    w.conn()
        .execute(
            "UPDATE tasks SET land_check = NULL WHERE task_id = ?1",
            [&judged],
        )
        .unwrap();

    w.pass();

    assert_ne!(w.status(&judged), "verifying");
    assert_eq!(w.outcome(&judged).as_deref(), Some("rejected"));
}
