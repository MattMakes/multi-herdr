//! The task branch, the bundle and the feature are entities of the 1 door
//! (docs/specs/fleetd/design.md, "Ledger", rule 1 "One door"; "Landing a
//! task branch on its feature branch"; "The bundle state machine"; "The
//! feature state machine"; docs/tickets/fleetd-p4/06-integration-entities-in-the-door.md).
//!
//! Each state change goes through `transition()` with 1 event. The rows
//! store of integration inserts the rows of the 6 integration tables, reads
//! them by key and writes each fact that is not a state with 1 event.
//! Each test makes its own temporary ledger. No test reads the clock, the
//! load or the limits of the machine.

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, NewRun as NewJob};
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError, Moved};
use horch_core::fleetd::fleet_runtime::domain::jobs::Kind;
use horch_core::fleetd::fleet_runtime::domain::ledger::{
    decide, refused_key, Decision, Entity, EXITS, REFUSED_KIND,
};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::adapters::sqlite::{
    self as rows, BranchFact, BundleFact, FeatureFact, NewBranch, NewBundle, NewFeature, NewGateRun,
};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::Store;
use rusqlite::Connection;

const FEATURE: &str = "F-0001";

/// The 3 entities of this ticket.
const ENTITIES: [Entity; 3] = [Entity::TaskBranch, Entity::Bundle, Entity::Feature];

/// The column "Leaves when" of the task branch table of the design. Each
/// state that is not terminal also leaves to `abandoned`: `horch feature
/// abandon` moves each branch of the feature that is not landed, and
/// `horch branch abandon` moves a `conflicted` or `gate_failed` branch.
const BRANCH_EXITS: &[(&str, &str)] = &[
    ("building", "verifying"),
    ("building", "abandoned"),
    ("verifying", "ready"),
    ("verifying", "building"),
    ("verifying", "abandoned"),
    ("ready", "in_bundle"),
    ("ready", "abandoned"),
    ("in_bundle", "landed"),
    ("in_bundle", "ready"),
    ("in_bundle", "gate_failed"),
    ("in_bundle", "conflicted"),
    ("in_bundle", "abandoned"),
    ("conflicted", "ready"),
    ("conflicted", "abandoned"),
    ("gate_failed", "verifying"),
    ("gate_failed", "abandoned"),
];

/// The column "Leaves when" of the bundle table of the design.
const BUNDLE_EXITS: &[(&str, &str)] = &[
    ("building", "gating"),
    ("building", "dropped"),
    ("gating", "passed"),
    ("gating", "failed"),
    ("gating", "dropped"),
    ("passed", "landed"),
    ("passed", "dropped"),
];

/// The column "Leaves when" of the feature table of the design.
const FEATURE_EXITS: &[(&str, &str)] = &[
    ("open", "ready"),
    ("open", "merged"),
    ("open", "abandoned"),
    ("ready", "open"),
    ("ready", "merged"),
    ("ready", "abandoned"),
];

/// The states of each table, and the terminal states among them.
fn states(entity: Entity) -> (&'static [&'static str], &'static [&'static str]) {
    match entity {
        Entity::TaskBranch => (
            &[
                "building",
                "verifying",
                "ready",
                "in_bundle",
                "landed",
                "conflicted",
                "gate_failed",
                "abandoned",
            ],
            &["landed", "abandoned"],
        ),
        Entity::Bundle => (
            &[
                "building", "gating", "passed", "landed", "failed", "dropped",
            ],
            &["landed", "failed", "dropped"],
        ),
        Entity::Feature => (
            &["open", "ready", "merged", "abandoned"],
            &["merged", "abandoned"],
        ),
        other => panic!("{other} is not an entity of this ticket"),
    }
}

fn design_exits(entity: Entity) -> &'static [(&'static str, &'static str)] {
    match entity {
        Entity::TaskBranch => BRANCH_EXITS,
        Entity::Bundle => BUNDLE_EXITS,
        Entity::Feature => FEATURE_EXITS,
        other => panic!("{other} is not an entity of this ticket"),
    }
}

