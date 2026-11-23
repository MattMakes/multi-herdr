use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause, DoorError, Moved};
use horch_core::fleetd::fleet_runtime::domain::ledger::{
    decide, refused_key, Decision, Entity, EXITS, REFUSED_KIND,
};
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea, Requirement};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::ideas::domain::idea::{FIRST, STATES, TERMINAL};
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::Store;
use rusqlite::Connection;

const FEATURE: &str = "F-0001";

const LEFT: [&str; 7] = [
    "captured",
    "clarifying",
    "awaiting_approval",
    "specifying",
    "ticketing",
    "in_progress",
    "integrating",
];

fn design_exits() -> Vec<(&'static str, &'static str)> {
    let mut exits = vec![
        ("captured", "clarifying"),
        ("captured", "ticketing"),
        ("clarifying", "awaiting_approval"),
        ("awaiting_approval", "specifying"),
        ("awaiting_approval", "clarifying"),
        ("specifying", "ticketing"),
        ("specifying", "awaiting_approval"),
        ("ticketing", "in_progress"),
        ("ticketing", "awaiting_approval"),
        ("in_progress", "integrating"),
        ("in_progress", "awaiting_approval"),
        ("integrating", "merged"),
        ("integrating", "in_progress"),
        ("integrating", "awaiting_approval"),
        ("parked", "cancelled"),
    ];
    for left in LEFT {
        exits.push((left, "parked"));
        exits.push((left, "cancelled"));
        exits.push(("parked", left));
    }
    exits.sort_unstable();
    exits
}

struct World {
    _dir: tempfile::TempDir,
    store: Store,
    ideas: u32,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let new = NewFeature {
        id: FEATURE,
        name: Some("f1"),
        branch: Some("fleet/f1"),
        primary_branch: "main",
    };
    store
        .immediate(|tx| rows::insert_feature(tx, None, &new, cause("feature accepted")))
        .unwrap();
    World {
        _dir: dir,
        store,
        ideas: 0,
    }
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

    fn idea(&mut self) -> String {
        self.ideas += 1;
        let id = format!("I-{:04}", self.ideas);
        let new = NewIdea {
            id: &id,
            feature_id: FEATURE,
            priority: 0,
        };
        self.store
            .immediate(|tx| ideas_store::insert_idea(tx, None, &new, cause("idea added")))
            .unwrap();
        id
    }

    fn put(&self, id: &str, state: &str) {
        let sql = "UPDATE ideas SET state = ?2 WHERE id = ?1";
        assert_eq!(self.conn().execute(sql, [id, state]).unwrap(), 1);
    }

