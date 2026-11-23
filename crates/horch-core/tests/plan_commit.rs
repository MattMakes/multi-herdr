#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::ideas::application::plan_commit::{
    validate_plan, PlanCommit, Verdict, PLAN_YAML, VERDICT_FILE,
};
use horch_core::fleetd::ideas::domain::plan_validator::{DeclaredGates, RosterProfile};
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::fleetd::task_execution::domain::structural;
use horch_core::fleetd::task_execution::domain::task::judged_task;
use horch_core::taskledger::model::{Actor, FileMode};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, ticket_sha};
use horch_core::taskledger::Store;
use horch_platform::channel::User;
use rusqlite::Connection;
use serde_json::Value;

const IDEA: &str = "I-0007";
const FEATURE: &str = "F-0001";

const TICKET_A: &str = "# 01: Add backoff

<!-- horch-ticket: 1 -->
**Persona:** primary `coder-claude`
**Kind:** feature
**Check:** `cargo test`
**Own:** `src/a.rs`

## Goal

Retry with backoff.
";

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
    ticket: tickets/01-a.md
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
      - run: cargo clippy
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

struct World {
    dir: tempfile::TempDir,
    store: Store,
    run_id: String,
    roster: Vec<RosterProfile>,
    gates: DeclaredGates,
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

fn world(plan: &str) -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let root = dir.path().to_path_buf();
    let run = store
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
    let w = World {
        dir,
        store,
        run_id: run.run_id,
        roster: vec![
            profile("coder-claude", "claude", "claude-opus-5-5", "coder-codex"),
            profile("coder-codex", "codex", "gpt-5", "coder-claude"),
        ],
        gates: DeclaredGates {
            fast: Some("cargo test".into()),
            full: Some("cargo test --workspace".into()),
        },
    };
    std::fs::create_dir_all(w.folder().join("tickets")).unwrap();
    for folder in [w.ideas(), w.folder(), w.folder().join("tickets")] {
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    w.write("assumptions.md", ASSUMPTIONS);
    w.write("requirements.yaml", REQUIREMENTS);
    w.write("tickets/01-a.md", TICKET_A);
    w.write(PLAN_YAML, plan);
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

    fn commit(&mut self) -> anyhow::Result<Verdict> {
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
        validate_plan(&mut self.store, &ctx, IDEA)
    }

    fn idea(&self) -> ideas_store::IdeaRow {
        ideas_store::idea(self.conn(), IDEA).unwrap().unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn verdict(&self) -> Option<Value> {
        let text = std::fs::read_to_string(self.folder().join(VERDICT_FILE)).ok()?;
        Some(serde_json::from_str(&text).unwrap())
    }

    fn task(&self, key: &str) -> TaskRow {
        let sql = "SELECT task_id, status, primary_persona, work_kind, kind, parent_task_id,
                run_id, spec_json, task_file, ticket_snapshot, ticket_sha, land_check
             FROM tasks WHERE idea_id = ?1 AND json_extract(spec_json, '$.key') = ?2";
        self.conn()
            .query_row(sql, [IDEA, key], |r| {
                Ok(TaskRow {
                    id: r.get(0)?,
                    status: r.get(1)?,
                    profile: r.get(2)?,
                    work_kind: r.get(3)?,
                    kind: r.get(4)?,
                    parent: r.get(5)?,
                    run_id: r.get(6)?,
                    spec: serde_json::from_str(&r.get::<_, String>(7)?).unwrap(),
                    task_file: r.get(8)?,
                    snapshot: r.get(9)?,
                    sha: r.get(10)?,
                    land_check: r.get(11)?,
                })
            })
            .unwrap()
    }
}

#[derive(Debug)]
struct TaskRow {
    id: String,
    status: String,
    profile: String,
    work_kind: Option<String>,
    kind: String,
    parent: Option<String>,
    run_id: String,
    spec: Value,
    task_file: String,
    snapshot: Option<String>,
    sha: Option<String>,
    land_check: Option<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    ticket_sha(std::str::from_utf8(bytes).unwrap())
}

fn assert_nothing_committed(w: &World) {
    assert_eq!(
        w.count("SELECT count(*) FROM tasks WHERE idea_id = 'I-0007'"),
        0
    );
    assert_eq!(w.count("SELECT count(*) FROM task_deps"), 0);
    assert_eq!(w.count("SELECT count(*) FROM task_covers"), 0);
    assert_eq!(
        w.count("SELECT count(*) FROM requirements WHERE idea_id = 'I-0007'"),
        0
    );
    let idea = w.idea();
    assert_eq!(idea.state, "ticketing");
    assert_eq!(idea.plan_sha256, None);
}

fn assert_task_rows(w: &World, tasks: &[(String, String)]) -> [TaskRow; 4] {
    let keys: Vec<&str> = tasks.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["a", "b", "doc", "doc-judge"]);
    assert_eq!(
        w.count("SELECT count(*) FROM tasks WHERE idea_id = 'I-0007'"),
        4
    );
    let rows = [w.task("a"), w.task("b"), w.task("doc"), w.task("doc-judge")];
    for ((key, id), t) in tasks.iter().zip(&rows) {
        assert_eq!(t.run_id, w.run_id, "{key}: the internal run of {FEATURE}");
        assert_eq!(t.kind, "work", "{key}");
        assert_eq!(t.spec["key"], key.as_str());
        assert_eq!(id, &t.id, "{key}: the verdict names the task id");
    }
    let [a, b, doc, judge] = &rows;
    assert_eq!(
        [&a.profile, &b.profile, &doc.profile, &judge.profile],
        ["coder-claude", "coder-codex", "coder-claude", "coder-codex"]
    );
    assert_eq!(a.work_kind.as_deref(), Some("feature"));
    assert_eq!(doc.work_kind.as_deref(), Some("docs"));
    assert_eq!(judge.work_kind, None, "judge is not a ticket Kind of today");
    assert_eq!(judge.spec["kind"], "judge");
    assert_eq!(doc.spec["acceptance"][0]["sections"][1], "## Retries");
    assert_eq!(
        b.land_check.as_deref(),
        Some("cargo test -p x && cargo clippy")
    );
    assert_eq!(doc.land_check, None, "a structural check is no command");
    assert_eq!(
        [&a.status, &b.status, &doc.status, &judge.status],
        ["ready", "pending", "ready", "pending"]
    );
    rows
}

fn pairs(w: &World, sql: &str) -> Vec<(String, String)> {
    w.conn()
        .prepare(sql)
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn assert_links_and_covers(w: &World, [a, b, doc, judge]: &[TaskRow; 4]) {
    let deps = pairs(w, "SELECT task_id, blocked_by FROM task_deps");
    assert_eq!(deps, vec![(b.id.clone(), a.id.clone())]);
    assert_eq!(judge.parent.as_deref(), Some(doc.id.as_str()));
    assert_eq!(
        judged_task(&judge.kind, judge.parent.as_deref()),
        Some(doc.id.as_str())
    );
    assert_eq!(judged_task(&doc.kind, doc.parent.as_deref()), None);
    let reqs: Vec<String> = ideas_store::requirements(w.conn(), IDEA)
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(reqs, ["R1", "R2"]);
    let covers = pairs(
        w,
        "SELECT task_id, req_id FROM task_covers WHERE idea_id = 'I-0007'
          ORDER BY task_id, req_id",
    );
    let mut want = vec![
        (a.id.clone(), "R1".to_string()),
        (b.id.clone(), "R1".to_string()),
        (doc.id.clone(), "R2".to_string()),
    ];
    want.sort();
    assert_eq!(covers, want);
}

fn assert_valid_verdict_file(w: &World) {
    let verdict = w.verdict().expect("plan.verdict.json");
    assert_eq!(verdict["idea"], IDEA);
    assert_eq!(verdict["valid"], true);
    assert_eq!(verdict["problems"], serde_json::json!([]));
    let mode = std::fs::metadata(w.folder().join(VERDICT_FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn valid_plan_commits_each_task_in_1_transaction() {
    let mut w = world(PLAN);
    let verdict = w.commit().unwrap();
    let Verdict::Committed { tasks, .. } = &verdict else {
        panic!("the plan is valid: {verdict:?}");
    };
    let rows = assert_task_rows(&w, tasks);
    assert_links_and_covers(&w, &rows);
    let idea = w.idea();
    assert_eq!(idea.state, "in_progress");
    assert_eq!(idea.phase_attempts, 0);
    assert_eq!(
        w.count(
            "SELECT count(DISTINCT e.task_id) FROM events e JOIN tasks t ON t.task_id = e.task_id
              WHERE t.idea_id = 'I-0007'"
        ),
        4
    );
    assert_valid_verdict_file(&w);
    assert!(w.commit().is_err());
    assert_eq!(
        w.count("SELECT count(*) FROM tasks WHERE idea_id = 'I-0007'"),
        4
    );
}

#[test]
fn a_failure_in_the_middle_commits_nothing() {
    let mut w = world(PLAN);
    w.conn()
        .execute_batch(
            "CREATE TEMP TRIGGER fault_on_3rd_task BEFORE INSERT ON main.tasks
             WHEN (SELECT count(*) FROM main.tasks WHERE idea_id = 'I-0007') = 2
             BEGIN SELECT RAISE(ABORT, 'fault seam: the 3rd task insert fails'); END;",
        )
        .unwrap();
    let err = w.commit().unwrap_err();
    assert!(format!("{err:#}").contains("fault seam"), "{err:#}");
    assert_nothing_committed(&w);
    assert_eq!(w.idea().phase_attempts, 0, "a fault is not a refused plan");
    assert_eq!(
        w.verdict(),
        None,
        "no verdict for a plan that did not commit"
    );

    w.conn()
        .execute_batch("DROP TRIGGER temp.fault_on_3rd_task")
        .unwrap();
    assert!(matches!(w.commit().unwrap(), Verdict::Committed { .. }));
    assert_eq!(
        w.count("SELECT count(*) FROM tasks WHERE idea_id = 'I-0007'"),
        4
    );
    assert_eq!(w.idea().state, "in_progress");
}

#[test]
fn refused_plan_writes_the_verdict_and_stays_in_ticketing() {
    let uncovered = PLAN.replace("covers: [R2]", "covers: [R1]");
    let mut w = world(&uncovered);
    let verdict = w.commit().unwrap();
    let Verdict::Refused { problems } = &verdict else {
        panic!("R2 has no task: {verdict:?}");
    };
    assert!(
        problems
            .iter()
            .any(|p| p.rule == "coverage" && p.message.contains("R2")),
        "{problems:?}"
    );
    assert_nothing_committed(&w);
    assert_eq!(w.idea().phase_attempts, 1);

    let file = w.verdict().expect("plan.verdict.json");
    assert_eq!(file["idea"], IDEA);
    assert_eq!(file["valid"], false);
    let listed = file["problems"].as_array().unwrap();
    assert_eq!(listed.len(), problems.len(), "each problem is in the file");
    assert!(listed
        .iter()
        .any(|p| p["rule"] == "coverage" && p["message"].as_str().unwrap().contains("R2")));

    w.commit().unwrap();
    assert_eq!(w.idea().phase_attempts, 2);
    assert_eq!(w.idea().state, "ticketing");

    std::fs::set_permissions(
        w.folder().join(PLAN_YAML),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let Verdict::Refused { problems } = w.commit().unwrap() else {
        panic!("a plan.yaml of mode 0644 is refused");
    };
    assert!(
        problems.iter().any(|p| p.message.contains(PLAN_YAML)),
        "{problems:?}"
    );
    assert_eq!(w.idea().phase_attempts, 3);

    w.write(PLAN_YAML, &PLAN.replace("idea: I-0007", "idea: I-0008"));
    let Verdict::Refused { problems } = w.commit().unwrap() else {
        panic!("a plan of another idea is refused");
    };
    assert!(
        problems.iter().any(|p| p.message.contains("I-0008")),
        "{problems:?}"
    );
    assert_nothing_committed(&w);
}

#[test]
fn committed_task_holds_the_ticket_snapshot() {
    let mut w = world(PLAN);
    w.commit().unwrap();
    let a = w.task("a");
    assert_eq!(a.snapshot.as_deref(), Some(TICKET_A));
    assert_eq!(a.sha.as_deref(), Some(ticket_sha(TICKET_A).as_str()));
    assert_eq!(
        Path::new(&a.task_file),
        w.folder().join("tickets/01-a.md"),
        "the task file is the ticket in the idea folder"
    );
    w.write("tickets/01-a.md", "# changed\n");
    assert_eq!(w.task("a").snapshot.as_deref(), Some(TICKET_A));
    let b = w.task("b");
    assert_eq!((b.snapshot, b.sha), (None, None));
}

#[test]
fn plan_sha256_is_on_the_idea() {
    let mut w = world(PLAN);
    w.commit().unwrap();
    let want = sha256_hex(PLAN.as_bytes());
    assert_eq!(w.idea().plan_sha256.as_deref(), Some(want.as_str()));
    assert_eq!(w.verdict().unwrap()["plan_sha256"], want.as_str());
}

#[test]
fn committed_task_has_its_own_rows() {
    let mut w = world(PLAN);
    w.commit().unwrap();
    let own = |key: &str| tasks::files(w.conn(), &w.task(key).id, FileMode::Own).unwrap();
    assert_eq!(own("a"), ["src/a.rs"], "the Own paths of the ticket");
    assert_eq!(own("b"), Vec::<String>::new(), "no outputs and no ticket");
    assert_eq!(own("doc"), ["docs/sync.md"]);
    assert_eq!(own("doc-judge"), ["docs/sync.review.md"]);
    let changed = ["docs/sync.md".to_string(), "src/other.rs".to_string()];
    assert_eq!(
        structural::outputs_of(&own("doc"), &changed),
        ["docs/sync.md"],
        "the structural check reads the Own rows"
    );
}