/// The word that the text of each event of `entity` names.
fn noun(entity: Entity) -> &'static str {
    match entity {
        Entity::TaskBranch => "task branch",
        Entity::Bundle => "bundle",
        Entity::Feature => "feature",
        other => panic!("{other} is not an entity of this ticket"),
    }
}

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _dir: tempfile::TempDir,
    store: Store,
    run: Run,
    tasks: u32,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let new = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = store
        .immediate(|tx| runs::create(tx, dir.path(), &new))
        .unwrap();
    let mut w = World {
        _dir: dir,
        store,
        run,
        tasks: 0,
    };
    w.feature(FEATURE, "fleet/r1");
    w
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

impl World {
    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    fn feature(&mut self, id: &str, branch: &str) {
        let new = NewFeature {
            id,
            name: Some(&id.to_lowercase()),
            branch: Some(branch),
            primary_branch: "main",
        };
        self.store
            .immediate(|tx| rows::insert_feature(tx, None, &new, cause("feature accepted")))
            .unwrap();
    }

    /// A new task and its task branch `name` of [`FEATURE`], in `building`.
    fn task_branch(&mut self, name: &str) -> String {
        self.tasks += 1;
        let n = self.tasks;
        let new = NewTask::work(&format!("t{n}"), &format!("t{n}.md"), "dev");
        let run = self.run.clone();
        let task = self
            .store
            .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id;
        let new = NewBranch {
            name,
            feature_id: FEATURE,
            task_id: Some(&task),
            owner: "worker",
            due_at: 100,
        };
        self.store
            .immediate(|tx| rows::insert_branch(tx, None, &new, cause("task bound")))
            .unwrap();
        name.to_string()
    }

    /// A new bundle of [`FEATURE`] with 1 new task branch `member`.
    fn bundle(&mut self, member: &str) -> String {
        self.task_branch(member);
        let new = NewBundle {
            feature_id: FEATURE,
            base_tip: "base0",
            parent_id: None,
            members: &[member],
        };
        let id = self
            .store
            .immediate(|tx| rows::insert_bundle(tx, None, &new, cause("bundle built")))
            .unwrap();
        id.to_string()
    }

    /// A new row of `entity` with the id `n`, in its first state.
    fn row(&mut self, entity: Entity, n: usize) -> String {
        match entity {
            Entity::TaskBranch => self.task_branch(&format!("fleet/r1/t{n}")),
            Entity::Bundle => self.bundle(&format!("fleet/r1/b{n}")),
            Entity::Feature => {
                let id = format!("F-1{n:03}");
                self.feature(&id, &format!("fleet/f{n}"));
                id
            }
            other => panic!("{other} is not an entity of this ticket"),
        }
    }

    /// Put the row `id` of `entity` in `state` with no event: the set-up of
    /// a test, not a move.
    fn put(&self, entity: Entity, id: &str, state: &str) {
        let sql = match entity {
            Entity::TaskBranch => "UPDATE branches SET landing_state = ?2 WHERE name = ?1",
            Entity::Bundle => "UPDATE bundles SET state = ?2 WHERE id = ?1",
            Entity::Feature => "UPDATE features SET state = ?2 WHERE id = ?1",
            other => panic!("{other} is not an entity of this ticket"),
        };
        assert_eq!(self.conn().execute(sql, [id, state]).unwrap(), 1);
    }

    fn state(&self, entity: Entity, id: &str) -> String {
        let sql = match entity {
            Entity::TaskBranch => "SELECT landing_state FROM branches WHERE name = ?1",
            Entity::Bundle => "SELECT state FROM bundles WHERE id = ?1",
            Entity::Feature => "SELECT state FROM features WHERE id = ?1",
            other => panic!("{other} is not an entity of this ticket"),
        };
        self.conn().query_row(sql, [id], |r| r.get(0)).unwrap()
    }

    fn moved(&mut self, entity: Entity, id: &str, to: &str) -> Result<Moved, DoorError> {
        self.store
            .immediate(|tx| {
                Ok(transition(
                    tx,
                    None,
                    entity,
                    id,
                    to,
                    cause("test"),
                    &NeedsYouList,
                ))
            })
            .unwrap()
    }