    fn state(&self, entity: Entity, id: &str) -> String {
        let sql = match entity {
            Entity::Idea => "SELECT state FROM ideas WHERE id = ?1",
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
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT kind, from_state, to_state, cause, text FROM events
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
            })
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    fn items(&self, entity: Entity, id: &str) -> Vec<(String, String, String)> {
        let mut stmt = self
            .conn()
            .prepare("SELECT key, kind, state FROM needs_you WHERE entity = ?1 AND entity_id = ?2")
            .unwrap();
        stmt.query_map([entity.as_str(), id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Ev {
    kind: String,
    from: Option<String>,
    to: Option<String>,
    cause: String,
    text: String,
}

fn assert_refused(w: &mut World, id: &str, to: &str) {
    let from = w.state(Entity::Idea, id);
    let before = w.events(Entity::Idea, id).len();
    let out = w.moved(Entity::Idea, id, to);
    assert!(
        matches!(out, Err(DoorError::Refused { .. })),
        "idea {from} -> {to}: {out:?}"
    );
    assert_eq!(
        w.state(Entity::Idea, id),
        from,
        "a refused row does not change"
    );
    let events = w.events(Entity::Idea, id);
    assert_eq!(events.len(), before + 1);
    let e = events.last().unwrap();
    assert_eq!(e.cause, "illegal_transition: test");
    assert_eq!(
        (e.from.as_deref(), e.to.as_deref()),
        (Some(from.as_str()), Some(to))
    );
    assert_eq!(
        w.items(Entity::Idea, id),
        [(
            refused_key(Entity::Idea, id),
            REFUSED_KIND.to_string(),
            "open".to_string()
        )]
    );
}

#[test]
fn each_exit_of_the_design_is_allowed() {
    let mut table: Vec<(&str, &str)> = EXITS
        .iter()
        .filter(|(e, _, _)| *e == Entity::Idea)
        .map(|(_, from, to)| (*from, *to))
        .collect();
    table.sort_unstable();
    assert_eq!(
        table,
        design_exits(),
        "the door has the exits of the design"
    );
    for word in table.iter().flat_map(|(from, to)| [from, to]) {
        assert!(STATES.contains(word), "idea {word} is not a state");
    }
    assert_eq!(STATES.len(), 10);

    let mut w = world();
    for (from, to) in design_exits() {
        let id = w.idea();
        if from == "parked" {
            let left = if to == "cancelled" { "in_progress" } else { to };
            w.put(&id, left);
            assert_eq!(
                w.moved(Entity::Idea, &id, "parked").unwrap(),
                Moved::Changed
            );
        } else {
            w.put(&id, from);
        }
        let before = w.events(Entity::Idea, &id).len();
        assert_eq!(
            w.moved(Entity::Idea, &id, to).unwrap(),
            Moved::Changed,
            "idea {from} -> {to}"
        );
        assert_eq!(w.state(Entity::Idea, &id), to);
        let events = w.events(Entity::Idea, &id);
        assert_eq!(events.len(), before + 1, "1 event for idea {from} -> {to}");
        let e = events.last().unwrap();
        assert_eq!(
            (e.from.as_deref(), e.to.as_deref(), e.cause.as_str()),
            (Some(from), Some(to), "test")
        );
    }
}

#[test]
fn merged_and_cancelled_are_terminal() {
    assert_eq!(TERMINAL, ["merged", "cancelled"]);
    for from in TERMINAL {
        for to in STATES.iter().filter(|to| **to != from) {
            assert_eq!(
                decide(Entity::Idea, from, to),
                Decision::Refused,
                "idea {from} -> {to}"
            );
        }
    }
    let mut w = world();
    for from in TERMINAL {
        let id = w.idea();
        w.put(&id, from);
        assert_refused(&mut w, &id, "captured");
    }
}

#[test]
fn an_illegal_idea_transition_is_refused_and_goes_to_needs_you() {
    let mut w = world();
    let id = w.idea();
    let other = w.idea();
    let (refused, legal) = w
        .store
        .immediate(|tx| {
            let refused = transition(
                tx,
                None,
                Entity::Idea,
                &id,
                "merged",
                cause("pass"),
                &NeedsYouList,
            );
            let legal = transition(
                tx,
                None,
                Entity::Idea,
                &other,
                "clarifying",
                cause("pass"),
                &NeedsYouList,
            );
            Ok((refused, legal))
        })
        .unwrap();
    assert!(
        matches!(refused, Err(DoorError::Refused { .. })),
        "{refused:?}"
    );
    assert_eq!(legal.unwrap(), Moved::Changed, "the pass goes on");
    assert_eq!(w.state(Entity::Idea, &other), "clarifying");
    assert_eq!(w.state(Entity::Idea, &id), FIRST);
    let e = w.events(Entity::Idea, &id).pop().unwrap();
    assert_eq!(e.cause, "illegal_transition: pass");
    assert_eq!(e.kind, "transition");
    assert_eq!(w.items(Entity::Idea, &id).len(), 1);

    let parked = w.idea();
    w.put(&parked, "clarifying");
    w.moved(Entity::Idea, &parked, "parked").unwrap();
    assert_refused(&mut w, &parked, "in_progress");
    assert_eq!(
        w.moved(Entity::Idea, &parked, "clarifying").unwrap(),
        Moved::Changed
    );

    let unknown = w.idea();
    w.put(&unknown, "parked");
    assert_refused(&mut w, &unknown, "captured");
    assert_eq!(
        w.moved(Entity::Idea, &unknown, "cancelled").unwrap(),
        Moved::Changed
    );
}

#[test]
fn feature_open_to_merged_is_allowed() {
    let mut w = world();
    assert_eq!(w.state(Entity::Feature, FEATURE), "open");
    assert_eq!(
        w.moved(Entity::Feature, FEATURE, "merged").unwrap(),
        Moved::Changed
    );
    assert_eq!(w.state(Entity::Feature, FEATURE), "merged");
    let e = w.events(Entity::Feature, FEATURE).pop().unwrap();
    assert_eq!(
        (e.from.as_deref(), e.to.as_deref()),
        (Some("open"), Some("merged"))
    );
}

#[test]
fn feature_ready_to_open_is_allowed() {
    let mut w = world();
    w.moved(Entity::Feature, FEATURE, "ready").unwrap();
    assert_eq!(
        w.moved(Entity::Feature, FEATURE, "open").unwrap(),
        Moved::Changed
    );
    assert_eq!(w.state(Entity::Feature, FEATURE), "open");
    let e = w.events(Entity::Feature, FEATURE).pop().unwrap();
    assert_eq!(
        (e.from.as_deref(), e.to.as_deref()),
        (Some("ready"), Some("open"))
    );
}

#[test]
fn idea_event_names_the_entity() {
    let mut w = world();
    let id = w.idea();
    assert_eq!(id, "I-0001");
    let created = w.events(Entity::Idea, &id);
    assert_eq!(created.len(), 1, "a new idea has 1 event");
    assert_eq!(created[0].kind, "transition");
    assert_eq!(created[0].to.as_deref(), Some(FIRST));
    assert_eq!(created[0].cause, "idea added");
    assert!(
        created[0].text.ends_with("idea I-0001: none -> captured"),
        "{}",
        created[0].text
    );

    w.put(&id, "clarifying");
    w.moved(Entity::Idea, &id, "awaiting_approval").unwrap();
    let e = w.events(Entity::Idea, &id).pop().unwrap();
    assert_eq!(e.kind, "transition");
    assert_eq!(e.cause, "test");
    assert_eq!(
        e.text, "test; idea I-0001: clarifying -> awaiting_approval",
        "the text names the entity, the row and both states"
    );

    let row = ideas_store::idea(w.conn(), &id).unwrap().unwrap();
    assert_eq!(
        (row.feature_id.as_str(), row.state.as_str()),
        (FEATURE, "awaiting_approval")
    );
    let reqs = [
        Requirement {
            req_id: "FR-1",
            text: "one",
        },
        Requirement {
            req_id: "FR-2",
            text: "two",
        },
    ];
    w.store
        .immediate(|tx| {
            ideas_store::insert_requirements(tx, None, &id, &reqs, cause("spec validated"))
        })
        .unwrap();
    assert_eq!(
        ideas_store::requirements(w.conn(), &id).unwrap(),
        [
            ("FR-1".to_string(), "one".to_string()),
            ("FR-2".to_string(), "two".to_string())
        ]
    );
    let e = w.events(Entity::Idea, &id).pop().unwrap();
    assert_eq!(
        (e.kind.as_str(), e.cause.as_str()),
        ("note", "spec validated")
    );
    assert_eq!(e.to, None, "a requirement is not a state");
}
