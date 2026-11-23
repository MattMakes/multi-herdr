#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::ideas::application::plan_commit::{
    validate_plan, PlanCommit, Verdict, PLAN_YAML,
};
use horch_core::fleetd::ideas::application::replan::{
    replan_file, replan_task, verdict_file, Replan, Replanned,
};
use horch_core::fleetd::ideas::domain::plan_validator::{DeclaredGates, RosterProfile};
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::task_execution::application::retry::{burn_attempt, Policy};
use horch_core::fleetd::task_execution::domain::retry::Outcome;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::Requeue;
use horch_core::taskledger::Store;
use horch_platform::channel::User;
use rusqlite::Connection;
use serde_json::Value;

const IDEA: &str = "I-0007";
const FEATURE: &str = "F-0001";
const START: i64 = 1000;

const ASSUMPTIONS: &str = "# Assumptions of I-0007

feature: retry-sync
1. [confirmed] The client retries a failed request.
2. [assumed] The retry log goes to docs/sync.md.
";

const REQUIREMENTS: &str = "idea: I-0007
requirements:
  - id: R1
    text: The client retries with backoff.
    cites: [A1]
  - id: R2
    text: The retry behaviour is documented.
    cites: [A2]
";

const PLAN: &str = "idea: I-0007
repo: /fleet/repos/app
feature: F-0001
base_branch: fleet/retry-sync
tasks:
  - key: a
    title: Add backoff
    kind: feature
    profile: coder-claude
    covers: [R1]
    acceptance:
      - run: cargo test
  - key: b
    title: Log each retry
    kind: feature
    profile: coder-codex
    covers: [R1]
    after: [a]
    acceptance:
      - run: cargo test -p x
  - key: doc
    title: Document the retries
    kind: docs
    profile: coder-claude
    covers: [R2]
    outputs: [docs/sync.md]
    acceptance:
      - sections: [docs/sync.md, \"## Retries\"]
  - key: doc-judge
    title: Judge docs/sync.md
    kind: judge
    judges: doc
    profile: coder-codex
    outputs: [docs/sync.review.md]
";

const REPLACE_A: &str = "replace:
  - key: a
    title: Add backoff, in 2 steps
    kind: feature
    profile: coder-codex
    covers: [R1]
    acceptance:
      - run: cargo test -p sync
  - key: a-log
    title: Log the backoff
    kind: feature
    profile: coder-claude
    covers: [R1]
    after: [a]
    acceptance:
      - run: cargo test -p log
";

struct World {
    dir: tempfile::TempDir,
    store: Store,
    roster: Vec<RosterProfile>,
    gates: DeclaredGates,
    config: FleetConfig,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn profile(name: &str, provider: &str, model: &str, fallback: &str) -> RosterProfile {
    RosterProfile {
        name: name.into(),
        provider: provider.into(),
        model: model.into(),
        effort: Some("medium".into()),
        fallback: Some(fallback.into()),
        sandbox_here: true,
    }
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, 1, 1, 'then', ?1, 'then')",
            [START],
        )
        .unwrap();
    let root = dir.path().to_path_buf();
    store
        .immediate(|tx| {
            runs::create(
                tx,
                &root,
                &NewRun {
                    slug: "retry-sync".into(),
                    base_branch: Some("main".into()),
                    run_branch: Some("fleet/retry-sync".into()),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let feature = NewFeature {
        id: FEATURE,
        name: Some("retry-sync"),
        branch: Some("fleet/retry-sync"),
        primary_branch: "main",
    };
    store
        .immediate(|tx| rows::insert_feature(tx, None, &feature, cause("feature opened")))
        .unwrap();
    let idea = NewIdea {
        id: IDEA,
        feature_id: FEATURE,
        priority: 0,
    };
    store
        .immediate(|tx| {
            ideas_store::insert_idea(tx, None, &idea, cause("idea added"))?;
            transition(
                tx,
                None,
                Entity::Idea,
                IDEA,
                "ticketing",
                cause("the test starts in ticketing"),
                &NeedsYouList,
            )?;
            Ok(())
        })
        .unwrap();
    let mut w = World {
        dir,
        store,
        roster: vec![
            profile("coder-claude", "claude", "claude-opus-5-5", "coder-codex"),
            profile("coder-codex", "codex", "gpt-5", "coder-claude"),
        ],
        gates: DeclaredGates {
            fast: Some("cargo test".into()),
            full: Some("cargo test --workspace".into()),
        },
        config: FleetConfig::default(),
    };
    std::fs::create_dir_all(w.folder()).unwrap();
    for folder in [w.ideas(), w.folder()] {
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    w.write("assumptions.md", ASSUMPTIONS);
    w.write("requirements.yaml", REQUIREMENTS);
    w.write(PLAN_YAML, PLAN);
    let verdict = w.commit_plan();
    assert!(matches!(verdict, Verdict::Committed { .. }), "{verdict:?}");
    w
}

impl World {
    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    fn ideas(&self) -> PathBuf {
        self.dir.path().join("ideas")
    }

    fn folder(&self) -> PathBuf {
        self.ideas().join(IDEA)
    }

    fn write(&self, name: &str, text: &str) {
        let path = self.folder().join(name);
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn commit_plan(&mut self) -> Verdict {
        let ideas = self.ideas();
        let owner = User::current().unwrap();
        let ctx = PlanCommit {
            ideas: &ideas,
            owner: &owner,
            roster: &self.roster,
            gates: &self.gates,
            guard: None,
            refusals: &NeedsYouList,
        };
        validate_plan(&mut self.store, &ctx, IDEA).unwrap()
    }

    fn set_clock(&self, clock: i64) {
        self.conn()
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [clock])
            .unwrap();
    }

    fn replan(&mut self, key: &str, clock: i64) -> anyhow::Result<Replanned> {
        let id = self.live(key).id;
        let ideas = self.ideas();
        let owner = User::current().unwrap();
        let ctx = Replan {
            plan: PlanCommit {
                ideas: &ideas,
                owner: &owner,
                roster: &self.roster,
                gates: &self.gates,
                guard: None,
                refusals: &NeedsYouList,
            },
            jobs: JobCtx {
                guard: None,
                config: &self.config,
                needs_you: &NeedsYouList,
            },
        };
        replan_task(&mut self.store, &ctx, &id, FleetSeconds(clock))
    }

    fn live(&self, key: &str) -> TaskRow {
        let rows = self.tasks(key);
        let live: Vec<TaskRow> = rows
            .into_iter()
            .filter(|t| t.status != "cancelled")
            .collect();
        assert_eq!(live.len(), 1, "1 live task of {key}: {live:?}");
        live.into_iter().next().unwrap()
    }

    fn tasks(&self, key: &str) -> Vec<TaskRow> {
        let sql = "SELECT task_id, status, attempts, attempt_floor, replans, primary_persona
             FROM tasks WHERE idea_id = ?1 AND json_extract(spec_json, '$.key') = ?2
             ORDER BY rowid";
        let mut stmt = self.conn().prepare(sql).unwrap();
        let rows = stmt
            .query_map([IDEA, key], |r| {
                Ok(TaskRow {
                    id: r.get(0)?,
                    status: r.get(1)?,
                    used: r.get::<_, i64>(2)? - r.get::<_, i64>(3)?,
                    replans: r.get(4)?,
                    profile: r.get(5)?,
                })
            })
            .unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    }

    fn blockers(&self, id: &str) -> Vec<String> {
        let mut stmt = self
            .conn()
            .prepare("SELECT blocked_by FROM task_deps WHERE task_id = ?1 ORDER BY blocked_by")
            .unwrap();
        let rows = stmt.query_map([id], |r| r.get(0)).unwrap();
        rows.collect::<rusqlite::Result<_>>().unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn needs_you_of(&self, id: &str) -> i64 {
        self.conn()
            .query_row(
                "SELECT count(*) FROM needs_you WHERE entity_id = ?1 AND state = 'open'",
                [id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn fail(&mut self, key: &str) -> Requeue {
        loop {
            let id = self.live(key).id;
            self.conn()
                .execute(
                    "UPDATE tasks SET status = 'running', attempts = attempts + 1,
                        not_before = NULL WHERE task_id = ?1",
                    [&id],
                )
                .unwrap();
            let r = self
                .store
                .immediate(|tx| {
                    burn_attempt(tx, &id, Outcome::Crashed, &Policy::default(), "the test")
                })
                .unwrap();
            if r != Requeue::Queued {
                return r;
            }
        }
    }

    fn verdict(&self, key: &str) -> Value {
        let text = std::fs::read_to_string(self.folder().join(verdict_file(key))).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}

#[derive(Debug, Clone)]
struct TaskRow {
    id: String,
    status: String,
    used: i64,
    replans: i64,
    profile: Option<String>,
}

#[test]
fn failed_task_is_replaced_in_1_transaction() {
    let mut w = world();
    assert_eq!(w.fail("a"), Requeue::Failed);
    let old = w.live("a");
    let b = w.live("b");
    assert_eq!(w.blockers(&b.id), vec![old.id.clone()]);
    w.write(&replan_file("a"), REPLACE_A);

    w.conn()
        .execute_batch(
            "CREATE TEMP TRIGGER fault_on_2nd_new_task BEFORE INSERT ON main.tasks
             WHEN (SELECT count(*) FROM main.tasks WHERE idea_id = 'I-0007') = 5
             BEGIN SELECT RAISE(ABORT, 'fault seam: the 2nd new task insert fails'); END;",
        )
        .unwrap();
    let err = w.replan("a", START + 10).unwrap_err();
    assert!(format!("{err:#}").contains("fault seam"), "{err:#}");
    assert_eq!(w.count("SELECT count(*) FROM tasks"), 4);
    let still = w.live("a");
    assert_eq!(
        (still.id.as_str(), still.status.as_str()),
        (old.id.as_str(), "failed")
    );
    assert_eq!(still.replans, 0);
    assert_eq!(w.blockers(&b.id), vec![old.id.clone()]);

    w.conn()
        .execute_batch("DROP TRIGGER temp.fault_on_2nd_new_task")
        .unwrap();
    let Replanned::Replaced { tasks } = w.replan("a", START + 20).unwrap() else {
        panic!("the replan of a is not a replace");
    };
    let keys: Vec<&str> = tasks.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["a", "a-log"]);

    let all_a = w.tasks("a");
    assert_eq!(all_a.len(), 2);
    assert_eq!(
        (all_a[0].status.as_str(), all_a[0].replans),
        ("cancelled", 1)
    );
    let new_a = w.live("a");
    assert_eq!(new_a.status, "ready");
    assert_eq!(new_a.profile.as_deref(), Some("coder-codex"));
    assert_eq!(new_a.replans, 1);
    let log = w.live("a-log");
    assert_eq!((log.status.as_str(), log.replans), ("pending", 1));
    assert_eq!(w.blockers(&log.id), vec![new_a.id.clone()]);
    assert_eq!(w.blockers(&b.id), vec![new_a.id.clone()]);
    let covers = w.count(&format!(
        "SELECT count(*) FROM task_covers WHERE task_id IN ('{}', '{}') AND req_id = 'R1'",
        new_a.id, log.id
    ));
    assert_eq!(covers, 2);
    let verdict = w.verdict("a");
    assert_eq!(verdict["valid"], true, "{verdict}");
}

#[test]
fn failed_task_retry_resets_attempts() {
    let mut w = world();
    assert_eq!(w.fail("a"), Requeue::Failed);
    let failed = w.live("a");
    assert!(failed.used > 0);
    w.write(&replan_file("a"), "retry: the ticket of a is fixed\n");

    assert_eq!(w.replan("a", START + 10).unwrap(), Replanned::Retried);
    let t = w.live("a");
    assert_eq!(t.id, failed.id, "a retry keeps the task");
    assert_eq!(t.status, "ready");
    assert_eq!(t.used, 0, "the attempt count starts again at 0");
    assert_eq!(t.replans, 1);
    assert_eq!(w.needs_you_of(&t.id), 0);
}

#[test]
fn second_failure_after_a_replan_escalates() {
    let mut w = world();
    assert_eq!(w.fail("a"), Requeue::Failed);
    w.write(&replan_file("a"), "retry: the ticket of a is fixed\n");
    assert_eq!(w.replan("a", START + 10).unwrap(), Replanned::Retried);

    assert_eq!(w.fail("a"), Requeue::Escalated);
    let t = w.live("a");
    assert_eq!((t.status.as_str(), t.replans), ("escalated", 1));
    assert_eq!(w.needs_you_of(&t.id), 1);
    let err = w.replan("a", START + 20).unwrap_err();
    assert!(format!("{err:#}").contains("not failed"), "{err:#}");

    let mut w = world();
    assert_eq!(w.fail("a"), Requeue::Failed);
    w.write(&replan_file("a"), REPLACE_A);
    assert!(matches!(
        w.replan("a", START + 10).unwrap(),
        Replanned::Replaced { .. }
    ));
    assert_eq!(w.fail("a"), Requeue::Escalated);
    let t = w.live("a");
    assert_eq!(t.status, "escalated");
    assert_eq!(w.needs_you_of(&t.id), 1);
}

#[test]
fn failed_task_past_replan_limit_escalates() {
    let mut w = world();
    assert_eq!(w.fail("a"), Requeue::Failed);
    let limit = i64::try_from(w.config.replan_limit_s).unwrap();
    let id = w.live("a").id;

    w.set_clock(START + limit - 1);
    assert_eq!(
        w.replan("a", START + limit - 1).unwrap(),
        Replanned::Waiting
    );
    assert_eq!(w.live("a").status, "failed");

    w.write(&replan_file("a"), "retry: too late\n");
    w.set_clock(START + limit);
    assert_eq!(w.replan("a", START + limit).unwrap(), Replanned::Escalated);
    let t = w.live("a");
    assert_eq!(
        (t.id.as_str(), t.status.as_str()),
        (id.as_str(), "escalated")
    );
    assert_eq!(t.replans, 0);
    assert_eq!(w.needs_you_of(&id), 1);
}

#[test]
fn replan_that_leaves_a_judgment_task_without_judge_is_refused_here_too() {
    let mut w = world();
    assert_eq!(w.fail("doc-judge"), Requeue::Failed);
    let judge = w.live("doc-judge");
    w.write(
        &replan_file("doc-judge"),
        "replace:
  - key: doc-check
    title: Check docs/sync.md
    kind: feature
    profile: coder-codex
    covers: [R2]
    acceptance:
      - run: test -f docs/sync.md
",
    );

    let Replanned::Refused { problems } = w.replan("doc-judge", START + 10).unwrap() else {
        panic!("the replan is not refused");
    };
    assert!(
        problems
            .iter()
            .any(|p| p.rule == "judges" && p.task.as_deref() == Some("doc")),
        "{problems:?}"
    );
    let t = w.live("doc-judge");
    assert_eq!(
        (t.id.as_str(), t.status.as_str()),
        (judge.id.as_str(), "failed")
    );
    assert_eq!(t.replans, 0);
    assert!(w.tasks("doc-check").is_empty());
    let verdict = w.verdict("doc-judge");
    assert_eq!(verdict["valid"], false, "{verdict}");
    assert!(
        verdict["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["rule"] == "judges"),
        "{verdict}"
    );
}
