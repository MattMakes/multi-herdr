#![cfg(unix)]

use std::fs;

use horch_core::digest::sha256_bytes;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, IdeaRow, NewIdea};
use horch_core::fleetd::ideas::application::approval::{
    approve, check_i7, revise, trusted_approval, Approved, Revised,
};
use horch_core::fleetd::ideas::application::needs_you::open_items;
use horch_core::fleetd::ideas::domain::approval::{i7_key, Refusal, APPROVAL_JSON, BY_OPERATOR};
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::Store;
use horch_platform::channel::User;

const FEATURE: &str = "F-0001";
const IDEA: &str = "I-0001";

const ASSUMPTIONS: &str = "# I-0001 assumptions
feature: approve-flow

1. [confirmed] The command prints the assumptions first. (Q1)
2. [assumed] An approval names 1 file.
";

struct World {
    _dir: tempfile::TempDir,
    store: Store,
    folder: IdeaFolder,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn sha(text: &str) -> String {
    sha256_bytes(text.as_bytes()).hex()
}

fn world(state: &str) -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    let feature = NewFeature {
        id: FEATURE,
        name: Some("approve-flow"),
        branch: Some("fleet/approve-flow"),
        primary_branch: "main",
    };
    let idea = NewIdea {
        id: IDEA,
        feature_id: FEATURE,
        priority: 0,
    };
    store
        .immediate(|tx| {
            rows::insert_feature(tx, None, &feature, cause("feature accepted"))?;
            ideas_store::insert_idea(tx, None, &idea, cause("idea added"))
        })
        .unwrap();
    let w = World {
        folder: IdeaFolder::create(&dir.path().join("ideas"), IDEA, User::current().unwrap())
            .unwrap(),
        _dir: dir,
        store,
    };
    w.put(state);
    w.folder.write_assumptions(ASSUMPTIONS).unwrap();
    w
}

impl World {
    fn put(&self, state: &str) {
        let sql = "UPDATE ideas SET state = ?2 WHERE id = ?1";
        assert_eq!(self.store.conn().execute(sql, [IDEA, state]).unwrap(), 1);
    }

    fn row(&self) -> IdeaRow {
        ideas_store::idea(self.store.conn(), IDEA).unwrap().unwrap()
    }

    fn approve(&mut self, seen: &str) -> Approved {
        let folder = self.folder.clone();
        let approved = self
            .store
            .immediate(|tx| approve(tx, None, &folder, IDEA, seen))
            .unwrap();
        if let Approved::Approved(file) = &approved {
            file.write(&self.folder).unwrap();
        }
        approved
    }

    fn i7(&mut self) -> bool {
        let folder = self.folder.clone();
        self.store
            .immediate(|tx| check_i7(tx, None, &folder, IDEA))
            .unwrap()
    }

    fn trusted(&mut self) -> Option<String> {
        let folder = self.folder.clone();
        self.store
            .immediate(|tx| trusted_approval(tx, None, &folder, IDEA))
            .unwrap()
            .map(|a| a.file)
    }