    fn events(&self, entity: Entity, id: &str) -> Vec<Ev> {
        events(self.conn(), entity, id)
    }
}

/// 1 event row: its kind, states, cause, text and data.
#[derive(Debug, Clone, PartialEq)]
struct Ev {
    kind: String,
    from: Option<String>,
    to: Option<String>,
    cause: String,
    text: String,
    data: Option<String>,
}

fn events(conn: &Connection, entity: Entity, id: &str) -> Vec<Ev> {
    let mut stmt = conn
        .prepare(
            "SELECT kind, from_state, to_state, cause, text, data_json FROM events
             WHERE entity = ?1 AND entity_id = ?2 ORDER BY event_id",
        )
        .unwrap();
    stmt.query_map([entity.as_str(), id], |r| {
        Ok(Ev {
            kind: r.get(0)?,
            from: r.get(1)?,
            to: r.get(2)?,
            cause: r.get(3)?,
            text: r.get(4)?,
            data: r.get(5)?,
        })
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

// ─── the state tables ───────────────────────────────────────────────────

#[test]
fn each_allowed_exit_of_the_3_tables_moves_and_records_1_event() {
    for entity in ENTITIES {
        let mut table: Vec<(&str, &str)> = EXITS
            .iter()
            .filter(|(e, _, _)| *e == entity)
            .map(|(_, from, to)| (*from, *to))
            .collect();
        let mut design = design_exits(entity).to_vec();
        table.sort_unstable();
        design.sort_unstable();
        assert_eq!(
            table, design,
            "the exits of {entity} are not the design table"
        );
    }
    let mut w = world();
    for entity in ENTITIES {
        for (n, (from, to)) in design_exits(entity).iter().enumerate() {
            let id = w.row(entity, n);
            w.put(entity, &id, from);
            let before = w.events(entity, &id).len();
            assert_eq!(
                w.moved(entity, &id, to).unwrap(),
                Moved::Changed,
                "{entity} {from} -> {to}"
            );
            assert_eq!(w.state(entity, &id), *to);
            let events = w.events(entity, &id);
            assert_eq!(events.len(), before + 1, "{entity} {from} -> {to}");
            let e = events.last().unwrap();
            assert_eq!(e.kind, "transition");
            assert_eq!(
                (e.from.as_deref(), e.to.as_deref()),
                (Some(*from), Some(*to))
            );
            assert_eq!(e.cause, "test");
            assert!(
                e.text.contains(&format!("{} {id}", noun(entity))),
                "the event text does not name the {}: {}",
                noun(entity),
                e.text
            );
        }
    }
}

#[test]
fn illegal_exit_is_refused_and_opens_1_needs_you_item() {
    let mut w = world();
    let illegal = [
        (Entity::TaskBranch, "ready", "landed"),
        (Entity::Bundle, "building", "landed"),
        (Entity::Feature, "merged", "open"),
    ];
    for (n, (entity, from, to)) in illegal.into_iter().enumerate() {
        let id = w.row(entity, n);
        w.put(entity, &id, from);
        let before = w.events(entity, &id).len();
        // The refusal and a legal move of another row share 1 pass.
        let other = w.task_branch(&format!("fleet/r1/other{n}"));
        let (refused, legal) = w
            .store
            .immediate(|tx| {
                let refused = transition(tx, None, entity, &id, to, cause("pass"), &NeedsYouList);
                let legal = transition(
                    tx,
                    None,
                    Entity::TaskBranch,
                    &other,
                    "verifying",
                    cause("pass"),
                    &NeedsYouList,
                );
                Ok((refused, legal))
            })
            .unwrap();
        assert!(
            matches!(refused, Err(DoorError::Refused { .. })),
            "{entity} {from} -> {to}: {refused:?}"
        );
        assert_eq!(legal.unwrap(), Moved::Changed, "the pass goes on");
        assert_eq!(w.state(Entity::TaskBranch, &other), "verifying");
        assert_eq!(w.state(entity, &id), from, "a refused row does not change");
        let events = w.events(entity, &id);
        assert_eq!(events.len(), before + 1);
        let e = events.last().unwrap();
        assert_eq!(e.cause, "illegal_transition: pass");
        assert!(e.text.contains(noun(entity)), "{}", e.text);
        let items: Vec<(String, String, String)> = {
            let mut stmt = w
                .conn()
                .prepare(
                    "SELECT key, kind, state FROM needs_you WHERE entity = ?1 AND entity_id = ?2",
                )
                .unwrap();
            stmt.query_map([entity.as_str(), id.as_str()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
        };
        assert_eq!(
            items,
            [(
                refused_key(entity, &id),
                REFUSED_KIND.to_string(),
                "open".to_string()
            )]
        );
    }
}

#[test]
fn terminal_states_have_no_exit() {
    for entity in ENTITIES {
        let (all, terminal) = states(entity);
        for from in terminal {
            for to in all.iter().filter(|to| *to != from) {
                assert_eq!(
                    decide(entity, from, to),
                    Decision::Refused,
                    "{entity} {from} -> {to}"
                );
            }
        }
        // Each other state has an exit.
        for from in all.iter().filter(|s| !terminal.contains(s)) {
            assert!(
                EXITS.iter().any(|(e, f, _)| *e == entity && f == from),
                "{entity} {from} has no exit"
            );
        }
    }
    let mut w = world();
    let id = w.row(Entity::Bundle, 0);
    w.put(Entity::Bundle, &id, "landed");
    assert!(matches!(
        w.moved(Entity::Bundle, &id, "building"),
        Err(DoorError::Refused { .. })
    ));
    assert_eq!(w.state(Entity::Bundle, &id), "landed");
}

// ─── the rows store ─────────────────────────────────────────────────────

#[test]
fn bundle_with_its_members_is_1_transaction() {
    let mut w = world();
    let a = w.task_branch("fleet/r1/a");
    let b = w.task_branch("fleet/r1/b");
    let new = NewBundle {
        feature_id: FEATURE,
        base_tip: "base0",
        parent_id: None,
        members: &[&b, &a],
    };
    let id = w
        .store
        .immediate(|tx| rows::insert_bundle(tx, None, &new, cause("bundle built")))
        .unwrap();
    let row = rows::bundle(w.conn(), id).unwrap().unwrap();
    assert_eq!(row.id, id);
    assert_eq!(row.feature_id, FEATURE);
    assert_eq!(row.base_tip, "base0");
    assert_eq!(row.state, "building");
    assert_eq!(row.parent_id, None);
    assert_eq!((row.candidate_ref, row.candidate_tip), (None, None));
    assert_eq!(row.members, [b.clone(), a.clone()], "queue order");
    let events = w.events(Entity::Bundle, &id.to_string());
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].from.as_deref(), events[0].to.as_deref()),
        (None, Some("building"))
    );
    assert_eq!(events[0].cause, "bundle built");

    // `a` is in the open bundle `id` (I12), and `nope` has no row. Each
    // insert fails as a whole, and the caller's transaction goes on.
    let c = w.task_branch("fleet/r1/c");
    let bundles = count(w.conn(), "SELECT count(*) FROM bundles");
    let members = count(w.conn(), "SELECT count(*) FROM bundle_members");
    let all_events = count(w.conn(), "SELECT count(*) FROM events");
    for bad in [[c.as_str(), a.as_str()], [c.as_str(), "nope"]] {
        let new = NewBundle {
            feature_id: FEATURE,
            base_tip: "base1",
            parent_id: Some(id),
            members: &bad,
        };
        let (insert, other) = w
            .store
            .immediate(|tx| {
                let insert = rows::insert_bundle(tx, None, &new, cause("bundle built"));
                let other = transition(
                    tx,
                    None,
                    Entity::TaskBranch,
                    &c,
                    "verifying",
                    cause("pass"),
                    &NeedsYouList,
                );
                Ok((insert, other))
            })
            .unwrap();
        assert!(insert.is_err(), "{bad:?}");
        assert!(other.is_ok());
        assert_eq!(count(w.conn(), "SELECT count(*) FROM bundles"), bundles);
        assert_eq!(
            count(w.conn(), "SELECT count(*) FROM bundle_members"),
            members
        );
        w.put(Entity::TaskBranch, &c, "building");
    }
    // Only the 2 moves of `c` wrote events.
    assert_eq!(
        count(w.conn(), "SELECT count(*) FROM events"),
        all_events + 2
    );
    // A bundle has at least 1 member.
    let empty = NewBundle {
        feature_id: FEATURE,
        base_tip: "base1",
        parent_id: None,
        members: &[],
    };
    let out = w
        .store
        .immediate(|tx| Ok(rows::insert_bundle(tx, None, &empty, cause("bundle built"))))
        .unwrap();
    assert!(out.is_err());
}

#[test]
fn fact_writes_record_an_event_with_the_entity() {
    let mut w = world();
    let branch = w.task_branch("fleet/r1/a");
    let bundle = w.bundle("fleet/r1/b");
    let bundle_id: i64 = bundle.parse().unwrap();

    // Each fact of a task branch, of a bundle and of a feature, and each
    // fact of a feature branch row, which has no landing state: its event
    // is an event of its feature.
    let feature_branch = "fleet/r1";
    let new = NewBranch {
        name: feature_branch,
        feature_id: FEATURE,
        task_id: None,
        owner: "fleetd",
        due_at: 500,
    };
    w.store
        .immediate(|tx| rows::insert_branch(tx, None, &new, cause("feature branch made")))
        .unwrap();
    let fb = rows::branch(w.conn(), feature_branch).unwrap().unwrap();
    assert_eq!(fb.landing_state, None);
    assert_eq!(fb.task_id, None);

    let branch_facts = [
        BranchFact::SeenTip("tip1"),
        BranchFact::SeenRemoteTip("tip0"),
        BranchFact::QueueSeq(Some(7)),
        BranchFact::RepairAttempts(2),
        BranchFact::DueAt(900),
    ];
    for (name, entity, id) in [
        (branch.as_str(), Entity::TaskBranch, branch.as_str()),
        (feature_branch, Entity::Feature, FEATURE),
    ] {
        for fact in branch_facts {
            let before = w.events(entity, id).len();
            w.store
                .immediate(|tx| rows::set_branch_fact(tx, None, name, fact, cause("observed")))
                .unwrap();
            let events = w.events(entity, id);
            assert_eq!(events.len(), before + 1, "{name}: {fact:?}");
            let e = events.last().unwrap();
            assert_eq!(e.kind, "note");
            assert_eq!((e.from.as_deref(), e.to.as_deref()), (None, None));
            assert_eq!(e.cause, "observed");
            assert!(
                e.text.contains(&format!("{} {id}", noun(entity))),
                "{}",
                e.text
            );
            assert!(e.data.as_deref().unwrap().contains(name), "{:?}", e.data);
        }
        let row = rows::branch(w.conn(), name).unwrap().unwrap();
        assert_eq!(row.seen_tip.as_deref(), Some("tip1"));
        assert_eq!(row.seen_remote_tip.as_deref(), Some("tip0"));
        assert_eq!(row.queue_seq, Some(7));
        assert_eq!(row.repair_attempts, 2);
        assert_eq!(row.due_at, 900);
    }
    let row = rows::branch(w.conn(), &branch).unwrap().unwrap();
    assert_eq!(row.landing_state.as_deref(), Some("building"));
    assert_eq!(row.feature_id, FEATURE);
    assert_eq!(row.owner, "worker");
    assert!(!row.orphan);

    for fact in [
        BundleFact::CandidateRef(Some("refs/horch/bundle/1")),
        BundleFact::CandidateTip("cand1"),
    ] {
        let before = w.events(Entity::Bundle, &bundle).len();
        w.store
            .immediate(|tx| rows::set_bundle_fact(tx, None, bundle_id, fact, cause("built")))
            .unwrap();
        let events = w.events(Entity::Bundle, &bundle);
        assert_eq!(events.len(), before + 1, "{fact:?}");
        assert!(events
            .last()
            .unwrap()
            .text
            .contains(&format!("bundle {bundle}")));
    }
    let row = rows::bundle(w.conn(), bundle_id).unwrap().unwrap();
    assert_eq!(row.candidate_ref.as_deref(), Some("refs/horch/bundle/1"));
    assert_eq!(row.candidate_tip.as_deref(), Some("cand1"));

    for fact in [
        FeatureFact::PushState("failed"),
        FeatureFact::PushFailures(3),
        FeatureFact::PushNotBefore(Some(1200)),
        FeatureFact::PrState("open"),
    ] {
        let before = w.events(Entity::Feature, FEATURE).len();
        w.store
            .immediate(|tx| rows::set_feature_fact(tx, None, FEATURE, fact, cause("pushed")))
            .unwrap();
        let events = w.events(Entity::Feature, FEATURE);
        assert_eq!(events.len(), before + 1, "{fact:?}");
        assert!(events
            .last()
            .unwrap()
            .text
            .contains(&format!("feature {FEATURE}")));
    }
    let row = rows::feature(w.conn(), FEATURE).unwrap().unwrap();
    assert_eq!(row.state, "open");
    assert_eq!(row.name.as_deref(), Some("f-0001"));
    assert_eq!(row.branch.as_deref(), Some("fleet/r1"));
    assert_eq!(row.primary_branch, "main");
    assert_eq!(row.push_state.as_deref(), Some("failed"));
    assert_eq!(row.push_failures, 3);
    assert_eq!(row.push_not_before, Some(1200));
    assert_eq!(row.pr_state, "open");

    // A fact of a row that does not exist writes nothing.
    let all_events = count(w.conn(), "SELECT count(*) FROM events");
    let out = w
        .store
        .immediate(|tx| {
            Ok(rows::set_branch_fact(
                tx,
                None,
                "fleet/none",
                BranchFact::DueAt(1),
                cause("observed"),
            ))
        })
        .unwrap();
    assert!(out.is_err());
    assert_eq!(count(w.conn(), "SELECT count(*) FROM events"), all_events);
}

#[test]
fn gate_run_and_remote_tip_read_back() {
    let mut w = world();
    let job = jobs_store::insert(
        w.conn(),
        &NewJob {
            kind: Kind::FastGate,
            subject: FEATURE,
            repo: None,
            input_key: "tip1",
            run: 1,
            abandoned_runs: 0,
            not_before: None,
            time_limit_s: 60,
        },
    )
    .unwrap();
    let new = NewGateRun {
        feature_id: FEATURE,
        gate: "fast",
        tip: "tip1",
        base_tip: "base0",
        job_id: job,
    };
    let before = w.events(Entity::Feature, FEATURE).len();
    let id = w
        .store
        .immediate(|tx| rows::insert_gate_run(tx, None, &new, cause("gate queued")))
        .unwrap();
    let events = w.events(Entity::Feature, FEATURE);
    assert_eq!(events.len(), before + 1);
    assert_eq!(events.last().unwrap().cause, "gate queued");
    let row = rows::gate_run(w.conn(), id).unwrap().unwrap();
    assert_eq!(
        (row.feature_id.as_str(), row.gate.as_str(), row.tip.as_str()),
        (FEATURE, "fast", "tip1")
    );
    assert_eq!((row.base_tip.as_str(), row.job_id), ("base0", job));
    assert_eq!(row.result, None);

    for tip in ["r1", "r2"] {
        w.store
            .immediate(|tx| rows::put_remote_tip(tx, None, "/repo", "main", tip))
            .unwrap();
        let row = rows::remote_tip(w.conn(), "/repo", "main")
            .unwrap()
            .unwrap();
        assert_eq!(row.tip, tip);
        assert_eq!(row.stale_since, None);
    }
    assert_eq!(rows::remote_tip(w.conn(), "/repo", "dev").unwrap(), None);
    assert_eq!(rows::feature(w.conn(), "F-9999").unwrap(), None);
    assert_eq!(rows::branch(w.conn(), "fleet/none").unwrap(), None);
    assert_eq!(rows::bundle(w.conn(), 99).unwrap(), None);
}