    fn approval_json(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.folder.dir().join(APPROVAL_JSON)).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn events_with(&self, prefix: &str) -> usize {
        let pattern = format!("{prefix}%");
        self.store
            .conn()
            .query_row(
                "SELECT count(*) FROM events
                 WHERE entity = 'idea' AND entity_id = ?1 AND cause LIKE ?2",
                [IDEA, pattern.as_str()],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn open_i7_items(&self) -> usize {
        open_items(self.store.conn())
            .unwrap()
            .iter()
            .filter(|i| i.key == i7_key(IDEA))
            .count()
    }

    fn forge(&self, text: &str) {
        self.folder.write_approval(text).unwrap();
    }
}

#[test]
fn approve_writes_approval_json_and_moves_to_specifying() {
    let mut w = world("awaiting_approval");
    let seen = sha(ASSUMPTIONS);
    assert!(matches!(w.approve(&seen), Approved::Approved(_)));

    let row = w.row();
    assert_eq!(row.state, "specifying");
    assert_eq!(row.approved_sha256.as_deref(), Some(seen.as_str()));
    let on_disk = fs::read_to_string(w.folder.dir().join(APPROVAL_JSON)).unwrap();
    assert_eq!(row.approval_sha256, Some(sha(&on_disk)));

    let json = w.approval_json();
    assert_eq!(json["idea"], IDEA);
    assert_eq!(json["file"], "assumptions.md");
    assert_eq!(json["sha256"], seen.as_str());
    assert_eq!(json["by"], BY_OPERATOR);
    assert!(json["approved_at"].as_str().is_some_and(|t| !t.is_empty()));

    assert_eq!(w.trusted().as_deref(), Some("assumptions.md"));
    assert!(w.i7());
}

#[test]
fn approve_with_a_stale_hash_is_refused() {
    let mut w = world("awaiting_approval");
    let seen = sha(ASSUMPTIONS);
    let changed = ASSUMPTIONS.replace("1 file", "2 files");
    w.folder.write_assumptions(&changed).unwrap();

    match w.approve(&seen) {
        Approved::Refused(Refusal::AssumptionsChanged { seen: s, now, .. }) => {
            assert_eq!(s, seen);
            assert_eq!(now, sha(&changed));
        }
        other => panic!("not refused for a changed file: {other:?}"),
    }
    let row = w.row();
    assert_eq!(row.state, "awaiting_approval");
    assert_eq!(row.approved_sha256, None);
    assert_eq!(row.approval_sha256, None);
    assert!(!w.folder.dir().join(APPROVAL_JSON).exists());

    w.put("clarifying");
    assert!(matches!(
        w.approve(&sha(&changed)),
        Approved::Refused(Refusal::NotAwaitingApproval { .. })
    ));
    assert_eq!(w.row().state, "clarifying");
}

#[test]
fn fm32_forged_approval_is_ignored() {
    let mut w = world("awaiting_approval");
    let seen = sha(ASSUMPTIONS);
    let forged = format!(
        r#"{{"idea":"{IDEA}","file":"assumptions.md","sha256":"{seen}","approved_at":"2026-11-18T00:00:00Z","by":"operator"}}"#
    );
    w.forge(&forged);
    assert_eq!(w.trusted(), None);
    assert_eq!(w.events_with("approval.json ignored"), 1);
    assert_eq!(w.trusted(), None);
    assert_eq!(w.events_with("approval.json ignored"), 1);
    let row = w.row();
    assert_eq!(row.state, "awaiting_approval");
    assert_eq!(row.approved_sha256, None);

    w.put("ticketing");
    assert!(!w.i7());
    assert_eq!(w.open_i7_items(), 1);

    w.put("awaiting_approval");
    assert!(matches!(w.approve(&seen), Approved::Approved(_)));
    w.folder
        .write_assumptions(&ASSUMPTIONS.replace("1 file", "2 files"))
        .unwrap();
    let other = format!(
        r#"{{"idea":"{IDEA}","file":"spec.md","sha256":"{}","approved_at":"2026-11-18T00:00:00Z","by":"operator"}}"#,
        sha("spec")
    );
    w.forge(&other);
    w.folder.write_spec("spec").unwrap();
    assert_eq!(w.trusted(), None);
    assert!(!w.i7());
}

#[test]
fn revise_sends_the_idea_back_to_clarifying() {
    let mut w = world("awaiting_approval");
    let folder = w.folder.clone();
    let text = "Use 1 approval for each file.\n";
    let revised = w
        .store
        .immediate(|tx| revise(tx, None, &folder, IDEA, text))
        .unwrap();
    let Revised::Revised(amendment) = revised else {
        panic!("not revised: {revised:?}");
    };
    amendment.write(&w.folder).unwrap();
    assert_eq!(w.row().state, "clarifying");
    assert_eq!(w.folder.read_amendments().unwrap(), vec![text.to_string()]);

    let again = w
        .store
        .immediate(|tx| revise(tx, None, &folder, IDEA, "again\n"))
        .unwrap();
    assert!(matches!(
        again,
        Revised::Refused(Refusal::NotAwaitingApproval { .. })
    ));
    assert_eq!(w.folder.read_amendments().unwrap().len(), 1);
}

#[test]
fn i7_changed_assumptions_after_approval_hold_the_idea() {
    let mut w = world("awaiting_approval");
    let seen = sha(ASSUMPTIONS);
    assert!(matches!(w.approve(&seen), Approved::Approved(_)));
    w.put("in_progress");
    assert!(w.i7());
    assert_eq!(w.open_i7_items(), 0);

    w.folder
        .write_assumptions(&ASSUMPTIONS.replace("1 file", "2 files"))
        .unwrap();
    assert!(!w.i7());
    assert_eq!(w.open_i7_items(), 1);
    assert!(!w.i7());
    assert_eq!(w.open_i7_items(), 1);

    w.folder.write_assumptions(ASSUMPTIONS).unwrap();
    assert!(w.i7());
    assert_eq!(w.open_i7_items(), 0);
}
